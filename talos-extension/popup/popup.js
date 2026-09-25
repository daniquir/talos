import { applyDom, applyServerLang, initI18n, t } from "../shared/i18n.js";
import { generatePassword } from "../shared/password.js";

const $ = (id) => document.getElementById(id);

const matchError = $("match-error");
const matchEmpty = $("match-empty");
const matchList = $("match-list");
const matchStatus = $("match-status");
const currentHost = $("current-host");
const viewSite = $("view-site");
const viewVault = $("view-vault");
const viewSecret = $("view-secret");
const viewUnlock = $("view-unlock");
const unlockHint = $("unlock-hint");
const masterKeyInput = $("master-key");
const lockError = $("lock-error");
const btnUnlock = $("btn-unlock");
const btnLock = $("btn-lock");
const btnOptions = $("btn-options");
const btnOpenWeb = $("btn-open-web");
const tabSite = $("tab-site");
const tabVault = $("tab-vault");
const vaultTree = $("vault-tree");
const vaultSearch = $("tree-search");
const vaultSearchWrap = $("vault-search-wrap");
const vaultEmpty = $("vault-empty");
const vaultLocked = $("vault-locked");
const vaultError = $("vault-error");
const secretPathEl = $("secret-path");
const secretUser = $("secret-user");
const secretPass = $("secret-pass");
const secretUrl = $("secret-url");
const secretNotes = $("secret-notes");
const secretError = $("secret-error");
const secretOk = $("secret-ok");
const genLength = $("gen-length");
const genLengthOut = $("gen-length-out");
const genUpper = $("gen-upper");
const genNums = $("gen-nums");
const genSyms = $("gen-syms");

/** @type {{ path: string, title?: string } | null} */
let pendingFill = null;
/** @type {{ path: string } | null} */
let pendingEdit = null;
let unlocked = false;
/** @type {"site" | "vault" | "secret"} */
let activeTab = "site";
/** @type {"site" | "vault"} */
let returnTab = "site";
/** @type {Array<{ name: string, path: string, is_dir: boolean, children?: any[] }> | null} */
let treeCache = null;
/** @type {string | null} */
let editingPath = null;

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

function escapeHtml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function showToast(msg) {
  const el = document.createElement("div");
  el.className = "toast";
  el.textContent = msg;
  document.body.appendChild(el);
  setTimeout(() => el.remove(), 1400);
}

async function copyText(text, okKey) {
  const value = String(text || "");
  if (!value) {
    showToast(t("ext_copy_empty"));
    return;
  }
  try {
    await navigator.clipboard.writeText(value);
    try {
      await send("SCHEDULE_CLIPBOARD_CLEAR", { delaySec: 45 });
    } catch {
      /* best-effort */
    }
    showToast(t(okKey));
  } catch {
    showToast(t("ext_copy_failed"));
  }
}

function hidePanels() {
  viewSite.hidden = true;
  viewVault.hidden = true;
  viewSecret.hidden = true;
}

function setTab(tab) {
  if (tab === "secret") return;
  activeTab = tab;
  returnTab = tab;
  const site = tab === "site";
  tabSite.classList.toggle("active", site);
  tabVault.classList.toggle("active", !site);
  tabSite.setAttribute("aria-selected", site ? "true" : "false");
  tabVault.setAttribute("aria-selected", site ? "false" : "true");
  hidePanels();
  viewSite.hidden = !site;
  viewVault.hidden = site;
  if (site) {
    // Locked: keep / restore passphrase form when switching tabs.
    // Unlocked: only hide unlock if nothing pending (fill/edit).
    if (!unlocked) {
      showUnlock(pendingFill, pendingEdit);
    } else if (!pendingFill && !pendingEdit) {
      hideUnlock();
    }
  } else {
    void loadVaultTree();
  }
}

function showUnlock(forFill, forEdit = null) {
  if (unlocked && !forFill && !forEdit) {
    hideUnlock();
    return;
  }
  viewUnlock.hidden = false;
  pendingFill = forFill;
  pendingEdit = forEdit;
  if (forFill) {
    unlockHint.textContent = t("ext_unlock_hint_fill", { title: forFill.title || forFill.path });
    btnUnlock.textContent = t("ext_unlock_fill");
  } else if (forEdit) {
    unlockHint.textContent = t("ext_unlock_hint_edit", { title: forEdit.path });
    btnUnlock.textContent = t("ext_unlock");
  } else {
    unlockHint.textContent = t("ext_unlock_hint");
    btnUnlock.textContent = t("ext_unlock");
  }
  masterKeyInput.focus();
}

function hideUnlock() {
  viewUnlock.hidden = true;
  pendingFill = null;
  pendingEdit = null;
  lockError.hidden = true;
  masterKeyInput.value = "";
}

