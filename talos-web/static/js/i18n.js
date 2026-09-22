/**
 * Lightweight i18n for Talos web UI (en / es).
 * Preference is shared with the extension via GET/PUT /api/settings.
 * localStorage `talos.lang` is a cache / offline fallback.
 */

const SUPPORTED = ["en", "es"];
const STORAGE_KEY = "talos.lang";

/** @type {Record<string, string>} */
let catalog = {};
let currentLang = "en";
/** @type {string} */
let currentPref = "auto";

function detectBrowserLang() {
  const nav = (navigator.language || "en").toLowerCase();
  if (nav.startsWith("es")) return "es";
  return "en";
}

export function getStoredLangPref() {
  try {
    return localStorage.getItem(STORAGE_KEY) || "auto";
  } catch {
    return "auto";
  }
}

export function getLangPref() {
  return currentPref || getStoredLangPref() || "auto";
}

export function resolveLang(pref = getLangPref()) {
  if (pref && pref !== "auto" && SUPPORTED.includes(pref)) return pref;
  return detectBrowserLang();
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

export async function loadLocale(lang) {
  const code = SUPPORTED.includes(lang) ? lang : "en";
  const res = await fetch(`/js/locales/${code}.json`, { cache: "no-cache" });
  if (!res.ok) throw new Error(`Locale ${code} missing`);
  catalog = await res.json();
  currentLang = code;
  document.documentElement.lang = code;
  return code;
}

/** Apply data-i18n / data-i18n-placeholder / data-i18n-title on the document. */
export function applyDom(root = document) {
  root.querySelectorAll("[data-i18n]").forEach((el) => {
    const key = el.getAttribute("data-i18n");
    if (!key) return;
    const attr = el.getAttribute("data-i18n-attr");
    if (attr) {
      el.setAttribute(attr, t(key));
    } else {
      el.textContent = t(key);
    }
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
  // Refresh icons if Lucide replaced node contents elsewhere
  try {
    // @ts-ignore
    if (typeof lucide !== "undefined") lucide.createIcons();
  } catch {
    /* ignore */
  }
}

function cachePref(pref) {
  currentPref = pref;
  try {
    localStorage.setItem(STORAGE_KEY, pref);
  } catch {
    /* ignore */
  }
}

async function pushSettingsToServer(lang) {
  const res = await fetch("/api/settings", {
    method: "PUT",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify({ lang }),
  });
  if (!res.ok) {
    const err = await res.json().catch(() => ({}));
    throw new Error(err.error || res.statusText || "Settings update failed");
  }
  return res.json();
}

async function pullSettingsFromServer() {
  const res = await fetch("/api/settings", { headers: { Accept: "application/json" } });
  if (!res.ok) throw new Error(res.statusText);
  return res.json();
}

/**
 * Apply a language preference locally (and optionally persist to the server).
 * @param {string} pref auto|en|es
 * @param {{ syncServer?: boolean }} [opts]
 */
export async function setLangPref(pref, opts = {}) {
  const syncServer = opts.syncServer !== false;
  const normalized =
    pref === "en" || pref === "es" || pref === "auto" ? pref : "auto";
  cachePref(normalized);
  await loadLocale(resolveLang(normalized));
  applyDom();
  window.dispatchEvent(
    new CustomEvent("talos:lang", { detail: { lang: currentLang, pref: normalized } })
  );

  if (syncServer) {
    // Requires auth (cookie/Bearer). Pre-login changes stay in localStorage only.
    pushSettingsToServer(normalized).catch((err) => {
      console.warn("[talos i18n] could not persist language:", err.message || err);
    });
  }
}

/** Pull shared preference from the server and apply it. */
export async function syncLangFromServer() {
  try {
    const settings = await pullSettingsFromServer();
    if (!settings?.lang) return getLangPref();
    await setLangPref(settings.lang, { syncServer: false });
    return settings.lang;
  } catch {
    /* ignore */
  }
  return getLangPref();
}

export async function initI18n() {
  // Local cache first for fast paint, then server preference wins.
  currentPref = getStoredLangPref();
  try {
    await loadLocale(resolveLang(currentPref));
    applyDom();
  } catch (err) {
    console.warn("[talos i18n] locale load failed:", err);
  }
  try {
    await syncLangFromServer();
  } catch {
    /* ignore */
  }
  return currentLang;
}
