/**
 * Inline autofill UI (native password-manager style).
 * Suggests Talos matches only while unlocked. Master key is never typed here —
 * unlock happens only in the extension popup.
 */

(() => {
if (globalThis.__talosContentVersion === 8) return;
globalThis.__talosContentVersion = 8;

const HOST_ATTR = "data-talos-field";
const ROOT_ID = "talos-inline-root";
const BTN_SIZE = 22;

function t(key, vars) {
  if (globalThis.TalosI18n?.t) return globalThis.TalosI18n.t(key, vars);
  if (typeof globalThis.talosT === "function") return globalThis.talosT(key, vars);
  return key;
}

async function i18nReady() {
  try {
    await globalThis.TalosI18n?.ready;
  } catch {
    /* ignore */
  }
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

function deepQueryAll(root, selector) {
  const out = [];
  const visit = (node) => {
    if (!node?.querySelectorAll) return;
    try {
      out.push(...node.querySelectorAll(selector));
    } catch {
      /* invalid selector scope */
    }
    let elements;
    try {
      elements = node.querySelectorAll("*");
    } catch {
      return;
    }
    for (const el of elements) {
      if (el.shadowRoot) visit(el.shadowRoot);
    }
  };
  visit(root);
  return out;
}

/** Prefer shallow query; pierce open shadow only if nothing found (expensive). */
function queryInputs(root, selector) {
  let list = [];
  try {
    list = Array.from(root.querySelectorAll(selector));
  } catch {
    list = [];
  }
  if (list.length) return list;
  return deepQueryAll(root, selector);
}

function findPasswordInputs(root = document) {
  return queryInputs(root, 'input[type="password"]').filter(isVisible);
}

function isVisible(el) {
  if (!el || el.disabled) return false;
  const r = el.getBoundingClientRect();
  if (r.width < 8 || r.height < 8) return false;
  const st = window.getComputedStyle(el);
  return st.visibility !== "hidden" && st.display !== "none" && st.opacity !== "0";
}

function fieldHints(el) {
  return `${el.name || ""} ${el.id || ""} ${el.placeholder || ""} ${el.getAttribute("aria-label") || ""} ${el.getAttribute("autocomplete") || ""}`.toLowerCase();
}

function isExcludedIdentityField(hints) {
  return /search|query|filter|otp|totp|one.?time|verif|captcha|csrf|token|password|comment|message|subject|title|address|street|city|zip|postal|country|first.?name|last.?name|fullname.?name|apellido|nombre.?completo|fullname|lastname|firstname|birthday|birth|age|card|cvv|cvc|cc-|qty|quantity|amount|promo|coupon|newsletter|subscribe|chat|tweet|post|note|description|company(?!.*login)|org(?!.*login)/.test(
    hints
  );
}

function hasStrongLoginHints(hints) {
  return (
    /\b(user(name)?|user[_-]?id|login|signin|sign[_-]?in|log[_-]?in|email|e[_-]?mail|correo|usuario|identifier|identity|account|phone|mobile|tel)\b/.test(
      hints
    ) ||
    /(username|user_name|userid|loginid|loginemail|emailaddress)/.test(hints)
  );
}

function isLoginPageContext() {
  const path = `${location.pathname} ${location.hash} ${location.search}`.toLowerCase();
  if (/login|log-in|signin|sign-in|sign_in|auth|sso|oauth|passwd|password|account\/(login|signin)|session|register|signup|sign-up|sign_up|create.?account/.test(path)) {
    return true;
  }
  const title = (document.title || "").toLowerCase();
  if (/\b(log\s*in|sign\s*in|sign\s*up|register|iniciar sesi[oó]n|acceso|cuenta)\b/.test(title)) {
    return true;
  }
  return false;
}

/**
 * Username / email / phone fields for credential fill only.
 * Conservative: avoid generic text/email inputs on normal pages.
 */
function isUsernameLike(el, pageHasPassword) {
  if (!el || !isVisible(el)) return false;
  const type = (el.type || "text").toLowerCase();
  if (["password", "hidden", "submit", "button", "checkbox", "radio", "file", "range", "color", "search", "number", "date", "datetime-local", "month", "week", "time"].includes(type)) {
    return false;
  }

  const ac = (el.autocomplete || "").toLowerCase().split(/\s+/).pop() || "";
  if (["new-password", "current-password", "one-time-code", "cc-number", "cc-csc", "cc-exp", "name", "given-name", "family-name", "street-address", "postal-code", "organization"].includes(ac)) {
    return false;
  }
  // Explicit browser identity autocomplete — always treat as login-related.
  if (["username", "email", "tel", "nickname"].includes(ac)) return true;

  const hints = fieldHints(el);
  if (isExcludedIdentityField(hints)) return false;

  const hasPwd =
    typeof pageHasPassword === "boolean" ? pageHasPassword : findPasswordInputs().length > 0;
  const strong = hasStrongLoginHints(hints);
  const loginCtx = hasPwd || isLoginPageContext();

  // email/tel only when clearly identity / login context
  if (type === "email" || type === "tel") {
    return strong || loginCtx;
  }

  if (type !== "text" && type !== "url") return false;

  // Plain text: require strong login naming, and either a password nearby or a login page.
  if (!strong) return false;
  if (hasPwd) {
    // Prefer fields in the same form as a password; otherwise still OK on login pages.
    if (el.form?.querySelector('input[type="password"]')) return true;
    return loginCtx;
  }
  return loginCtx;
}

function findUsernameInputs(root = document, pageHasPassword) {
  return queryInputs(root, "input").filter((el) => isUsernameLike(el, pageHasPassword));
}

/** All fields that should show the Talos affordance. */
function findFillableInputs(root = document) {
  const seen = new Set();
  const out = [];
  const passwords = findPasswordInputs(root);
  const hasPwd = passwords.length > 0;
  for (const el of [...passwords, ...findUsernameInputs(root, hasPwd)]) {
    if (seen.has(el)) continue;
    seen.add(el);
    out.push(el);
  }
  return out;
}

function findUsernameInputNear(anchor) {
  const form = anchor?.form;
  const scope = form || document;
  const candidates = findUsernameInputs(scope);
  if (!candidates.length) return null;
  if (candidates.includes(anchor)) return anchor;

  const all = queryInputs(scope, "input").filter((el) => el.type !== "hidden");
  const idx = all.indexOf(anchor);
  if (idx >= 0) {
    for (let i = idx - 1; i >= 0; i--) {
      if (candidates.includes(all[i])) return all[i];
    }
    for (let i = idx + 1; i < all.length; i++) {
      if (candidates.includes(all[i])) return all[i];
    }
  }
  return candidates[0];
}

function findPasswordInputNear(anchor) {
  const form = anchor?.form;
  const scope = form || document;
  const passwords = findPasswordInputs(scope);
  if (!passwords.length) return null;
  if (passwords.includes(anchor)) return anchor;

  const all = queryInputs(scope, "input").filter((el) => el.type !== "hidden");
  const idx = all.indexOf(anchor);
  if (idx >= 0) {
    for (let i = idx + 1; i < all.length; i++) {
      if (passwords.includes(all[i])) return all[i];
    }
    for (let i = idx - 1; i >= 0; i--) {
      if (passwords.includes(all[i])) return all[i];
    }
  }
  return passwords[0];
}

function setNativeValue(input, value) {
  if (!input) return;
  const proto = Object.getPrototypeOf(input);
  const desc = Object.getOwnPropertyDescriptor(proto, "value");
  if (desc?.set) desc.set.call(input, value);
  else input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  input.dispatchEvent(new Event("change", { bubbles: true }));
}

/**
 * Fill whatever login fields exist. Works on password-only, username-only (step 1),
 * or both. `preferField` is the field that opened the menu.
 */
function fillCredentials(username, password, preferField = null) {
  const anchor = preferField && document.contains(preferField) ? preferField : null;
  let user = null;
  let pwd = null;

  if (anchor?.type === "password") {
    pwd = anchor;
    user = findUsernameInputNear(anchor);
  } else if (anchor) {
    user = isUsernameLike(anchor) ? anchor : findUsernameInputNear(anchor);
    pwd = findPasswordInputNear(anchor);
  } else {
    pwd = findPasswordInputs()[0] || null;
    user = findUsernameInputNear(pwd) || findUsernameInputs()[0] || null;
  }

  let filled = false;
  if (user && username) {
    setNativeValue(user, username);
    filled = true;
  }
  if (pwd && password) {
    setNativeValue(pwd, password);
    filled = true;
  }
  // Username-step with no password field: still OK if we wrote the user.
  if (!filled && user && !pwd && username) {
    setNativeValue(user, username);
    filled = true;
  }
  if (!user && !pwd) {
    return { ok: false, error: "No login field found on this page" };
  }
  return { ok: filled, filledUser: Boolean(user && username), filledPassword: Boolean(pwd && password) };
}

/** @type {HTMLElement | null} */
let uiHost = null;
/** @type {ShadowRoot | null} */
let shadow = null;
/** @type {HTMLElement | null} */
let menuEl = null;
/** @type {HTMLInputElement | null} */
let activeField = null;
/** @type {Map<HTMLInputElement, { btn: HTMLButtonElement, prevPad: string }>} */
const tracked = new Map();
let menuOpenFor = null;
let suppressOutsideCloseUntil = 0;
let posRaf = 0;

function ensureUi() {
  if (uiHost && shadow && menuEl) return shadow;

  uiHost = document.getElementById(ROOT_ID);
  if (!uiHost) {
    uiHost = document.createElement("div");
    uiHost.id = ROOT_ID;
    Object.assign(uiHost.style, {
      all: "initial",
      position: "fixed",
      inset: "0",
      width: "100%",
      height: "0",
      overflow: "visible",
      zIndex: "2147483646",
      pointerEvents: "none",
    });
    document.documentElement.appendChild(uiHost);
  }

  // Open mode so composedPath / contains work reliably across Firefox & Chrome.
  shadow = uiHost.shadowRoot || uiHost.attachShadow({ mode: "open" });
  if (!shadow.querySelector("style")) {
    const style = document.createElement("style");
    style.textContent = `
      :host { all: initial; }
      .btn {
        position: fixed;
        width: ${BTN_SIZE}px;
        height: ${BTN_SIZE}px;
        padding: 0;
        border: 0;
        border-radius: 2px;
        cursor: pointer;
        pointer-events: auto;
        background: #09090b center / 14px 14px no-repeat;
        background-image: var(--talos-icon);
        box-shadow: 0 0 0 1px #22c55e, 0 0 10px rgba(34,197,94,.35);
        z-index: 2147483645;
        visibility: hidden;
        opacity: 0;
        top: 0;
        left: 0;
      }
      .btn.ready {
        visibility: visible;
        opacity: 1;
        transition: opacity 0.12s ease-out;
      }
      .btn:hover { filter: brightness(1.15); }
      .btn.has-match { box-shadow: 0 0 0 1px #4ade80, 0 0 14px rgba(34,197,94,.55); }
      .menu {
        position: fixed;
        min-width: 260px;
        max-width: min(340px, calc(100vw - 16px));
        background: #09090b;
        color: #4ade80;
        border: 1px solid #27272a;
        border-radius: 2px;
        box-shadow: 0 0 24px rgba(34,197,94,.18), 0 12px 32px rgba(0,0,0,.55);
        font: 12px/1.45 "Fira Code", ui-monospace, monospace;
        text-shadow: 0 0 2px rgba(34,197,94,.35);
        overflow: hidden;
        z-index: 2147483647;
        pointer-events: auto;
      }
      .menu[hidden] { display: none !important; }
      .head {
        display: flex;
        align-items: center;
        justify-content: space-between;
        padding: 10px 12px;
        border-bottom: 1px solid #27272a;
        letter-spacing: .18em;
        text-transform: uppercase;
        font-size: 10px;
        color: #22c55e;
        background: #020202;
      }
      .list { list-style: none; margin: 0; padding: 6px; display: flex; flex-direction: column; gap: 4px; }
      .list button {
        width: 100%;
        text-align: left;
        background: #18181b;
        border: 1px solid #27272a;
        border-radius: 2px;
        color: #a1a1aa;
        padding: 8px 10px;
        cursor: pointer;
        font: inherit;
        text-shadow: none;
      }
      .list button:hover {
        border-color: #22c55e;
        color: #4ade80;
        background: rgba(34,197,94,.12);
        text-shadow: 0 0 2px rgba(34,197,94,.35);
      }
      .list button.sel {
        border-color: #22c55e;
        color: #4ade80;
        background: rgba(34,197,94,.18);
        text-shadow: 0 0 2px rgba(34,197,94,.35);
        box-shadow: inset 0 0 0 1px rgba(34,197,94,.35);
      }
      .title { display: block; font-weight: 500; letter-spacing: .04em; }
      .meta {
        display: block;
        color: #71717a;
        font-size: 10px;
        margin-top: 2px;
        text-transform: uppercase;
        letter-spacing: .06em;
      }
      .empty, .err, .hint {
        margin: 0;
        padding: 10px 12px;
        color: #71717a;
        text-transform: uppercase;
        letter-spacing: .06em;
        font-size: 10px;
      }
      .err { color: #ef4444; text-shadow: 0 0 6px rgba(239,68,68,.4); }
      .cta {
        margin: 8px 12px 12px;
        background: rgba(20,83,45,.35);
        color: #22c55e;
        border: 1px solid #16a34a;
        border-radius: 2px;
        padding: 8px 10px;
        font: 700 10px/1.2 "Fira Code", ui-monospace, monospace;
        letter-spacing: .16em;
        text-transform: uppercase;
        cursor: pointer;
        width: calc(100% - 24px);
      }
      .cta:hover {
        background: #22c55e;
        color: #000;
        box-shadow: 0 0 16px rgba(34,197,94,.35);
      }
    `;
    shadow.appendChild(style);
  }

  menuEl = shadow.querySelector(".menu");
  if (!menuEl) {
    menuEl = document.createElement("div");
    menuEl.className = "menu";
    menuEl.hidden = true;
    shadow.appendChild(menuEl);
  }
  return shadow;
}

function iconUrl() {
  return chrome.runtime.getURL("icons/icon32.png");
}

function schedulePositions() {
  if (posRaf) return;
  posRaf = requestAnimationFrame(() => {
    posRaf = 0;
    for (const [input, entry] of tracked) {
      positionButton(input, entry.btn, { reveal: true });
    }
    if (menuOpenFor && menuEl && !menuEl.hidden) {
      positionMenu(menuOpenFor);
    }
  });
}

function positionButton(input, btn, { reveal = false } = {}) {
  if (!document.contains(input) || !isVisible(input)) {
    btn.style.visibility = "hidden";
    btn.classList.remove("ready");
    return;
  }
  const r = input.getBoundingClientRect();
  // Layout not ready yet (0-size) — keep hidden and retry.
  if (r.width < 8 || r.height < 8) {
    btn.classList.remove("ready");
    return;
  }
  const top = r.top + (r.height - BTN_SIZE) / 2;
  const left = r.right - BTN_SIZE - 6;
  btn.style.top = `${Math.round(top)}px`;
  btn.style.left = `${Math.round(left)}px`;
  if (reveal || btn.classList.contains("ready")) {
    btn.style.visibility = "visible";
    btn.classList.add("ready");
  }
}

function positionMenu(anchorInput) {
  if (!menuEl || !anchorInput) return;
  const r = anchorInput.getBoundingClientRect();
  const width = Math.min(340, Math.max(260, menuEl.offsetWidth || 260));
  let top = r.bottom + 6;
  let left = r.left;
  if (top + 220 > window.innerHeight) {
    top = Math.max(8, r.top - 8 - (menuEl.offsetHeight || 180));
  }
  if (left + width > window.innerWidth - 8) {
    left = Math.max(8, window.innerWidth - width - 8);
  }
  menuEl.style.top = `${Math.round(top)}px`;
  menuEl.style.left = `${Math.round(left)}px`;
}

function attachButton(fieldInput) {
  if (tracked.has(fieldInput)) return;
  fieldInput.setAttribute(HOST_ATTR, "1");
  ensureUi();

  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "btn";
  btn.title = "talos-vault";
  btn.setAttribute("aria-label", "talos-vault autofill");
  btn.style.setProperty("--talos-icon", `url("${iconUrl()}")`);
  // Stay invisible until coordinates are correct (avoids 0,0 flash).
  btn.style.visibility = "hidden";
  shadow.appendChild(btn);

  const prevPad = fieldInput.style.paddingRight;
  const computedPad = parseFloat(window.getComputedStyle(fieldInput).paddingRight) || 0;
  if (computedPad < BTN_SIZE + 10) {
    fieldInput.style.paddingRight = `${BTN_SIZE + 12}px`;
  }
  // Force layout after padding so the first measure is accurate.
  void fieldInput.offsetWidth;

  let ro = null;
  if (typeof ResizeObserver !== "undefined") {
    ro = new ResizeObserver(() => schedulePositions());
    try {
      ro.observe(fieldInput);
    } catch {
      ro = null;
    }
  }

  tracked.set(fieldInput, { btn, prevPad, ro });

  fieldInput.addEventListener("focus", () => {
    activeField = fieldInput;
    schedulePositions();
    openMenu(fieldInput);
  });

  btn.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    e.stopPropagation();
    activeField = fieldInput;
    if (!menuEl?.hidden && menuOpenFor === fieldInput) {
      closeMenu();
      return;
    }
    openMenu(fieldInput);
  });
  btn.addEventListener("click", (e) => {
    e.preventDefault();
    e.stopPropagation();
  });

  // Position now, then again after paint so fonts/CSS settle — still hidden until ready.
  positionButton(fieldInput, btn, { reveal: true });
  requestAnimationFrame(() => {
    positionButton(fieldInput, btn, { reveal: true });
    requestAnimationFrame(() => positionButton(fieldInput, btn, { reveal: true }));
  });
}

function closeMenu() {
  if (menuEl) menuEl.hidden = true;
  menuOpenFor = null;
}

function isEventFromTalosUi(e) {
  const path = typeof e.composedPath === "function" ? e.composedPath() : [];
  for (const n of path) {
    if (!n || n === window || n === document) continue;
    if (n === uiHost || n === menuEl || n === shadow) return true;
    if (n.id === ROOT_ID) return true;
    if (n.classList?.contains?.("btn") || n.classList?.contains?.("menu") || n.classList?.contains?.("door")) return true;
  }
  // Fallback when composedPath omits closed/open shadow internals.
  const t = e.target;
  if (t && shadow && (t === uiHost || shadow.contains(t))) return true;
  return false;
}

async function openMenu(fieldInput) {
  ensureUi();
  if (fieldInput && !tracked.has(fieldInput)) {
    const isPwd = (fieldInput.type || "").toLowerCase() === "password";
    if (isPwd || isUsernameLike(fieldInput)) attachButton(fieldInput);
  }
  const entry = tracked.get(fieldInput);
  if (!entry) return;
  const { btn } = entry;

  suppressOutsideCloseUntil = Date.now() + 1000;
  menuOpenFor = fieldInput;
  schedulePositions();
  menuEl.hidden = false;
  positionMenu(fieldInput);
  menuEl.innerHTML = `<div class="head"><span>talos-vault</span><span>…</span></div><p class="hint">${escapeHtml(t("ext_scanning"))}</p>`;

  try {
    const data = await send("MATCH", { host: location.host });
    if (menuOpenFor !== fieldInput) return;
    suppressOutsideCloseUntil = Date.now() + 600;
    const unlocked = Boolean(data.unlocked);
    const matches = data.matches || [];
    btn.classList.toggle("has-match", unlocked && matches.length > 0);

    if (!unlocked || data.needsUnlock) {
      menuEl.innerHTML = `
        <div class="head"><span>talos-vault</span><span>${escapeHtml(t("ext_sealed"))}</span></div>
        <p class="hint">${escapeHtml(t("ext_unlock_in_popup"))}</p>
        <button type="button" class="cta" data-role="open-ext">${escapeHtml(t("ext_open_popup"))}</button>
      `;
      positionMenu(fieldInput);
      const openBtn = menuEl.querySelector('[data-role="open-ext"]');
      openBtn?.addEventListener("pointerdown", (e) => e.stopPropagation());
      openBtn?.addEventListener("click", async (e) => {
        e.preventDefault();
        e.stopPropagation();
        try {
          await send("REQUEST_UNLOCK", { reason: "inline" });
          closeMenu();
        } catch (err) {
          menuEl.innerHTML = `<div class="head"><span>talos-vault</span></div><p class="err">${escapeHtml(err.message)}</p>`;
        }
      });
      return;
    }

    if (!matches.length) {
      menuEl.innerHTML = `<div class="head"><span>talos-vault</span></div><p class="empty">${escapeHtml(t("ext_no_creds_site"))}</p>`;
      positionMenu(fieldInput);
      return;
    }

    const list = matches
      .map(
        (m, i) => `<li><button type="button" data-i="${i}">
          <span class="title">${escapeHtml(m.title || m.path)}</span>
          <span class="meta">${escapeHtml(m.username || m.path)}</span>
        </button></li>`
      )
      .join("");

    menuEl.innerHTML = `
      <div class="head"><span>talos-vault</span><span>${escapeHtml(t("ext_unsealed"))}</span></div>
      <ul class="list">${list}</ul>
      <p class="err" data-role="err" hidden></p>
    `;
    positionMenu(fieldInput);

    const errEl = menuEl.querySelector('[data-role="err"]');
    const listButtons = () => menuEl.querySelectorAll(".list button");

    listButtons().forEach((b) => {
      b.addEventListener("pointerdown", (e) => e.stopPropagation());
      b.addEventListener("click", async (e) => {
        e.preventDefault();
        e.stopPropagation();
        const m = matches[Number(b.getAttribute("data-i"))];
        if (!m) return;
        errEl.hidden = true;
        listButtons().forEach((el) => el.classList.toggle("sel", el === b));
        try {
          await fillFromPath(m.path, fieldInput);
          closeMenu();
        } catch (err) {
          if (err.status === 401) {
            try {
              await send("REQUEST_UNLOCK_FILL", { path: m.path });
              closeMenu();
            } catch (openErr) {
              errEl.hidden = false;
              errEl.textContent = openErr.message;
            }
            return;
          }
          errEl.hidden = false;
          errEl.textContent = err.message;
        }
      });
    });
  } catch (err) {
    menuEl.innerHTML = `<div class="head"><span>talos-vault</span></div><p class="err">${escapeHtml(err.message)}</p>`;
  }
}

async function fillFromPath(path, fieldInput) {
  const data = await send("REVEAL_CREDENTIAL", { path });
  fillCredentials(data.username || "", data.password || "", fieldInput);
}

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

let matchHint = { host: "", count: 0, at: 0 };
/** @type {Promise<number> | null} */
let matchHintInflight = null;

async function updateMatchHint() {
  if (await isOnVaultOrigin()) return 0;
  const host = location.host;
  const now = Date.now();
  if (matchHint.host === host && now - matchHint.at < 8000) return matchHint.count;
  if (matchHintInflight) return matchHintInflight;

  matchHintInflight = (async () => {
    try {
      const data = await send("MATCH", { host });
      const count = data.unlocked ? (data.matches || []).length : 0;
      matchHint = { host, count, at: Date.now() };
    } catch {
      matchHint = { host, count: 0, at: Date.now() };
    } finally {
      matchHintInflight = null;
    }
    return matchHint.count;
  })();
  return matchHintInflight;
}

/** @type {string | null} */
let cachedVaultOrigin = null;
let vaultOriginReady = null;

function loadVaultOrigin() {
  if (vaultOriginReady) return vaultOriginReady;
  vaultOriginReady = chrome.storage.local
    .get(["serverUrl"])
    .then((data) => {
      try {
        cachedVaultOrigin = new URL(data.serverUrl || "http://localhost:3000").origin;
      } catch {
        cachedVaultOrigin = "http://localhost:3000";
      }
      return cachedVaultOrigin;
    })
    .catch(() => {
      cachedVaultOrigin = "http://localhost:3000";
      return cachedVaultOrigin;
    });
  return vaultOriginReady;
}

async function isOnVaultOrigin() {
  const origin = cachedVaultOrigin || (await loadVaultOrigin());
  return Boolean(origin && location.origin === origin);
}

function scan() {
  if (document.hidden) return;
  if (cachedVaultOrigin && location.origin === cachedVaultOrigin) return;
  for (const field of findFillableInputs()) attachButton(field);
  // Drop detached inputs
  for (const [input, { btn, prevPad, ro }] of tracked) {
    if (!document.contains(input)) {
      try {
        ro?.disconnect();
      } catch {
        /* ignore */
      }
      btn.remove();
      tracked.delete(input);
      try {
        input.style.paddingRight = prevPad;
      } catch {
        /* ignore */
      }
    }
  }
  if (tracked.size) {
    updateMatchHint().then((count) => {
      for (const [, { btn }] of tracked) {
        btn.classList.toggle("has-match", count > 0);
      }
    });
  }
  schedulePositions();
}

let scanTimer = null;
function scheduleScan() {
  if (document.hidden) return;
  if (cachedVaultOrigin && location.origin === cachedVaultOrigin) return;
  clearTimeout(scanTimer);
  scanTimer = setTimeout(scan, 500);
}

function mutationLooksRelevant(mutations) {
  for (const m of mutations) {
    if (m.target?.id === ROOT_ID || m.target?.closest?.(`#${ROOT_ID}`)) continue;
    for (const node of m.addedNodes) {
      if (!(node instanceof Element)) continue;
      if (node.id === ROOT_ID) continue;
      if (node.matches?.("input, form") || node.querySelector?.("input")) return true;
    }
    for (const node of m.removedNodes) {
      if (!(node instanceof Element)) continue;
      if (node.matches?.("input, form") || node.querySelector?.("input")) return true;
    }
  }
  return false;
}

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message?.type === "FILL_CREDENTIALS") {
    try {
      const prefer =
        activeField && document.contains(activeField)
          ? activeField
          : document.activeElement instanceof HTMLInputElement
            ? document.activeElement
            : null;
      sendResponse(fillCredentials(message.username || "", message.password || "", prefer));
    } catch (err) {
      sendResponse({ ok: false, error: err.message || String(err) });
    }
    return true;
  }
  if (message?.type === "OPEN_FILL_MENU") {
    (async () => {
      await i18nReady();
      const active = document.activeElement;
      let field =
        active instanceof HTMLInputElement &&
        ((active.type || "").toLowerCase() === "password" || isUsernameLike(active))
          ? active
          : null;
      if (!field) field = findFillableInputs()[0] || null;
      if (!field) {
        sendResponse({ ok: false, error: "No login field found" });
        return;
      }
      activeField = field;
      await openMenu(field);
      if (message.match?.path && menuEl && !menuEl.hidden) {
        // Prefer highlighting — user still picks; unlock UI ready if locked
      }
      sendResponse({ ok: true });
    })().catch((err) => sendResponse({ ok: false, error: err.message || String(err) }));
    return true;
  }
  return false;
});