async function tryAutofill(path) {
  await send("AUTOFILL", { path });
  window.close();
}

async function ensureUnlockedFor(action) {
  if (unlocked) return true;
  showUnlock(action.fill || null, action.edit || null);
  return false;
}

async function onFillClick(m) {
  matchError.hidden = true;
  vaultError.hidden = true;
  if (!(await ensureUnlockedFor({ fill: { path: m.path, title: m.title || m.name || m.path } }))) {
    return;
  }
  try {
    await tryAutofill(m.path);
  } catch (err) {
    if (err.status === 401) {
      unlocked = false;
      btnLock.hidden = true;
      treeCache = null;
      showUnlock({ path: m.path, title: m.title || m.name || m.path });
      return;
    }
    const el = activeTab === "vault" ? vaultError : matchError;
    el.hidden = false;
    el.textContent = err.message;
  }
}

async function onCopyUser(m) {
  if (!(await ensureUnlockedFor({ fill: { path: m.path, title: m.title || m.path } }))) return;
  try {
    const cred = await send("REVEAL_CREDENTIAL", { path: m.path });
    await copyText(cred.username, "ext_copied_user");
  } catch (err) {
    if (err.status === 401) {
      unlocked = false;
      showUnlock({ path: m.path, title: m.title || m.path });
      return;
    }
    showToast(err.message);
  }
}

async function onCopyPass(m) {
  if (!(await ensureUnlockedFor({ fill: { path: m.path, title: m.title || m.path } }))) return;
  try {
    const cred = await send("REVEAL_CREDENTIAL", { path: m.path });
    await copyText(cred.password, "ext_copied_pass");
  } catch (err) {
    if (err.status === 401) {
      unlocked = false;
      showUnlock({ path: m.path, title: m.title || m.path });
      return;
    }
    showToast(err.message);
  }
}

async function openSecretEditor(path) {
  if (!(await ensureUnlockedFor({ edit: { path } }))) return;
  secretError.hidden = true;
  secretOk.hidden = true;
  try {
    const cred = await send("GET_SECRET", { path });
    editingPath = path;
    secretPathEl.textContent = path;
    secretUser.value = cred.username || "";
    secretPass.type = "password";
    secretPass.value = cred.password || "";
    secretUrl.value = cred.url || "";
    secretNotes.value = cred.notes || "";
    hidePanels();
    hideUnlock();
    viewSecret.hidden = false;
    activeTab = "secret";
    tabSite.classList.remove("active");
    tabVault.classList.remove("active");
  } catch (err) {
    if (err.status === 401) {
      unlocked = false;
      btnLock.hidden = true;
      showUnlock(null, { path });
      return;
    }
    showToast(err.message);
  }
}

function appendItemActions(container, m) {
  const actions = document.createElement("div");
  actions.className = "item-actions";
  const mk = (labelKey, fn) => {
    const b = document.createElement("button");
    b.type = "button";
    b.textContent = t(labelKey);
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      fn();
    });
    return b;
  };
  actions.append(
    mk("ext_act_fill", () => onFillClick(m)),
    mk("ext_act_copy_user", () => onCopyUser(m)),
    mk("ext_act_copy_pass", () => onCopyPass(m)),
    mk("ext_act_edit", () => openSecretEditor(m.path))
  );
  container.appendChild(actions);
}

function nodeMatches(node, q) {
  if (!q) return true;
  const hay = `${node.name || ""} ${node.path || ""}`.toLowerCase();
  if (hay.includes(q)) return true;
  if (node.is_dir && Array.isArray(node.children)) {
    return node.children.some((c) => nodeMatches(c, q));
  }
  return false;
}

function renderTreeNodes(nodes, depth, filter) {
  const frag = document.createDocumentFragment();
  for (const node of nodes || []) {
    if (filter && !nodeMatches(node, filter)) continue;

    const row = document.createElement("div");
    row.className = "row";
    row.style.paddingLeft = `${depth * 12}px`;

    const twist = document.createElement("button");
    twist.type = "button";
    twist.className = "twist";
    const isDir = Boolean(node.is_dir);
    const hasKids = isDir && Array.isArray(node.children) && node.children.length > 0;
    if (!hasKids) twist.classList.add("leaf");
    twist.textContent = hasKids ? "▾" : "";
    twist.setAttribute("aria-label", isDir ? t("ext_tree_toggle") : "");

    const btn = document.createElement("button");
    btn.type = "button";
    btn.className = `node ${isDir ? "folder" : "secret"}`;
    btn.textContent = node.name || node.path || "";
    btn.title = node.path || node.name || "";

    row.append(twist, btn);
    frag.appendChild(row);

    if (isDir && hasKids) {
      const kids = document.createElement("div");
      kids.className = "children";
      kids.appendChild(renderTreeNodes(node.children, depth + 1, filter));
      frag.appendChild(kids);

      const syncTwist = () => {
        twist.textContent = kids.classList.contains("collapsed") ? "▸" : "▾";
      };
      twist.addEventListener("click", (e) => {
        e.stopPropagation();
        kids.classList.toggle("collapsed");
        syncTwist();
      });
      btn.addEventListener("click", () => {
        kids.classList.toggle("collapsed");
        syncTwist();
      });
    } else if (!isDir) {
      const wrap = document.createElement("div");
      wrap.className = "item-row";
      wrap.style.marginLeft = `${depth * 12 + 18}px`;
      wrap.style.marginBottom = "4px";
      appendItemActions(wrap, { path: node.path, title: node.name, name: node.name });
      frag.appendChild(wrap);
      btn.addEventListener("click", () => openSecretEditor(node.path));
    }
  }
  return frag;
}

