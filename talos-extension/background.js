/**
 * Background service worker.
 * Match / decrypt / autofill require an unlocked session (Bearer).
 * Master key is accepted only from extension UI (popup/options), never from
 * content scripts on third-party pages.
 *
 * Default: Bearer only in memory (lost when the worker suspends).
 * Opt-in: also mirror Bearer to chrome.storage.session (cleared when
 * the browser closes). Idle auto-lock via chrome.alarms (default 15m).
 * The master key is never persisted.
 */

import {
  API,
  addCaptureNeverHost,
  assertSecureServerUrl,
  clearSessionToken,
  ensureHostPermission,
  formatPassContent,
  getConfig,
  getSessionStore,
  isCaptureNeverHost,
  loadSessionToken,
  normalizeUsername,
  parseCredential,
  saveSessionToken,
} from "./shared/api.js";

const AUTOLOCK_ALARM = "talos-autolock";
const CLIPBOARD_ALARM = "talos-clipboard-clear";
const MENU_FILL = "talos-fill-field";
const MENU_OPEN = "talos-open-popup";

/** @type {Map<number, { username: string, password: string, url: string, host: string, at: number }>} */
const pendingCaptures = new Map();

/**
 * Pending unlock/fill requested from a content script (no master key on page).
 * @type {{ type: string, path?: string, tabId?: number, frameId?: number, at: number } | null}
 */
let pendingAction = null;

/** @type {{ token: string|null, expiresAt: number|null }} */
let session = { token: null, expiresAt: null };
let hydratePromise = null;

/** True only for popup / options / SW — never content scripts (those have sender.tab). */
function isExtensionUiSender(sender) {
  if (!sender) return false;
  if (sender.tab) return false;
  const url = sender.url || "";
  if (!url) return true;
  return url.startsWith(chrome.runtime.getURL(""));
}

async function tryOpenPopup() {
  try {
    await chrome.action.openPopup();
    return true;
  } catch {
    try {
      await chrome.action.setBadgeText({ text: "!" });
      await chrome.action.setTitle({
        title: "talos-vault // Unlock required — click the extension icon",
      });
    } catch {
      /* ignore */
    }
    return false;
  }
}

function takePendingAction() {
  const p = pendingAction;
  pendingAction = null;
  if (!p) return null;
  if (Date.now() - p.at > 120_000) return null;
  return p;
}

function setPendingAction(action) {
  pendingAction = { ...action, at: Date.now() };
}

function memoryUnlocked() {
  if (!session.token) return false;
  if (session.expiresAt && Date.now() >= session.expiresAt) {
    session = { token: null, expiresAt: null };
    return false;
  }
  return true;
}

async function ensureSession() {
  if (memoryUnlocked()) return true;
  if (!hydratePromise) {
    hydratePromise = hydrateFromSessionStore().finally(() => {
      hydratePromise = null;
    });
  }
  await hydratePromise;
  return memoryUnlocked();
}

async function hydrateFromSessionStore() {
  const cfg = await getConfig();
  if (!cfg.persistSessionToken) return;

  const saved = await loadSessionToken();
  if (!saved?.token) return;
  if (saved.expiresAt && Date.now() >= saved.expiresAt) {
    await clearSessionToken();
    return;
  }

  try {
    const status = await API.status(saved.token);
    if (!status.authenticated) {
      await clearSessionToken();
      return;
    }
    session = {
      token: saved.token,
      expiresAt: saved.expiresAt || null,
    };
    await scheduleAutoLock();
  } catch {
    await clearSessionToken();
  }
}

async function scheduleAutoLock() {
  try {
    await chrome.alarms.clear(AUTOLOCK_ALARM);
  } catch {
    /* ignore */
  }
  if (!memoryUnlocked()) return;
  const cfg = await getConfig();
  const mins = cfg.autoLockMinutes;
  if (!mins) return;
  await chrome.alarms.create(AUTOLOCK_ALARM, { delayInMinutes: mins });
}

async function touchSession() {
  if (!memoryUnlocked()) return;
  await scheduleAutoLock();
}

