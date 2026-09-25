export const API = {
    async _errorMessage(res, fallback) {
        const text = await res.text().catch(() => '');
        if (!text) {
            if (res.status === 401) return 'Session expired — unlock again';
            if (res.status === 403) return 'Forbidden';
            if (res.status === 502 || res.status === 503) return 'Storage/Bunker unavailable';
            return fallback || res.statusText || `HTTP ${res.status}`;
        }
        try {
            const err = JSON.parse(text);
            if (typeof err === 'string') return err;
            return err.error || err.message || fallback || `HTTP ${res.status}`;
        } catch (_) {
            return text.slice(0, 200) || fallback || `HTTP ${res.status}`;
        }
    },

    async fetchTree() {
        const res = await fetch(`/api/tree`);
        if (!res.ok) throw new Error(await this._errorMessage(res, res.statusText));
        return await res.json();
    },

    async decrypt(path, reveal = false) {
        const res = await fetch('/api/decrypt', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ path, reveal })
        });
        if (!res.ok) {
            throw new Error(await this._errorMessage(res, 'Decryption failed'));
        }
        return await res.json();
    },

    async save(path, content, original_path) {
        const res = await fetch('/api/save', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ path, content, original_path })
        });
        if (!res.ok) {
            throw new Error(await this._errorMessage(res, 'Save failed'));
        }
    },

    async delete(path) {
        const res = await fetch('/api/delete', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ path })
        });
        if (!res.ok) {
            throw new Error(await this._errorMessage(res, 'Delete failed'));
        }
    },

    async createCategory(path) {
        const res = await fetch('/api/create_category', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ path })
        });
        if (!res.ok) {
            throw new Error(await this._errorMessage(res, 'Create category failed'));
        }
    },

    async renameCategory(path, original_path) {
        const res = await fetch('/api/rename_category', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ path, original_path })
        });
        if (!res.ok) {
            throw new Error(await this._errorMessage(res, 'Rename category failed'));
        }
    },

    async restore(file) {
        const formData = new FormData();
        formData.append('backup', file);
        
        const res = await fetch('/api/restore', {
            method: 'POST',
            body: formData
        });
        if (!res.ok) {
            throw new Error(await this._errorMessage(res, 'Restore failed'));
        }
    },

    async checkHealth() {
        try {
            const res = await fetch('/api/health');
            return await res.json();
        } catch(e) {
            return { storage: false, bunker: false };
        }
    },

    async fetchVersion() {
        const res = await fetch('/api/version');
        return await res.json();
    },

    async fetchAuthStatus() {
        const res = await fetch('/api/auth/status');
        return await res.json();
    },

    async fetchSettings() {
        const res = await fetch('/api/settings');
        if (!res.ok) throw new Error(await this._errorMessage(res, res.statusText));
        return await res.json();
    },

    async updateSettings(patch) {
        const res = await fetch('/api/settings', {
            method: 'PUT',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(patch || {})
        });
        if (!res.ok) {
            throw new Error(await this._errorMessage(res, 'Settings update failed'));
        }
        return await res.json();
    },

    async fetchAuditLogs() {
        const res = await fetch('/api/audit');
        return await res.json();
    },

    async initializeSystem(masterKey) {
        const res = await fetch('/api/initialize', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ key: masterKey })
        });
        if (!res.ok) {
            throw new Error(await this._errorMessage(res, 'Initialization failed'));
        }
    },

    async importSystem(privateKey, passphrase) {
        const res = await fetch('/api/initialize/import', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ key: privateKey, passphrase })
        });
        if (!res.ok) {
            throw new Error(await this._errorMessage(res, 'Import failed'));
        }
    },

    async login(masterKey) {
        const res = await fetch('/api/auth/login', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ key: masterKey })
        });
        if (!res.ok) {
            throw new Error(await this._errorMessage(res, 'Login failed'));
        }
    },

    async logout() {
        await fetch('/api/auth/logout', {
            method: 'POST'
        });
    }
};