function applyTreeFilter() {
  if (!treeCache) return;
  const q = (vaultSearch.value || "").trim().toLowerCase();
  vaultTree.innerHTML = "";
  vaultTree.appendChild(renderTreeNodes(treeCache, 0, q || null));
  const any = vaultTree.querySelector(".row");
  vaultEmpty.hidden = Boolean(any);
  if (!any) vaultEmpty.textContent = q ? t("ext_tree_no_filter") : t("ext_tree_empty");
}

function showVaultLocked() {
  vaultSearchWrap.hidden = true;
  vaultTree.hidden = true;
  vaultTree.innerHTML = "";
  vaultEmpty.hidden = true;
  vaultError.hidden = true;
  vaultLocked.hidden = false;
  if (!unlocked) showUnlock(null);
}

async function loadVaultTree() {
  vaultError.hidden = true;
  vaultLocked.hidden = true;
  vaultEmpty.hidden = true;

  if (!unlocked) {
    showVaultLocked();
    return;
  }

  hideUnlock();
  vaultSearchWrap.hidden = false;
  vaultTree.hidden = false;
  vaultTree.innerHTML = `<p class="muted">${escapeHtml(t("ext_tree_loading"))}</p>`;

  try {
    const res = await send("GET_TREE");
    treeCache = Array.isArray(res.tree) ? res.tree : [];
    if (!treeCache.length) {
      vaultTree.hidden = true;
      vaultTree.innerHTML = "";
      vaultEmpty.hidden = false;
      vaultEmpty.textContent = t("ext_tree_empty");
      return;
    }
    applyTreeFilter();
  } catch (err) {
    if (err.status === 401) {
      unlocked = false;
      btnLock.hidden = true;
      treeCache = null;
      showVaultLocked();
      return;
    }
    vaultTree.hidden = true;
    vaultError.hidden = false;
    vaultError.textContent = err.message;
  }
}

async function loadMatches() {
  matchError.hidden = true;
  matchEmpty.hidden = true;
  matchList.innerHTML = "";
  matchStatus.textContent = t("ext_looking_up");

  try {
    const session = await send("GET_SESSION");
    unlocked = Boolean(session.unlocked);
    btnLock.hidden = !unlocked;
    if (unlocked) hideUnlock();

    const data = await send("MATCH");
    currentHost.textContent = data.host || "—";
    const matches = data.matches || [];

    if (!matches.length) {
      matchStatus.textContent = unlocked ? t("ext_vault_unlocked") : t("ext_vault_locked_ok");
      matchEmpty.hidden = false;
      matchEmpty.textContent = unlocked ? t("ext_no_matches") : t("ext_unlock_to_see_matches");
      if (!unlocked) showUnlock(null);
      return;
    }

    matchStatus.textContent = t("ext_matches_unlocked", { count: matches.length });

    for (const m of matches) {
      const li = document.createElement("li");
      li.className = "item";
      const main = document.createElement("button");
      main.type = "button";
      main.className = "item-main";
      main.innerHTML = `<span class="title">${escapeHtml(m.title || m.path)}</span>
        <span class="meta">${escapeHtml(m.username || m.path)}</span>`;
      main.addEventListener("click", () => openSecretEditor(m.path));
      li.appendChild(main);
      appendItemActions(li, m);
      matchList.appendChild(li);
    }
  } catch (err) {
    matchStatus.textContent = "";
    matchError.hidden = false;
    matchError.textContent = err.message;
  }
}