async function lock(revokeServer = true) {
  const token = session.token;
  session = { token: null, expiresAt: null };
  await clearSessionToken();
  invalidateMatchCache();
  try {
    await chrome.alarms.clear(AUTOLOCK_ALARM);
  } catch {
    /* ignore */
  }
  if (revokeServer && token) {
    try {
      await API.logout(token);
    } catch {
      /* ignore */
    }
  }
}

async function applyUnlock(accessToken, expiresInSec) {
  const expiresAt = Date.now() + Number(expiresInSec || 7200) * 1000;
  session = { token: accessToken, expiresAt };

  const cfg = await getConfig();
  if (cfg.persistSessionToken) {
    await saveSessionToken(accessToken, expiresAt);
  } else {
    await clearSessionToken();
  }
  await scheduleAutoLock();
}

function requireUnlocked() {
  if (!memoryUnlocked()) {
    const err = new Error("Vault is locked");
    err.status = 401;
    throw err;
  }
}

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  handleMessage(message, sender)
    .then((result) => sendResponse({ ok: true, ...result }))
    .catch((err) =>
      sendResponse({
        ok: false,
        error: err.message || String(err),
        status: err.status,
      })
    );
  return true;
});

/** @type {Map<string, { at: number, result?: any, promise?: Promise<any> }>} */
const matchCache = new Map();
const MATCH_TTL_MS = 8000;
/** @type {Map<number, ReturnType<typeof setTimeout>>} */
const badgeDebounce = new Map();

function isVaultServerHost(host, serverUrl) {
  if (!host || !serverUrl) return false;
  try {
    return new URL(serverUrl).host === host;
  } catch {
    return false;
  }
}

async function matchForHost(host) {
  const key = String(host || "").toLowerCase();
  if (!key) return { host: key, matches: [], unlocked: memoryUnlocked() };

  const cfg = await getConfig();
  if (isVaultServerHost(key, cfg.serverUrl)) {
    return { host: key, matches: [], unlocked: memoryUnlocked(), skipped: true };
  }

  // No anonymous match: vault metadata must not leak while locked.
  if (!memoryUnlocked()) {
    return { host: key, matches: [], unlocked: false, needsUnlock: true };
  }

  const now = Date.now();
  const hit = matchCache.get(key);
  if (hit?.result && now - hit.at < MATCH_TTL_MS) {
    return { ...hit.result, unlocked: true };
  }
  if (hit?.promise) return hit.promise;

  const promise = (async () => {
    await ensureHostPermission(cfg.serverUrl).catch(() => false);
    const data = await API.match(key, session.token);
    const result = {
      host: key,
      matches: data.matches || [],
      unlocked: true,
    };
    matchCache.set(key, { at: Date.now(), result });
    return result;
  })().catch((err) => {
    matchCache.delete(key);
    throw err;
  });

  matchCache.set(key, { at: now, promise });
  return promise;
}

function invalidateMatchCache() {
  matchCache.clear();
}

/** @type {{ at: number, data?: any, promise?: Promise<any> } | null} */
let settingsCache = null;
const SETTINGS_TTL_MS = 60_000;

async function fetchSettingsCached() {
  const now = Date.now();
  if (settingsCache?.data && now - settingsCache.at < SETTINGS_TTL_MS) {
    return settingsCache.data;
  }
  if (settingsCache?.promise) return settingsCache.promise;
  const promise = (async () => {
    await ensureSession();
    const data = await API.settings(memoryUnlocked() ? session.token : undefined);
    if (data?.lang) {
      await chrome.storage.local.set({ uiLanguage: data.lang });
    }
    settingsCache = { at: Date.now(), data };
    return data;
  })().catch((err) => {
    settingsCache = null;
    throw err;
  });
  settingsCache = { at: now, promise };
  return promise;
}

