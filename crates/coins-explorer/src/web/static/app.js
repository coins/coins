class ExplorerApp {
    constructor() {
        this.ws = null;
        this.currentSection = 'overview';
        this.previousSection = null;
        this.currentPage = 0;
        this.apiBase = '/api/v1';
        this.network = 'regtest';
        this.currentAccountPk = null;
        this.balancesExpanded = false;
        this.secsUntilNextLoop = null;
        this.intervalSecs = 60;

        // Service status
        this.serviceStatus = {
            indexer: false,
            publisher: false,
            websocket: false
        };

        // Data cache for instant navigation (invalidated on new block)
        this.cache = {
            accounts: new Map(),      // pk -> { account, allTxs, pkToId }
            blocks: new Map()         // height -> { block }
        };

        // Navigation history (one entry per view TYPE, keeps first instance)
        this.navHistory = [];
        this.atHistoryEnd = true;  // Are we at the last history entry (vs past it)?

        // Initialize
        this.initWebSocket();
        this.initSearch();
        this.fetchNetworkInfo();
        this.loadStats();
        this.loadOverview();
        this.checkServiceStatus();

        // Handle browser navigation
        window.addEventListener('popstate', (e) => this.handlePopState(e));

        // Check initial hash
        this.handleInitialHash();

        // Periodically check services and update next block timer
        setInterval(() => this.checkServiceStatus(), 30000);
        setInterval(() => this.updateNextBlockTimer(), 1000);
    }

    // ========================================
    // Navigation
    // ========================================

    showSection(section, params = {}, pushState = true) {
        // Hide all sections
        document.querySelectorAll('.explorer-section').forEach(s => {
            s.classList.remove('active');
        });

        // Update pills
        document.querySelectorAll('.pill').forEach(p => {
            p.classList.toggle('active', p.dataset.section === section);
        });

        // Track previous section for back navigation
        if (['overview', 'blocks', 'mempool'].includes(section)) {
            this.previousSection = section;
        }

        // Show target section
        const sectionEl = document.getElementById(`section-${section}`);
        if (sectionEl) {
            sectionEl.classList.add('active');
        }

        this.currentSection = section;

        // Load content based on section
        switch (section) {
            case 'overview':
                this.loadOverview();
                break;
            case 'blocks':
                this.loadBlocks(params.page || 0);
                break;
            case 'mempool':
                this.loadMempool();
                break;
            case 'block-detail':
                this.loadBlockDetail(params.height);
                break;
            case 'account-detail':
                this.loadAccountDetail(params.pk);
                break;
            case 'tx-detail':
                this.loadTxDetail(params.blockHeight, params.txIndex);
                break;
        }

        // Update URL and navigation history
        if (pushState) {
            let hash = section;
            if (section === 'block-detail' && params.height !== undefined) {
                hash = `block/${params.height}`;
            } else if (section === 'account-detail' && params.pk) {
                hash = `account/${params.pk}`;
            } else if (section === 'tx-detail' && params.blockHeight !== undefined && params.txIndex !== undefined) {
                hash = `tx/${params.blockHeight}/${params.txIndex}`;
            } else if (section === 'blocks' && params.page) {
                hash = `blocks/${params.page}`;
            }

            // History keeps ONE entry per view TYPE (the first visited)
            // Same type = don't add (keep first), different type = push new entry
            const currentType = this.navHistory.length > 0 ? this.navHistory[this.navHistory.length - 1].section : null;

            if (section !== currentType) {
                // Different view type - push new entry
                this.navHistory.push({ section, params, hash });
                this.atHistoryEnd = true;  // We're at the new entry
            } else {
                // Same view type - don't add, we're now past the history end
                this.atHistoryEnd = false;
            }

            history.replaceState({ section, params }, '', `#${hash}`);
        }
    }

    goBack() {
        if (this.navHistory.length === 0) return;

        if (this.atHistoryEnd) {
            // We're at the last history entry - pop it and go to previous
            if (this.navHistory.length > 1) {
                this.navHistory.pop();
                const prev = this.navHistory[this.navHistory.length - 1];
                this.showSection(prev.section, prev.params, false);
                history.replaceState({ section: prev.section, params: prev.params }, '', `#${prev.hash}`);
                // Still at history end after popping
            } else {
                // Only one entry left - go to overview
                this.navHistory = [];
                this.showSection('overview', {}, false);
                history.replaceState({ section: 'overview', params: {} }, '', '#overview');
            }
        } else {
            // We're past the last history entry - go back to it (don't pop)
            const last = this.navHistory[this.navHistory.length - 1];
            this.showSection(last.section, last.params, false);
            history.replaceState({ section: last.section, params: last.params }, '', `#${last.hash}`);
            this.atHistoryEnd = true;  // Now we're at the history end
        }
    }

    handlePopState(e) {
        if (e.state) {
            this.showSection(e.state.section, e.state.params || {}, false);
        } else {
            this.handleInitialHash();
        }
    }

    handleInitialHash() {
        const hash = window.location.hash.slice(1);
        if (!hash) {
            this.showSection('overview', {}, true);
            return;
        }

        const parts = hash.split('/');
        const section = parts[0];

        if (section === 'block' && parts[1]) {
            this.showSection('block-detail', { height: parseInt(parts[1]) }, true);
        } else if (section === 'account' && parts[1]) {
            this.showSection('account-detail', { pk: parts[1] }, true);
        } else if (section === 'tx' && parts[1] && parts[2]) {
            this.showSection('tx-detail', { blockHeight: parseInt(parts[1]), txIndex: parseInt(parts[2]) }, true);
        } else if (section === 'blocks') {
            this.showSection('blocks', { page: parseInt(parts[1]) || 0 }, true);
        } else if (['overview', 'mempool'].includes(section)) {
            this.showSection(section, {}, true);
        } else {
            this.showSection('overview', {}, true);
        }
    }

    // ========================================
    // Enhanced Search
    // ========================================

    initSearch() {
        const container = document.getElementById('search-container');
        const input = document.getElementById('global-search');
        const resultsEl = document.getElementById('search-results');

        this.searchState = {
            selectedIndex: 0,
            results: [],
            debounceTimer: null,
            isOpen: false,
            currentQuery: null  // Track current query to avoid stale results
        };

        // Focus handling
        input.addEventListener('focus', () => {
            container.classList.add('focused');
            const query = input.value.trim();
            if (query.length > 0) {
                this.updateSearchResults(query);
            }
        });

        input.addEventListener('blur', () => {
            container.classList.remove('focused');
            // Delay hiding to allow click events on results
            setTimeout(() => {
                if (!container.contains(document.activeElement)) {
                    this.hideSearchResults();
                }
            }, 150);
        });

        // Input handling with debounce
        input.addEventListener('input', () => {
            const query = input.value.trim();
            container.classList.toggle('has-value', query.length > 0);

            clearTimeout(this.searchState.debounceTimer);

            if (query.length === 0) {
                this.hideSearchResults();
                this.searchState.currentQuery = null;
                return;
            }

            // Show immediate type detection feedback
            const searchType = this.detectSearchType(query);
            this.renderSearchLoading(searchType);
            this.showSearchResults();

            // Debounce API calls (longer delay for network requests)
            this.searchState.debounceTimer = setTimeout(() => {
                this.searchState.currentQuery = query;
                this.updateSearchResults(query);
            }, 250);
        });

        // Keyboard navigation
        input.addEventListener('keydown', (e) => {
            // Escape always exits the search
            if (e.key === 'Escape') {
                e.preventDefault();
                this.hideSearchResults();
                input.blur();
                return;
            }

            if (!this.searchState.isOpen) {
                if (e.key === 'Enter') {
                    const query = input.value.trim();
                    if (query.length > 0) {
                        this.executeSearch(query);
                    }
                }
                return;
            }

            switch (e.key) {
                case 'ArrowDown':
                    e.preventDefault();
                    this.navigateSearchResults(1);
                    break;
                case 'ArrowUp':
                    e.preventDefault();
                    this.navigateSearchResults(-1);
                    break;
                case 'Enter':
                    e.preventDefault();
                    this.selectSearchResult();
                    break;
            }
        });

        // Global keyboard shortcut
        document.addEventListener('keydown', (e) => {
            if (e.key === '/' && !this.isInputFocused()) {
                e.preventDefault();
                input.focus();
            }
        });
    }

    isInputFocused() {
        const active = document.activeElement;
        return active && (active.tagName === 'INPUT' || active.tagName === 'TEXTAREA');
    }

    detectSearchType(query) {
        // Pure number - could be block height or account ID
        if (/^\d+$/.test(query)) {
            return { type: 'block-height', label: 'Block/Account', icon: '⛃' };
        }

        // Transaction format: blockHeight:txIndex (e.g., "123:0" or "123/0")
        const txMatch = query.match(/^(\d+)[:\/](\d+)$/);
        if (txMatch) {
            return {
                type: 'transaction',
                label: 'Transaction',
                icon: 'TX',
                blockHeight: parseInt(txMatch[1]),
                txIndex: parseInt(txMatch[2])
            };
        }

        // Account by ID (prefixed with # or "account" or "acc")
        const accIdMatch = query.match(/^(?:#|account\s*|acc\s*)(\d+)$/i);
        if (accIdMatch) {
            return { type: 'account-id', label: 'Account ID', icon: '@', id: parseInt(accIdMatch[1]) };
        }

        // Hex string - could be Bitcoin tx hash (64 chars) or public key
        if (/^[0-9a-fA-F]+$/.test(query)) {
            const len = query.length;
            if (len === 64) {
                return { type: 'btc-txid', label: 'Bitcoin TX / Block', icon: '₿' };
            } else if (len >= 32) {
                return { type: 'public-key', label: 'Public Key', icon: '@' };
            } else {
                return { type: 'hex-partial', label: 'Partial Hex', icon: '?' };
            }
        }

        return { type: 'unknown', label: 'Search', icon: '?' };
    }

    async updateSearchResults(query) {
        const resultsEl = document.getElementById('search-results');
        const searchType = this.detectSearchType(query);
        const searchId = query; // Track this search to prevent stale results

        // Build results based on detected type with actual API validation
        const results = [];

        try {
            switch (searchType.type) {
                case 'block-height': {
                    const num = parseInt(query);
                    // Check both block and account in parallel
                    const [block, account] = await Promise.all([
                        this.fetchAPI(`/blocks/${num}`).catch(() => null),
                        this.fetchAPI(`/accounts/by-id/${num}`).catch(() => null)
                    ]);
                    if (block) {
                        const txCount = block.sub_block?.txs?.length || 0;
                        results.push({
                            type: 'block',
                            title: `Block #${num}`,
                            subtitle: `${txCount} transaction${txCount !== 1 ? 's' : ''} · ${block.btc_txid ? block.btc_txid.slice(0, 12) + '...' : 'No BTC txid'}`,
                            action: () => this.showSection('block-detail', { height: num })
                        });
                    }
                    if (account && account.pk) {
                        const pk = this.bytesToHex(account.pk);
                        const balanceCount = account.balances ? Object.keys(account.balances).length : 0;
                        results.push({
                            type: 'account',
                            title: `Account #${num}`,
                            subtitle: `${balanceCount} token${balanceCount !== 1 ? 's' : ''} · ${pk.slice(0, 8)}...${pk.slice(-6)}`,
                            action: () => this.showSection('account-detail', { pk })
                        });
                    }
                    break;
                }

                case 'transaction': {
                    const block = await this.fetchAPI(`/blocks/${searchType.blockHeight}`);
                    if (block && block.sub_block?.txs?.[searchType.txIndex]) {
                        const tx = block.sub_block.txs[searchType.txIndex];
                        results.push({
                            type: 'tx',
                            title: `Transaction ${searchType.blockHeight}:${searchType.txIndex}`,
                            subtitle: `${tx.amount} sats · ${tx.token_id > 0 ? 'Token ' + tx.token_id : 'Native'}`,
                            action: () => this.showSection('tx-detail', {
                                blockHeight: searchType.blockHeight,
                                txIndex: searchType.txIndex
                            })
                        });
                    }
                    break;
                }

                case 'account-id': {
                    const account = await this.fetchAPI(`/accounts/by-id/${searchType.id}`);
                    if (account && account.pk) {
                        const pk = this.bytesToHex(account.pk);
                        const balanceCount = account.balances ? Object.keys(account.balances).length : 0;
                        results.push({
                            type: 'account',
                            title: `Account #${searchType.id}`,
                            subtitle: `${balanceCount} token${balanceCount !== 1 ? 's' : ''} · ${pk.slice(0, 8)}...${pk.slice(-6)}`,
                            action: () => this.showSection('account-detail', { pk })
                        });
                    }
                    break;
                }

                case 'btc-txid': {
                    // Search for block by Bitcoin txid
                    const latest = await this.fetchAPI('/blocks/latest');
                    if (latest) {
                        const blocks = await this.fetchAPI(`/blocks?from=0&to=${latest.height}`);
                        const foundBlock = blocks?.find(b => b.btc_txid?.toLowerCase() === query.toLowerCase());
                        if (foundBlock) {
                            const txCount = foundBlock.sub_block?.txs?.length || 0;
                            results.push({
                                type: 'block',
                                title: `Block #${foundBlock.height}`,
                                subtitle: `${txCount} transaction${txCount !== 1 ? 's' : ''} · Found by BTC txid`,
                                action: () => this.showSection('block-detail', { height: foundBlock.height })
                            });
                        }
                    }
                    // Also check if it's a valid account public key
                    const account = await this.fetchAPI(`/accounts/${query}`);
                    if (account) {
                        const balanceCount = account.balances ? Object.keys(account.balances).length : 0;
                        results.push({
                            type: 'account',
                            title: `Account #${account.id}`,
                            subtitle: `${balanceCount} token${balanceCount !== 1 ? 's' : ''} · ${query.slice(0, 8)}...${query.slice(-6)}`,
                            action: () => this.showSection('account-detail', { pk: query })
                        });
                    }
                    break;
                }

                case 'public-key':
                case 'hex-partial': {
                    // Check if it's a valid account
                    const account = await this.fetchAPI(`/accounts/${query}`);
                    if (account) {
                        const balanceCount = account.balances ? Object.keys(account.balances).length : 0;
                        results.push({
                            type: 'account',
                            title: `Account #${account.id}`,
                            subtitle: `${balanceCount} token${balanceCount !== 1 ? 's' : ''} · ${query.slice(0, 8)}...${query.slice(-6)}`,
                            action: () => this.showSection('account-detail', { pk: query })
                        });
                    }
                    // For 64-char hex, also check if it's a Bitcoin txid
                    if (query.length === 64) {
                        const latest = await this.fetchAPI('/blocks/latest');
                        if (latest) {
                            const blocks = await this.fetchAPI(`/blocks?from=0&to=${latest.height}`);
                            const foundBlock = blocks?.find(b => b.btc_txid?.toLowerCase() === query.toLowerCase());
                            if (foundBlock) {
                                const txCount = foundBlock.sub_block?.txs?.length || 0;
                                results.push({
                                    type: 'block',
                                    title: `Block #${foundBlock.height}`,
                                    subtitle: `${txCount} transaction${txCount !== 1 ? 's' : ''} · Found by BTC txid`,
                                    action: () => this.showSection('block-detail', { height: foundBlock.height })
                                });
                            }
                        }
                    }
                    break;
                }
            }
        } catch (error) {
            console.warn('Search error:', error);
        }

        // Check if this is still the current search (prevent stale results)
        if (this.searchState.currentQuery !== searchId) {
            return;
        }

        this.searchState.results = results;
        this.searchState.selectedIndex = 0;

        // Render results
        this.renderSearchResults(searchType, results);
    }

    renderSearchLoading(searchType) {
        const resultsEl = document.getElementById('search-results');
        const iconClass = searchType.type === 'block-height' ? 'block' :
                          searchType.type === 'transaction' ? 'tx' :
                          searchType.type.includes('account') || searchType.type.includes('key') ? 'account' : 'unknown';

        resultsEl.innerHTML = `
            <div class="search-type-header">
                <span class="search-type-icon ${iconClass}">${searchType.icon}</span>
                <span class="search-type-label">${searchType.label}</span>
            </div>
            <div class="search-loading">
                <span class="search-loading-spinner"></span>
                <span>Searching...</span>
            </div>
        `;
    }

    renderSearchResults(searchType, results) {
        const resultsEl = document.getElementById('search-results');

        if (results.length === 0) {
            resultsEl.innerHTML = `
                <div class="search-type-header">
                    <span class="search-type-icon unknown">?</span>
                    <span class="search-type-label">No matches</span>
                </div>
                <div class="search-no-results">
                    <div class="search-no-results-icon">∅</div>
                    <div class="search-no-results-text">No results found</div>
                    <div class="search-no-results-hint">Try a block number, account ID, or hex address</div>
                </div>
                <div class="search-help">
                    <span class="search-help-item"><code>123</code> Block height</span>
                    <span class="search-help-item"><code>#5</code> Account ID</span>
                    <span class="search-help-item"><code>10:0</code> Transaction</span>
                </div>
            `;
        } else {
            const iconClass = searchType.type === 'block-height' ? 'block' :
                              searchType.type === 'transaction' ? 'tx' :
                              searchType.type.includes('account') || searchType.type.includes('key') ? 'account' : 'unknown';

            resultsEl.innerHTML = `
                <div class="search-type-header">
                    <span class="search-type-icon ${iconClass}">${searchType.icon}</span>
                    <span class="search-type-label">${searchType.label}</span>
                    <span class="search-type-hint">↵ to select</span>
                </div>
                ${results.map((result, i) => `<div class="search-result-item${i === this.searchState.selectedIndex ? ' selected' : ''}" data-index="${i}"><span class="search-result-icon ${result.type}">${result.type === 'block' ? '⛃' : result.type === 'account' ? '@' : result.type === 'tx' ? '↔' : '?'}</span><div class="search-result-content"><div class="search-result-title">${result.title}</div><div class="search-result-subtitle">${result.subtitle}</div></div><span class="search-result-action"><kbd style="${i === 0 ? '' : 'visibility:hidden'}">↵</kbd></span></div>`).join('')}
                <div class="search-help">
                    <span class="search-help-item"><kbd>↑↓</kbd> Navigate</span>
                    <span class="search-help-item"><kbd>↵</kbd> Select</span>
                    <span class="search-help-item"><kbd>esc</kbd> Close</span>
                </div>
            `;

            // Add click handlers
            resultsEl.querySelectorAll('.search-result-item').forEach(item => {
                item.addEventListener('click', () => {
                    const index = parseInt(item.dataset.index);
                    this.searchState.selectedIndex = index;
                    this.selectSearchResult();
                });
                item.addEventListener('mouseenter', () => {
                    const index = parseInt(item.dataset.index);
                    this.searchState.selectedIndex = index;
                    this.updateSelectedResult();
                });
            });
        }

        this.showSearchResults();
    }

    showSearchResults() {
        const resultsEl = document.getElementById('search-results');
        resultsEl.classList.add('visible');
        this.searchState.isOpen = true;
    }

    hideSearchResults() {
        const resultsEl = document.getElementById('search-results');
        resultsEl.classList.remove('visible');
        this.searchState.isOpen = false;
    }

    navigateSearchResults(direction) {
        const newIndex = this.searchState.selectedIndex + direction;
        if (newIndex >= 0 && newIndex < this.searchState.results.length) {
            this.searchState.selectedIndex = newIndex;
            this.updateSelectedResult();
        }
    }

    updateSelectedResult() {
        const resultsEl = document.getElementById('search-results');
        resultsEl.querySelectorAll('.search-result-item').forEach((item, i) => {
            item.classList.toggle('selected', i === this.searchState.selectedIndex);
            const action = item.querySelector('.search-result-action');
            if (action) {
                action.innerHTML = i === this.searchState.selectedIndex ? '<kbd>↵</kbd>' : '';
            }
        });
    }

    selectSearchResult() {
        const result = this.searchState.results[this.searchState.selectedIndex];
        if (result && result.action) {
            result.action();
            this.clearSearch();
        }
    }

    clearSearch() {
        const input = document.getElementById('global-search');
        const container = document.getElementById('search-container');
        input.value = '';
        container.classList.remove('has-value');
        this.hideSearchResults();
    }

    async executeSearch(query) {
        // Direct search without dropdown - try to find first matching result
        const searchType = this.detectSearchType(query);

        switch (searchType.type) {
            case 'block-height': {
                // For numbers, prefer block if it exists, otherwise try account
                const num = parseInt(query);
                const block = await this.fetchAPI(`/blocks/${num}`).catch(() => null);
                if (block) {
                    this.showSection('block-detail', { height: num });
                } else {
                    this.lookupAccountById(num);
                }
                break;
            }
            case 'transaction':
                this.showSection('tx-detail', {
                    blockHeight: searchType.blockHeight,
                    txIndex: searchType.txIndex
                });
                break;
            case 'account-id':
                this.lookupAccountById(searchType.id);
                break;
            case 'btc-txid':
                this.searchBlockByBtcTxid(query);
                break;
            default:
                if (/^[0-9a-fA-F]+$/.test(query)) {
                    this.showSection('account-detail', { pk: query });
                }
                break;
        }
        this.clearSearch();
    }

    async lookupAccountById(id) {
        try {
            const account = await this.fetchAPI(`/accounts/by-id/${id}`);
            if (account && account.pk) {
                const pk = this.bytesToHex(account.pk);
                this.showSection('account-detail', { pk });
            } else {
                console.warn('Account not found:', id);
            }
        } catch (error) {
            console.error('Error looking up account:', error);
        }
    }

    async searchBlockByBtcTxid(txid) {
        try {
            // Get latest block to know the range
            const latest = await this.fetchAPI('/blocks/latest');
            if (!latest) return;

            // Search through recent blocks
            const blocks = await this.fetchAPI(`/blocks?from=0&to=${latest.height}`);
            const found = blocks.find(b => b.btc_txid && b.btc_txid.toLowerCase() === txid.toLowerCase());

            if (found) {
                this.showSection('block-detail', { height: found.height });
            } else {
                console.warn('Block not found with txid:', txid);
            }
        } catch (error) {
            console.error('Error searching block:', error);
        }
    }

    globalSearch() {
        // Legacy method - now handled by initSearch
        const input = document.getElementById('global-search');
        const query = input.value.trim();
        if (query) {
            this.executeSearch(query);
        }
    }

    // ========================================
    // WebSocket
    // ========================================

    initWebSocket() {
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/ws`;

        this.ws = new WebSocket(wsUrl);

        this.ws.onopen = () => {
            console.log('WebSocket connected');
            this.serviceStatus.websocket = true;
            this.updateStatusIndicator();
        };

        this.ws.onmessage = (event) => {
            const msg = JSON.parse(event.data);
            this.handleWSMessage(msg);
        };

        this.ws.onclose = () => {
            console.log('WebSocket disconnected, reconnecting...');
            this.serviceStatus.websocket = false;
            this.updateStatusIndicator();
            setTimeout(() => this.initWebSocket(), 3000);
        };

        this.ws.onerror = (error) => {
            console.error('WebSocket error:', error);
        };
    }

    handleWSMessage(msg) {
        switch (msg.type) {
            case 'new_block':
                // Invalidate cache when new block is mined
                this.cache.accounts.clear();
                this.cache.blocks.clear();
                // Fall through to stats_update
            case 'stats_update':
                this.loadStats();
                if (this.currentSection === 'overview') {
                    this.loadOverview();
                } else if (this.currentSection === 'blocks') {
                    this.loadBlocks(this.currentPage);
                } else if (this.currentSection === 'account-detail' && this.currentAccountPk) {
                    this.loadAccountDetail(this.currentAccountPk);
                }
                break;

            case 'pending_txs_update':
                this.loadStats();
                if (this.currentSection === 'mempool') {
                    this.loadMempool();
                } else if (this.currentSection === 'account-detail' && this.currentAccountPk) {
                    this.loadAccountDetail(this.currentAccountPk);
                }
                break;

            case 'confirmation_update':
                if (this.currentSection === 'account-detail' && this.currentAccountPk) {
                    this.loadAccountDetail(this.currentAccountPk);
                } else if (this.currentSection === 'mempool') {
                    this.loadMempool();
                }
                break;
        }
    }

    // ========================================
    // API
    // ========================================

    async fetchAPI(endpoint) {
        try {
            const response = await fetch(this.apiBase + endpoint);
            if (!response.ok) {
                if (response.status === 404) return null;
                throw new Error(`API Error: ${response.status}`);
            }
            return await response.json();
        } catch (error) {
            if (error.message === 'Failed to fetch') {
                throw new Error('INDEXER_UNAVAILABLE');
            }
            throw error;
        }
    }

    async fetchNetworkInfo() {
        try {
            const stats = await this.fetchAPI('/stats');
            if (stats && stats.network) {
                this.network = stats.network;
            }
        } catch (error) {
            console.warn('Could not fetch network info:', error);
        }
    }

    // ========================================
    // Data Loading
    // ========================================

    async loadStats() {
        try {
            const stats = await this.fetchAPI('/stats');
            let pendingCount = 0;
            try {
                const pending = await this.fetchAPI('/pending-transactions') || [];
                pendingCount = pending.length;
            } catch (e) {}

            this.updateStat('total-blocks', stats?.total_blocks || 0);
            this.updateStat('total-accounts', stats?.total_accounts || 0);
            this.updateStat('total-supply', stats?.total_supply || 0);
            this.updateStat('pending-txs', pendingCount);
        } catch (error) {
            console.warn('Could not load stats:', error);
        }
    }

    updateStat(key, value) {
        const el = document.querySelector(`[data-stat="${key}"]`);
        if (el) el.textContent = value;
    }

    async loadOverview() {
        const container = document.getElementById('latest-block-card');
        try {
            const block = await this.fetchAPI('/blocks/latest');
            if (block) {
                container.innerHTML = this.renderBlockCard(block, true);
            } else {
                container.innerHTML = this.renderEmptyState();
            }
        } catch (error) {
            container.innerHTML = this.renderError(error);
        }
    }

    async loadBlocks(page = 0) {
        this.currentPage = page;
        const container = document.getElementById('blocks-list');

        try {
            const latestBlock = await this.fetchAPI('/blocks/latest');
            if (!latestBlock) {
                container.innerHTML = this.renderEmptyState();
                return;
            }

            const allBlocks = await this.fetchAPI(`/blocks?from=0&to=${latestBlock.height}`);
            allBlocks.sort((a, b) => b.height - a.height);

            const blocksPerPage = 15;
            const totalPages = Math.ceil(allBlocks.length / blocksPerPage);
            const startIdx = page * blocksPerPage;
            const blocks = allBlocks.slice(startIdx, startIdx + blocksPerPage);

            let html = blocks.map(b => this.renderBlockCard(b)).join('');

            if (totalPages > 1) {
                html += this.renderPagination(page, totalPages);
            }

            container.innerHTML = html || '<div class="empty-state-compact"><p class="empty-desc">No blocks found</p></div>';
        } catch (error) {
            container.innerHTML = this.renderError(error);
        }
    }

    async loadMempool() {
        const container = document.getElementById('mempool-content');

        try {
            const pendingTxs = await this.fetchAPI('/pending-transactions') || [];

            if (pendingTxs.length === 0) {
                container.innerHTML = `
                    <div class="empty-state-compact">
                        <div class="empty-icon">&#8634;</div>
                        <p class="empty-title">Mempool Empty</p>
                        <p class="empty-desc">No pending transactions at the moment.</p>
                    </div>
                `;
                return;
            }

            const publishing = pendingTxs.filter(tx => tx.status === 'publishing');
            const broadcasting = pendingTxs.filter(tx => tx.status === 'broadcasting');
            const unconfirmed = pendingTxs.filter(tx => tx.status === 'unconfirmed');

            let html = '';

            if (unconfirmed.length > 0) {
                html += this.renderMempoolGroup('Unconfirmed', unconfirmed, 'unconfirmed');
            }
            if (broadcasting.length > 0) {
                html += this.renderMempoolGroup('Broadcasting', broadcasting, 'broadcasting');
            }
            if (publishing.length > 0) {
                html += this.renderMempoolGroup('Publishing', publishing, 'publishing');
            }

            container.innerHTML = html;
        } catch (error) {
            container.innerHTML = this.renderError(error);
        }
    }

    async loadBlockDetail(height) {
        const container = document.getElementById('block-detail-content');

        try {
            const block = await this.fetchAPI(`/blocks/${height}`);
            if (!block) {
                container.innerHTML = '<p class="has-text-grey">Block not found</p>';
                return;
            }

            const publisherPk = this.bytesToHex(block.sub_block.publisher_pk);
            const txCount = block.sub_block?.txs?.length || 0;
            const explorerUrl = this.getBitcoinExplorerUrl(block.btc_txid);

            // Determine confirmation status
            const confirmations = block.confirmations || 0;
            const isFinalized = confirmations >= 6;
            const confLabel = isFinalized ? 'Confirmed' : `${confirmations} Confirmation${confirmations !== 1 ? 's' : ''}`;
            const confClass = isFinalized ? 'confirmed' : 'pending';

            let html = `
                <div class="account-header">
                    <span class="account-id">Block #${block.height}</span>
                </div>
                <div class="account-pk">
                    ${block.btc_txid}
                    ${explorerUrl ? `<a href="${explorerUrl}" target="_blank" class="explorer-link">View on Explorer ↗</a>` : ''}
                </div>

                <div class="tx-history-header" style="margin-top: 1.5rem;">Block Info</div>
                <div class="balances-grid">
                    <div class="balance-item">
                        <div class="balance-amount">${txCount}</div>
                        <div class="balance-token">Transactions</div>
                    </div>
                    <div class="balance-item">
                        <div class="balance-amount"><span class="conf-badge ${confClass}" style="font-size: 1rem;">${confLabel}</span></div>
                        <div class="balance-token">Status</div>
                    </div>
                </div>

                <div class="tx-history-header" style="margin-top: 1.5rem;">Publisher</div>
                <div class="account-pk" style="margin-bottom: 1.5rem;" onclick="app.showSection('account-detail', {pk: '${publisherPk}'})">${this.renderTruncatablePk(publisherPk, 12)}</div>
            `;

            if (txCount > 0) {
                // Look up recipient account IDs
                const recipientPks = [...new Set(block.sub_block.txs.map(tx => this.bytesToHex(tx.recipient_pk)))];
                const pkToId = {};
                await Promise.all(recipientPks.map(async (rpk) => {
                    try {
                        const recipientAccount = await this.fetchAPI(`/accounts/${rpk}`);
                        if (recipientAccount && recipientAccount.id !== undefined) {
                            pkToId[rpk] = recipientAccount.id;
                        }
                    } catch (e) {}
                }));

                // Look up sender PKs for clickable links
                const senderIds = [...new Set(block.sub_block.txs.map(tx => tx.sender_id))];
                const senderIdToPk = {};
                await Promise.all(senderIds.map(async (sid) => {
                    try {
                        const senderAccount = await this.fetchAPI(`/accounts/by-id/${sid}`);
                        if (senderAccount && senderAccount.pk) {
                            senderIdToPk[sid] = this.bytesToHex(senderAccount.pk);
                        }
                    } catch (e) {
                        console.warn(`Failed to fetch sender account ${sid}:`, e);
                    }
                }));

                html += `
                    <div class="tx-history-header">Transactions</div>
                    <div class="tx-table">
                        ${block.sub_block.txs.map((tx, index) => {
                            const recipientPk = this.bytesToHex(tx.recipient_pk);
                            const recipientId = pkToId[recipientPk];
                            const hasRecipientId = recipientId !== undefined && recipientId !== null;
                            const senderPk = senderIdToPk[tx.sender_id];

                            return `
                                <div class="block-tx-row" onclick="app.showSection('tx-detail', {blockHeight: ${height}, txIndex: ${index}})">
                                    <span class="block-tx-index">${index}</span>
                                    <span class="block-tx-flow">
                                        <span class="block-tx-account" ${senderPk ? `onclick="event.stopPropagation(); app.showSection('account-detail', {pk: '${senderPk}'})"` : ''}>#${tx.sender_id}</span>
                                        <span class="block-tx-arrow">→</span>
                                        <span class="block-tx-account" onclick="event.stopPropagation(); app.showSection('account-detail', {pk: '${recipientPk}'})">${hasRecipientId ? `#${recipientId}` : 'New'}</span>
                                    </span>
                                    <span class="block-tx-amount">${tx.amount}</span>
                                    <span class="block-tx-fee">${tx.fee} fee</span>
                                    <span class="block-tx-token">${tx.token_id > 0 ? `Token ${tx.token_id}` : 'Native'}</span>
                                </div>
                            `;
                        }).join('')}
                    </div>
                `;
            } else {
                html += `
                    <div class="tx-history-header">Transactions</div>
                    <p class="has-text-grey" style="text-align: center; padding: 1rem;">No transactions in this block</p>
                `;
            }

            container.innerHTML = html;
        } catch (error) {
            container.innerHTML = this.renderError(error);
        }
    }

    async loadTxDetail(blockHeight, txIndex) {
        const container = document.getElementById('tx-detail-content');
        container.innerHTML = '<div class="loading-placeholder">Loading...</div>';

        try {
            // Fetch block data
            const response = await fetch(`/api/v1/blocks/${blockHeight}`);
            if (!response.ok) throw new Error('Block not found');
            const block = await response.json();

            if (!block.sub_block || !block.sub_block.txs || !block.sub_block.txs[txIndex]) {
                throw new Error('Transaction not found');
            }

            const tx = block.sub_block.txs[txIndex];
            const recipientPk = this.bytesToHex(tx.recipient_pk);

            // Build pk to id map from all recipients
            const pkToId = {};
            block.sub_block.txs.forEach(t => {
                const pk = this.bytesToHex(t.recipient_pk);
                if (t.recipient_account_id !== undefined && t.recipient_account_id !== null) {
                    pkToId[pk] = t.recipient_account_id;
                }
            });

            // Fetch sender account to get their pk
            let senderPk = null;
            try {
                const senderResponse = await fetch(`/api/v1/accounts/by-id/${tx.sender_id}`);
                if (senderResponse.ok) {
                    const senderAccount = await senderResponse.json();
                    senderPk = senderAccount.pk;
                }
            } catch (e) {
                console.warn('Failed to fetch sender account:', e);
            }

            const recipientId = pkToId[recipientPk];
            const hasRecipientId = recipientId !== undefined && recipientId !== null;

            // Confirmation status
            const confirmations = block.confirmations || 0;
            const isFinalized = block.finalized || confirmations >= 6;
            const confLabel = isFinalized ? 'Finalized' : confirmations > 0 ? `${confirmations} Confirmation${confirmations !== 1 ? 's' : ''}` : 'Unconfirmed';
            const confClass = isFinalized ? 'finalized' : confirmations > 0 ? 'confirming' : 'pending';

            const html = `
                <div class="tx-detail-card">
                    <div class="tx-detail-header">
                        <span class="tx-detail-title">Transaction Details</span>
                        <span class="tx-detail-index">Block #${blockHeight} · Tx ${txIndex}</span>
                    </div>

                    <div class="tx-detail-grid">
                        <div class="tx-detail-item">
                            <span class="tx-detail-label">Amount</span>
                            <span class="tx-detail-value success">${tx.amount.toLocaleString()}</span>
                        </div>
                        <div class="tx-detail-item">
                            <span class="tx-detail-label">Token</span>
                            <span class="tx-detail-value">${tx.token_id > 0 ? `Token ${tx.token_id}` : 'Native'}</span>
                        </div>
                        <div class="tx-detail-item">
                            <span class="tx-detail-label">Fee</span>
                            <span class="tx-detail-value">${tx.fee}</span>
                        </div>
                        <div class="tx-detail-item">
                            <span class="tx-detail-label">Status</span>
                            <span class="tx-detail-value"><span class="conf-badge ${confClass}">${confLabel}</span></span>
                        </div>
                    </div>

                    <div class="tx-detail-section">
                        <div class="tx-detail-section-title">Sender</div>
                        <div class="tx-detail-item">
                            <span class="tx-detail-label">Account ID</span>
                            <span class="tx-detail-value accent clickable" ${senderPk ? `onclick="app.showSection('account-detail', {pk: '${senderPk}'})"` : ''}>#${tx.sender_id}</span>
                        </div>
                        ${senderPk ? `
                        <div class="tx-detail-item" style="margin-top: 0.75rem;">
                            <span class="tx-detail-label">Public Key</span>
                            <span class="tx-detail-value tx-detail-pk clickable" onclick="app.showSection('account-detail', {pk: '${senderPk}'})">${senderPk}</span>
                        </div>
                        ` : ''}
                    </div>

                    <div class="tx-detail-section">
                        <div class="tx-detail-section-title">Recipient</div>
                        <div class="tx-detail-item">
                            <span class="tx-detail-label">Account ID</span>
                            <span class="tx-detail-value accent clickable" onclick="app.showSection('account-detail', {pk: '${recipientPk}'})">${hasRecipientId ? `#${recipientId}` : 'New Account'}</span>
                        </div>
                        <div class="tx-detail-item" style="margin-top: 0.75rem;">
                            <span class="tx-detail-label">Public Key</span>
                            <span class="tx-detail-value tx-detail-pk clickable" onclick="app.showSection('account-detail', {pk: '${recipientPk}'})">${recipientPk}</span>
                        </div>
                    </div>

                    <div class="tx-detail-section">
                        <div class="tx-detail-section-title">Block</div>
                        <div class="tx-detail-item">
                            <span class="tx-detail-label">Block Height</span>
                            <span class="tx-detail-value accent clickable" onclick="app.showSection('block-detail', {height: ${blockHeight}})">#${blockHeight}</span>
                        </div>
                        ${block.btc_txid ? `
                        <div class="tx-detail-item" style="margin-top: 0.75rem;">
                            <span class="tx-detail-label">Bitcoin Transaction</span>
                            <span class="tx-detail-value tx-detail-pk">
                                <a href="https://mutinynet.com/tx/${block.btc_txid}" target="_blank" rel="noopener" style="color: var(--coins-accent);">${block.btc_txid}</a>
                            </span>
                        </div>
                        ` : ''}
                    </div>
                </div>
            `;

            container.innerHTML = html;
        } catch (error) {
            container.innerHTML = this.renderError(error);
        }
    }

    async loadAccountDetail(pk) {
        const container = document.getElementById('account-detail-content');
        const isNewAccount = this.currentAccountPk !== pk;
        const cached = this.cache.accounts.get(pk);

        // Reset UI state when navigating to a different account
        if (isNewAccount) {
            this.balancesExpanded = false;
            this.scrollState = null;

            // Show cached data immediately if available, otherwise show loading
            if (cached) {
                container.innerHTML = this.renderAccountHTML(cached.account, cached.allTxs, cached.pkToId);
            } else {
                container.innerHTML = '<div class="loading-placeholder">Loading...</div>';
            }
        } else {
            // Save scroll state before re-render (for WebSocket updates)
            this.saveScrollState();
        }

        this.currentAccountPk = pk;

        try {
            const account = await this.fetchAPI(`/accounts/${pk}`);
            if (!account) {
                container.innerHTML = '<p class="has-text-grey">Account not found</p>';
                this.cache.accounts.delete(pk);
                return;
            }

            // Load transactions
            const txs = await this.fetchAPI(`/accounts/${pk}/transactions`) || [];

            // Also get pending
            let pendingTxs = [];
            try {
                const sentPending = await this.fetchAPI(`/pending-transactions?sender_id=${account.id}`) || [];
                const receivedPending = await this.fetchAPI(`/pending-transactions?recipient_pk=${pk}`) || [];

                pendingTxs = [
                    ...sentPending.map(tx => ({ ...tx, direction: 'outgoing', isPending: true })),
                    ...receivedPending.map(tx => ({ ...tx, direction: 'incoming', isPending: true }))
                ];

                // Filter out already indexed
                const indexedKeys = new Set(txs.map(tx => `${this.bytesToHex(tx.sender_pk)}-${tx.nonce}`));
                pendingTxs = pendingTxs.filter(tx => !indexedKeys.has(`${tx.sender_pk}-${tx.nonce}`));
            } catch (e) {}

            const allTxs = [...txs, ...pendingTxs];

            // Look up account IDs for outgoing transaction recipients
            const recipientPks = [...new Set(allTxs
                .filter(tx => tx.direction === 'outgoing')
                .map(tx => this.bytesToHex(tx.recipient_pk)))];

            const pkToId = {};
            await Promise.all(recipientPks.map(async (rpk) => {
                try {
                    const recipientAccount = await this.fetchAPI(`/accounts/${rpk}`);
                    if (recipientAccount && recipientAccount.id !== undefined) {
                        pkToId[rpk] = recipientAccount.id;
                    }
                } catch (e) {}
            }));

            // Update cache
            this.cache.accounts.set(pk, { account, allTxs, pkToId });

            // Only update DOM if still viewing this account
            if (this.currentAccountPk === pk) {
                const html = this.renderAccountHTML(account, allTxs, pkToId);
                container.innerHTML = html;
                this.restoreScrollState();
            }
        } catch (error) {
            container.innerHTML = this.renderError(error);
        }
    }

    renderAccountHTML(account, allTxs, pkToId) {
        const pkHex = this.bytesToHex(account.pk);
        const balances = account.balances ? Object.entries(account.balances).sort((a, b) => parseInt(a[0]) - parseInt(b[0])) : [];

        let html = `
            <div class="account-header">
                <span class="account-id">Account #${account.id}</span>
            </div>
            <div class="account-pk copyable" onclick="app.copyToClipboard('${pkHex}', this)">
                ${this.renderTruncatablePk(pkHex, 12)}
            </div>

            ${this.renderBalances(balances)}
        `;

        html += `
            <div class="tx-history-header">Transaction History</div>
            <div class="tx-table">
                ${allTxs.length > 0 ? allTxs.map(tx => {
                    const isIncoming = tx.direction === 'incoming';
                    const counterpartyPk = isIncoming ? this.bytesToHex(tx.sender_pk) : this.bytesToHex(tx.recipient_pk);
                    const counterpartyId = isIncoming ? tx.sender_id : pkToId[counterpartyPk];
                    const hasId = counterpartyId !== undefined && counterpartyId !== null;
                    const amountClass = isIncoming ? 'positive' : 'negative';
                    const amountPrefix = isIncoming ? '+' : '-';

                    let statusBadge = '';
                    if (tx.isPending) {
                        statusBadge = `<span class="tx-status-badge ${tx.status}">${tx.status}</span>`;
                    } else if (tx.finalized) {
                        statusBadge = '<span class="conf-badge confirmed">Confirmed</span>';
                    } else {
                        const conf = tx.confirmations || 0;
                        statusBadge = `<span class="conf-badge pending">${conf} Confirmation${conf !== 1 ? 's' : ''}</span>`;
                    }

                    // Only make row clickable if we have block info (not pending)
                    const isClickable = !tx.isPending && tx.block_height !== undefined;
                    const rowClick = isClickable ? `onclick="app.showSection('tx-detail', {blockHeight: ${tx.block_height}, txIndex: ${tx.tx_index}})"` : '';

                    return `
                        <div class="tx-table-row${isClickable ? ' clickable' : ''}" ${rowClick}>
                            <span class="tx-type-badge ${isIncoming ? 'received' : 'sent'}">${isIncoming ? 'Received' : 'Sent'}</span>
                            <span class="tx-counterparty-cell"><span class="tx-counterparty-wrapper" onclick="event.stopPropagation(); app.showSection('account-detail', {pk: '${counterpartyPk}'})"><span class="tx-counterparty-pk">${this.renderTruncatablePk(counterpartyPk, 8)}</span>${hasId ? `<span class="tx-counterparty-divider">|</span><span class="tx-counterparty-id">#${counterpartyId}</span>` : ''}</span></span>
                            <span class="tx-table-amount ${amountClass}">${amountPrefix}${tx.amount}</span>
                            <span class="tx-table-token">${tx.token_id > 0 ? `Token ${tx.token_id}` : 'Native'}</span>
                            ${statusBadge}
                        </div>
                    `;
                }).join('') : '<p class="has-text-grey" style="text-align: center; padding: 1rem;">No transactions</p>'}
            </div>
        `;

        return html;
    }

    // ========================================
    // Renderers
    // ========================================

    renderBlockCard(block, isLatest = false) {
        const txCount = block.sub_block?.txs?.length || 0;
        const confirmations = block.confirmations || 0;
        const confLabel = confirmations >= 6 ? 'Confirmed' :
                          confirmations >= 1 ? `${confirmations} Confirmation${confirmations !== 1 ? 's' : ''}` : 'Unconfirmed';
        const confClass = confirmations >= 6 ? 'confirmed' : 'pending';

        return `
            <div class="block-card-compact" onclick="app.showSection('block-detail', {height: ${block.height}})">
                <div class="block-info">
                    <span class="block-height-badge">#${block.height}</span>
                    <div class="block-meta">
                        <span class="block-txcount">${txCount} transaction${txCount !== 1 ? 's' : ''}</span>
                        <span class="block-hash">${block.btc_txid}</span>
                    </div>
                </div>
                <div class="block-status">
                    <span class="conf-badge ${confClass}">${confLabel}</span>
                </div>
            </div>
        `;
    }

    renderMempoolGroup(title, txs, status) {
        return `
            <div class="mempool-group">
                <div class="mempool-group-header">
                    <span class="mempool-group-title">${title}</span>
                    <span class="mempool-group-count">${txs.length}</span>
                </div>
                ${txs.map(tx => `
                    <div class="tx-row-compact">
                        <span class="tx-sender">Account #${tx.sender_id}</span>
                        <span class="tx-arrow">→</span>
                        <span class="tx-recipient">
                            <a onclick="app.showSection('account-detail', {pk: '${tx.recipient_pk}'})">${this.renderTruncatablePk(tx.recipient_pk, 8)}</a>
                        </span>
                        <span class="tx-amount">${tx.amount} sats</span>
                        <span class="tx-status-badge ${status}">${status}</span>
                    </div>
                `).join('')}
            </div>
        `;
    }

    renderEmptyState() {
        return `
            <div class="empty-state-compact">
                <div class="empty-icon">&#9633;</div>
                <p class="empty-title">No Blocks Yet</p>
                <p class="empty-desc">This chain is brand new. Send a transaction to create the first block.</p>
            </div>
        `;
    }

    renderError(error) {
        if (error.message === 'INDEXER_UNAVAILABLE') {
            return `
                <div class="empty-state-compact">
                    <div class="empty-icon" style="border-color: var(--coins-danger);">&#9888;</div>
                    <p class="empty-title">Indexer Unavailable</p>
                    <p class="empty-desc">Cannot connect to the indexer service. Please check that it's running.</p>
                </div>
            `;
        }
        return `
            <div class="empty-state-compact">
                <div class="empty-icon" style="border-color: var(--coins-danger);">&#9888;</div>
                <p class="empty-title">Error</p>
                <p class="empty-desc">${error.message}</p>
            </div>
        `;
    }

    renderPagination(currentPage, totalPages) {
        let buttons = '';

        // Previous button
        buttons += `<button class="page-btn" onclick="app.loadBlocks(${currentPage - 1})" ${currentPage === 0 ? 'disabled' : ''}>← Prev</button>`;

        // Page numbers
        const maxVisible = 5;
        let start = Math.max(0, currentPage - Math.floor(maxVisible / 2));
        let end = Math.min(totalPages, start + maxVisible);
        if (end - start < maxVisible) start = Math.max(0, end - maxVisible);

        for (let i = start; i < end; i++) {
            buttons += `<button class="page-btn ${i === currentPage ? 'active' : ''}" onclick="app.loadBlocks(${i})">${i + 1}</button>`;
        }

        // Next button
        buttons += `<button class="page-btn" onclick="app.loadBlocks(${currentPage + 1})" ${currentPage >= totalPages - 1 ? 'disabled' : ''}>Next →</button>`;

        return `<div class="pagination-compact">${buttons}</div>`;
    }

    // ========================================
    // Service Status
    // ========================================

    async checkServiceStatus() {
        // Check indexer
        try {
            const response = await fetch(this.apiBase + '/stats');
            this.serviceStatus.indexer = response.ok;
        } catch (e) {
            this.serviceStatus.indexer = false;
        }

        // Check publisher and get loop timing
        try {
            const response = await fetch('/api/publisher/status');
            if (response.ok) {
                const data = await response.json();
                this.serviceStatus.publisher = true;
                this.secsUntilNextLoop = data.secs_until_next_loop;
                this.intervalSecs = data.interval_secs || 60;
            } else {
                this.serviceStatus.publisher = false;
                this.secsUntilNextLoop = null;
            }
        } catch (e) {
            this.serviceStatus.publisher = false;
            this.secsUntilNextLoop = null;
        }

        this.updateStatusIndicator();
    }

    updateStatusIndicator() {
        const mainDot = document.getElementById('connection-dot');
        const indexerDot = document.getElementById('indexer-status-dot');
        const indexerText = document.getElementById('indexer-status-text');
        const publisherDot = document.getElementById('publisher-status-dot');
        const publisherText = document.getElementById('publisher-status-text');
        const wsDot = document.getElementById('ws-status-dot');
        const wsText = document.getElementById('ws-status-text');

        // Update individual service indicators
        if (indexerDot) {
            indexerDot.classList.toggle('connected', this.serviceStatus.indexer);
            indexerDot.classList.toggle('disconnected', !this.serviceStatus.indexer);
        }
        if (indexerText) {
            indexerText.textContent = this.serviceStatus.indexer ? 'Connected' : 'Offline';
        }

        if (publisherDot) {
            publisherDot.classList.toggle('connected', this.serviceStatus.publisher);
            publisherDot.classList.toggle('disconnected', !this.serviceStatus.publisher);
        }
        if (publisherText) {
            publisherText.textContent = this.serviceStatus.publisher ? 'Connected' : 'Offline';
        }

        if (wsDot) {
            wsDot.classList.toggle('connected', this.serviceStatus.websocket);
            wsDot.classList.toggle('disconnected', !this.serviceStatus.websocket);
        }
        if (wsText) {
            wsText.textContent = this.serviceStatus.websocket ? 'Connected' : 'Disconnected';
        }

        // Update main status dot
        if (mainDot) {
            const allConnected = this.serviceStatus.indexer &&
                                 this.serviceStatus.publisher &&
                                 this.serviceStatus.websocket;
            const someConnected = this.serviceStatus.indexer ||
                                  this.serviceStatus.publisher ||
                                  this.serviceStatus.websocket;

            mainDot.classList.remove('disconnected', 'partial');
            if (!someConnected) {
                mainDot.classList.add('disconnected');
            } else if (!allConnected) {
                mainDot.classList.add('partial');
            }
        }

        this.updateNextBlockTimer();
    }

    updateNextBlockTimer() {
        const nextBlockText = document.getElementById('next-block-text');
        if (!nextBlockText) return;

        if (!this.serviceStatus.publisher || this.secsUntilNextLoop == null) {
            nextBlockText.textContent = '--';
            return;
        }

        const secs = this.secsUntilNextLoop;

        if (secs <= 0) {
            nextBlockText.textContent = 'Any moment...';
        } else if (secs >= 60) {
            const mins = Math.floor(secs / 60);
            const remSecs = secs % 60;
            nextBlockText.textContent = `~${mins}m ${remSecs}s`;
        } else {
            nextBlockText.textContent = `~${secs}s`;
        }

        // Decrement locally between status checks
        if (this.secsUntilNextLoop > 0) {
            this.secsUntilNextLoop--;
        }
    }

    // ========================================
    // Helpers
    // ========================================

    bytesToHex(bytes) {
        if (typeof bytes === 'string') return bytes;
        if (Array.isArray(bytes)) {
            return bytes.map(b => b.toString(16).padStart(2, '0')).join('');
        }
        return bytes;
    }

    formatPk(pk) {
        if (!pk || pk.length < 16) return pk;
        return pk.slice(0, 8) + '...' + pk.slice(-8);
    }

    // Render a public key with middle truncation that shows as much as possible
    renderTruncatablePk(pk, endChars = 8) {
        if (!pk || pk.length <= endChars * 2) {
            return `<span class="pk-truncate"><span class="pk-full">${pk}</span></span>`;
        }
        const start = pk.slice(0, -endChars);
        const end = pk.slice(-endChars);
        return `<span class="pk-truncate"><span class="pk-start">${start}</span><span class="pk-end">${end}</span></span>`;
    }

    async copyToClipboard(text, element) {
        try {
            await navigator.clipboard.writeText(text);
            if (element) {
                element.classList.add('copied');
                setTimeout(() => element.classList.remove('copied'), 1000);
            }
        } catch (err) {
            console.error('Failed to copy:', err);
        }
    }

    getBitcoinExplorerUrl(txid) {
        if (!txid) return null;
        switch (this.network.toLowerCase()) {
            case 'mutinynet':
                return `https://mutinynet.com/tx/${txid}`;
            case 'signet':
                return `https://mempool.space/signet/tx/${txid}`;
            case 'testnet':
                return `https://mempool.space/testnet/tx/${txid}`;
            case 'mainnet':
            case 'bitcoin':
                return `https://mempool.space/tx/${txid}`;
            default:
                return null;
        }
    }

    // Format large numbers compactly (e.g., 1000000000000 -> "1T")
    formatLargeNumber(num) {
        if (num >= 1e12) return (num / 1e12).toFixed(num % 1e12 === 0 ? 0 : 1) + 'T';
        if (num >= 1e9) return (num / 1e9).toFixed(num % 1e9 === 0 ? 0 : 1) + 'B';
        if (num >= 1e6) return (num / 1e6).toFixed(num % 1e6 === 0 ? 0 : 1) + 'M';
        if (num >= 1e3) return (num / 1e3).toFixed(num % 1e3 === 0 ? 0 : 1) + 'K';
        return num.toString();
    }

    // Render balances with adaptive display based on token count
    renderBalances(balances) {
        if (balances.length === 0) {
            return `
                <div class="tx-history-header" style="margin-top: 1.5rem;">Balances</div>
                <p class="has-text-grey" style="text-align: center; padding: 1rem;">No tokens</p>
            `;
        }

        // Few tokens (1-4): simple grid
        if (balances.length <= 4) {
            return `
                <div class="tx-history-header" style="margin-top: 1.5rem;">Balances</div>
                <div class="balances-grid">
                    ${balances.map(([tokenId, balance]) => `
                        <div class="balance-item${tokenId === '0' ? ' native' : ''}">
                            <div class="balance-amount">${this.formatLargeNumber(balance)}</div>
                            <div class="balance-token">${tokenId === '0' ? 'Native' : `Token ${tokenId}`}</div>
                        </div>
                    `).join('')}
                </div>
            `;
        }

        // Many tokens (5+): summary + horizontal scroll
        const totalBalance = balances.reduce((sum, [_, bal]) => sum + bal, 0);
        const tokenCount = balances.length;
        const showSummary = tokenCount > 10;

        let html = `<div class="balances-header">
            <span class="tx-history-header" style="margin: 0; padding: 0; border: none;">Balances</span>
            ${tokenCount > 15 ? `<button class="balances-toggle${this.balancesExpanded ? ' expanded' : ''}" onclick="app.toggleBalancesExpanded()">
                <span>${this.balancesExpanded ? 'Hide' : 'Show All'}</span>
                <span class="balances-toggle-icon">▼</span>
            </button>` : ''}
        </div>`;

        // Summary bar for many tokens
        if (showSummary) {
            const previewTokens = balances.slice(0, 4);
            const moreCount = tokenCount - 4;
            html += `
                <div class="balances-summary">
                    <div class="summary-stat">
                        <span class="summary-stat-value">${tokenCount}</span>
                        <span class="summary-stat-label">Token Types</span>
                    </div>
                    <div class="summary-divider"></div>
                    <div class="summary-stat">
                        <span class="summary-stat-value">~${this.formatLargeNumber(totalBalance)}</span>
                        <span class="summary-stat-label">Total Balance</span>
                    </div>
                    <div class="summary-divider"></div>
                    <div class="summary-tokens-preview">
                        ${previewTokens.map(([tokenId]) => `
                            <span class="token-id-chip${tokenId === '0' ? ' native' : ''}">${tokenId === '0' ? 'Native' : `#${tokenId}`}</span>
                        `).join('')}
                        <span class="tokens-more">+${moreCount} more</span>
                    </div>
                </div>
            `;
        }

        // Horizontal scroll strip
        html += `
            <div class="balances-scroll-container">
                <div class="balances-strip">
                    ${balances.map(([tokenId, balance]) => `
                        <div class="balance-chip${tokenId === '0' ? ' native' : ''}">
                            <span class="balance-chip-amount">${this.formatLargeNumber(balance)}</span>
                            <span class="balance-chip-token">${tokenId === '0' ? 'Native' : `Token ${tokenId}`}</span>
                        </div>
                    `).join('')}
                </div>
            </div>
        `;

        // Expanded grid (hidden by default, preserves state across re-renders)
        if (tokenCount > 15) {
            html += `
                <div class="balances-grid-expanded${this.balancesExpanded ? ' visible' : ''}" id="balances-expanded">
                    ${balances.map(([tokenId, balance]) => `
                        <div class="balance-item-compact">
                            <span class="token-label${tokenId === '0' ? ' native' : ''}">${tokenId === '0' ? 'Native' : `#${tokenId}`}</span>
                            <span class="token-amount">${this.formatLargeNumber(balance)}</span>
                        </div>
                    `).join('')}
                </div>
            `;
        }

        return html;
    }

    toggleBalancesExpanded() {
        const grid = document.getElementById('balances-expanded');
        const btn = document.querySelector('.balances-toggle');
        if (!grid || !btn) return;

        this.balancesExpanded = !this.balancesExpanded;
        grid.classList.toggle('visible', this.balancesExpanded);
        btn.classList.toggle('expanded', this.balancesExpanded);
        btn.querySelector('span:first-child').textContent = this.balancesExpanded ? 'Hide' : 'Show All';
    }

    // Save scroll position before re-render
    saveScrollState() {
        const strip = document.querySelector('.balances-strip');
        const expandedGrid = document.getElementById('balances-expanded');
        const txTable = document.querySelector('.tx-table');

        this.scrollState = {
            balancesStrip: strip?.scrollLeft || 0,
            expandedGrid: expandedGrid?.scrollTop || 0,
            txTable: txTable?.parentElement?.scrollTop || 0
        };
    }

    // Restore scroll position after re-render
    restoreScrollState() {
        if (!this.scrollState) return;

        requestAnimationFrame(() => {
            const strip = document.querySelector('.balances-strip');
            const expandedGrid = document.getElementById('balances-expanded');
            const txTable = document.querySelector('.tx-table');

            if (strip) strip.scrollLeft = this.scrollState.balancesStrip;
            if (expandedGrid) expandedGrid.scrollTop = this.scrollState.expandedGrid;
            if (txTable?.parentElement) txTable.parentElement.scrollTop = this.scrollState.txTable;
        });
    }
}

// Initialize
const app = new ExplorerApp();