btnUnlock.addEventListener("click", async () => {
  if (btnUnlock.disabled) return;
  lockError.hidden = true;
  let masterKey = masterKeyInput.value;
  masterKeyInput.value = "";
  if (!masterKey) {
    lockError.hidden = false;
    lockError.textContent = t("ext_key_required");
    return;
  }
  const label = btnUnlock.textContent;
  btnUnlock.disabled = true;
  masterKeyInput.disabled = true;
  btnUnlock.textContent = t("ext_loading");
  try {
    await send("UNLOCK", { masterKey });
    unlocked = true;
    btnLock.hidden = false;
    const fill = pendingFill;
    const edit = pendingEdit;
    hideUnlock();
    if (fill?.path) {
      btnUnlock.textContent = t("ext_filling");
      await tryAutofill(fill.path);
      return;
    }
    if (edit?.path) {
      await openSecretEditor(edit.path);
      return;
    }
    await loadMatches();
    if (returnTab === "vault" || activeTab === "vault") await loadVaultTree();
  } catch (err) {
    lockError.hidden = false;
    lockError.textContent = err.message;
  } finally {
    masterKey = "";
    btnUnlock.disabled = false;
    masterKeyInput.disabled = false;
    btnUnlock.textContent = label || t("ext_unlock_fill");
  }
});

masterKeyInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") {
    e.preventDefault();
    if (!btnUnlock.disabled) btnUnlock.click();
  }
});

vaultSearch.addEventListener("input", () => {
  if (treeCache) applyTreeFilter();
});

tabSite.addEventListener("click", () => setTab("site"));
tabVault.addEventListener("click", () => setTab("vault"));

btnLock.addEventListener("click", async () => {
  await send("LOCK");
  unlocked = false;
  btnLock.hidden = true;
  treeCache = null;
  editingPath = null;
  secretUser.value = "";
  secretPass.value = "";
  secretUrl.value = "";
  secretNotes.value = "";
  hideUnlock();
  setTab(returnTab === "vault" ? "site" : "site");
  await loadMatches();
});

btnOptions.addEventListener("click", () => {
  chrome.runtime.openOptionsPage();
});

btnOpenWeb.addEventListener("click", async () => {
  try {
    const cfg = await send("GET_CONFIG");
    const url = cfg.serverUrl || "http://localhost:3000";
    await chrome.tabs.create({ url });
  } catch (err) {
    showToast(err.message);
  }
});

$("btn-copy-user").addEventListener("click", () => copyText(secretUser.value, "ext_copied_user"));
$("btn-copy-pass").addEventListener("click", () => copyText(secretPass.value, "ext_copied_pass"));
$("btn-toggle-pass").addEventListener("click", () => {
  secretPass.type = secretPass.type === "password" ? "text" : "password";
});

genLength.addEventListener("input", () => {
  genLengthOut.textContent = genLength.value;
});

$("btn-generate").addEventListener("click", () => {
  secretPass.value = generatePassword({
    length: Number(genLength.value) || 24,
    useUpper: genUpper.checked,
    useNumbers: genNums.checked,
    useSymbols: genSyms.checked,
  });
  secretPass.type = "text";
});

$("btn-secret-back").addEventListener("click", () => {
  editingPath = null;
  setTab(returnTab);
});

$("btn-secret-fill").addEventListener("click", async () => {
  if (!editingPath) return;
  try {
    await tryAutofill(editingPath);
  } catch (err) {
    secretError.hidden = false;
    secretError.textContent = err.message;
  }
});

$("btn-secret-save").addEventListener("click", async () => {
  if (!editingPath) return;
  secretError.hidden = true;
  secretOk.hidden = true;
  const btn = $("btn-secret-save");
  const label = btn.textContent;
  btn.disabled = true;
  btn.textContent = t("ext_saving");
  try {
    await send("UPDATE_SECRET", {
      path: editingPath,
      password: secretPass.value,
      username: secretUser.value,
      url: secretUrl.value,
      notes: secretNotes.value,
      full: true,
    });
    secretOk.hidden = false;
    secretOk.textContent = t("ext_secret_saved");
    await send("TOUCH_SESSION");
  } catch (err) {
    secretError.hidden = false;
    secretError.textContent = err.message;
  } finally {
    btn.disabled = false;
    btn.textContent = label;
  }
});

(async () => {
  await initI18n();
  applyDom();
  try {
    const session = await send("GET_SESSION");
    unlocked = Boolean(session.unlocked);
    btnLock.hidden = !unlocked;
    if (session.unlocked) {
      const res = await send("GET_SETTINGS");
      if (res.settings?.lang) {
        await applyServerLang(res.settings.lang);
        applyDom();
      }
    }
  } catch {
    /* ignore */
  }

  try {
    const { pending } = await send("TAKE_PENDING_ACTION");
    if (pending?.type === "fill" && pending.path) {
      if (unlocked) {
        await tryAutofill(pending.path);
        return;
      }
      showUnlock({ path: pending.path, title: pending.path });
    } else if (pending?.type === "fill_primary") {
      if (unlocked) {
        await send("FILL_PRIMARY");
        window.close();
        return;
      }
      showUnlock(null);
    } else if (!unlocked) {
      showUnlock(null);
    }
  } catch {
    if (!unlocked) showUnlock(null);
  }

  await loadMatches();
})();
