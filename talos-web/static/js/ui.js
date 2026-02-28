import { API } from './api.js';

export const UI = {
    elements: {},

    init() {
        this.elements = {
            treeContainer: $('#tree-container'), // Use jQuery selector for jstree
            header: document.getElementById('viewer-header'),
            treeSearch: document.getElementById('tree-search'),
            clearSearch: document.getElementById('clear-search'),
            viewer: document.getElementById('viewer-content'),
            modal: document.getElementById('modal'),
            form: document.getElementById('encrypt-form'),
            statusStorage: document.getElementById('status-storage'),
            statusBunker: document.getElementById('status-bunker'),
            // Auth Indicator
            authIndicator: document.getElementById('auth-indicator'),
            authIcon: document.getElementById('auth-icon'),
            authText: document.getElementById('auth-text'),
            entryPath: document.getElementById('entry-path'),
            entryOriginalPath: document.getElementById('entry-original-path'),
            entryUser: document.getElementById('entry-user'),
            entrySecret: document.getElementById('entry-secret'),
            entryUrl: document.getElementById('entry-url'),
            entryDesc: document.getElementById('entry-desc'),
            btnEntryGen: document.getElementById('btn-entry-gen'),
            entryGenLength: document.getElementById('entry-gen-length'),
            entryGenLenVal: document.getElementById('entry-gen-len-val'),
            entryGenUpper: document.getElementById('entry-gen-upper'),
            entryGenNums: document.getElementById('entry-gen-nums'),
            entryGenSyms: document.getElementById('entry-gen-syms'),
            systemFreeze: document.getElementById('system-freeze'),
            btnReconnect: document.getElementById('btn-reconnect'),
            // Setup Elements
            setupModal: document.getElementById('setup-modal'),
            setupForm: document.getElementById('setup-form'),
            setupKey: document.getElementById('setup-key'),
            btnSetupGen: document.getElementById('btn-setup-gen'),
            setupGenLength: document.getElementById('setup-gen-length'),
            setupGenLenVal: document.getElementById('setup-gen-len-val'),
            setupGenUpper: document.getElementById('setup-gen-upper'),
            setupGenNums: document.getElementById('setup-gen-nums'),
            setupGenSyms: document.getElementById('setup-gen-syms'),
            importForm: document.getElementById('import-form'),
            importKey: document.getElementById('import-key'),
            importPassphrase: document.getElementById('import-passphrase'),
            tabGenerate: document.getElementById('tab-generate'),
            tabImport: document.getElementById('tab-import'),
            // Settings Elements
            settingsModal: document.getElementById('settings-modal'),
            btnCloseSettings: document.getElementById('btn-close-settings'),
            // Audit Elements
            auditModal: document.getElementById('audit-modal'),
            auditTableBody: document.getElementById('audit-table-body'),
            btnCloseAudit: document.getElementById('btn-close-audit'),
            // Login Elements
            loginModal: document.getElementById('login-modal'),
            loginForm: document.getElementById('login-form'),
            loginKey: document.getElementById('login-key'),
            notificationArea: document.getElementById('notification-area'),
        };
    },

    openModal() { this.elements.modal.classList.remove('hidden'); },
    closeModal() { this.elements.modal.classList.add('hidden'); },

    openSetupModal() { this.elements.setupModal.classList.remove('hidden'); },
    closeSetupModal() { this.elements.setupModal.classList.add('hidden'); },

    openSettingsModal() { this.elements.settingsModal.classList.remove('hidden'); },
    closeSettingsModal() { this.elements.settingsModal.classList.add('hidden'); },

    openAuditModal() { this.elements.auditModal.classList.remove('hidden'); },
    closeAuditModal() { this.elements.auditModal.classList.add('hidden'); },

    openLoginModal() { this.elements.loginModal.classList.remove('hidden'); },
    closeLoginModal() { this.elements.loginModal.classList.add('hidden'); },

    showNotification(message, type = 'info') {
        const notif = document.createElement('div');
        let colors = 'border-zinc-500 text-zinc-300 shadow-[0_0_10px_rgba(113,113,122,0.3)]';
        if (type === 'success') colors = 'border-green-500 text-green-500 shadow-[0_0_10px_rgba(34,197,94,0.3)]';
        if (type === 'error') colors = 'border-red-500 text-red-500 shadow-[0_0_10px_rgba(239,68,68,0.3)]';

        notif.className = `bg-black border p-4 text-xs font-mono uppercase tracking-wider transition-all duration-500 transform translate-x-full opacity-0 ${colors} pointer-events-auto`;
        notif.innerText = message;

        this.elements.notificationArea.appendChild(notif);

        // Animate in
        requestAnimationFrame(() => {
            notif.classList.remove('translate-x-full', 'opacity-0');
        });

        // Remove after 3s
        setTimeout(() => {
            notif.classList.add('translate-x-full', 'opacity-0');
            setTimeout(() => notif.remove(), 500);
        }, 3000);
    },

    updateHealth(status) {
        const updateIndicator = (el, ok) => {
            el.className = `w-3 h-3 rounded-full transition-all duration-500 ${ok ? 'bg-green-500 shadow-[0_0_15px_#22c55e]' : 'bg-red-500 shadow-[0_0_15px_#ef4444]'}`;
        };
        updateIndicator(this.elements.statusStorage, status.storage);
        updateIndicator(this.elements.statusBunker, status.bunker);
    },

    setAuthMethod(method) {
        this.elements.authIndicator.classList.remove('hidden');
        if (method === 'mtls') {
            this.elements.authIcon.setAttribute('data-lucide', 'award');
            this.elements.authIcon.classList.replace('text-zinc-500', 'text-yellow-500');
            this.elements.authText.innerText = 'DIPLOMATIC';
            this.elements.authText.classList.replace('text-zinc-500', 'text-yellow-500');
        } else {
            this.elements.authIcon.setAttribute('data-lucide', 'key');
            this.elements.authIcon.classList.replace('text-green-500', 'text-zinc-500');
            this.elements.authText.innerText = 'MASTER KEY';
            this.elements.authText.classList.replace('text-green-500', 'text-zinc-500');
        }
        // @ts-ignore
        lucide.createIcons();
    },

    setFreezeState(frozen) {
        if (frozen) {
            this.elements.systemFreeze.classList.remove('hidden');
        } else {
            this.elements.systemFreeze.classList.add('hidden');
        }
    },

    transformDataForJsTree(nodes) {
        return nodes.map(node => ({
            text: node.name,
            icon: node.is_dir ? 'jstree-folder' : 'jstree-file',
            children: node.children ? this.transformDataForJsTree(node.children) : [],
            data: {
                path: node.path,
                is_dir: node.is_dir
            }
        }));
    },

    renderTree(nodes, contextMenuItems) {
        const treeData = this.transformDataForJsTree(nodes);
        
        const instance = this.elements.treeContainer.jstree(true);
        if (instance) {
            // If tree exists, update data and refresh
            instance.settings.core.data = treeData;
            instance.refresh();
        } else {
            // If tree doesn't exist, create it for the first time
            this.elements.treeContainer.jstree({
                'core': {
                    'data': treeData,
                    'check_callback': true,
                    'themes': { 'name': 'default-dark', 'responsive': true }
                },
                'plugins': ['contextmenu', 'search'],
                'contextmenu': { 'items': contextMenuItems },
                'search': {
                    'case_insensitive': true,
                    'show_only_matches': true,
                }
            });
        }
    },
    
    setDecryptingStatus(path) {
        this.elements.header.innerText = `DECRYPTING: ${path}...`;
    },
    
    parseContentToForm(text) {
        const lines = text.split('\n');
        // If secret is hidden, show empty or placeholder, but DO NOT fill the value with the marker
        if (lines[0] === '__TALOS_HIDDEN_SECRET__') {
            this.elements.entrySecret.value = '';
            this.elements.entrySecret.placeholder = '(Unchanged) Leave empty to keep current password';
        } else {
            this.elements.entrySecret.value = lines[0] || '';
        }
        
        // Reset other fields
        this.elements.entryUser.value = '';
        this.elements.entryUrl.value = '';
        this.elements.entryDesc.value = '';

        let descAccumulator = [];
        
        for (let i = 1; i < lines.length; i++) {
            const line = lines[i];
            if (line.startsWith('User: ')) this.elements.entryUser.value = line.substring(6);
            else if (line.startsWith('URL: ')) this.elements.entryUrl.value = line.substring(5);
            else descAccumulator.push(line);
        }
        this.elements.entryDesc.value = descAccumulator.join('\n').trim();
    },

    getFormData() {
        const pass = this.elements.entrySecret.value;
        const user = this.elements.entryUser.value;
        const url = this.elements.entryUrl.value;
        const desc = this.elements.entryDesc.value;
        
        // If pass is empty and we are editing (original_path exists), we might need to handle "keep existing" logic
        // But since the backend expects the full content to overwrite, 
        // for this specific requirement "never obtain value for editing", 
        // we effectively force the user to re-enter the password if they want to change anything, OR we need a way to tell backend "keep pass".
        // However, standard pass/gpg overwrites the file. 
        // To support "keep password" without revealing it to frontend, the backend would need a specific "patch" endpoint.
        // For now, based on "never obtain value", if the user leaves it empty, we assume they want to set it to empty? 
        // OR, we can implement a "merge" in backend. 
        // Let's assume for high security: if you edit, you must re-enter or generate a new password if you want to change it.
        // BUT, to keep the old password without seeing it, we need to fetch it in backend.
        // Let's implement a special marker.
        
        let content = pass;
        if (pass === '' && this.elements.entrySecret.placeholder.includes('(Unchanged)')) {
             content = '__TALOS_KEEP_SECRET__';
        }

        if (user) content += `\nUser: ${user}`;
        if (url) content += `\nURL: ${url}`;
        if (desc) content += `\n${desc}`;

        return {
            path: this.elements.entryPath.value,
            original_path: this.elements.entryOriginalPath.value || null,
            content: content
        };
    },

    generatePassword(length = 24, useUpper = true, useNumbers = true, useSymbols = true) {
        const lower = "abcdefghijklmnopqrstuvwxyz";
        const upper = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
        const nums = "0123456789";
        const syms = "!@#$%^&*()_+~`|}{[]:;?><,./-=";

        let chars = lower;
        if (useUpper) chars += upper;
        if (useNumbers) chars += nums;
        if (useSymbols) chars += syms;

        // Fallback if nothing selected
        if (chars === "") chars = lower;

        return Array.from({length}, () => chars[Math.floor(Math.random() * chars.length)]).join('');
    },

    clearForm() {
        this.elements.form.reset();
        this.elements.entryOriginalPath.value = '';
    },

    renderSecretView(path, text) {
        const viewer = this.elements.viewer;
        viewer.innerHTML = ''; // Clear previous content

        // 1. Parse data
        const lines = text.split('\n');
        const name = path.split('/').pop();
        let user = '', url = '', description = [];

        for (let i = 1; i < lines.length; i++) {
            const line = lines[i];
            if (line.startsWith('User: ')) {
                user = line.substring(6);
            } else if (line.startsWith('URL: ')) {
                url = line.substring(5);
            } else {
                description.push(line);
            }
        }
        const descText = description.join('\n').trim();

        // 2. Create and append title and description
        const titleEl = document.createElement('h2');
        titleEl.className = 'text-2xl font-bold text-green-400 mb-1 tracking-wider';
        titleEl.innerText = name;

        const descEl = document.createElement('p');
        descEl.className = 'text-sm text-zinc-400 mb-8 italic';
        descEl.innerText = descText || 'No description.';

        viewer.appendChild(titleEl);
        viewer.appendChild(descEl);

        // 3. Create and append field rows
        const fieldsContainer = document.createElement('div');
        fieldsContainer.className = 'space-y-6';

        const createMetadataRow = (label, value) => {
            if (!value) return null;

            const row = document.createElement('div');
            row.className = 'group flex items-center gap-4';
            
            const labelEl = document.createElement('span');
            labelEl.className = 'w-20 text-zinc-500 text-xs uppercase tracking-widest';
            labelEl.innerText = label;

            const valueEl = document.createElement('span');
            valueEl.className = 'flex-1 text-zinc-300';
            valueEl.innerText = value;

            const copyBtn = document.createElement('button');
            copyBtn.innerHTML = '<i data-lucide="copy" class="w-4 h-4"></i>';
            copyBtn.className = 'text-zinc-600 hover:text-white transition-all opacity-0 group-hover:opacity-100';
            copyBtn.onclick = () => {
                navigator.clipboard.writeText(value);
                copyBtn.innerHTML = '<i data-lucide="check" class="w-4 h-4 text-green-500"></i>';
                setTimeout(() => { copyBtn.innerHTML = '<i data-lucide="copy" class="w-4 h-4"></i>'; }, 2000);
            };

            row.appendChild(labelEl);
            row.appendChild(valueEl);
            row.appendChild(copyBtn);
            return row;
        };

        const passwordRow = document.createElement('div');
        passwordRow.className = 'group flex items-center gap-4';
        const passLabel = document.createElement('span');
        passLabel.className = 'w-20 text-zinc-500 text-xs uppercase tracking-widest';
        passLabel.innerText = 'Password';
        const passValue = document.createElement('span');
        passValue.className = 'flex-1 text-zinc-300 font-bold';
        passValue.innerText = '••••••••••••'; // Fixed length mask
        const passButtons = document.createElement('div');
        passButtons.className = 'flex items-center gap-3 opacity-0 group-hover:opacity-100 transition-opacity';
        
        const fetchSecret = async () => {
            const fullData = await API.decrypt(path, true); // reveal=true
            return fullData.split('\n')[0];
        };

        const showBtn = document.createElement('button');
        showBtn.innerHTML = '<i data-lucide="eye" class="w-4 h-4 text-zinc-400 hover:text-white"></i>';
        showBtn.onmousedown = async () => { 
            const secret = await fetchSecret();
            passValue.innerText = secret; 
        };
        showBtn.onmouseup = () => { passValue.innerText = '••••••••••••'; };
        showBtn.onmouseleave = () => { passValue.innerText = '••••••••••••'; };
        const copyBtn = document.createElement('button');
        copyBtn.innerHTML = '<i data-lucide="copy" class="w-4 h-4 text-zinc-400 hover:text-white"></i>';
        copyBtn.onclick = async () => {
            const secret = await fetchSecret();
            navigator.clipboard.writeText(secret);
            copyBtn.innerHTML = '<i data-lucide="check" class="w-4 h-4 text-green-500"></i>';
            setTimeout(() => { copyBtn.innerHTML = '<i data-lucide="copy" class="w-4 h-4 text-zinc-400 hover:text-white"></i>'; lucide.createIcons(); }, 2000);
            lucide.createIcons();
        };
        passButtons.appendChild(showBtn);
        passButtons.appendChild(copyBtn);
        passwordRow.appendChild(passLabel);
        passwordRow.appendChild(passValue);
        passwordRow.appendChild(passButtons);

        if (url) fieldsContainer.appendChild(createMetadataRow('URL', url));
        if (user) fieldsContainer.appendChild(createMetadataRow('User', user));
        fieldsContainer.appendChild(passwordRow);

        viewer.appendChild(fieldsContainer);

        // @ts-ignore
        lucide.createIcons();
    },

    renderAuditLogs(logs) {
        this.elements.auditTableBody.innerHTML = '';
        logs.forEach(log => {
            const row = document.createElement('tr');
            row.className = 'border-b border-zinc-900/50 hover:bg-zinc-900/30 transition-colors';

            let actionColor = 'text-zinc-300';
            if (log.action.includes('SUCCESS') || log.action.includes('SAVE') || log.action.includes('BACKUP')) actionColor = 'text-green-400';
            if (log.action.includes('FAILURE') || log.action.includes('DELETE')) actionColor = 'text-red-400';
            if (log.action.includes('DECRYPT')) actionColor = 'text-blue-400';
            if (log.action.includes('LOGOUT')) actionColor = 'text-yellow-500';

            row.innerHTML = `
                <td class="py-2 text-zinc-500 text-xs">${new Date(log.timestamp + 'Z').toLocaleString()}</td>
                <td class="py-2 font-bold ${actionColor}">${log.action}</td>
                <td class="py-2 text-zinc-300">${log.target}</td>
                <td class="py-2 text-zinc-400">${log.ip_address || '-'}</td>
                <td class="py-2 text-zinc-500 text-xs truncate max-w-xs" title="${log.user_agent || ''}">${log.user_agent || '-'}</td>
                <td class="py-2 text-zinc-400">${log.auth_method || 'system'}</td>
            `;
            this.elements.auditTableBody.appendChild(row);
        });
    }
};