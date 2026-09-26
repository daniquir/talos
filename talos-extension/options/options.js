import {
  API,
  clearSessionToken,
  ensureHostPermission,
  getConfig,
  getSessionStore,
  setConfig,
} from "../shared/api.js";
import { applyDom, getPref, initI18n, setLangPref, t } from "../shared/i18n.js";

const serverUrlInput = document.getElementById("server-url");
const oidcIssuerInput = document.getElementById("oidc-issuer");
const oidcClientInput = document.getElementById("oidc-client");
const oidcRedirectInput = document.getElementById("oidc-redirect");
const persistSessionInput = document.getElementById("persist-session");
const autoLockSelect = document.getElementById("auto-lock");
const langSelect = document.getElementById("ui-language");
const sessionApiWarn = document.getElementById("session-api-warn");
const statusEl = document.getElementById("status");
const btnSave = document.getElementById("btn-save");
const btnTest = document.getElementById("btn-test");

function setStatus(msg, ok) {
  statusEl.hidden = false;
  statusEl.textContent = msg;
  statusEl.className = `status ${ok ? "ok" : "err"}`;
}

function send(type, payload = {}) {
  return new Promise((resolve, reject) => {
    chrome.runtime.sendMessage({ type, ...payload }, (res) => {
      if (chrome.runtime.lastError) {
        reject(new Error(chrome.runtime.lastError.message));
        return;
      }
      if (!res?.ok) {
        reject(Object.assign(new Error(res?.error || "Request failed"), { status: res?.status }));
        return;
      }
      resolve(res);
    });
  });
}

async function load() {
  await initI18n();
  applyDom();

  try {
    const res = await send("GET_SETTINGS");
    if (res.settings?.lang) {
      await setLangPref(res.settings.lang, { syncServer: false });
      applyDom();
    }
  } catch {
    /* server unreachable */
  }

  const cfg = await getConfig();
  serverUrlInput.value = cfg.serverUrl || "";
  oidcIssuerInput.value = cfg.oidcIssuer || "";
  oidcClientInput.value = cfg.oidcClientId || "talos-extension";
  try {
    oidcRedirectInput.value = chrome.identity.getRedirectURL();
  } catch {
    oidcRedirectInput.value = "";
  }
  langSelect.value = await getPref();
  autoLockSelect.value = String(cfg.autoLockMinutes || 0);

  const hasSessionApi = Boolean(getSessionStore());
  if (!hasSessionApi) {
    persistSessionInput.checked = false;
    persistSessionInput.disabled = true;
    sessionApiWarn.hidden = false;
  } else {
    persistSessionInput.checked = Boolean(cfg.persistSessionToken);
    sessionApiWarn.hidden = true;
  }
}

langSelect.addEventListener("change", async () => {
  await setLangPref(langSelect.value, { syncServer: true });
  applyDom();
});

btnSave.addEventListener("click", async () => {
  const serverUrl = serverUrlInput.value.trim();
  const oidcIssuer = oidcIssuerInput.value.trim();
  const oidcClientId = oidcClientInput.value.trim();
  const persistSessionToken = Boolean(persistSessionInput.checked) && Boolean(getSessionStore());
  const autoLockMinutes = Number(autoLockSelect.value) || 0;
  try {
    const granted = await ensureHostPermission(serverUrl);
    if (!granted) {
      setStatus(t("ext_perm_denied"), false);
      return;
    }
    if (oidcIssuer) {
      try {
        await ensureHostPermission(oidcIssuer);
      } catch {
        /* optional until OIDC used */
      }
    }
    await setConfig({
      serverUrl,
      oidcIssuer,
      oidcClientId,
      persistSessionToken,
      autoLockMinutes,
    });
    await setLangPref(langSelect.value, { syncServer: true });
    applyDom();
    if (!persistSessionToken) {
      await clearSessionToken();
    }
    try {
      await send("TOUCH_SESSION");
    } catch {
      /* locked */
    }
    setStatus(persistSessionToken ? t("ext_saved_persist") : t("ext_saved_memory"), true);
  } catch (err) {
    setStatus(err.message, false);
  }
});

btnTest.addEventListener("click", async () => {
  const serverUrl = serverUrlInput.value.trim();
  try {
    const granted = await ensureHostPermission(serverUrl);
    if (!granted) {
      setStatus(t("ext_perm_denied"), false);
      return;
    }
    await setConfig({
      serverUrl,
      oidcIssuer: oidcIssuerInput.value.trim(),
      oidcClientId: oidcClientInput.value.trim(),
      persistSessionToken: Boolean(persistSessionInput.checked) && Boolean(getSessionStore()),
    });
    const health = await API.health();
    const version = await API.version().catch(() => ({}));
    const status = await API.authStatus().catch(() => ({}));
    const bunker =
      health.bunker === true ||
      health.bunker === "INITIALIZED" ||
      health.bunker === "SEALED" ||
      health.bunker === "UNSEALED";
    let base = t("ext_connected", {
      storage: health.storage ? "ok" : "down",
      bunker: bunker ? "reachable" : "down",
    });
    if (status.oidc_enabled) {
      base += ` · OIDC ${status.custody_mode || "strict"}`;
    }
    setStatus(version.version ? `${base} · v${version.version}` : base, Boolean(health.storage));
  } catch (err) {
    setStatus(err.message, false);
  }
});

load();
