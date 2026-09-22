/**
 * Extension i18n (en / es).
 * Local cache: chrome.storage.local `uiLanguage` (auto|en|es).
 * Source of truth when online: GET/PUT /api/settings on the Talos server
 * (shared with the web UI).
 */

const SUPPORTED = ["en", "es"];

/** @type {Record<string, string>} */
let catalog = {};
let currentLang = "en";
let readyPromise = null;

function detectBrowserLang() {
  const nav = (typeof navigator !== "undefined" && navigator.language
    ? navigator.language
    : "en"
  ).toLowerCase();
  if (nav.startsWith("es")) return "es";
  return "en";
}

export function getLang() {
  return currentLang;
}

export function t(key, vars = {}) {
  let text = catalog[key] ?? key;
  for (const [k, v] of Object.entries(vars)) {
    text = text.replaceAll(`{${k}}`, String(v ?? ""));
  }
  return text;
}

export async function getPref() {
  try {
    const data = await chrome.storage.local.get(["uiLanguage"]);
    return data.uiLanguage || "auto";
  } catch {
    return "auto";
  }
}

export function resolveLang(pref) {
  if (pref && pref !== "auto" && SUPPORTED.includes(pref)) return pref;
  return detectBrowserLang();
}

export async function loadLocale(lang) {
  const code = SUPPORTED.includes(lang) ? lang : "en";
  // Prefer embedded catalogs when present (content scripts / offline).
  if (globalThis.TALOS_LOCALES?.[code]) {
    catalog = { ...globalThis.TALOS_LOCALES[code] };
    currentLang = code;
  } else {
    const url = chrome.runtime.getURL(`shared/locales/${code}.json`);
    const res = await fetch(url, { cache: "no-cache" });
    if (!res.ok) throw new Error(`Locale ${code} missing`);
    catalog = await res.json();
    currentLang = code;
  }
  if (typeof document !== "undefined") {
    document.documentElement.lang = code;
  }
  return code;
}

export function applyDom(root = document) {
  root.querySelectorAll("[data-i18n]").forEach((el) => {
    const key = el.getAttribute("data-i18n");
    if (!key) return;
    el.textContent = t(key);
  });
  root.querySelectorAll("[data-i18n-placeholder]").forEach((el) => {
    const key = el.getAttribute("data-i18n-placeholder");
    if (key) el.setAttribute("placeholder", t(key));
  });
  root.querySelectorAll("[data-i18n-title]").forEach((el) => {
    const key = el.getAttribute("data-i18n-title");
    if (key) el.setAttribute("title", t(key));
  });
  root.querySelectorAll("[data-i18n-aria]").forEach((el) => {
    const key = el.getAttribute("data-i18n-aria");
    if (key) el.setAttribute("aria-label", t(key));
  });
}

/**
 * @param {string} pref
 * @param {{ syncServer?: boolean }} [opts]
 */
export async function setLangPref(pref, opts = {}) {
  const syncServer = opts.syncServer !== false;
  const normalized = pref === "en" || pref === "es" || pref === "auto" ? pref : "auto";
  await chrome.storage.local.set({ uiLanguage: normalized });
  readyPromise = null;
  await loadLocale(resolveLang(normalized));
  if (typeof document !== "undefined") applyDom();

  if (syncServer) {
    try {
      await chrome.runtime.sendMessage({ type: "UPDATE_SETTINGS", lang: normalized });
    } catch {
      /* locked or no background */
    }
  }
  return currentLang;
}

/** Apply preference from server without writing back. */
export async function applyServerLang(lang) {
  if (!lang) return currentLang;
  const normalized = lang === "en" || lang === "es" || lang === "auto" ? lang : "auto";
  await chrome.storage.local.set({ uiLanguage: normalized });
  readyPromise = null;
  await loadLocale(resolveLang(normalized));
  if (typeof document !== "undefined") applyDom();
  return currentLang;
}

export async function initI18n() {
  if (!readyPromise) {
    readyPromise = (async () => {
      const pref = await getPref();
      await loadLocale(resolveLang(pref));
      return currentLang;
    })();
  }
  return readyPromise;
}

// Re-init when options / background sync language
if (typeof chrome !== "undefined" && chrome.storage?.onChanged) {
  chrome.storage.onChanged.addListener((changes, area) => {
    if (area !== "local" || !changes.uiLanguage) return;
    readyPromise = null;
    initI18n().then(() => {
      if (typeof document !== "undefined") applyDom();
    });
  });
}

const api = {
  t,
  initI18n,
  applyDom,
  setLangPref,
  applyServerLang,
  getLang,
  getPref,
  loadLocale,
  resolveLang,
};
if (typeof globalThis !== "undefined") {
  globalThis.TalosI18n = api;
}

export default api;
