import { API } from './api.js';
import { UI } from './ui.js';

const App = {
    async init() {
        UI.init();
        // Initialize Lucide icons (ignoring TS warning for global library)
        // @ts-ignore
        lucide.createIcons();
        
        // Initial system check
        this.verifySystemStatus();
        this.fetchAndDisplayVersion();

        // Initialize Event Listeners for UI interactions
        UI.elements.form.onsubmit = (e) => this.handleSave(e);
        document.getElementById('btn-new-secret').onclick = () => this.handleNewSecret();
        document.getElementById('btn-cancel-secret').onclick = () => UI.closeModal();
        document.getElementById('btn-backup').onclick = () => window.location.href = '/api/backup';
        document.getElementById('btn-restore').onclick = () => document.getElementById('file-restore').click();
        document.getElementById('file-restore').onchange = (e) => this.handleRestore(e);
        document.getElementById('btn-gen-pass').onclick = () => UI.generatePassword();
        document.getElementById('btn-new-category').onclick = () => this.handleNewCategory();
        UI.elements.btnReconnect.onclick = () => this.verifySystemStatus();

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
        
        if (isHealthy && UI.elements.treeContainer.is(':empty')) {
             this.loadFiles();
        }
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
        const tree = await API.fetchTree();
        UI.renderTree(tree, (node) => this.getContextMenuItems(node));
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
                alert("ERROR: " + err.message);
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
                alert("Failed to copy password: " + err.message);
            }
        });
    },

    async handleSave(e) {
        e.preventDefault();
        this.executeSafe(async () => {
            try {
                const { path, content, original_path } = UI.getFormData();
                if (!path || path.endsWith('/')) {
                    alert("ERROR: A name for the secret is required.");
                    return;
                }

                await API.save(path, content, original_path);
                UI.closeModal();
                this.loadFiles();
            } catch (err) {
                alert("ERROR SAVING: " + err.message);
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
            alert("ERROR DELETING: " + err.message);
        }
    },

    async handleRestore(e) {
        const file = e.target.files[0];
        if (!file) return;
        
        this.executeSafe(async () => {
            if (confirm("WARNING: This will overwrite existing secrets. Continue?")) {
                try {
                    await API.restore(file);
                    alert("Restored successfully!");
                    this.loadFiles();
                } catch (err) {
                    alert("ERROR RESTORING: " + err.message);
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
                    alert("ERROR CREATING CATEGORY: " + err.message);
                }
            }
        });
    }
};

// Start the application when DOM is ready
document.addEventListener('DOMContentLoaded', () => App.init());