class ExplorerApp {
    constructor() {
        this.ws = null;
        this.currentView = 'home';
        this.currentPage = 0;
        this.apiBase = '/api/v1';

        this.initWebSocket();
        this.navigate('home');

        // Handle browser back/forward
        window.addEventListener('popstate', (e) => {
            if (e.state && e.state.view) {
                this.navigate(e.state.view, e.state.params, false);
            }
        });
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
            history.pushState({ view, params }, '', `#${view}`);
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
        const content = document.getElementById('content');

        content.innerHTML = `
            <h1 class="title">Network Statistics</h1>

            <div class="stats-grid">
                <div class="stat-card">
                    <div class="stat-value">${stats.network.total_blocks}</div>
                    <div class="stat-label">Total Blocks</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">${stats.network.finalized_blocks}</div>
                    <div class="stat-label">Finalized Blocks</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">${stats.network.total_accounts}</div>
                    <div class="stat-label">Total Accounts</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">${stats.network.total_transactions}</div>
                    <div class="stat-label">Total Transactions</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">${stats.bitcoin.current_height}</div>
                    <div class="stat-label">Bitcoin Height</div>
                </div>
                <div class="stat-card">
                    <div class="stat-value">${stats.bitcoin.finality_depth}</div>
                    <div class="stat-label">Finality Depth</div>
                </div>
            </div>

            <h2 class="title is-4">Recent Blocks</h2>
            <table class="table is-fullwidth is-striped is-hoverable blocks-table">
                <thead>
                    <tr>
                        <th>BTC Height</th>
                        <th>BTC Txid</th>
                        <th>Confirmations</th>
                        <th>Transactions</th>
                        <th>Status</th>
                    </tr>
                </thead>
                <tbody>
                    ${stats.recent_blocks.map(block => `
                        <tr onclick="app.navigate('block', {height: ${block.btc_height}})">
                            <td><strong>${block.btc_height}</strong></td>
                            <td class="mono truncate">${block.btc_txid}</td>
                            <td>${block.btc_confirmations}</td>
                            <td>${block.tx_count}</td>
                            <td>
                                <span class="${block.finalized ? 'status-finalized' : 'status-pending'}">
                                    ${block.finalized ? '✓ Finalized' : '⏳ Pending'}
                                </span>
                            </td>
                        </tr>
                    `).join('')}
                </tbody>
            </table>

            <div class="has-text-centered" style="margin-top: 2rem;">
                <button class="button is-primary" onclick="app.navigate('blocks')">
                    View All Blocks
                </button>
            </div>
        `;
    }

    async renderBlocks(page = 0) {
        const data = await this.fetchAPI(`/blocks?page=${page}&limit=20`);
        const content = document.getElementById('content');

        const totalPages = Math.ceil(data.total_count / data.limit);

        content.innerHTML = `
            <div class="level">
                <div class="level-left">
                    <div class="level-item">
                        <h1 class="title">Blocks</h1>
                    </div>
                </div>
                <div class="level-right">
                    <div class="level-item">
                        <span class="tag is-info">Total: ${data.total_count}</span>
                    </div>
                    <div class="level-item">
                        <span class="tag is-primary">BTC Height: ${data.current_btc_height}</span>
                    </div>
                </div>
            </div>

            <table class="table is-fullwidth is-striped is-hoverable blocks-table">
                <thead>
                    <tr>
                        <th>BTC Height</th>
                        <th>BTC Txid</th>
                        <th>Confirmations</th>
                        <th>Transactions</th>
                        <th>Publisher</th>
                        <th>Status</th>
                    </tr>
                </thead>
                <tbody>
                    ${data.blocks.map(block => `
                        <tr onclick="app.navigate('block', {height: ${block.btc_height}})">
                            <td><strong>${block.btc_height}</strong></td>
                            <td class="mono truncate">${block.btc_txid}</td>
                            <td>${block.btc_confirmations}</td>
                            <td>${block.tx_count}</td>
                            <td class="mono truncate">${block.publisher_pk}</td>
                            <td>
                                <span class="${block.finalized ? 'status-finalized' : 'status-pending'}">
                                    ${block.finalized ? '✓ Finalized' : '⏳ Pending'}
                                </span>
                            </td>
                        </tr>
                    `).join('')}
                </tbody>
            </table>

            ${this.renderPagination(page, totalPages)}
        `;
    }

