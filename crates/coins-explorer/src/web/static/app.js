class ExplorerApp {
    constructor() {
        this.ws = null;
        this.currentSection = 'overview';
        this.previousSection = null;
        this.currentPage = 0;
        this.apiBase = '/api/v1';
        this.network = 'regtest';
        this.currentAccountPk = null;
        this.secsUntilNextLoop = null;
        this.intervalSecs = 60;

        // Service status
        this.serviceStatus = {
            indexer: false,
            publisher: false,
            websocket: false
        };

        // Initialize
        this.initWebSocket();
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
        }

        // Update URL
        if (pushState) {
            let hash = section;
            if (section === 'block-detail' && params.height !== undefined) {
                hash = `block/${params.height}`;
            } else if (section === 'account-detail' && params.pk) {
                hash = `account/${params.pk}`;
            } else if (section === 'blocks' && params.page) {
                hash = `blocks/${params.page}`;
            }
            history.pushState({ section, params }, '', `#${hash}`);
        }
    }

    goBack() {
        history.back();
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
            this.showSection('overview', {}, false);
            return;
        }

        const parts = hash.split('/');
        const section = parts[0];

        if (section === 'block' && parts[1]) {
            this.showSection('block-detail', { height: parseInt(parts[1]) }, false);
        } else if (section === 'account' && parts[1]) {
            this.showSection('account-detail', { pk: parts[1] }, false);
        } else if (section === 'blocks') {
            this.showSection('blocks', { page: parseInt(parts[1]) || 0 }, false);
        } else if (['overview', 'mempool'].includes(section)) {
            this.showSection(section, {}, false);
        } else {
            this.showSection('overview', {}, false);
        }
    }

    globalSearch() {
        const input = document.getElementById('global-search');
        const query = input.value.trim();
        if (!query) return;

        if (/^\d+$/.test(query)) {
            this.showSection('block-detail', { height: parseInt(query) });
            input.value = '';
        } else if (/^[0-9a-fA-F]+$/.test(query)) {
            this.showSection('account-detail', { pk: query });
            input.value = '';
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
                    } catch (e) {}
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
                                <div class="block-tx-row">
                                    <span class="block-tx-index">${index + 1}</span>
                                    <span class="block-tx-flow">
                                        <span class="block-tx-account" ${senderPk ? `onclick="app.showSection('account-detail', {pk: '${senderPk}'})"` : ''}>#${tx.sender_id}</span>
                                        <span class="block-tx-arrow">→</span>
                                        <span class="block-tx-account" onclick="app.showSection('account-detail', {pk: '${recipientPk}'})">${hasRecipientId ? `#${recipientId}` : 'New'}</span>
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

    async loadAccountDetail(pk) {
        this.currentAccountPk = pk;
        const container = document.getElementById('account-detail-content');

        try {
            const account = await this.fetchAPI(`/accounts/${pk}`);
            if (!account) {
                container.innerHTML = '<p class="has-text-grey">Account not found</p>';
                return;
            }

            const pkHex = this.bytesToHex(account.pk);
            const balances = account.balances ? Object.entries(account.balances).sort((a, b) => parseInt(a[0]) - parseInt(b[0])) : [];

            let html = `
                <div class="account-header">
                    <span class="account-id">Account #${account.id}</span>
                </div>
                <div class="account-pk">${this.renderTruncatablePk(pkHex, 12)}</div>

                <div class="tx-history-header" style="margin-top: 1.5rem;">Balances</div>
                <div class="balances-grid">
                    ${balances.length > 0 ? balances.map(([tokenId, balance]) => `
                        <div class="balance-item">
                            <div class="balance-amount">${balance}</div>
                            <div class="balance-token">${tokenId === '0' ? 'Native' : `Token ${tokenId}`}</div>
                        </div>
                    `).join('') : '<p class="has-text-grey" style="grid-column: 1/-1; text-align: center;">No tokens</p>'}
                </div>
            `;

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

                        return `
                            <div class="tx-table-row">
                                <span class="tx-type-badge ${isIncoming ? 'received' : 'sent'}">${isIncoming ? 'Received' : 'Sent'}</span>
                                <span class="tx-counterparty-cell"><span class="tx-counterparty-wrapper" onclick="app.showSection('account-detail', {pk: '${counterpartyPk}'})"><span class="tx-counterparty-pk">${this.renderTruncatablePk(counterpartyPk, 8)}</span>${hasId ? `<span class="tx-counterparty-divider">|</span><span class="tx-counterparty-id">#${counterpartyId}</span>` : ''}</span></span>
                                <span class="tx-table-amount ${amountClass}">${amountPrefix}${tx.amount}</span>
                                <span class="tx-table-token">${tx.token_id > 0 ? `Token ${tx.token_id}` : 'Native'}</span>
                                ${statusBadge}
                            </div>
                        `;
                    }).join('') : '<p class="has-text-grey" style="text-align: center; padding: 1rem;">No transactions</p>'}
                </div>
            `;

            container.innerHTML = html;
        } catch (error) {
            container.innerHTML = this.renderError(error);
        }
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
}

// Initialize
const app = new ExplorerApp();
