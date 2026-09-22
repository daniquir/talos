/**
 * Classic (non-module) i18n bootstrap for content scripts.
 * Uses embedded catalogs (TALOS_LOCALES) so UI never flashes raw keys.
 * Language comes from chrome.storage.local only (no per-frame HTTP).
 */
(function (g) {
  const SUPPORTED = ["en", "es"];
  /** @type {Record<string, string>} */
  let catalog = { ...(g.TALOS_LOCALES?.en || {}) };
  let currentLang = "en";

  function detectBrowserLang() {
    const nav = (g.navigator?.language || "en").toLowerCase();
    return nav.startsWith("es") ? "es" : "en";
  }

  function resolveLang(pref) {
    if (pref && pref !== "auto" && SUPPORTED.includes(pref)) return pref;
    return detectBrowserLang();
  }

  function t(key, vars) {
    let text = catalog[key] ?? g.TALOS_LOCALES?.en?.[key] ?? key;
    if (vars) {
      for (const k of Object.keys(vars)) {
        text = text.split(`{${k}}`).join(String(vars[k] ?? ""));
      }
    }
    return text;
  }

  function applyEmbedded(lang) {
    const code = SUPPORTED.includes(lang) ? lang : "en";
    const pack = g.TALOS_LOCALES?.[code] || g.TALOS_LOCALES?.en || {};
    catalog = { ...pack };
    currentLang = code;
    return code;
  }

  async function loadLocale(lang) {
    const code = applyEmbedded(lang);
    try {
      const url = chrome.runtime.getURL(`shared/locales/${code}.json`);
      const res = await fetch(url, { cache: "force-cache" });
      if (res.ok) {
        catalog = await res.json();
        currentLang = code;
      }
    } catch {
      /* keep embedded */
    }
    return currentLang;
  }

  async function initI18n() {
    let pref = "auto";
    try {
      const data = await chrome.storage.local.get(["uiLanguage"]);
      pref = data.uiLanguage || "auto";
    } catch {
      /* ignore */
    }
    return loadLocale(resolveLang(pref));
  }

  applyEmbedded("en");
  const ready = initI18n().catch(() => applyEmbedded("en"));

  g.TalosI18n = {
    t,
    initI18n,
    getLang: () => currentLang,
    ready,
  };

  chrome.storage?.onChanged?.addListener((changes, area) => {
    if (area !== "local" || !changes.uiLanguage) return;
    initI18n();
  });
})(globalThis);
