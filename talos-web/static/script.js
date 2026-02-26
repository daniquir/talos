const UI = {
    fileList: document.getElementById('file-list'),
    viewer: document.getElementById('viewer-content'),
    header: document.getElementById('viewer-header'),
    modal: document.getElementById('modal'),
    form: document.getElementById('encrypt-form'),
    ledStorage: document.getElementById('led-storage'),
    ledBunker: document.getElementById('led-bunker'),

    openModal: () => UI.modal.classList.remove('hidden'),
    closeModal: () => UI.modal.classList.add('hidden'),

    typewriter: (text, element) => {
        element.innerText = '';
        let i = 0;
        const type = () => {
            if (i < text.length) {
                element.innerText += text.charAt(i);
                i++;
                setTimeout(type, 5);
            }
        };
        type();
    }
};

const API = {
    async fetchFiles(path = "") {
        const res = await fetch(`/api/list/${path}`);
        const data = await res.json();
        UI.fileList.innerHTML = '';
        data.forEach(f => {
            const el = document.createElement('div');
            el.className = 'p-2 text-[11px] cursor-pointer hover:bg-green-500/10 border-l border-zinc-800 hover:border-green-500 transition-all flex items-center gap-2';
            el.innerHTML = `<i data-lucide="${f.is_dir ? 'folder' : 'key'}" class="w-3 h-3"></i> ${f.name}`;
            el.onclick = () => f.is_dir ? API.fetchFiles(f.name) : API.decrypt(f.name);
            UI.fileList.appendChild(el);
        });
        lucide.createIcons();
    },

    async decrypt(path) {
        const pass = prompt("AUTH_REQUIRED: GPG_PASSPHRASE");
        if (pass === null) return;
        
        UI.header.innerText = `DECRYPTING: ${path}...`;
        const res = await fetch('/api/decrypt', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({ path, passphrase: pass })
        });
        const data = await res.json();
        UI.typewriter(typeof data === 'string' ? data : JSON.stringify(data), UI.viewer);
    },

    async checkHealth() {
        try {
            const res = await fetch('/api/health');
            const status = await res.json();
            const updateLED = (el, ok) => {
                el.className = `w-1.5 h-1.5 rounded-full transition-all duration-500 ${ok ? 'bg-green-500 shadow-[0_0_8px_#22c55e]' : 'bg-red-500 shadow-[0_0_8px_#ef4444]'}`;
            };
            updateLED(UI.ledStorage, status.storage);
            updateLED(UI.ledBunker, status.bunker);
        } catch(e) {}
    }
};

window.UI = UI;
document.addEventListener('DOMContentLoaded', () => {
    lucide.createIcons();
    API.fetchFiles();
    setInterval(API.checkHealth, 5000);
    API.checkHealth();
    UI.form.onsubmit = async (e) => {
        e.preventDefault();
        await fetch('/api/save', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
                path: document.getElementById('entry-path').value,
                content: document.getElementById('entry-content').value,
                passphrase: document.getElementById('entry-pass').value
            })
        });
        UI.closeModal();
        API.fetchFiles();
    };
});