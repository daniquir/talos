export const API = {
    async fetchTree() {
        const res = await fetch(`/api/tree`);
        if (!res.ok) throw new Error(res.statusText);
        return await res.json();
    },

    async decrypt(path, reveal = false) {
        const res = await fetch('/api/decrypt', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ path, reveal })
        });
        if (!res.ok) {
            const err = await res.json();
            throw new Error(typeof err === 'string' ? err : (err.error || 'Decryption failed'));
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
            const err = await res.json();
            throw new Error(err.error || 'Save failed');
        }
    },

    async delete(path) {
        const res = await fetch('/api/delete', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ path })
        });
        if (!res.ok) {
            const err = await res.json();
            throw new Error(err.error || 'Delete failed');
        }
    },

    async createCategory(path) {
        const res = await fetch('/api/create_category', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ path })
        });
        if (!res.ok) {
            const err = await res.json();
            throw new Error(err.error || 'Create category failed');
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
            const err = await res.json();
            throw new Error(err.error || 'Restore failed');
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
            const err = await res.json();
            throw new Error(err.error || 'Initialization failed');
        }
    },

    async importSystem(privateKey, passphrase) {
        const res = await fetch('/api/initialize/import', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ key: privateKey, passphrase })
        });
        if (!res.ok) {
            const err = await res.json();
            throw new Error(err.error || 'Import failed');
        }
    },

    async login(masterKey) {
        const res = await fetch('/api/auth/login', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ key: masterKey })
        });
        if (!res.ok) {
            const err = await res.json();
            throw new Error(err.error || 'Login failed');
        }
    },

    async logout() {
        await fetch('/api/auth/logout', {
            method: 'POST'
        });
    }
};