async function handleMessage(message, sender) {
  switch (message.type) {
    case "GET_SESSION": {
      const unlocked = await ensureSession();
      const cfg = await getConfig();
      return {
        unlocked,
        persistSessionToken: cfg.persistSessionToken,
        autoLockMinutes: cfg.autoLockMinutes,
      };
    }

    case "LOCK": {
      await lock(true);
      invalidateMatchCache();
      return { unlocked: false };
    }

    case "UNLOCK": {
      // Hard rule: never accept the master key from a content script / web page.
      if (!isExtensionUiSender(sender)) {
        const err = new Error("Master key must be entered in the talos-vault extension UI");
        err.status = 403;
        throw err;
      }
      let masterKey = message.masterKey;
      const cfg = await getConfig();
      assertSecureServerUrl(cfg.serverUrl);
      const granted = await ensureHostPermission(cfg.serverUrl);
      if (!granted) throw new Error("Host permission denied for Talos server");
      if (cfg.oidcIssuer) {
        try {
          await ensureHostPermission(cfg.oidcIssuer);
        } catch {
          /* optional until OIDC used; token fetch may still fail with a clear error */
        }
      }

      try {
        let data;
        const status = await API.authStatus().catch(() => ({}));
        if (status.oidc_enabled) {
          const { loginOidc } = await import("./shared/oidc.js");
          const oidc = await loginOidc({
            issuer: cfg.oidcIssuer,
            clientId: cfg.oidcClientId,
          });
          // Strict custody requires vault passphrase; convenience may omit after wrap.
          if (status.custody_mode !== "convenience" && !masterKey) {
            throw new Error("Vault passphrase required");
          }
          data = await API.issueTokenOidc(oidc.idToken, masterKey || undefined);
        } else {
          if (!masterKey) throw new Error("Master key required");
          data = await API.issueToken(masterKey);
        }
        await applyUnlock(data.access_token, data.expires_in);
        invalidateMatchCache();
        let lang = null;
        try {
          const settings = await API.settings(session.token);
          if (settings?.lang) {
            lang = settings.lang;
            await chrome.storage.local.set({ uiLanguage: settings.lang });
          }
        } catch {
          /* ignore */
        }
        try {
          await refreshBadgeForActiveTab();
        } catch {
          /* ignore */
        }
        return { unlocked: true, expiresIn: data.expires_in, lang };
      } finally {
        masterKey = "";
        try {
          message.masterKey = "";
        } catch {
          /* ignore */
        }
      }
    }

    case "REQUEST_UNLOCK": {
      setPendingAction({
        type: message.reason || "generic",
        tabId: sender?.tab?.id,
        frameId: sender?.frameId,
      });
      const opened = await tryOpenPopup();
      return { pending: true, opened };
    }

    case "REQUEST_UNLOCK_FILL": {
      const path = String(message.path || "").trim();
      if (!path) throw new Error("path required");
      setPendingAction({
        type: "fill",
        path,
        tabId: sender?.tab?.id ?? message.tabId,
        frameId: sender?.frameId ?? message.frameId,
      });
      const opened = await tryOpenPopup();
      return { pending: true, opened };
    }

    case "TAKE_PENDING_ACTION": {
      if (!isExtensionUiSender(sender)) {
        const err = new Error("Forbidden");
        err.status = 403;
        throw err;
      }
      return { pending: takePendingAction() };
    }

    case "SCHEDULE_CLIPBOARD_CLEAR": {
      if (!isExtensionUiSender(sender)) {
        const err = new Error("Forbidden");
        err.status = 403;
        throw err;
      }
      const delaySec = Math.min(120, Math.max(15, Number(message.delaySec) || 45));
      try {
        await chrome.alarms.clear(CLIPBOARD_ALARM);
      } catch {
        /* ignore */
      }
      // chrome.alarms minimum is ~1 minute in Chrome; also arm a SW timer as best-effort.
      const delayMin = Math.max(1, delaySec / 60);
      await chrome.alarms.create(CLIPBOARD_ALARM, { delayInMinutes: delayMin });
      const clearAt = Date.now() + delaySec * 1000;
      setTimeout(() => {
        if (Date.now() >= clearAt - 500) void clearExtensionClipboard();
      }, delaySec * 1000);
      return { scheduled: true, delaySec };
    }

    case "MATCH": {
      await ensureSession();
      const host = message.host || (await activeHost());
      return matchForHost(host);
    }

    case "AUTOFILL": {
      await ensureSession();
      requireUnlocked();
      await touchSession();
      const path = message.path;
      if (!path) throw new Error("path required");
      const content = await API.decrypt(path, session.token, true);
      const cred = parseCredential(content);
      const tabId = message.tabId || (await activeTabId());
      const frameId = message.frameId;
      await fillTab(tabId, cred.username, cred.password, frameId);
      return { filled: true };
    }

    case "FILL_PRIMARY": {
      await ensureSession();
      if (!memoryUnlocked()) {
        setPendingAction({ type: "fill_primary", tabId: await activeTabId() });
        await tryOpenPopup();
        return { locked: true };
      }
      const host = await activeHost();
      const data = await matchForHost(host);
      const matches = data.matches || [];
      if (!matches.length) throw new Error("No matching credentials for this site");
      const first = matches[0];
      await touchSession();
      const content = await API.decrypt(first.path, session.token, true);
      const cred = parseCredential(content);
      await fillTab(await activeTabId(), cred.username, cred.password);
      return { filled: true, path: first.path };
    }

    case "GET_SECRET": {
      await ensureSession();
      requireUnlocked();
      await touchSession();
      const path = String(message.path || "").trim();
      if (!path) throw new Error("path required");
      const content = await API.decrypt(path, session.token, true);
      const cred = parseCredential(content);
      return { path, ...cred };
    }

    case "REVEAL_CREDENTIAL": {
      await ensureSession();
      requireUnlocked();
      await touchSession();
      const path = message.path;
      if (!path) throw new Error("path required");
      const content = await API.decrypt(path, session.token, true);
      const cred = parseCredential(content);
      return {
        username: cred.username,
        password: cred.password,
        url: cred.url,
        notes: cred.notes,
      };
    }

    case "GET_CONFIG":
      return getConfig();

    case "GET_SETTINGS": {
      const data = await fetchSettingsCached();
      return { settings: data };
    }

    case "UPDATE_SETTINGS": {
      await ensureSession();
      if (!memoryUnlocked()) {
        if (message.lang) {
          await chrome.storage.local.set({ uiLanguage: message.lang });
        }
        return { cached: true, unlocked: false };
      }
      await touchSession();
      const patch = {};
      if (message.lang !== undefined) patch.lang = message.lang;
      const data = await API.updateSettings(patch, session.token);
      if (data?.lang) {
        await chrome.storage.local.set({ uiLanguage: data.lang });
      }
      settingsCache = { at: Date.now(), data };
      return { settings: data, unlocked: true };
    }

    case "CAPTURE_OFFER": {
      const host = String(message.host || "").trim();
      const username = String(message.username || "").trim();
      if (!host) throw new Error("host required");
      if (await isCaptureNeverHost(host)) {
        return { kind: "ignored", reason: "never" };
      }
      if (!username) {
        return { kind: "need_username", unlocked: await ensureSession() };
      }
      const { serverUrl } = await getConfig();
      await ensureHostPermission(serverUrl).catch(() => false);
      await ensureSession();
      const data = await matchForHost(host);
      const matches = data.matches || [];
      const want = normalizeUsername(username);
      const hit = matches.find(
        (m) => m.username && normalizeUsername(m.username) === want
      );
      if (hit) {
        return {
          kind: "update",
          path: hit.path,
          title: hit.title || hit.path,
          username: hit.username || username,
          unlocked: memoryUnlocked(),
        };
      }
      return {
        kind: "save",
        username,
        unlocked: memoryUnlocked(),
        hostMatchCount: matches.length,
      };
    }

    case "CAPTURE_NEVER": {
      const host = String(message.host || "").trim();
      if (!host) throw new Error("host required");
      await addCaptureNeverHost(host);
      return { never: true, host };
    }

    case "STASH_CAPTURE": {
      const tabId = _sender?.tab?.id;
      if (!tabId) return { stashed: false };
      const password = String(message.password || "");
      if (password.length < 2) return { stashed: false };
      const payload = {
        username: String(message.username || "").trim(),
        password,
        url: String(message.url || "").trim(),
        host: String(message.host || "").trim(),
        at: Date.now(),
      };
      const store = getSessionStore();
      if (store) {
        await store.set({ [`capture:${tabId}`]: payload });
      } else {
        pendingCaptures.set(tabId, payload);
      }
      return { stashed: true };
    }

    case "TAKE_CAPTURE_STASH": {
      const tabId = _sender?.tab?.id;
      if (!tabId) return { capture: null };
      const key = `capture:${tabId}`;
      const store = getSessionStore();
      let capture = null;
      if (store) {
        const data = await store.get(key);
        capture = data?.[key] || null;
        await store.remove(key);
      } else {
        capture = pendingCaptures.get(tabId) || null;
        pendingCaptures.delete(tabId);
      }
      if (capture && Date.now() - (capture.at || 0) > 60_000) {
        return { capture: null };
      }
      return { capture };
    }

    case "GET_TREE": {
      await ensureSession();
      requireUnlocked();
      await touchSession();
      const tree = await API.tree(session.token);
      return { tree: Array.isArray(tree) ? tree : [] };
    }

    case "SAVE_SECRET": {
      await ensureSession();
      requireUnlocked();
      await touchSession();
      const path = String(message.path || "").trim().replace(/^\/+|\/+$/g, "");
      if (!path || path.endsWith("/")) throw new Error("Invalid secret path");
      const password = String(message.password || "");
      if (!password || password.length < 2) throw new Error("Password too short");
      const username = String(message.username || "").trim();
      const url = String(message.url || "").trim();
      const notes = String(message.notes || "");
      const content = formatPassContent({ password, username, url, notes });
      await API.save(path, content, session.token, null);
      invalidateMatchCache();
      try {
        await refreshBadgeForActiveTab();
      } catch {
        /* ignore */
      }
      return { saved: true, path };
    }

    case "UPDATE_SECRET": {
      await ensureSession();
      requireUnlocked();
      await touchSession();
      const path = String(message.path || "").trim();
      if (!path) throw new Error("path required");
      const password = String(message.password ?? "");
      const username = message.username !== undefined ? String(message.username).trim() : undefined;
      const url = message.url !== undefined ? String(message.url).trim() : undefined;
      const notes = message.notes !== undefined ? String(message.notes) : undefined;
      const fullReplace = Boolean(message.full);

      let existing;
      try {
        const raw = await API.decrypt(path, session.token, true);
        existing = parseCredential(raw);
      } catch {
        existing = { password: "", username: "", url: "", notes: "" };
      }

      const nextPassword = password || existing.password;
      if (!nextPassword || nextPassword.length < 2) throw new Error("Password too short");

      const next = {
        password: nextPassword,
        username: username !== undefined ? username : existing.username,
        url: url !== undefined ? url : existing.url,
        notes: notes !== undefined ? notes : existing.notes,
      };

      if (
        !fullReplace &&
        next.password === existing.password &&
        next.username === existing.username &&
        next.url === existing.url &&
        next.notes === existing.notes
      ) {
        return { saved: false, unchanged: true, path };
      }

      // Capture-style update: only password change was historically short-circuited
      if (
        !fullReplace &&
        username === undefined &&
        url === undefined &&
        notes === undefined &&
        existing.password === nextPassword
      ) {
        return { saved: false, unchanged: true, path };
      }

      const content = formatPassContent(next);
      await API.save(path, content, session.token, path);
      invalidateMatchCache();
      try {
        await refreshBadgeForActiveTab();
      } catch {
        /* ignore */
      }
      return { saved: true, path };
    }

    case "TOUCH_SESSION": {
      await ensureSession();
      await touchSession();
      return { unlocked: memoryUnlocked() };
    }

    default:
      throw new Error(`Unknown message: ${message.type}`);
  }
}

