import { API } from './api.js';
import { UI } from './ui.js';

const App = {
    sessionTimeout: 900, // 15 minutes in seconds
    sessionTimer: null,

    async init() {
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
            } catch (e) { UI.showNotification("Failed to fetch logs", "error"); }
        };
        UI.elements.btnCloseAudit.onclick = () => UI.closeAuditModal();
        
        // Settings & WebAuthn
        document.getElementById('btn-settings').onclick = () => UI.openSettingsModal();
        UI.elements.btnCloseSettings.onclick = () => UI.closeSettingsModal();

        // Entry Generator Logic
        const runEntryGen = () => {
            const len = parseInt(UI.elements.entryGenLength.value);
            const upper = UI.elements.entryGenUpper.checked;
            const nums = UI.elements.entryGenNums.checked;
            const syms = UI.elements.entryGenSyms.checked;
            UI.elements.entrySecret.value = UI.generatePassword(len, upper, nums, syms);
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
            const searchTerm = e.target.value;
            UI.elements.treeContainer.jstree(true).search(searchTerm);
            UI.elements.clearSearch.classList.toggle('hidden', !searchTerm);
        });

        UI.elements.clearSearch.addEventListener('click', () => {
            UI.elements.treeSearch.value = '';
            UI.elements.treeContainer.jstree(true).clear_search();
            UI.elements.clearSearch.classList.add('hidden');
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
    },

    async checkAuthStatus() {
        try {
            const status = await API.fetchAuthStatus();
            if (!status.initialized) {
                this.initSetupMode();
            } else if (!status.authenticated) {
                this.initLoginMode();
            } else {
                // Authenticated: Show method
                UI.setAuthMethod(status.auth_method);
                this.startSessionTimer();
                this.loadFiles();
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

    initSetupMode() {
        UI.openSetupModal();
        
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
            btn.innerText = "INITIALIZING...";

            // Auto-copy to clipboard
            try {
                await navigator.clipboard.writeText(key);
                UI.showNotification("KEY COPIED TO CLIPBOARD", "success");
            } catch (c) { console.error(c); }

            try {
                await API.initializeSystem(key);
                UI.showNotification("SYSTEM INITIALIZED. RELOADING...", "success");
                setTimeout(() => window.location.reload(), 2000);
            } catch (err) {
                UI.showNotification("INITIALIZATION FAILED: " + err.message, "error");
                btn.disabled = false;
                btn.innerText = "INITIALIZE SYSTEM";
            }
        };

        UI.elements.importForm.onsubmit = async (e) => {
            e.preventDefault();
            const btn = UI.elements.importForm.querySelector('button[type="submit"]');
            const privateKey = UI.elements.importKey.value;
            const passphrase = UI.elements.importPassphrase.value;
            if (!privateKey) {
                UI.showNotification("Please provide the GPG private key.", "error");
                return;
            }
            btn.disabled = true;
            btn.innerText = "IMPORTING...";
            try {
                await API.importSystem(privateKey, passphrase);
                UI.showNotification("SYSTEM IMPORTED. RELOADING...", "success");
                setTimeout(() => window.location.reload(), 2000);
            } catch (err) {
                UI.showNotification("IMPORT FAILED: " + err.message, "error");
                btn.disabled = false;
                btn.innerText = "IMPORT KEY & INITIALIZE";
            }
        };
    },

    initLoginMode() {
        UI.openLoginModal();
        UI.elements.loginForm.onsubmit = async (e) => {
            e.preventDefault();
            const key = UI.elements.loginKey.value;
            try {
                await API.login(key);
                window.location.reload();
            } catch (err) {
                UI.showNotification("ACCESS DENIED", "error");
                UI.elements.loginKey.value = '';
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
                label: "New Secret",
                icon: "https://cdn.jsdelivr.net/npm/lucide-static@latest/icons/key.svg",
                action: () => {
                    UI.clearForm();
                    UI.elements.entryPath.value = node.data.path + '/';
                    UI.openModal();
                }
            };
            items.newCategory = {
                label: "New Sub-category",
                icon: "https://cdn.jsdelivr.net/npm/lucide-static@latest/icons/folder-plus.svg",
                action: () => {
                    const name = prompt("Enter new sub-category name:");
                    if (name) this.handleNewCategory(`${node.data.path}/${name}`);
                }
            };
        } else { // It's a file
            items.editSecret = {
                label: "Edit",
                icon: "https://cdn.jsdelivr.net/npm/lucide-static@latest/icons/file-edit.svg",
                action: async () => {
                    const content = await API.decrypt(node.data.path);
                    UI.elements.entryPath.value = node.data.path;
                    UI.elements.entryOriginalPath.value = node.data.path;
                    UI.parseContentToForm(content);
                    UI.openModal();
                }
            };
        }

        items.delete = {
            label: "Delete",
            icon: "https://cdn.jsdelivr.net/npm/lucide-static@latest/icons/trash-2.svg",
            action: () => this.executeSafe(() => {
                if (confirm(`DELETE ${node.data.path} PERMANENTLY?`)) {
                    this.handleDelete(node.data.path);
                }
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
                UI.elements.header.innerText = `OPEN: ${path}`;
            } catch (err) {
                UI.showNotification("ERROR: " + err.message, "error");
                UI.elements.header.innerText = "ERROR";
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
                    UI.elements.header.innerText = `COPIED: ${path}`;
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
                UI.showNotification("Failed to copy password: " + err.message, "error");
            }
        });
    },

    async handleSave(e) {
        e.preventDefault();
        this.executeSafe(async () => {
            try {
                const { path, content, original_path } = UI.getFormData();
                if (!path || path.endsWith('/')) {
                    UI.showNotification("ERROR: A name for the secret is required.", "error");
                    return;
                }

                await API.save(path, content, original_path);
                UI.closeModal();
                this.loadFiles();
            } catch (err) {
                UI.showNotification("ERROR SAVING: " + err.message, "error");
            }
        });
    },

    async handleDelete(path) {
        try {
            await API.delete(path);
            UI.elements.header.innerText = 'IDLE_SYSTEM';
            UI.elements.viewer.innerText = '';
            this.loadFiles();
        } catch (err) {
            UI.showNotification("ERROR DELETING: " + err.message, "error");
        }
    },

    async handleRestore(e) {
        const file = e.target.files[0];
        if (!file) return;
        
        this.executeSafe(async () => {
            if (confirm("WARNING: This will overwrite existing secrets. Continue?")) {
                try {
                    await API.restore(file);
                    UI.showNotification("Restored successfully!", "success");
                    this.loadFiles();
                } catch (err) {
                    UI.showNotification("ERROR RESTORING: " + err.message, "error");
                }
            }
            e.target.value = ''; // reset input
        });
    },

    handleNewSecret() {
        UI.clearForm();
        UI.openModal();
    },

    async handleNewCategory(path) {
        this.executeSafe(async () => {
            const finalPath = path || prompt("Enter new root category name (e.g., 'Work' or 'Social')");
            if (finalPath) {
                try {
                    await API.createCategory(finalPath);
                    this.loadFiles();
                } catch (err) {
                    UI.showNotification("ERROR CREATING CATEGORY: " + err.message, "error");
                }
            }
        });
    }
};

// Start the application when DOM is ready
document.addEventListener('DOMContentLoaded', () => App.init());