    async renderBlock(height) {
        const block = await this.fetchAPI(`/blocks/${height}`);
        const content = document.getElementById('content');

        content.innerHTML = `
            <nav class="breadcrumb">
                <ul>
                    <li><a onclick="app.navigate('home')">Home</a></li>
                    <li><a onclick="app.navigate('blocks')">Blocks</a></li>
                    <li class="is-active"><a>Block ${height}</a></li>
                </ul>
            </nav>

            <h1 class="title">Block at Bitcoin Height ${block.btc_height}</h1>

            <div class="box">
                <div class="block-detail-grid">
                    <div class="block-detail-label">Bitcoin Txid:</div>
                    <div class="block-detail-value mono">
                        ${block.btc_txid}
                        <div style="margin-top: 0.5rem;">
                            <a href="${block.bitcoin_links.mempool_space}" target="_blank" class="button is-small is-link">
                                mempool.space
                            </a>
                            <a href="${block.bitcoin_links.blockstream}" target="_blank" class="button is-small is-link">
                                blockstream.info
                            </a>
                        </div>
                    </div>

                    <div class="block-detail-label">Confirmations:</div>
                    <div class="block-detail-value">${block.btc_confirmations}</div>

                    <div class="block-detail-label">Status:</div>
                    <div class="block-detail-value">
                        <span class="${block.finalized ? 'status-finalized' : 'status-pending'}">
                            ${block.finalized ? '✓ Finalized' : '⏳ Pending'}
                        </span>
                    </div>

                    <div class="block-detail-label">Publisher:</div>
                    <div class="block-detail-value mono">${block.publisher_pk}</div>

                    <div class="block-detail-label">Timestamp:</div>
                    <div class="block-detail-value">
                        ${block.btc_timestamp ? new Date(block.btc_timestamp * 1000).toLocaleString() : 'N/A'}
                    </div>
                </div>
            </div>

            <h2 class="title is-4">Transactions (${block.txs.length})</h2>

            <table class="table is-fullwidth is-striped is-hoverable">
                <thead>
                    <tr>
                        <th>Sender ID</th>
                        <th>Recipient</th>
                        <th>Amount</th>
                        <th>Fee</th>
                    </tr>
                </thead>
                <tbody>
                    ${block.txs.map(tx => `
                        <tr>
                            <td><strong>${tx.sender_id}</strong></td>
                            <td>
                                <a onclick="app.navigate('account', {pk: '${tx.recipient_pk}'})" class="mono truncate">
                                    ${tx.recipient_pk}
                                </a>
                                ${tx.recipient_id !== null ? ` (ID: ${tx.recipient_id})` : ''}
                            </td>
                            <td>${tx.amount}</td>
                            <td>${tx.fee}</td>
                        </tr>
                    `).join('')}
                </tbody>
            </table>
        `;
    }

    async renderAccount(pk) {
        const data = await this.fetchAPI(`/accounts/${pk}`);
        const content = document.getElementById('content');

        if (!data.account) {
            content.innerHTML = `
                <div class="notification is-warning">
                    <strong>Account Not Found</strong><br>
                    No account exists with public key: <code>${pk}</code>
                </div>
            `;
            return;
        }

        const account = data.account;

        content.innerHTML = `
            <nav class="breadcrumb">
                <ul>
                    <li><a onclick="app.navigate('home')">Home</a></li>
                    <li class="is-active"><a>Account ${account.id}</a></li>
                </ul>
            </nav>

            <h1 class="title">Account ${account.id}</h1>

            <div class="box">
                <div class="block-detail-grid">
                    <div class="block-detail-label">Account ID:</div>
                    <div class="block-detail-value"><strong>${account.id}</strong></div>

                    <div class="block-detail-label">Public Key:</div>
                    <div class="block-detail-value mono">${account.pk}</div>

                    <div class="block-detail-label">Balance:</div>
                    <div class="block-detail-value"><strong>${account.balance}</strong> satoshis</div>

                    <div class="block-detail-label">Nonce:</div>
                    <div class="block-detail-value">${account.nonce}</div>

                    <div class="block-detail-label">Transactions:</div>
                    <div class="block-detail-value">${account.tx_count}</div>
                </div>
            </div>

            <h2 class="title is-4">Transaction History</h2>
            <div id="account-txs">
                <div class="loading"></div>
            </div>
        `;

        // Load transaction history
        this.loadAccountTransactions(pk, 0);
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