async function fillTab(tabId, username, password, preferFrameId) {
  const payload = { type: "FILL_CREDENTIALS", username, password };

  async function tryFrame(frameId) {
    try {
      await chrome.tabs.sendMessage(tabId, payload, frameId != null ? { frameId } : undefined);
      return true;
    } catch {
      return false;
    }
  }

  if (preferFrameId != null && (await tryFrame(preferFrameId))) return;

  if (await tryFrame(undefined)) return;

  // Inject into all frames, then broadcast fill.
  try {
    const injected = await chrome.scripting.executeScript({
      target: { tabId, allFrames: true },
      files: [
        "shared/locales-embed.js",
        "shared/i18n-classic.js",
        "content/content.js",
        "content/capture.js",
      ],
    });
    let any = false;
    for (const result of injected || []) {
      if (result.frameId == null) continue;
      if (await tryFrame(result.frameId)) any = true;
    }
    if (any) return;
  } catch {
    /* ignore */
  }

  if (!(await tryFrame(undefined))) {
    throw new Error("Could not reach page content script");
  }
}

async function openFillMenuOnTab(tabId, match) {
  try {
    await chrome.tabs.sendMessage(tabId, {
      type: "OPEN_FILL_MENU",
      match,
    });
  } catch {
    try {
      await chrome.scripting.executeScript({
        target: { tabId, allFrames: true },
        files: [
          "shared/locales-embed.js",
          "shared/i18n-classic.js",
          "content/content.js",
          "content/capture.js",
        ],
      });
      await chrome.tabs.sendMessage(tabId, { type: "OPEN_FILL_MENU", match });
    } catch {
      /* ignore */
    }
  }
}