// Bubble phase: allow shadow targets to be identifiable; never close on our UI.
document.addEventListener(
  "pointerdown",
  (e) => {
    if (!menuEl || menuEl.hidden) return;
    if (Date.now() < suppressOutsideCloseUntil) return;
    if (isEventFromTalosUi(e)) {
      suppressOutsideCloseUntil = Date.now() + 1500;
      return;
    }
    closeMenu();
  },
  false
);

document.addEventListener("keydown", (e) => {
  if (e.key === "Escape") closeMenu();
});

window.addEventListener("scroll", schedulePositions, true);
window.addEventListener("resize", schedulePositions);
window.visualViewport?.addEventListener("resize", schedulePositions);
window.visualViewport?.addEventListener("scroll", schedulePositions);

scan();
i18nReady().then(() => scheduleScan());
if (document.fonts?.ready) {
  document.fonts.ready.then(() => schedulePositions()).catch(() => {});
}
window.addEventListener("load", () => schedulePositions(), { once: true });

const mo = new MutationObserver((mutations) => {
  if (!mutationLooksRelevant(mutations)) return;
  scheduleScan();
});
mo.observe(document.documentElement, { childList: true, subtree: true });
document.addEventListener("visibilitychange", () => {
  if (!document.hidden) scheduleScan();
});

