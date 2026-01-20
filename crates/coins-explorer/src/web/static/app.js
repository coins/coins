class ExplorerApp {
    constructor() {
        this.ws = null;
        this.currentView = 'home';
        this.currentPage = 0;
        this.apiBase = '/api/v1';
        this.network = 'regtest'; // Default, will be updated from API
        this.currentAccountPk = null; // Track current account for targeted refreshes

        // Initialize WebSocket for live updates
        this.initWebSocket();

        // Fetch network info
        this.fetchNetworkInfo();

        // Navigate to hash or home
        this.navigateFromHash();

        // Handle browser back/forward and hash changes
        window.addEventListener('popstate', () => {
            this.navigateFromHash();
        });
        window.addEventListener('hashchange', () => {
            this.navigateFromHash();
        });
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

    getBitcoinExplorerUrl(txid) {
        // Return explorer URL based on network
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
            case 'regtest':
            default:
                return null; // No public explorer for regtest
        }
    }

    bytesToHex(bytes) {
        // Convert byte array to hex string
        if (Array.isArray(bytes)) {
            return bytes.map(b => b.toString(16).padStart(2, '0')).join('');
        }
        return bytes;
    }

    formatPublicKey(pk) {
        // Format long public keys by showing first 8 and last 8 characters
        if (typeof pk === 'string' && pk.length > 16) {
            return pk.slice(0, 8) + '...' + pk.slice(-8);
        }
        return pk;
    }

    navigateFromHash() {
        const hash = window.location.hash.slice(1); // Remove #
        if (!hash) {
            this.navigate('home', {}, false);
            return;
        }

        const parts = hash.split('/');
        const view = parts[0];
        const params = {};

        // Parse parameters based on view type
        if (view === 'block' && parts[1]) {
            params.height = parseInt(parts[1]);
        } else if (view === 'account' && parts[1]) {
            params.pk = parts[1];
        } else if (view === 'blocks' && parts[1]) {
            params.page = parseInt(parts[1]) || 0;
        }

        this.navigate(view, params, false);
    }

    initWebSocket() {
        const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
        const wsUrl = `${protocol}//${window.location.host}/ws`;

        this.ws = new WebSocket(wsUrl);

        this.ws.onopen = () => {
            console.log('WebSocket connected');
            document.getElementById('connection-status').textContent = '● Connected';
            document.getElementById('connection-status').className = 'tag is-success';
        };

        this.ws.onmessage = (event) => {
            const msg = JSON.parse(event.data);
            this.handleWSMessage(msg);
        };

        this.ws.onclose = () => {
            console.log('WebSocket disconnected, reconnecting...');
            document.getElementById('connection-status').textContent = '● Disconnected';
            document.getElementById('connection-status').className = 'tag is-danger is-disconnected';
            setTimeout(() => this.initWebSocket(), 3000);
        };

        this.ws.onerror = (error) => {
            console.error('WebSocket error:', error);
        };
    }

    handleWSMessage(msg) {
        console.log('WebSocket message:', msg);

        switch (msg.type) {
            case 'new_block':
            case 'stats_update':
                // Targeted update for home page stats
                if (this.currentView === 'home') {
                    this.updateHomeStats();
                } else if (this.currentView === 'blocks') {
                    // For blocks list, need full re-render to show new block
                    this.navigate(this.currentView, {}, false);
                }
                // Update account balance if viewing an account
                if (this.currentView === 'account' && this.currentAccountPk) {
                    this.updateAccountBalance(this.currentAccountPk);
                    this.updateTransactionStatuses(this.currentAccountPk);
                }
                break;

            case 'pending_txs_update':
                // Targeted update for home page stats
                if (this.currentView === 'home') {
                    this.updateHomeStats();
                } else if (this.currentView === 'mempool') {
                    // Mempool view needs full re-render for new/removed txs
                    this.navigate('mempool', {}, false);
                }
                // Update transaction statuses in account view
                if (this.currentView === 'account' && this.currentAccountPk) {
                    this.updateTransactionStatuses(this.currentAccountPk);
                }
                break;

            case 'confirmation_update':
                // Refresh account view and mempool view for confirmation status updates
                if (this.currentView === 'account' && this.currentAccountPk) {
                    this.updateTransactionStatuses(this.currentAccountPk);
                } else if (this.currentView === 'mempool') {
                    this.navigate('mempool', {}, false);
                }
                break;
        }
    }

    // Targeted update: refresh only stats on home page
    async updateHomeStats() {
        try {
            const stats = await this.fetchAPI('/stats');
            let pendingCount = 0;
            try {
                const pending = await this.fetchAPI('/pending-transactions') || [];
                pendingCount = pending.length;
            } catch (e) {
                console.warn('Could not fetch pending transactions:', e);
            }

            const blocksEl = document.querySelector('[data-stat="total-blocks"]');
            const accountsEl = document.querySelector('[data-stat="total-accounts"]');
            const supplyEl = document.querySelector('[data-stat="total-supply"]');
            const pendingEl = document.querySelector('[data-stat="pending-txs"]');

            if (blocksEl) blocksEl.textContent = stats.total_blocks || 0;
            if (accountsEl) accountsEl.textContent = stats.total_accounts || 0;
            if (supplyEl) supplyEl.textContent = stats.total_supply || 0;
            if (pendingEl) pendingEl.textContent = pendingCount;
        } catch (error) {
            console.warn('Could not update home stats:', error);
        }
    }

    // Targeted update: refresh only account balance
    async updateAccountBalance(pk) {
        try {
            const account = await this.fetchAPI(`/accounts/${pk}`);
            const balanceEl = document.querySelector('[data-account-balance]');
            if (balanceEl && account) {
                balanceEl.textContent = `${account.balance} sats`;
            }
        } catch (error) {
            console.warn('Could not update account balance:', error);
        }
    }

    // Targeted update: refresh transaction statuses (re-renders tx table only)
    async updateTransactionStatuses(pk) {
        await this.loadAccountTransactions(pk);
    }

    showNewBlockNotification(block) {
        const notification = document.createElement('div');
        notification.className = 'notification is-success is-light is-new-block';
        notification.innerHTML = `
            <button class="delete" onclick="this.parentElement.remove()"></button>
            <strong>New Block!</strong><br>
            Height: ${block.btc_height} | Transactions: ${block.tx_count}
        `;

        document.body.appendChild(notification);

        setTimeout(() => {
            notification.remove();
        }, 5000);
    }

    async navigate(view, params = {}, pushState = true) {
        this.currentView = view;

        // Clear account tracking when leaving account view
        if (view !== 'account') {
            this.currentAccountPk = null;
        }

        if (pushState) {
            // Build hash URL with parameters
            let hash = view;
            if (view === 'block' && params.height !== undefined) {
                hash = `block/${params.height}`;
            } else if (view === 'account' && params.pk) {
                hash = `account/${params.pk}`;
            } else if (view === 'blocks' && params.page) {
                hash = `blocks/${params.page}`;
            }
            history.pushState({ view, params }, '', `#${hash}`);
        }

        const content = document.getElementById('content');
        content.innerHTML = '<div class="loading"></div>';

        try {
            switch (view) {
                case 'home':
                    await this.renderHome();
                    break;
                case 'blocks':
                    await this.renderBlocks(params.page || 0);
                    break;
                case 'block':
                    await this.renderBlock(params.height);
                    break;
                case 'account':
                    await this.renderAccount(params.pk);
                    break;
                case 'mempool':
                    await this.renderMempool();
                    break;
                default:
                    await this.renderHome();
            }
        } catch (error) {
            if (error.message === 'INDEXER_UNAVAILABLE') {
                content.innerHTML = `
                    <div class="modal is-active">
                        <div class="modal-background"></div>
                        <div class="modal-content">
                            <div class="box" style="max-width: 600px; margin: 0 auto;">
                                <article class="message is-warning">
                                    <div class="message-header">
                                        <p>⚠️ Indexer Service Unavailable</p>
                                    </div>
                                    <div class="message-body">
                                        <p><strong>The Coins indexer service is not reachable.</strong></p>
                                        <br>
                                        <p>This could mean:</p>
                                        <ul style="margin-left: 1.5rem; margin-top: 0.5rem;">
                                            <li>The indexer service is not running</li>
                                            <li>The indexer is starting up</li>
                                            <li>There's a network connectivity issue</li>
                                        </ul>
                                        <br>
                                        <p><strong>To fix this:</strong></p>
                                        <ol style="margin-left: 1.5rem; margin-top: 0.5rem;">
                                            <li>Check if the indexer is running on port 8083</li>
                                            <li>Start the indexer with: <code>./target/release/coins-indexer --config config/indexer-regtest.toml</code></li>
                                            <li>Or run the setup script: <code>./scripts/setup-regtest.sh</code></li>
                                        </ol>
                                        <br>
                                        <div style="text-align: center;">
                                            <button class="button is-primary" onclick="location.reload()">
                                                🔄 Retry Connection
                                            </button>
                                        </div>
                                    </div>
                                </article>
                            </div>
                        </div>
                    </div>
                `;
            } else {
                content.innerHTML = `
                    <div class="notification is-danger">
                        <strong>Error:</strong> ${error.message}
                    </div>
                `;
            }
        }
    }

    async renderHome() {
        const stats = await this.fetchAPI('/stats');
        const latestBlock = await this.fetchAPI('/blocks/latest');
        const content = document.getElementById('content');

        // Fetch pending transaction counts
        let pendingCount = 0;
        try {
            const pending = await this.fetchAPI('/pending-transactions') || [];
            pendingCount = pending.length;
        } catch (e) {
            console.warn('Could not fetch pending transactions:', e);
        }

        content.innerHTML = `
            <h1 class="title">Network Statistics</h1>

            <div class="stats-grid">
                <div class="stat-card">
                    <div class="stat-value" data-stat="total-blocks">${stats.total_blocks || 0}</div>
                    <div class="stat-label">Total Blocks</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value" data-stat="total-accounts">${stats.total_accounts || 0}</div>
                    <div class="stat-label">Total Accounts</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value" data-stat="total-supply">${stats.total_supply || 0}</div>
                    <div class="stat-label">Total Supply</div>
                </div>
                <div class="stat-card" onclick="app.navigate('mempool')" style="cursor: pointer;">
                    <div class="stat-value" data-stat="pending-txs">${pendingCount}</div>
                    <div class="stat-label">Pending Txs</div>
                </div>
            </div>

            <h2 class="title is-4">Latest Block</h2>
            ${latestBlock ? this.renderBlockCard(latestBlock) : '<p>No blocks yet</p>'}

            <div style="text-align: center; margin: 1rem 0;">
                <a onclick="app.navigate('blocks')" class="button is-primary is-medium">
                    📋 Browse All Blocks
                </a>
            </div>

            <h2 class="title is-4">Search Account</h2>
            <div class="box">
                <div class="field has-addons">
                    <div class="control is-expanded">
                        <input class="input" type="text" id="account-search" placeholder="Enter public key (hex)...">
                    </div>
                    <div class="control">
                        <button class="button is-primary" onclick="app.searchAccount()">
                            Search
                        </button>
                    </div>
                </div>
            </div>

            <div id="account-result"></div>
        `;
    }

    renderBlockCard(block) {
        const txCount = block.sub_block && block.sub_block.txs ? block.sub_block.txs.length : 0;
        return `
            <div class="box">
                <table class="table is-fullwidth">
                    <tr>
                        <th>Height</th>
                        <td>
                            <a onclick="app.navigate('block', {height: ${block.height}})" style="cursor: pointer; color: #3273dc;">
                                <strong>${block.height}</strong>
                            </a>
                        </td>
                    </tr>
                    <tr>
                        <th>Bitcoin Txid</th>
                        <td><code>${block.btc_txid}</code></td>
                    </tr>
                    <tr>
                        <th>Transactions</th>
                        <td>${txCount}</td>
                    </tr>
                </table>
                <a onclick="app.navigate('block', {height: ${block.height}})" class="button is-primary is-fullwidth" style="margin-top: 1rem;">
                    View Block Details
                </a>
            </div>
        `;
    }

    async searchAccount() {
        const input = document.getElementById('account-search');
        const pk = input.value.trim();

        if (!pk) {
            alert('Please enter a public key');
            return;
        }

        const resultDiv = document.getElementById('account-result');
        resultDiv.innerHTML = '<div class="loading"></div>';

        try {
            const account = await this.fetchAPI(`/accounts/${pk}`);
            const pk_hex = this.bytesToHex(account.pk);
            resultDiv.innerHTML = `
                <div class="box">
                    <h3 class="title is-5">Account Details</h3>
                    <table class="table is-fullwidth">
                        <tr>
                            <th>Account ID</th>
                            <td><strong>${account.id.toString()}</strong></td>
                        </tr>
                        <tr>
                            <th>Public Key</th>
                            <td><code style="font-size: 0.85em; word-break: break-all;">${pk_hex}</code></td>
                        </tr>
                        <tr>
                            <th>Balance</th>
                            <td><strong style="color: #23d160;">${account.balance} sats</strong></td>
                        </tr>
                        <tr>
                            <th>Nonce</th>
                            <td>${account.nonce}</td>
                        </tr>
                    </table>
                    <a onclick="app.navigate('account', {pk: '${pk_hex}'})" class="button is-link is-fullwidth">
                        View Full Account Page
                    </a>
                </div>
            `;
        } catch (error) {
            resultDiv.innerHTML = `
                <div class="notification is-danger">
                    Account not found or error: ${error.message}
                </div>
            `;
        }
    }

    async renderBlocks(page = 0) {
        const content = document.getElementById('content');
        const latestBlock = await this.fetchAPI('/blocks/latest');

        if (!latestBlock) {
            content.innerHTML = '<h1 class="title">Blocks</h1><p>No blocks yet</p>';
            return;
        }

        // Fetch ALL blocks (from 0 to latest height) - the API filters out non-existent ones
        const allBlocks = await this.fetchAPI(`/blocks?from=0&to=${latestBlock.height}`);

        // Sort descending by height
        allBlocks.sort((a, b) => b.height - a.height);

        // Paginate the actual blocks
        const blocksPerPage = 20;
        const totalBlocks = allBlocks.length;
        const totalPages = Math.ceil(totalBlocks / blocksPerPage);
        const startIdx = page * blocksPerPage;
        const endIdx = startIdx + blocksPerPage;
        const blocks = allBlocks.slice(startIdx, endIdx);

        content.innerHTML = `
            <h1 class="title">Blocks</h1>
            <div class="subtitle">Showing ${blocks.length} of ${totalBlocks} block(s) (page ${page + 1} of ${totalPages})</div>

            ${blocks.length > 0 ? `
                <table class="table is-fullwidth is-striped is-hoverable">
                    <thead>
                        <tr>
                            <th>Height</th>
                            <th>Bitcoin Txid</th>
                            <th>Transactions</th>
                            <th>Actions</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${blocks.map(block => {
                            const txCount = block.sub_block && block.sub_block.txs ? block.sub_block.txs.length : 0;
                            return `
                                <tr>
                                    <td><strong>${block.height}</strong></td>
                                    <td><code style="font-size: 0.85em;">${block.btc_txid.substring(0, 16)}...${block.btc_txid.substring(56)}</code></td>
                                    <td>${txCount}</td>
                                    <td>
                                        <a onclick="app.navigate('block', {height: ${block.height}})" class="button is-small is-primary">
                                            View Details
                                        </a>
                                    </td>
                                </tr>
                            `;
                        }).join('')}
                    </tbody>
                </table>
            ` : `<div class="notification is-info">No blocks found</div>`}

            ${this.renderPagination(page, totalPages)}
        `;
    }

    async renderMempool() {
        const content = document.getElementById('content');

        // Fetch all pending transactions from the unified endpoint
        let pendingTxs = [];
        try {
            pendingTxs = await this.fetchAPI('/pending-transactions') || [];
        } catch (e) {
            console.warn('Could not fetch pending transactions:', e);
        }

        // Group by status
        const publishingTxs = pendingTxs.filter(tx => tx.status === 'publishing');
        const broadcastingTxs = pendingTxs.filter(tx => tx.status === 'broadcasting');
        const unconfirmedTxs = pendingTxs.filter(tx => tx.status === 'unconfirmed');

        const totalPending = pendingTxs.length;

        content.innerHTML = `
            <h1 class="title">Mempool</h1>
            <p class="subtitle">${totalPending} pending transaction${totalPending !== 1 ? 's' : ''}</p>

            ${unconfirmedTxs.length > 0 ? `
                <h2 class="title is-4">Unconfirmed (${unconfirmedTxs.length})</h2>
                <p class="help">Transactions in Bitcoin mempool, indexed by indexer</p>
                ${this.renderPendingTxTable(unconfirmedTxs)}
            ` : ''}

            ${broadcastingTxs.length > 0 ? `
                <h2 class="title is-4"${unconfirmedTxs.length > 0 ? ' style="margin-top: 2rem;"' : ''}>Broadcasting (${broadcastingTxs.length})</h2>
                <p class="help">Transactions broadcast to Bitcoin, waiting for indexer discovery</p>
                ${this.renderPendingTxTable(broadcastingTxs)}
            ` : ''}

            ${publishingTxs.length > 0 ? `
                <h2 class="title is-4" style="margin-top: 2rem;">Publishing (${publishingTxs.length})</h2>
                <p class="help">Transactions in publisher mempool, waiting to be mined</p>
                ${this.renderPendingTxTable(publishingTxs)}
            ` : ''}

            ${totalPending === 0 ? `
                <div class="notification is-info">
                    <p>No pending transactions</p>
                </div>
            ` : ''}
        `;
    }

    renderMempoolTable(txs, status) {
        if (txs.length === 0) {
            return '<div class="notification is-light">No transactions</div>';
        }

        return `
            <div class="box">
                <table class="table is-fullwidth is-striped is-hoverable">
                    <thead>
                        <tr>
                            <th>Sender</th>
                            <th>Recipient</th>
                            <th>Amount</th>
                            <th>Fee</th>
                            <th>Status</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${txs.map(tx => {
                            const recipientPk = tx.recipient_pk;
                            const explorerUrl = status === 'broadcasting' && tx.btc_txid
                                ? this.getBitcoinExplorerUrl(tx.btc_txid)
                                : null;
                            const statusBadge = status === 'broadcasting'
                                ? (explorerUrl
                                    ? `<a href="${explorerUrl}" target="_blank" rel="noopener"><span class="tag is-link" style="cursor: pointer;">Broadcasting ↗</span></a>`
                                    : '<span class="tag is-link">Broadcasting</span>')
                                : '<span class="tag is-info">Publishing</span>';
                            return `
                                <tr>
                                    <td>Account #${tx.sender_id}</td>
                                    <td>
                                        <a onclick="app.navigate('account', {pk: '${recipientPk}'})" style="cursor: pointer; color: #3273dc;">
                                            <code style="font-size: 0.85em;">${recipientPk.substring(0, 12)}...${recipientPk.substring(56)}</code>
                                        </a>
                                    </td>
                                    <td><strong>${tx.amount} sats</strong></td>
                                    <td>${tx.fee} sats</td>
                                    <td>${statusBadge}</td>
                                </tr>
                            `;
                        }).join('')}
                    </tbody>
                </table>
            </div>
        `;
    }

    renderPendingTxTable(txs) {
        if (txs.length === 0) {
            return '<div class="notification is-light">No transactions</div>';
        }

        return `
            <div class="box">
                <table class="table is-fullwidth is-striped is-hoverable">
                    <thead>
                        <tr>
                            <th>Sender</th>
                            <th>Recipient</th>
                            <th>Amount</th>
                            <th>Fee</th>
                            <th>Status</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${txs.map(tx => {
                            const recipientPk = tx.recipient_pk;
                            const explorerUrl = tx.btc_txid ? this.getBitcoinExplorerUrl(tx.btc_txid) : null;

                            let statusBadge;
                            switch (tx.status) {
                                case 'unconfirmed':
                                    statusBadge = explorerUrl
                                        ? `<a href="${explorerUrl}" target="_blank" rel="noopener"><span class="tag is-warning" style="cursor: pointer;">Unconfirmed ↗</span></a>`
                                        : '<span class="tag is-warning">Unconfirmed</span>';
                                    break;
                                case 'broadcasting':
                                    statusBadge = explorerUrl
                                        ? `<a href="${explorerUrl}" target="_blank" rel="noopener"><span class="tag is-link" style="cursor: pointer;">Broadcasting ↗</span></a>`
                                        : '<span class="tag is-link">Broadcasting</span>';
                                    break;
                                case 'publishing':
                                default:
                                    statusBadge = '<span class="tag is-info">Publishing</span>';
                                    break;
                            }

                            return `
                                <tr>
                                    <td>Account #${tx.sender_id}</td>
                                    <td>
                                        <a onclick="app.navigate('account', {pk: '${recipientPk}'})" style="cursor: pointer; color: #3273dc;">
                                            <code style="font-size: 0.85em;">${recipientPk.substring(0, 12)}...${recipientPk.substring(56)}</code>
                                        </a>
                                    </td>
                                    <td><strong>${tx.amount} sats</strong></td>
                                    <td>${tx.fee} sats</td>
                                    <td>${statusBadge}</td>
                                </tr>
                            `;
                        }).join('')}
                    </tbody>
                </table>
            </div>
        `;
    }

    async renderBlock(height) {
        const block = await this.fetchAPI(`/blocks/${height}`);
        const content = document.getElementById('content');

        // Convert byte arrays to hex
        const publisher_pk_hex = this.bytesToHex(block.sub_block.publisher_pk);
        const txCount = block.sub_block && block.sub_block.txs ? block.sub_block.txs.length : 0;

        content.innerHTML = `
            <nav class="breadcrumb">
                <ul>
                    <li><a onclick="app.navigate('home')">Home</a></li>
                    <li><a onclick="app.navigate('blocks')">Blocks</a></li>
                    <li class="is-active"><a>Block ${height}</a></li>
                </ul>
            </nav>

            <h1 class="title">Block #${height}</h1>

            <div class="box">
                <table class="table is-fullwidth">
                    <tr>
                        <th>Height</th>
                        <td>${block.height}</td>
                    </tr>
                    <tr>
                        <th>Bitcoin Txid</th>
                        <td>
                            <code>${block.btc_txid}</code>
                            <div style="margin-top: 0.5rem;">
                                <a href="https://mutinynet.com/tx/${block.btc_txid}" target="_blank" class="button is-small is-link">
                                    View on Mutinynet Explorer
                                </a>
                            </div>
                        </td>
                    </tr>
                    <tr>
                        <th>Publisher</th>
                        <td><code style="font-size: 0.85em;">${publisher_pk_hex}</code></td>
                    </tr>
                    <tr>
                        <th>Transactions</th>
                        <td>${txCount}</td>
                    </tr>
                </table>
            </div>

            <h2 class="title is-4">Transactions (${txCount})</h2>

            ${txCount > 0 ? `
                <table class="table is-fullwidth is-striped is-hoverable">
                    <thead>
                        <tr>
                            <th>Sender ID</th>
                            <th>Recipient PK</th>
                            <th>Amount</th>
                            <th>Fee</th>
                        </tr>
                    </thead>
                    <tbody>
                        ${block.sub_block.txs.map(tx => {
                            const recipient_pk_hex = this.bytesToHex(tx.recipient_pk);
                            return `
                                <tr>
                                    <td><strong>${tx.sender_id}</strong></td>
                                    <td>
                                        <a onclick="app.navigate('account', {pk: '${recipient_pk_hex}'})" style="cursor: pointer; color: #3273dc;">
                                            <code style="font-size: 0.85em;">${recipient_pk_hex.substring(0, 16)}...${recipient_pk_hex.substring(56)}</code>
                                        </a>
                                    </td>
                                    <td>${tx.amount}</td>
                                    <td>${tx.fee}</td>
                                </tr>
                            `;
                        }).join('')}
                    </tbody>
                </table>
            ` : '<p>No transactions in this block</p>'}
        `;
    }

    bytesToHex(bytes) {
        return Array.from(bytes, byte => byte.toString(16).padStart(2, '0')).join('');
    }

    async renderAccount(pk) {
        // Track current account for WebSocket refreshes
        this.currentAccountPk = pk;

        const account = await this.fetchAPI(`/accounts/${pk}`);
        const content = document.getElementById('content');

        if (!account) {
            content.innerHTML = `
                <div class="notification is-warning">
                    <strong>Account Not Found</strong><br>
                    No account exists with public key: <code>${pk}</code>
                </div>
            `;
            return;
        }

        const pk_hex = this.bytesToHex(account.pk);

        content.innerHTML = `
            <nav class="breadcrumb">
                <ul>
                    <li><a onclick="app.navigate('home')">Home</a></li>
                    <li class="is-active"><a>Account #${account.id.toString()}</a></li>
                </ul>
            </nav>

            <h1 class="title">Account #${account.id.toString()}</h1>

            <div class="box">
                <table class="table is-fullwidth">
                    <tr>
                        <th style="width: 150px;">Account ID</th>
                        <td><strong>${account.id.toString()}</strong></td>
                    </tr>
                    <tr>
                        <th>Public Key</th>
                        <td><code style="font-size: 0.85em; word-break: break-all;">${pk_hex}</code></td>
                    </tr>
                    <tr>
                        <th>Balance</th>
                        <td><strong style="font-size: 1.2em; color: #23d160;" data-account-balance>${account.balance} sats</strong></td>
                    </tr>
                    <tr>
                        <th>Nonce</th>
                        <td>${account.nonce}</td>
                    </tr>
                </table>
            </div>

            <h2 class="title is-4" style="margin-top: 2rem;">Transaction History</h2>
            <div id="account-txs">
                <div class="loading"></div>
            </div>
        `;

        // Load transaction history
        this.loadAccountTransactions(pk_hex);
    }

    formatMempoolTx(tx, direction, currentPk) {
        return {
            sender_id: tx.sender_id,
            recipient_pk: tx.recipient_pk,  // Already hex from publisher API
            amount: tx.amount,
            fee: tx.fee,
            btc_height: 0,
            btc_txid: null,
            confirmations: 0,
            finalized: false,
            confirmations_remaining: 6,
            direction: direction,
            sender_pk: direction === 'outgoing' ? currentPk : null,
            is_mempool: true
        };
    }

    formatBroadcastingTx(tx, direction, currentPk) {
        return {
            sender_id: tx.sender_id,
            recipient_pk: tx.recipient_pk,  // Already hex from publisher API
            amount: tx.amount,
            fee: tx.fee,
            btc_height: 0,
            btc_txid: tx.btc_txid,  // Now available from publisher API
            confirmations: 0,
            finalized: false,
            confirmations_remaining: 6,
            direction: direction,
            sender_pk: direction === 'outgoing' ? currentPk : null,
            is_broadcasting: true  // In-flight to Bitcoin
        };
    }

    formatPendingTx(tx, direction, currentPk) {
        // Map backend status to frontend display flags
        const isMempool = tx.status === 'publishing';
        const isBroadcasting = tx.status === 'broadcasting';
        const isUnconfirmed = tx.status === 'unconfirmed';

        return {
            sender_id: tx.sender_id,
            recipient_pk: tx.recipient_pk,
            amount: tx.amount,
            fee: tx.fee,
            btc_height: 0,
            btc_txid: tx.btc_txid || null,
            confirmations: 0,
            finalized: false,
            confirmations_remaining: 6,
            direction: direction,
            sender_pk: direction === 'outgoing' ? currentPk : null,
            is_mempool: isMempool,
            is_broadcasting: isBroadcasting,
            is_unconfirmed: isUnconfirmed,
            status: tx.status
        };
    }

    async loadAccountTransactions(pk) {
        try {
            // Fetch indexed transactions
            const indexedTxs = await this.fetchAPI(`/accounts/${pk}/transactions`) || [];
            const container = document.getElementById('account-txs');

            // Save scroll position before updating
            const scrollY = window.scrollY;

            // Fetch pending transactions for this account (already deduplicated by backend)
            let pendingTxs = [];
            try {
                // Get account to find sender_id for outgoing tx lookup
                const account = await this.fetchAPI(`/accounts/${pk}`);

                if (account) {
                    // Fetch pending transactions - backend handles deduplication
                    const sentPending = await this.fetchAPI(`/pending-transactions?sender_id=${account.id}`) || [];
                    const receivedPending = await this.fetchAPI(`/pending-transactions?recipient_pk=${pk}`) || [];

                    // Format pending txs for display
                    pendingTxs = [
                        ...sentPending.map(tx => this.formatPendingTx(tx, 'outgoing', pk)),
                        ...receivedPending.map(tx => this.formatPendingTx(tx, 'incoming', pk))
                    ];

                    // Remove duplicates (same tx might appear in both sent and received if self-transfer)
                    const seenTxids = new Set();
                    pendingTxs = pendingTxs.filter(tx => {
                        if (tx.btc_txid && seenTxids.has(tx.btc_txid)) {
                            return false;
                        }
                        if (tx.btc_txid) {
                            seenTxids.add(tx.btc_txid);
                        }
                        return true;
                    });

                    // Filter out pending txs that are already in indexed (final dedup)
                    const indexedTxids = new Set(
                        indexedTxs
                            .filter(tx => tx.btc_txid)
                            .map(tx => tx.btc_txid)
                    );
                    pendingTxs = pendingTxs.filter(tx => !tx.btc_txid || !indexedTxids.has(tx.btc_txid));
                }
            } catch (pendingError) {
                console.warn('Could not fetch pending transactions:', pendingError);
                // Continue with just indexed txs
            }

            // Combine: indexed first, then pending txs at the bottom
            const transactions = [...indexedTxs, ...pendingTxs];

            if (!transactions || transactions.length === 0) {
                container.innerHTML = `
                    <div class="notification is-info">
                        <p>No transaction history found for this account.</p>
                        <p class="help">This shows all transactions involving this account (both sent and received).</p>
                    </div>
                `;
                window.scrollTo(0, scrollY);
                return;
            }

            container.innerHTML = `
                <div class="box">
                    <table class="table is-fullwidth is-striped is-hoverable">
                        <thead>
                            <tr>
                                <th>Type</th>
                                <th>Counterparty</th>
                                <th>Amount</th>
                                <th>Fee</th>
                                <th>Status</th>
                            </tr>
                        </thead>
                        <tbody>
                            ${transactions.map(tx => {
                                // Determine type badge and counterparty display
                                const isIncoming = tx.direction === 'incoming';
                                const typeBadge = isIncoming
                                    ? '<span class="tag is-success">Received</span>'
                                    : '<span class="tag is-danger">Sent</span>';

                                // Handle counterparty display differently for mempool/broadcasting vs indexed txs
                                const isPending = tx.is_mempool || tx.is_broadcasting;
                                let counterparty;
                                if (isPending && isIncoming) {
                                    // Pending incoming: show sender_id since we don't have sender_pk
                                    counterparty = `<span class="has-text-grey">From Account #${tx.sender_id}</span>`;
                                } else if (isIncoming) {
                                    // Indexed incoming: show sender_pk
                                    const senderPkHex = this.bytesToHex(tx.sender_pk);
                                    counterparty = `<a onclick="app.navigate('account', {pk: '${senderPkHex}'})" style="cursor: pointer; color: #3273dc;">
                                        <code style="font-size: 0.85em;">${senderPkHex.substring(0, 16)}...${senderPkHex.substring(56)}</code>
                                       </a>`;
                                } else {
                                    // Outgoing: show recipient_pk (works for both pending and indexed)
                                    const recipientPkHex = isPending ? tx.recipient_pk : this.bytesToHex(tx.recipient_pk);
                                    counterparty = `<a onclick="app.navigate('account', {pk: '${recipientPkHex}'})" style="cursor: pointer; color: #3273dc;">
                                        <code style="font-size: 0.85em;">${recipientPkHex.substring(0, 16)}...${recipientPkHex.substring(56)}</code>
                                       </a>`;
                                }

                                // Color amount based on direction
                                const amountColor = isIncoming ? '#23d160' : '#ff3860';

                                let statusBadge;
                                // Get explorer URL if we have a btc_txid (even if btc_height is 0 for mempool)
                                const explorerUrl = tx.btc_txid ? this.getBitcoinExplorerUrl(tx.btc_txid) : null;
                                const isClickable = explorerUrl !== null;
                                const clickableStyle = isClickable ? 'cursor: pointer;' : '';
                                const onclickAttr = isClickable ? `onclick="window.open('${explorerUrl}', '_blank')"` : '';
                                const hoverAttrs = isClickable ? `onmouseover="this.style.filter='brightness(0.85)'" onmouseout="this.style.filter=''"` : '';

                                if (tx.is_mempool) {
                                    // Mempool transaction - in publisher mempool
                                    statusBadge = `<span class="status-tooltip">
                                           <span class="tag is-info" style="cursor: help;">Publishing</span>
                                           <span class="tooltip-text">In Publisher Mempool</span>
                                       </span>`;
                                } else if (tx.is_broadcasting) {
                                    // Recently broadcast - in flight to Bitcoin
                                    statusBadge = `<span class="status-tooltip">
                                           <span class="tag is-link" style="cursor: help;">Broadcasting</span>
                                           <span class="tooltip-text">Broadcast to Bitcoin, waiting for indexer</span>
                                       </span>`;
                                } else if (tx.is_unconfirmed) {
                                    // Indexed but in Bitcoin mempool
                                    statusBadge = `<span class="status-tooltip">
                                           <span class="tag is-warning" style="cursor: help; ${clickableStyle}" ${onclickAttr} ${hoverAttrs}>Unconfirmed</span>
                                           <span class="tooltip-text">In Bitcoin mempool, indexed by indexer${isClickable ? ' (click to view on Bitcoin explorer)' : ''}</span>
                                       </span>`;
                                } else if (tx.finalized) {
                                    statusBadge = `<span class="tag is-success" style="${clickableStyle}" ${onclickAttr} ${hoverAttrs} title="${isClickable ? 'View on Bitcoin explorer' : ''}">Confirmed</span>`;
                                } else if (tx.confirmations === 0) {
                                    // Transaction is in Bitcoin mempool (btc_height = 0)
                                    statusBadge = `<span class="status-tooltip">
                                           <span class="tag is-warning" style="cursor: help; ${clickableStyle}" ${onclickAttr} ${hoverAttrs}>Unconfirmed</span>
                                           <span class="tooltip-text">in Bitcoin mempool${isClickable ? ' (click to view on Bitcoin explorer)' : ''}</span>
                                       </span>`;
                                } else {
                                    // Transaction has some confirmations but not finalized yet
                                    statusBadge = `<span class="status-tooltip">
                                           <span class="tag is-warning" style="cursor: help; ${clickableStyle}" ${onclickAttr} ${hoverAttrs}>Unconfirmed</span>
                                           <span class="tooltip-text">${tx.confirmations_remaining} block${tx.confirmations_remaining !== 1 ? 's' : ''} remaining${isClickable ? ' (click to view on Bitcoin explorer)' : ''}</span>
                                       </span>`;
                                }

                                return `
                                    <tr>
                                        <td>${typeBadge}</td>
                                        <td>${counterparty}</td>
                                        <td><strong style="color: ${amountColor};">${isIncoming ? '+' : '-'}${tx.amount} sats</strong></td>
                                        <td>${tx.fee} sats</td>
                                        <td>
                                            ${statusBadge}
                                        </td>
                                    </tr>
                                `;
                            }).join('')}
                        </tbody>
                    </table>
                    <p class="help">
                        Showing ${transactions.length} transaction${transactions.length !== 1 ? 's' : ''}
                        (${transactions.filter(tx => tx.direction === 'incoming').length} received,
                        ${transactions.filter(tx => tx.direction === 'outgoing').length} sent)
                        ${transactions.filter(tx => tx.is_mempool).length > 0
                            ? ` — ${transactions.filter(tx => tx.is_mempool).length} publishing`
                            : ''}
                        ${transactions.filter(tx => tx.is_broadcasting).length > 0
                            ? ` — ${transactions.filter(tx => tx.is_broadcasting).length} broadcasting`
                            : ''}
                        ${transactions.filter(tx => !tx.finalized && !tx.is_mempool && !tx.is_broadcasting).length > 0
                            ? ` — ${transactions.filter(tx => !tx.finalized && !tx.is_mempool && !tx.is_broadcasting).length} pending finalization`
                            : ''}
                    </p>
                </div>
            `;

            // Restore scroll position after content update
            window.scrollTo(0, scrollY);
        } catch (error) {
            const container = document.getElementById('account-txs');
            container.innerHTML = `
                <div class="notification is-warning">
                    Error loading transactions: ${error.message}
                </div>
            `;
        }
    }

    renderPagination(currentPage, totalPages, callback = null) {
        if (totalPages <= 1) return '';

        const pages = [];
        const maxVisible = 5;
        let start = Math.max(0, currentPage - Math.floor(maxVisible / 2));
        let end = Math.min(totalPages, start + maxVisible);

        if (end - start < maxVisible) {
            start = Math.max(0, end - maxVisible);
        }

        for (let i = start; i < end; i++) {
            pages.push(i);
        }

        return `
            <nav class="pagination is-centered pagination-wrapper" role="navigation">
                <a class="pagination-previous"
                   ${currentPage === 0 ? 'disabled' : ''}
                   onclick="${callback ? `(${callback})(${currentPage - 1})` : `app.navigate('${this.currentView}', {page: ${currentPage - 1}})`}">
                    Previous
                </a>
                <a class="pagination-next"
                   ${currentPage >= totalPages - 1 ? 'disabled' : ''}
                   onclick="${callback ? `(${callback})(${currentPage + 1})` : `app.navigate('${this.currentView}', {page: ${currentPage + 1}})`}">
                    Next
                </a>
                <ul class="pagination-list">
                    ${pages.map(page => `
                        <li>
                            <a class="pagination-link ${page === currentPage ? 'is-current' : ''}"
                               onclick="${callback ? `(${callback})(${page})` : `app.navigate('${this.currentView}', {page: ${page}})`}">
                                ${page + 1}
                            </a>
                        </li>
                    `).join('')}
                </ul>
            </nav>
        `;
    }

    async search() {
        const input = document.getElementById('search-input');
        const pk = input.value.trim();

        if (!pk) {
            alert('Please enter an account public key');
            return;
        }

        if (!/^[0-9a-fA-F]{64}$/.test(pk)) {
            alert('Invalid public key format. Must be 64 hex characters (32 bytes).');
            return;
        }

        this.navigate('account', { pk });
        input.value = '';
    }

    async fetchAPI(endpoint) {
        try {
            const response = await fetch(this.apiBase + endpoint);

            if (!response.ok) {
                // Check if it's a server error (500+) which usually means indexer is down
                if (response.status >= 500) {
                    throw new Error('INDEXER_UNAVAILABLE');
                }
                const text = await response.text();
                throw new Error(`API Error: ${response.status} - ${text}`);
            }

            return await response.json();
        } catch (error) {
            // Network errors (indexer completely unreachable)
            if (error.message === 'Failed to fetch' || error.name === 'TypeError') {
                throw new Error('INDEXER_UNAVAILABLE');
            }
            throw error;
        }
    }
}

// Initialize app when DOM is ready
const app = new ExplorerApp();