async function activeTabId() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab?.id) throw new Error("No active tab");
  return tab.id;
}

async function activeHost() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (!tab?.url) throw new Error("No active tab URL");
  return hostFromUrl(tab.url);
}

function hostFromUrl(url) {
  try {
    const u = new URL(url);
    if (u.protocol !== "http:" && u.protocol !== "https:") return null;
    return u.host;
  } catch {
    return null;
  }
}

async function setBadge(tabId, count) {
  const text = count > 0 ? String(count) : "";
  const color = count > 0 ? "#22c55e" : "#3f3f46";
  try {
    await chrome.action.setBadgeText({ tabId, text });
    await chrome.action.setBadgeBackgroundColor({ tabId, color });
    await chrome.action.setTitle({
      tabId,
      title: count > 0 ? `talos-vault // ${count} match(es)` : "talos-vault",
    });
  } catch {
    /* tab may be gone */
  }
}

async function refreshBadgeForTab(tabId, url) {
  const host = hostFromUrl(url || "");
  if (!host) {
    await setBadge(tabId, 0);
    return;
  }
  try {
    await ensureSession();
    // Locked: never probe match (no metadata leak via badge count).
    if (!memoryUnlocked()) {
      await setBadge(tabId, 0);
      return;
    }
    const { serverUrl } = await getConfig();
    if (isVaultServerHost(host, serverUrl)) {
      await setBadge(tabId, 0);
      return;
    }
    const granted = await chrome.permissions.contains({
      origins: [new URL(serverUrl).origin + "/*"],
    });
    if (!granted) {
      await setBadge(tabId, 0);
      return;
    }
    const data = await matchForHost(host);
    await setBadge(tabId, (data.matches || []).length);
  } catch {
    await setBadge(tabId, 0);
  }
}