function detachAllButtons() {
  for (const [input, { btn, prevPad, ro }] of tracked) {
    try {
      ro?.disconnect();
    } catch {
      /* ignore */
    }
    btn.remove();
    try {
      input.style.paddingRight = prevPad;
    } catch {
      /* ignore */
    }
  }
  tracked.clear();
}

loadVaultOrigin().then((origin) => {
  if (location.origin === origin) {
    mo.disconnect();
    detachAllButtons();
    if (uiHost) uiHost.remove();
  }
});

chrome.storage?.onChanged?.addListener((changes, area) => {
  if (area !== "local" || !changes.serverUrl) return;
  cachedVaultOrigin = null;
  vaultOriginReady = null;
  loadVaultOrigin().then((origin) => {
    if (location.origin === origin) {
      mo.disconnect();
      detachAllButtons();
      if (uiHost) uiHost.remove();
    } else {
      scheduleScan();
    }
  });
});

// Shared with capture.js (same isolated world).
globalThis.talosContentApi = {
  send,
  ensureUi,
  escapeHtml,
  closeMenu,
  findUsernameInputNear,
  findUsernameInputs,
  fillCredentials,
  openMenu,
  isOnVaultOrigin,
  get shadow() {
    return shadow;
  },
};
})();

// Back-compat globals for capture.js
(() => {
  const api = globalThis.talosContentApi;
  if (!api) return;
  globalThis.send = api.send;
  globalThis.ensureUi = api.ensureUi;
  globalThis.escapeHtml = api.escapeHtml;
  globalThis.closeMenu = api.closeMenu;
  globalThis.findUsernameInputNear = api.findUsernameInputNear;
  globalThis.findUsernameInputs = api.findUsernameInputs;
  Object.defineProperty(globalThis, "shadow", {
    configurable: true,
    get() {
      return api.shadow;
    },
  });
})();
