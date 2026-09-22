/**
 * Firefox-style credential capture: offer Save / Update after submit or navigation
 * when the user actually typed into login fields.
 * Depends on content.js (same isolated world): send(), ensureUi(), escapeHtml, etc.
 */

(function talosCapture() {
  // Allow re-init after extension reload on the same tab.
  if (window.__talosCaptureInit && window.__talosCaptureVersion === 7) return;
  window.__talosCaptureInit = true;
  window.__talosCaptureVersion = 7;

  function t(key, vars) {
    if (globalThis.TalosI18n?.t) return globalThis.TalosI18n.t(key, vars);
    if (typeof globalThis.talosT === "function") return globalThis.talosT(key, vars);
    return key;
  }

  /** @type {{ username: string, password: string, url: string, host: string, touched: boolean } | null} */
  let pending = null;
  let offerBusy = false;
  /** @type {HTMLElement | null} */
  let door = null;
  let selectedFolder = "";

  function isCcOrOtpPassword(el) {
    const ac = (el.autocomplete || "").toLowerCase();
    if (ac.includes("cc-") || ac === "one-time-code") return true;
    const hints = `${el.name || ""} ${el.id || ""} ${el.placeholder || ""}`.toLowerCase();
    return /otp|totp|one.?time|verif|cvv|cvc|card.?number|ccnum/.test(hints);
  }

  function pickPassword(scope) {
    const passwords = Array.from(scope.querySelectorAll('input[type="password"]')).filter((el) => {
      if (!el || el.disabled) return false;
      if (isCcOrOtpPassword(el)) return false;
      return String(el.value || "").length >= 2;
    });
    if (!passwords.length) return null;
    const neu = passwords.find((el) => (el.autocomplete || "").toLowerCase() === "new-password");
    if (neu) return neu;
    // Prefer last password field (confirm / new) when several exist.
    return passwords[passwords.length - 1];
  }

  function pickUsername(scope, pwdEl) {
    if (typeof findUsernameInputNear === "function" && pwdEl) {
      const near = findUsernameInputNear(pwdEl);
      if (near?.value?.trim()) return near;
    }
    if (typeof findUsernameInputs === "function") {
      const all = findUsernameInputs(scope);
      const withVal = all.find((el) => el.value && el.value.trim());
      if (withVal) return withVal;
      return all[0] || null;
    }
    return null;
  }

  function fieldsTouched(scope) {
    const inputs = Array.from(scope.querySelectorAll("input")).filter((el) => {
      const t = (el.type || "text").toLowerCase();
      return t === "password" || t === "text" || t === "email" || t === "tel";
    });
    return inputs.some((el) => el.dataset.talosTouched === "1");
  }

  function markTouched(e) {
    if (!e.isTrusted) return;
    const el = e.target;
    if (!(el instanceof HTMLInputElement)) return;
    el.dataset.talosTouched = "1";
  }

  document.addEventListener("input", markTouched, true);
  document.addEventListener("change", markTouched, true);

  function captureFromScope(scope) {
    const pwdEl = pickPassword(scope);
    if (!pwdEl) return null;
    if (!fieldsTouched(scope) && pwdEl.dataset.talosTouched !== "1") return null;
    const userEl = pickUsername(scope, pwdEl);
    const password = String(pwdEl.value || "");
    if (password.length < 2) return null;
    return {
      username: String(userEl?.value || "").trim(),
      password,
      url: location.href.split("#")[0],
      host: location.host,
      touched: true,
    };
  }

  function rememberCapture(data) {
    if (!data) return;
    pending = data;
  }

  document.addEventListener(
    "submit",
    (e) => {
      const form = e.target;
      if (!(form instanceof HTMLFormElement)) return;
      const data = captureFromScope(form);
      if (data) {
        rememberCapture(data);
        // Same-document submits (preventDefault) — offer after the turn.
        queueMicrotask(() => maybeOffer(pending));
      }
    },
    true
  );

  // JS-only "submit" / leave page (Firefox-style navigation capture).
  window.addEventListener("pagehide", () => {
    if (!pending) {
      const data = captureFromScope(document);
      if (data) rememberCapture(data);
    }
    if (pending) {
      // Stash in the extension (never page sessionStorage — hostile JS can read it).
      try {
        chrome.runtime.sendMessage({
          type: "STASH_CAPTURE",
          username: pending.username,
          password: pending.password,
          url: pending.url,
          host: pending.host,
        });
      } catch {
        /* ignore */
      }
    }
  });

  async function restorePendingFromSession() {
    try {
      // Drop any legacy page-visible stash from older builds.
      try {
        sessionStorage.removeItem("talosPendingCapture");
      } catch {
        /* ignore */
      }
      const res = await send("TAKE_CAPTURE_STASH");
      const data = res?.capture;
      if (!data?.password || Date.now() - (data.at || 0) > 60_000) return;
      pending = {
        username: data.username || "",
        password: data.password,
        url: data.url || location.href,
        host: data.host || location.host,
        touched: true,
      };
      maybeOffer(pending);
    } catch {
      /* ignore */
    }
  }

  async function maybeOffer(capture) {
    if (!capture || offerBusy || door) return;
    try {
      if (await globalThis.talosContentApi?.isOnVaultOrigin?.()) {
        pending = null;
        return;
      }
    } catch {
      /* ignore */
    }
    offerBusy = true;
    try {
      try {
        await globalThis.TalosI18n?.ready;
      } catch {
        /* embedded fallback already active */
      }
      const offer = await send("CAPTURE_OFFER", {
        host: capture.host,
        username: capture.username,
      });
      if (offer.kind === "ignored") {
        pending = null;
        return;
      }
      showDoorhanger(capture, offer);
    } catch (err) {
      console.warn("[talos] capture offer failed", err);
    } finally {
      offerBusy = false;
    }
  }

  function ensureDoorStyles(shadowRoot) {
    if (shadowRoot.querySelector("style[data-talos-capture]")) return;
    const style = document.createElement("style");
    style.setAttribute("data-talos-capture", "1");
    style.textContent = `
      .door {
        position: fixed;
        right: 16px;
        bottom: 16px;
        width: min(360px, calc(100vw - 24px));
        max-height: min(70vh, 520px);
        overflow: auto;
        background: #09090b;
        color: #4ade80;
        border: 1px solid #27272a;
        border-radius: 2px;
        box-shadow: 0 0 28px rgba(34,197,94,.22), 0 16px 40px rgba(0,0,0,.6);
        font: 12px/1.45 "Fira Code", ui-monospace, monospace;
        text-shadow: 0 0 2px rgba(34,197,94,.35);
        z-index: 2147483647;
        pointer-events: auto;
      }
      .door .head {
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
      .door .body { padding: 12px; display: flex; flex-direction: column; gap: 10px; }
      .door .msg { margin: 0; color: #a1a1aa; text-shadow: none; }
      .door .msg strong { color: #4ade80; text-shadow: 0 0 2px rgba(34,197,94,.35); }
      .door .actions { display: flex; flex-wrap: wrap; gap: 8px; }
      .door button {
        border-radius: 2px;
        padding: 8px 10px;
        font: 700 10px/1.2 "Fira Code", ui-monospace, monospace;
        letter-spacing: .14em;
        text-transform: uppercase;
        cursor: pointer;
      }
      .door .primary {
        background: rgba(20,83,45,.35);
        color: #22c55e;
        border: 1px solid #16a34a;
      }
      .door .primary:hover { background: #22c55e; color: #000; }
      .door .primary:disabled { opacity: .55; cursor: wait; }
      .door .ghost {
        background: transparent;
        color: #71717a;
        border: 1px solid #27272a;
      }
      .door .ghost:hover { color: #4ade80; border-color: #22c55e; }
      .door .danger {
        background: transparent;
        color: #ef4444;
        border: 1px solid #7f1d1d;
      }
      .door input, .door .tree {
        background: #18181b;
        border: 1px solid #27272a;
        border-radius: 2px;
        color: #fff;
        padding: 8px 10px;
        font: inherit;
        text-shadow: none;
        width: 100%;
      }
      .door input:focus {
        outline: none;
        border-color: #22c55e;
        box-shadow: 0 0 12px rgba(34,197,94,.18);
      }
      .door .err {
        margin: 0;
        color: #ef4444;
        font-size: 10px;
        letter-spacing: .06em;
        text-transform: uppercase;
      }
      .door .tree {
        max-height: 180px;
        overflow: auto;
        padding: 6px;
        display: flex;
        flex-direction: column;
        gap: 2px;
      }
      .door .tree button {
        width: 100%;
        text-align: left;
        background: transparent;
        border: 1px solid transparent;
        color: #a1a1aa;
        padding: 6px 8px;
        letter-spacing: .04em;
        text-transform: none;
        font-weight: 400;
      }
      .door .tree button:hover, .door .tree button.sel {
        border-color: #22c55e;
        color: #4ade80;
        background: rgba(34,197,94,.12);
      }
      .door .path-preview {
        margin: 0;
        font-size: 10px;
        color: #71717a;
        letter-spacing: .06em;
        text-transform: uppercase;
      }
      .door .unlock-box {
        display: flex;
        flex-direction: column;
        gap: 8px;
        padding-top: 4px;
        border-top: 1px solid #27272a;
      }
      .door .pass-row {
        display: flex;
        gap: 6px;
        align-items: stretch;
      }
      .door .pass-row input { flex: 1; min-width: 0; }
      .door .pass-row .secondary,
      .door .pass-row .ghost-icon {
        background: transparent;
        color: #22c55e;
        border: 1px solid #16a34a;
        white-space: nowrap;
      }
      .door .pass-row .ghost-icon {
        color: #71717a;
        border-color: #27272a;
        min-width: 32px;
        padding: 8px 6px;
      }
      .door .pass-row .ghost-icon:hover {
        color: #4ade80;
        border-color: #22c55e;
      }
      .door .gen-flags {
        display: flex;
        flex-wrap: wrap;
        gap: 8px;
        color: #71717a;
        font-size: 10px;
        text-transform: uppercase;
        letter-spacing: .06em;
      }
      .door .gen-flags label { display: flex; align-items: center; gap: 4px; cursor: pointer; }
      .door .gen-len {
        display: flex;
        align-items: center;
        gap: 6px;
        color: #71717a;
        font-size: 10px;
        text-transform: uppercase;
      }
      .door .gen-len input[type=range] { flex: 1; }
    `;
    shadowRoot.appendChild(style);
  }

  function closeDoor() {
    if (door) {
      door.remove();
      door = null;
    }
    selectedFolder = "";
  }

  function generatePasswordLocal(length, useUpper, useNumbers, useSymbols) {
    const lower = "abcdefghijklmnopqrstuvwxyz";
    const upper = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
    const nums = "0123456789";
    const syms = "!@#$%^&*()_+~`|}{[]:;?><,./-=";
    let chars = lower;
    if (useUpper) chars += upper;
    if (useNumbers) chars += nums;
    if (useSymbols) chars += syms;
    if (!chars) chars = lower;
    const len = Math.max(4, Math.min(128, Number(length) || 24));
    const out = new Array(len);
    const max = 256 - (256 % chars.length);
    let i = 0;
    while (i < len) {
      const buf = new Uint8Array(len - i);
      crypto.getRandomValues(buf);
      for (const b of buf) {
        if (b >= max) continue;
        out[i++] = chars[b % chars.length];
        if (i >= len) break;
      }
    }
    return out.join("");
  }

  function attachPasswordEditor(body, capture) {
    if (body.querySelector("[data-role=pass]")) return;
    const wrap = document.createElement("div");
    wrap.setAttribute("data-role", "pass-wrap");
    wrap.innerHTML = `
      <div class="pass-row">
        <input type="password" data-role="pass" autocomplete="new-password" />
        <button type="button" class="ghost-icon" data-role="toggle-pass" title="${escapeHtml(t("ext_toggle_pass"))}" aria-label="${escapeHtml(t("ext_toggle_pass"))}">◉</button>
        <button type="button" class="secondary" data-role="gen">${escapeHtml(t("ext_gen_btn"))}</button>
      </div>
      <div class="gen-len">
        <span>${escapeHtml(t("ext_gen_length"))}</span>
        <input type="range" data-role="gen-len" min="12" max="64" value="24" />
        <span data-role="gen-len-out">24</span>
      </div>
      <div class="gen-flags">
        <label><input type="checkbox" data-role="gen-upper" checked /> A-Z</label>
        <label><input type="checkbox" data-role="gen-nums" checked /> 0-9</label>
        <label><input type="checkbox" data-role="gen-syms" checked /> !@#</label>
      </div>
    `;
    const actions = body.querySelector('[data-role="actions"]');
    body.insertBefore(wrap, actions);
    const passInput = wrap.querySelector("[data-role=pass]");
    passInput.value = capture.password || "";
    const lenInput = wrap.querySelector("[data-role=gen-len]");
    const lenOut = wrap.querySelector("[data-role=gen-len-out]");
    lenInput.addEventListener("input", () => {
      lenOut.textContent = lenInput.value;
    });
    wrap.querySelector("[data-role=toggle-pass]").addEventListener("click", () => {
      passInput.type = passInput.type === "password" ? "text" : "password";
    });
    wrap.querySelector("[data-role=gen]").addEventListener("click", () => {
      passInput.value = generatePasswordLocal(
        lenInput.value,
        wrap.querySelector("[data-role=gen-upper]").checked,
        wrap.querySelector("[data-role=gen-nums]").checked,
        wrap.querySelector("[data-role=gen-syms]").checked
      );
      passInput.type = "text";
    });
  }

  function readDoorPassword(capture) {
    const passInput = door?.querySelector("[data-role=pass]");
    if (passInput && String(passInput.value || "").length >= 2) {
      return { ...capture, password: String(passInput.value) };
    }
    return capture;
  }

  function showDoorhanger(capture, offer) {
    ensureUi();
    ensureDoorStyles(shadow);
    closeDoor();
    closeMenu?.();

    door = document.createElement("div");
    door.className = "door";
    door.setAttribute("role", "dialog");
    shadow.appendChild(door);

    const kind = offer.kind;
    const title =
      kind === "update"
        ? t("ext_update_password")
        : t("ext_save_password");

    door.innerHTML = `
      <div class="head"><span>talos-vault</span><span>${escapeHtml(t("ext_capture"))}</span></div>
      <div class="body" data-role="body">
        <p class="msg" data-role="msg"></p>
        <div data-role="user-wrap" hidden>
          <input type="text" data-role="user" placeholder="${escapeHtml(t("ext_ph_username"))}" autocomplete="username" />
        </div>
        <div class="actions" data-role="actions"></div>
        <p class="err" data-role="err" hidden></p>
      </div>
    `;

    const msg = door.querySelector('[data-role="msg"]');
    const actions = door.querySelector('[data-role="actions"]');
    const errEl = door.querySelector('[data-role="err"]');
    const userWrap = door.querySelector('[data-role="user-wrap"]');
    const userInput = door.querySelector('[data-role="user"]');
    const body = door.querySelector('[data-role="body"]');

    if (kind === "update") {
      msg.innerHTML = t("ext_update_msg", {
        path: `<strong>${escapeHtml(offer.path)}</strong>`,
        user: `<strong>${escapeHtml(capture.username || offer.username || "")}</strong>`,
      });
    } else if (kind === "need_username") {
      msg.textContent = t("ext_need_username");
      userWrap.hidden = false;
    } else {
      msg.innerHTML = t("ext_save_msg", {
        user: `<strong>${escapeHtml(capture.username)}</strong>`,
        host: `<strong>${escapeHtml(capture.host)}</strong>`,
      });
    }

    attachPasswordEditor(body, capture);

    const btnSave = document.createElement("button");
    btnSave.type = "button";
    btnSave.className = "primary";
    btnSave.textContent = kind === "update" ? t("ext_btn_update") : t("ext_btn_save");
    const btnDismiss = document.createElement("button");
    btnDismiss.type = "button";
    btnDismiss.className = "ghost";
    btnDismiss.textContent = t("ext_btn_not_now");
    const btnNever = document.createElement("button");
    btnNever.type = "button";
    btnNever.className = "danger";
    btnNever.textContent = t("ext_btn_never");

    actions.append(btnSave, btnDismiss, btnNever);

    const showErr = (text) => {
      errEl.hidden = false;
      errEl.textContent = text;
    };

    btnDismiss.addEventListener("click", () => {
      pending = null;
      closeDoor();
    });

    btnNever.addEventListener("click", async () => {
      try {
        await send("CAPTURE_NEVER", { host: capture.host });
      } catch {
        /* ignore */
      }
      pending = null;
      closeDoor();
    });

    btnSave.addEventListener("click", async () => {
      errEl.hidden = true;
      capture = readDoorPassword(capture);
      let username = capture.username;
      if (kind === "need_username" || !username) {
        username = String(userInput.value || "").trim();
        if (!username) {
          showErr(t("ext_username_required"));
          userWrap.hidden = false;
          return;
        }
        capture = { ...capture, username };
        try {
          const again = await send("CAPTURE_OFFER", {
            host: capture.host,
            username,
          });
          if (again.kind === "update") {
            await runUpdate(capture, again, btnSave, showErr);
            return;
          }
        } catch (err) {
          showErr(err.message);
          return;
        }
      }

      if (kind === "update") {
        await runUpdate(capture, offer, btnSave, showErr);
      } else {
        await runSaveFlow(capture, btnSave, showErr);
      }
    });
  }

  async function ensureUnlocked(container, showErr) {
    const session = await send("GET_SESSION");
    if (session.unlocked) return true;

    // Never accept the master key on a third-party page — open extension UI.
    let box = container.querySelector(".unlock-box");
    if (!box) {
      box = document.createElement("div");
      box.className = "unlock-box";
      box.innerHTML = `
        <p class="msg">${escapeHtml(t("ext_unlock_in_popup"))}</p>
        <button type="button" class="primary" data-role="unlock-go">${escapeHtml(t("ext_open_popup"))}</button>
      `;
      container.appendChild(box);
    }
    const go = box.querySelector('[data-role="unlock-go"]');

    return new Promise((resolve) => {
      const openExt = async () => {
        go.disabled = true;
        go.textContent = t("ext_loading");
        try {
          await send("REQUEST_UNLOCK", { reason: "capture" });
          showErr(t("ext_unlock_then_retry"));
          resolve(false);
        } catch (err) {
          showErr(err.message);
          go.disabled = false;
          go.textContent = t("ext_open_popup");
          resolve(false);
        }
      };
      go.onclick = (e) => {
        e.preventDefault();
        openExt();
      };
    });
  }

  async function runUpdate(capture, offer, btn, showErr) {
    btn.disabled = true;
    btn.textContent = t("ext_working");
    const body = door.querySelector('[data-role="body"]');
    capture = readDoorPassword(capture);
    try {
      if (!(await ensureUnlocked(body, showErr))) {
        btn.disabled = false;
        btn.textContent = t("ext_btn_update");
        return;
      }
      const res = await send("UPDATE_SECRET", {
        path: offer.path,
        password: capture.password,
        username: capture.username,
        url: capture.url,
      });
      pending = null;
      if (res.unchanged) {
        door.querySelector('[data-role="msg"]').textContent = t("ext_unchanged");
        door.querySelector('[data-role="actions"]').innerHTML = "";
        setTimeout(closeDoor, 1600);
        return;
      }
      door.querySelector('[data-role="msg"]').innerHTML = t("ext_updated", {
        path: `<strong>${escapeHtml(offer.path)}</strong>`,
      });
      door.querySelector('[data-role="actions"]').innerHTML = "";
      setTimeout(closeDoor, 1400);
    } catch (err) {
      if (err.status === 401) {
        btn.disabled = false;
        btn.textContent = t("ext_btn_update");
        await ensureUnlocked(body, showErr);
        return;
      }
      showErr(err.message);
      btn.disabled = false;
      btn.textContent = t("ext_btn_update");
    }
  }

  function collectFolders(nodes, out = []) {
    for (const n of nodes || []) {
      if (n.is_dir) {
        out.push({ path: n.path, name: n.name });
        if (n.children) collectFolders(n.children, out);
      }
    }
    return out;
  }

  async function runSaveFlow(capture, btn, showErr) {
    const body = door.querySelector('[data-role="body"]');
    capture = readDoorPassword(capture);
    btn.disabled = true;
    btn.textContent = t("ext_working");
    try {
      if (!(await ensureUnlocked(body, showErr))) {
        btn.disabled = false;
        btn.textContent = t("ext_btn_save");
        return;
      }
      const { tree } = await send("GET_TREE");
      const folders = collectFolders(tree);
      selectedFolder = folders[0]?.path || "";

      const msg = door.querySelector('[data-role="msg"]');
      msg.textContent = t("ext_choose_folder");
      const actions = door.querySelector('[data-role="actions"]');
      actions.innerHTML = "";

      const treeEl = document.createElement("div");
      treeEl.className = "tree";
      const nameInput = document.createElement("input");
      nameInput.type = "text";
      nameInput.placeholder = t("ext_secret_name");
      const safeName =
        capture.username.replace(/[^\w.@+-]+/g, "-").replace(/^-+|-+$/g, "") || "account";
      nameInput.value = safeName;

      const preview = document.createElement("p");
      preview.className = "path-preview";
      const updatePreview = () => {
        const name = nameInput.value.trim().replace(/^\/+|\/+$/g, "");
        const path = selectedFolder ? `${selectedFolder}/${name}` : name;
        preview.textContent = t("ext_path_preview", {
          path: path || t("ext_path_incomplete"),
        });
      };

      const rootBtn = document.createElement("button");
      rootBtn.type = "button";
      rootBtn.textContent = t("ext_vault_root");
      rootBtn.className = selectedFolder === "" ? "sel" : "";
      rootBtn.addEventListener("click", () => {
        selectedFolder = "";
        treeEl.querySelectorAll("button").forEach((b) => b.classList.remove("sel"));
        rootBtn.classList.add("sel");
        updatePreview();
      });
      treeEl.appendChild(rootBtn);
      for (const f of folders) {
        const b = document.createElement("button");
        b.type = "button";
        b.textContent = f.path;
        if (f.path === selectedFolder) b.classList.add("sel");
        b.addEventListener("click", () => {
          selectedFolder = f.path;
          treeEl.querySelectorAll("button").forEach((x) => x.classList.remove("sel"));
          b.classList.add("sel");
          updatePreview();
        });
        treeEl.appendChild(b);
      }

      nameInput.addEventListener("input", updatePreview);
      updatePreview();

      const confirm = document.createElement("button");
      confirm.type = "button";
      confirm.className = "primary";
      confirm.textContent = t("ext_save_to_vault");
      const cancel = document.createElement("button");
      cancel.type = "button";
      cancel.className = "ghost";
      cancel.textContent = t("ext_cancel");
      cancel.addEventListener("click", () => {
        pending = null;
        closeDoor();
      });

      confirm.addEventListener("click", async () => {
        const name = nameInput.value.trim().replace(/^\/+|\/+$/g, "");
        if (!name || name.includes("/")) {
          showErr(t("ext_name_invalid"));
          return;
        }
        const path = selectedFolder ? `${selectedFolder}/${name}` : name;
        capture = readDoorPassword(capture);
        confirm.disabled = true;
        confirm.textContent = t("ext_saving");
        try {
          await send("SAVE_SECRET", {
            path,
            password: capture.password,
            username: capture.username,
            url: capture.url,
          });
          pending = null;
          msg.innerHTML = t("ext_saved_path", {
            path: `<strong>${escapeHtml(path)}</strong>`,
          });
          treeEl.remove();
          nameInput.remove();
          preview.remove();
          actions.innerHTML = "";
          setTimeout(closeDoor, 1400);
        } catch (err) {
          showErr(err.message);
          confirm.disabled = false;
          confirm.textContent = t("ext_save_to_vault");
        }
      });

      body.insertBefore(treeEl, actions);
      body.insertBefore(nameInput, actions);
      body.insertBefore(preview, actions);
      actions.append(confirm, cancel);
      btn.remove();
    } catch (err) {
      showErr(err.message);
      btn.disabled = false;
      btn.textContent = t("ext_btn_save");
    }
  }

  // Also capture password fields outside forms when a button is clicked then navigates.
  document.addEventListener(
    "click",
    (e) => {
      const t = e.target;
      if (!(t instanceof Element)) return;
      const btn = t.closest("button, input[type=submit], [role=button]");
      if (!btn) return;
      if (btn.closest(".door, .menu, #talos-inline-root")) return;
      const data = captureFromScope(document);
      if (data) rememberCapture(data);
    },
    true
  );

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape") closeDoor();
  });

  restorePendingFromSession();
  // Same-document: if pending exists shortly after load from SPA, offer once.
  setTimeout(() => {
    if (pending) maybeOffer(pending);
  }, 400);
})();