function scheduleBadgeRefresh(tabId, url) {
  const prev = badgeDebounce.get(tabId);
  if (prev) clearTimeout(prev);
  badgeDebounce.set(
    tabId,
    setTimeout(() => {
      badgeDebounce.delete(tabId);
      refreshBadgeForTab(tabId, url);
    }, 450)
  );
}

async function refreshBadgeForActiveTab() {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  if (tab?.id) await refreshBadgeForTab(tab.id, tab.url);
}

function menuTitle(key, fallback) {
  try {
    const msg = chrome.i18n?.getMessage?.(key);
    if (msg) return msg;
  } catch {
    /* ignore */
  }
  return fallback;
}

async function ensureContextMenus() {
  try {
    await chrome.contextMenus.removeAll();
  } catch {
    /* ignore */
  }
  try {
    chrome.contextMenus.create({
      id: MENU_FILL,
      title: menuTitle("contextFill", "Fill with talos-vault…"),
      contexts: ["editable"],
    });
    chrome.contextMenus.create({
      id: MENU_OPEN,
      title: menuTitle("contextOpen", "Open talos-vault"),
      contexts: ["page", "editable", "frame"],
    });
  } catch {
    /* contextMenus may be unavailable */
  }
}

chrome.runtime.onInstalled.addListener(() => {
  ensureContextMenus();
});

