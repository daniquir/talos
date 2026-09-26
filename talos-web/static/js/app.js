import { API } from './api.js';
import { UI } from './ui.js';
import { applyDom, getLangPref, initI18n, setLangPref, t } from './i18n.js';

const App = {
    sessionTimeout: 900, // 15 minutes in seconds
    sessionTimer: null,

    async init() {
        await initI18n();
        const langSelect = document.getElementById('lang-select');
        if (langSelect) {
            langSelect.value = getLangPref();
            const onLangChange = async () => {
                await setLangPref(langSelect.value, { syncServer: true });
                const authText = document.getElementById('auth-text');
                if (authText && !document.getElementById('auth-indicator')?.classList.contains('hidden')) {
                    const method = authText.dataset.method;
                    if (method) UI.setAuthMethod(method);
                }
            };
            langSelect.addEventListener('change', onLangChange);
            langSelect.addEventListener('input', onLangChange);
            window.addEventListener('talos:lang', () => {
                langSelect.value = getLangPref();
            });
        }

        UI.init();
        // Initialize Lucide icons (ignoring TS warning for global library)
        // @ts-ignore
        lucide.createIcons();
        
        // Initial system check
        this.verifySystemStatus();
        this.fetchAndDisplayVersion();

        // Check Authentication Status (Genesis Protocol)
        await this.checkAuthStatus();

        // Initialize Event Listeners for UI interactions
        UI.elements.form.onsubmit = (e) => this.handleSave(e);
        document.getElementById('btn-new-secret').onclick = () => this.handleNewSecret();
        document.getElementById('btn-cancel-secret').onclick = () => UI.closeModal();
        document.getElementById('btn-backup').onclick = () => window.location.href = '/api/backup';
        document.getElementById('btn-restore').onclick = () => document.getElementById('file-restore').click();
        document.getElementById('file-restore').onchange = (e) => this.handleRestore(e);
        document.getElementById('btn-logout').onclick = async () => {
            await API.logout();
            window.location.reload();
        };

        // Audit Logs
        document.getElementById('btn-audit').onclick = async () => {
            try {
                const logs = await API.fetchAuditLogs(); // This now goes to /api/audit
                UI.renderAuditLogs(logs);
                UI.openAuditModal();
            } catch (e) { UI.showNotification(t('notif_fetch_logs_fail'), "error"); }
        };
        UI.elements.btnCloseAudit.onclick = () => UI.closeAuditModal();

        // Entry Generator Logic
        const runEntryGen = () => {
            const len = parseInt(UI.elements.entryGenLength.value);
            const upper = UI.elements.entryGenUpper.checked;
            const nums = UI.elements.entryGenNums.checked;
            const syms = UI.elements.entryGenSyms.checked;
            UI.elements.entrySecret.value = UI.generatePassword(len, upper, nums, syms);
            UI.elements.entrySecret.dataset.keepSecret = '';
            UI.elements.entrySecret.dataset.clearSecret = '';
            UI.elements.entrySecret.placeholder = t('ph_secret');
        };
        UI.elements.entryGenLength.oninput = (e) => { UI.elements.entryGenLenVal.innerText = e.target.value; runEntryGen(); };
        UI.elements.entryGenUpper.onchange = runEntryGen;
        UI.elements.entryGenNums.onchange = runEntryGen;
        UI.elements.entryGenSyms.onchange = runEntryGen;
        UI.elements.btnEntryGen.onclick = runEntryGen;

        document.getElementById('btn-new-category').onclick = () => this.handleNewCategory();
        UI.elements.btnReconnect.onclick = async () => {
            const healthy = await this.verifySystemStatus();
            if (healthy) this.loadFiles();
        };

        UI.elements.treeSearch.addEventListener('input', (e) => {
            const raw = e.target.value;
            const searchTerm = raw.trim();
            const tree = UI.elements.treeContainer.jstree(true);
            UI.elements.clearSearch.classList.toggle('hidden', !raw);
            if (!tree) return;
            // Empty query must clear the filter. Searching "" with show_only_matches
            // leaves the tree looking "full" in a confusing way.
            if (!searchTerm) {
                tree.clear_search();
                return;
            }
            tree.search(searchTerm);
        });

        UI.elements.clearSearch.addEventListener('click', () => {
            UI.elements.treeSearch.value = '';
            const tree = UI.elements.treeContainer.jstree(true);
            if (tree) tree.clear_search();
            UI.elements.clearSearch.classList.add('hidden');
        });

        // jsTree: zero matches + show_only_matches often redisplays the whole tree.
        // Force an empty view when the query is non-empty and nothing matched.
        UI.elements.treeContainer.on('search.jstree', (_e, data) => {
            const tree = UI.elements.treeContainer.jstree(true);
            if (!tree || !data?.str) return;
            const hits = data.res || data.nodes || [];
            if (hits.length === 0) {
                tree.hide_all();
            }
        });
        UI.elements.treeContainer.on('clear_search.jstree', () => {
            const tree = UI.elements.treeContainer.jstree(true);
            if (tree) tree.show_all();
        });

        // jsTree event listener for selection
        UI.elements.treeContainer.on('select_node.jstree', (e, data) => {
            if (!data.node.data.is_dir) {
                this.handleDecrypt(data.node.data.path);
            }
        });

        // Double click to copy password
        UI.elements.treeContainer.on('dblclick.jstree', (e) => {
            const instance = $.jstree.reference(e.target);
            const node = instance.get_node(e.target);
            if (node && !node.data.is_dir) {
                this.handleCopyPassword(node.data.path);
            }
        });

        window.addEventListener('talos:lang', () => applyDom());
    },

    async checkAuthStatus() {
        try {
            const status = await API.fetchAuthStatus();
            const params = new URLSearchParams(window.location.search);
            if (params.get("oidc_error")) {
                UI.showNotification(params.get("oidc_error"), "error");
            }
            // Per-user: after Keycloak, an empty vault must go through setup — not unlock.
            if (status.oidc_enabled && status.oidc_authenticated && !status.initialized) {
                this.initSetupMode(status);
            } else if (!status.initialized) {
                this.initSetupMode(status);
            } else if (!status.authenticated) {
                this.initLoginMode(status);
            } else {
                UI.setAuthMethod(status.auth_method);
                this.startSessionTimer();
                this.loadFiles();
            }
            if (params.get("need_vault_unlock") || params.get("oidc_error")) {
                window.history.replaceState({}, "", "/");
            }
        } catch (e) {
            console.error("Auth check failed", e);
        }
    },

    startSessionTimer() {
        const timerEl = document.getElementById('session-timer');
        const valEl = document.getElementById('timer-val');
        if(timerEl) timerEl.classList.remove('hidden');

        let timeLeft = this.sessionTimeout;

        const updateDisplay = () => {
            const m = Math.floor(timeLeft / 60).toString().padStart(2, '0');
            const s = (timeLeft % 60).toString().padStart(2, '0');
            if(valEl) valEl.innerText = `${m}:${s}`;
            
            if (timeLeft <= 0) {
                API.logout().then(() => window.location.reload());
            }
            timeLeft--;
        };

        const resetTimer = () => { timeLeft = this.sessionTimeout; };

        // Reset on activity
        window.addEventListener('mousemove', resetTimer);
        window.addEventListener('keydown', resetTimer);
        window.addEventListener('click', resetTimer);

        updateDisplay();
        this.sessionTimer = setInterval(updateDisplay, 1000);
    },

    initSetupMode(status = {}) {
        // Keep existing setup; when OIDC, require identity first for per-user init
        if (status.oidc_enabled && !status.oidc_authenticated) {
            this.initLoginMode(status);
            return;
        }
        UI.openSetupModal();

        const oidc = !!(status.oidc_enabled && status.oidc_authenticated);
        const lede = document.querySelector('#setup-modal [data-i18n="genesis_lede"]');
        const keyLabel = document.querySelector('#setup-modal [data-i18n="label_master_key"]');
        if (lede) lede.textContent = oidc ? t("genesis_lede_oidc") : t("genesis_lede");
        if (keyLabel) keyLabel.textContent = oidc ? t("label_vault_passphrase") : t("label_master_key");
        
        // Setup Generator Logic
        const runGen = () => {
            const len = parseInt(UI.elements.setupGenLength.value);
            const upper = UI.elements.setupGenUpper.checked;
            const nums = UI.elements.setupGenNums.checked;
            const syms = UI.elements.setupGenSyms.checked;
            UI.elements.setupKey.value = UI.generatePassword(len, upper, nums, syms);
        };
 
        // --- Auto-generation on parameter change ---
        UI.elements.setupGenLength.oninput = (e) => { 
            UI.elements.setupGenLenVal.innerText = e.target.value; 
            runGen();
        };
        UI.elements.setupGenUpper.onchange = runGen;
        UI.elements.setupGenNums.onchange = runGen;
        UI.elements.setupGenSyms.onchange = runGen;
        UI.elements.btnSetupGen.onclick = runGen;
        
        // Initial generation
        runGen();

        // --- Tab Switching ---
        UI.elements.tabGenerate.onclick = () => {
            UI.elements.setupForm.classList.remove('hidden');
            UI.elements.importForm.classList.add('hidden');
            UI.elements.tabGenerate.className = 'flex-1 py-2 text-xs uppercase tracking-wider border-b-2 border-green-500 text-white';
            UI.elements.tabImport.className = 'flex-1 py-2 text-xs uppercase tracking-wider border-b-2 border-transparent text-zinc-500 hover:text-white';
        };
        UI.elements.tabImport.onclick = () => {
            UI.elements.setupForm.classList.add('hidden');
            UI.elements.importForm.classList.remove('hidden');
            UI.elements.tabImport.className = 'flex-1 py-2 text-xs uppercase tracking-wider border-b-2 border-blue-500 text-white';
            UI.elements.tabGenerate.className = 'flex-1 py-2 text-xs uppercase tracking-wider border-b-2 border-transparent text-zinc-500 hover:text-white';
        };

        // --- Form Submissions ---
        UI.elements.setupForm.onsubmit = async (e) => {
            e.preventDefault();
            const btn = UI.elements.setupForm.querySelector('button[type="submit"]');
            const key = UI.elements.setupKey.value;
            
            // Disable UI to prevent double submission
            btn.disabled = true;
            btn.innerText = t("btn_initializing");

            // Auto-copy to clipboard
            try {
                await navigator.clipboard.writeText(key);
                UI.showNotification(t("notif_key_copied"), "success");
            } catch (c) { console.error(c); }

            try {
                await API.initializeSystem(key);
                UI.showNotification(t("notif_initialized"), "success");
                setTimeout(() => window.location.reload(), 2000);
            } catch (err) {
                UI.showNotification(t("notif_init_fail", { error: err.message }), "error");
                btn.disabled = false;
                btn.innerText = t("btn_initialize");
            }
        };

        UI.elements.importForm.onsubmit = async (e) => {
            e.preventDefault();
            const btn = UI.elements.importForm.querySelector('button[type="submit"]');
            const privateKey = UI.elements.importKey.value;
            const passphrase = UI.elements.importPassphrase.value;
            if (!privateKey) {
                UI.showNotification(t("notif_need_gpg"), "error");
                return;
            }
            btn.disabled = true;
            btn.innerText = t("btn_importing");
            try {
                await API.importSystem(privateKey, passphrase);
                UI.showNotification(t("notif_imported"), "success");
                setTimeout(() => window.location.reload(), 2000);
            } catch (err) {
                UI.showNotification(t("notif_import_fail", { error: err.message }), "error");
                btn.disabled = false;
                btn.innerText = t("btn_import_init");
            }
        };
    },

    initLoginMode(status = {}) {
        UI.openLoginModal();
        const oidcBtn = document.getElementById("btn-oidc-login");
        const oidcHint = document.getElementById("login-oidc-hint");
        const keyInput = UI.elements.loginKey;
        const oidcReady = !!(status.oidc_enabled && status.oidc_authenticated);
        if (keyInput) {
            keyInput.placeholder = oidcReady ? t("ph_vault_passphrase") : t("ph_master_key");
            keyInput.setAttribute("data-i18n-placeholder", oidcReady ? "ph_vault_passphrase" : "ph_master_key");
        }
        if (status.oidc_enabled) {
            if (oidcBtn) oidcBtn.classList.remove("hidden");
            if (!status.oidc_authenticated) {
                if (oidcHint) oidcHint.classList.add("hidden");
                if (keyInput) {
                    keyInput.classList.add("hidden");
                    keyInput.required = false;
                }
                const submit = UI.elements.loginForm?.querySelector('button[type="submit"]');
                if (submit) submit.classList.add("hidden");
            } else {
                if (oidcBtn) oidcBtn.classList.add("hidden");
                if (oidcHint) oidcHint.classList.remove("hidden");
                if (keyInput) {
                    keyInput.classList.remove("hidden");
                    keyInput.required = true;
                }
                const submit = UI.elements.loginForm?.querySelector('button[type="submit"]');
                if (submit) submit.classList.remove("hidden");
            }
        }
        UI.elements.loginForm.onsubmit = async (e) => {
            e.preventDefault();
            const btn = UI.elements.loginForm.querySelector('button[type="submit"]');
            if (btn?.disabled) return;

            const key = keyInput.value;
            if (!key) return;

            const label = btn ? btn.textContent : t("btn_unlock_vault");
            if (btn) {
                btn.disabled = true;
                btn.textContent = t("btn_unlocking");
            }
            keyInput.disabled = true;

            try {
                await API.login(key);
                window.location.href = "/";
            } catch (err) {
                UI.showNotification(t("notif_access_denied"), "error");
                keyInput.value = '';
                keyInput.disabled = false;
                if (btn) {
                    btn.disabled = false;
                    btn.textContent = label || t("btn_unlock_vault");
                }
                keyInput.focus();
            }
        };
    },

    // Wrapper to ensure system is healthy before any action
    async executeSafe(action) {
        const isHealthy = await this.verifySystemStatus();
        if (isHealthy) {
            await action();
        }
    },

    async verifySystemStatus() {
        const status = await API.checkHealth();
        UI.updateHealth(status);
        
        const isHealthy = status.storage && status.bunker;
        UI.setFreezeState(!isHealthy);
        
        return isHealthy;
    },

    async fetchAndDisplayVersion() {
        try {
            const data = await API.fetchVersion();
            const versionEl = document.getElementById('app-version');
            if (versionEl) {
                versionEl.innerText = `v${data.version}`;
            }
        } catch (e) {
            console.error("Failed to fetch version", e);
        }
    },

    async loadFiles() {
        try {
            const tree = await API.fetchTree();
            UI.renderTree(tree, (node) => this.getContextMenuItems(node));
        } catch (e) {
            console.warn("Tree load skipped:", e.message);
        }
    },

    getContextMenuItems(node) {
        const items = {};

        if (node.data.is_dir) {
            items.newSecret = {
                label: t("ctx_new_secret"),
                icon: "https://cdn.jsdelivr.net/npm/lucide-static@latest/icons/key.svg",
                action: () => {
                    UI.clearForm();
                    UI.elements.entryPath.value = node.data.path + '/';
                    UI.openModal({ focusPath: true });
                }
            };
            items.newCategory = {
                label: t("ctx_new_subcategory"),
                icon: "https://cdn.jsdelivr.net/npm/lucide-static@latest/icons/folder-plus.svg",
                action: async () => {
                    const name = await UI.prompt(t("prompt_subcategory"), {
                        title: t("dialog_new_subcategory"),
                        placeholder: t("prompt_subcategory"),
                    });
                    if (name) this.handleNewCategory(`${node.data.path}/${name}`);
                }
            };
            items.renameCategory = {
                label: t("ctx_rename"),
                icon: "https://cdn.jsdelivr.net/npm/lucide-static@latest/icons/pencil.svg",
                action: async () => {
                    const current = node.data.path.split('/').pop();
                    const parent = node.data.path.includes('/')
                        ? node.data.path.slice(0, node.data.path.lastIndexOf('/'))
                        : '';
                    const name = await UI.prompt(t("prompt_rename_category"), {
                        title: t("dialog_rename_category"),
                        defaultValue: current,
                        placeholder: t("prompt_rename_category"),
                    });
                    if (!name || name === current) return;
                    const newPath = parent ? `${parent}/${name}` : name;
                    this.handleRenameCategory(node.data.path, newPath);
                }
            };
        } else { // It's a file
            items.editSecret = {
                label: t("ctx_edit"),
                icon: "https://cdn.jsdelivr.net/npm/lucide-static@latest/icons/file-edit.svg",
                action: async () => {
                    const content = await API.decrypt(node.data.path);
                    UI.elements.entryPath.value = node.data.path;
                    UI.elements.entryOriginalPath.value = node.data.path;
                    UI.parseContentToForm(content);
                    UI.openModal({ focusPath: true });
                }
            };
        }

        items.delete = {
            label: t("ctx_delete"),
            icon: "https://cdn.jsdelivr.net/npm/lucide-static@latest/icons/trash-2.svg",
            action: () => this.executeSafe(async () => {
                const ok = await UI.confirm(t("confirm_delete", { path: node.data.path }));
                if (ok) this.handleDelete(node.data.path);
            })
        };
        return items;
    },

    async handleDecrypt(path) {
        this.executeSafe(async () => {
            try {
                UI.setDecryptingStatus(path);
                const data = await API.decrypt(path);
                const content = typeof data === 'string' ? data : JSON.stringify(data);
                UI.renderSecretView(path, content);
                UI.elements.header.innerText = t("status_open", { path });
            } catch (err) {
                UI.showNotification(t("notif_error", { error: err.message }), "error");
                UI.elements.header.innerText = t("status_error");
            }
        });
    },

    async handleCopyPassword(path) {
        this.executeSafe(async () => {
            try {
                const fullData = await API.decrypt(path, true);
                const password = fullData.split('\n')[0];
                if (password) {
                    await navigator.clipboard.writeText(password);
                    const originalText = UI.elements.header.innerText;
                    UI.elements.header.innerText = t("status_copied", { path });
                    UI.elements.header.classList.remove('text-zinc-600');
                    UI.elements.header.classList.add('text-green-400');
                    setTimeout(() => {
                        UI.elements.header.innerText = originalText;
                        UI.elements.header.classList.remove('text-green-400');
                        UI.elements.header.classList.add('text-zinc-600');
                    }, 2000);
                }
            } catch (err) {
                console.error(err);
                UI.showNotification(t("notif_copy_fail", { error: err.message }), "error");
            }
        });
    },

    async handleSave(e) {
        e.preventDefault();
        const btn = UI.elements.btnExecuteSecret
            || UI.elements.form.querySelector('button[type="submit"]');
        if (btn?.disabled) return;

        const prevLabel = btn ? btn.innerText : t("btn_execute");
        if (btn) {
            btn.disabled = true;
            btn.innerText = t("btn_saving");
        }

        this.executeSafe(async () => {
            try {
                const { path, content, original_path } = UI.getFormData();
                if (!path || path.endsWith('/')) {
                    UI.showNotification(t("notif_need_name"), "error");
                    return;
                }

                await API.save(path, content, original_path);
                UI.closeModal();
                this.loadFiles();
            } catch (err) {
                UI.showNotification(t("notif_save_fail", { error: err.message }), "error");
            } finally {
                if (btn && !UI.elements.modal.classList.contains('hidden')) {
                    btn.disabled = false;
                    btn.innerText = prevLabel;
                }
            }
        });
    },

    async handleDelete(path) {
        try {
            await API.delete(path);
            UI.elements.header.innerText = t('idle_system');
            UI.elements.viewer.innerText = '';
            this.loadFiles();
        } catch (err) {
            UI.showNotification(t("notif_delete_fail", { error: err.message }), "error");
        }
    },

    async handleRenameCategory(oldPath, newPath) {
        this.executeSafe(async () => {
            try {
                await API.renameCategory(newPath, oldPath);
                this.loadFiles();
            } catch (err) {
                UI.showNotification(t("notif_rename_fail", { error: err.message }), "error");
            }
        });
    },

    async handleRestore(e) {
        const file = e.target.files[0];
        if (!file) return;
        
        this.executeSafe(async () => {
            const ok = await UI.confirm(t("confirm_restore"));
            if (ok) {
                try {
                    await API.restore(file);
                    UI.showNotification(t("notif_restored"), "success");
                    this.loadFiles();
                } catch (err) {
                    UI.showNotification(t("notif_restore_fail", { error: err.message }), "error");
                }
            }
            e.target.value = ''; // reset input
        });
    },

    /** Folder path of the selected tree node (or parent of a selected secret). */
    getSelectedFolderPath() {
        const tree = UI.elements.treeContainer.jstree(true);
        if (!tree) return '';
        const selected = tree.get_selected(true);
        if (!selected.length) return '';
        const node = selected[0];
        if (node.data?.is_dir) return node.data.path || '';
        const path = node.data?.path || '';
        const idx = path.lastIndexOf('/');
        return idx > 0 ? path.slice(0, idx) : '';
    },

    handleNewSecret() {
        UI.clearForm();
        const folder = this.getSelectedFolderPath();
        if (folder) UI.elements.entryPath.value = folder + '/';
        UI.openModal({ focusPath: true });
    },

    async handleNewCategory(path) {
        this.executeSafe(async () => {
            let finalPath = path;
            if (!finalPath) {
                const folder = this.getSelectedFolderPath();
                const name = await UI.prompt(
                    folder ? t("prompt_subcategory") : t("prompt_root_category"),
                    {
                        title: folder ? t("dialog_new_subcategory") : t("dialog_new_category"),
                        placeholder: folder ? t("prompt_subcategory") : t("prompt_root_category"),
                    }
                );
                if (!name) return;
                finalPath = folder ? `${folder}/${name}` : name;
            }
            try {
                await API.createCategory(finalPath);
                this.loadFiles();
            } catch (err) {
                UI.showNotification(t("notif_category_fail", { error: err.message }), "error");
            }
        });
    }
};

// Start the application when DOM is ready
document.addEventListener('DOMContentLoaded', () => App.init());
