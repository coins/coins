class ExplorerApp {
    constructor() {
        this.ws = null;
        this.currentView = 'home';
        this.currentPage = 0;
        this.apiBase = '/api/v1';

        // WebSocket disabled in simple proxy mode
        // this.initWebSocket();
        document.getElementById('connection-status').textContent = '● Simple Mode';
        document.getElementById('connection-status').className = 'tag is-info';

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

        if (msg.type === 'new_block') {
            this.showNewBlockNotification(msg.block);

            // Refresh current view if we're on blocks or home
            if (this.currentView === 'blocks' || this.currentView === 'home') {
                this.navigate(this.currentView, {}, false);
            }
        } else if (msg.type === 'stats_update') {
            // Update stats if visible
            if (this.currentView === 'home') {
                this.navigate('home', {}, false);
            }
        }
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
                default:
                    await this.renderHome();
            }
        } catch (error) {
            content.innerHTML = `
                <div class="notification is-danger">
                    <strong>Error:</strong> ${error.message}
                </div>
            `;
        }
    }

    async renderHome() {
        const stats = await this.fetchAPI('/stats');
        const latestBlock = await this.fetchAPI('/blocks/latest');
        const content = document.getElementById('content');

        content.innerHTML = `
            <h1 class="title">Network Statistics</h1>

            <div class="stats-grid">
                <div class="stat-card">
                    <div class="stat-value">${stats.total_blocks || 0}</div>
                    <div class="stat-label">Total Blocks</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">${stats.total_accounts || 0}</div>
                    <div class="stat-label">Total Accounts</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">${stats.total_supply || 0}</div>
                    <div class="stat-label">Total Supply</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">${latestBlock ? latestBlock.height : 0}</div>
                    <div class="stat-label">Latest Block Height</div>
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
                                <a href="https://mempool.space/signet/tx/${block.btc_txid}" target="_blank" class="button is-small is-link">
                                    View on mempool.space
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
                        <td><strong style="font-size: 1.2em; color: #23d160;">${account.balance} sats</strong></td>
                    </tr>
                    <tr>
                        <th>Nonce</th>
                        <td>${account.nonce}</td>
                    </tr>
                </table>
            </div>
        `;
    }

    async loadAccountTransactions(pk, page = 0) {
        const data = await this.fetchAPI(`/accounts/${pk}/transactions?page=${page}&limit=20`);
        const container = document.getElementById('account-txs');

        const totalPages = Math.ceil(data.total_count / data.limit);

        container.innerHTML = `
            <table class="table is-fullwidth is-striped is-hoverable">
                <thead>
                    <tr>
                        <th>BTC Height</th>
                        <th>Type</th>
                        <th>From</th>
                        <th>To</th>
                        <th>Amount</th>
                        <th>Fee</th>
                        <th>Status</th>
                    </tr>
                </thead>
                <tbody>
                    ${data.transactions.map(tx => `
                        <tr>
                            <td>
                                <a onclick="app.navigate('block', {height: ${tx.btc_height}})">
                                    ${tx.btc_height}
                                </a>
                            </td>
                            <td>
                                <span class="${tx.tx_type === 'sent' ? 'tx-sent' : 'tx-received'}">
                                    ${tx.tx_type === 'sent' ? '↗ Sent' : '↘ Received'}
                                </span>
                            </td>
                            <td>${tx.sender_id}</td>
                            <td class="mono truncate">${tx.recipient_pk.substring(0, 16)}...</td>
                            <td>${tx.amount}</td>
                            <td>${tx.fee}</td>
                            <td>
                                <span class="${tx.finalized ? 'status-finalized' : 'status-pending'}">
                                    ${tx.finalized ? '✓' : '⏳'}
                                </span>
                            </td>
                        </tr>
                    `).join('')}
                </tbody>
            </table>

            ${totalPages > 1 ? this.renderPagination(page, totalPages, () => this.loadAccountTransactions(pk, page)) : ''}
        `;
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
        const response = await fetch(this.apiBase + endpoint);

        if (!response.ok) {
            const text = await response.text();
            throw new Error(`API Error: ${response.status} - ${text}`);
        }

        return await response.json();
    }
}

// Initialize app when DOM is ready
const app = new ExplorerApp();