chrome.runtime.onStartup.addListener(() => {
  ensureContextMenus();
});

ensureContextMenus();

chrome.contextMenus.onClicked.addListener(async (info, tab) => {
  if (!tab?.id) return;
  if (info.menuItemId === MENU_OPEN) {
    try {
      await chrome.action.openPopup();
    } catch {
      /* openPopup not always available — fall through to fill menu */
      try {
        await chrome.tabs.sendMessage(
          tab.id,
          { type: "OPEN_FILL_MENU" },
          info.frameId != null ? { frameId: info.frameId } : undefined
        );
      } catch {
        /* ignore */
      }
    }
    return;
  }
  if (info.menuItemId === MENU_FILL) {
    try {
      await chrome.tabs.sendMessage(
        tab.id,
        { type: "OPEN_FILL_MENU" },
        info.frameId != null ? { frameId: info.frameId } : undefined
      );
    } catch {
      try {
        await chrome.scripting.executeScript({
          target: { tabId: tab.id, frameIds: info.frameId != null ? [info.frameId] : undefined, allFrames: info.frameId == null },
          files: [
            "shared/locales-embed.js",
            "shared/i18n-classic.js",
            "content/content.js",
            "content/capture.js",
          ],
        });
        await chrome.tabs.sendMessage(
          tab.id,
          { type: "OPEN_FILL_MENU" },
          info.frameId != null ? { frameId: info.frameId } : undefined
        );
      } catch {
        /* ignore */
      }
    }
  }
});

chrome.commands.onCommand.addListener(async (command) => {
  if (command !== "fill-primary-match") return;
  try {
    await handleMessage({ type: "FILL_PRIMARY" }, null);
  } catch (err) {
    console.warn("[talos] fill-primary-match failed", err);
  }
});

chrome.alarms.onAlarm.addListener(async (alarm) => {
  if (alarm.name === AUTOLOCK_ALARM) {
    await lock(true);
    invalidateMatchCache();
    return;
  }
  if (alarm.name === CLIPBOARD_ALARM) {
    await clearExtensionClipboard();
  }
});

async function clearExtensionClipboard() {
  try {
    // Best-effort: requires clipboardWrite. Some Chromium builds restrict SW clipboard.
    if (typeof navigator !== "undefined" && navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText("");
    }
  } catch {
    /* ignore */
  }
}

chrome.tabs.onActivated.addListener(async ({ tabId }) => {
  try {
    const tab = await chrome.tabs.get(tabId);
    scheduleBadgeRefresh(tabId, tab.url);
  } catch {
    /* ignore */
  }
});

chrome.tabs.onUpdated.addListener((tabId, changeInfo, tab) => {
  if (changeInfo.status === "complete" || changeInfo.url) {
    scheduleBadgeRefresh(tabId, tab.url || changeInfo.url);
  }
});

chrome.storage.onChanged.addListener((changes, area) => {
  if (area !== "local") return;
  if (changes.persistSessionToken && !changes.persistSessionToken.newValue) {
    clearSessionToken();
  }
  if (changes.autoLockMinutes) {
    scheduleAutoLock();
  }
  if (changes.serverUrl) {
    invalidateMatchCache();
  }
});

// Warm shared language once (not per content-frame).
fetchSettingsCached().catch(() => {});