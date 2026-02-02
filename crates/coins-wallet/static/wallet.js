// Coins Wallet - Browser-based wallet with WASM BLS signing
// Uses Web Crypto API for AES-GCM encryption with PBKDF2 key derivation
// Includes WebSocket support for real-time updates

const STORAGE_KEY = 'coins_wallet_key';
const SESSION_KEY = 'coins_wallet_session_key';
const PBKDF2_ITERATIONS = 100000;
const SALT_SIZE = 16;
const IV_SIZE = 12;

// WASM module reference
let wasmModule = null;

// Current wallet key (in memory while unlocked)
let currentKey = null;

// WebSocket connection
let ws = null;
let wsReconnectTimeout = null;

/**
 * Initialize WebSocket connection for real-time updates
 */
function initWebSocket() {
    if (ws && ws.readyState === WebSocket.OPEN) return;

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/ws`;

    try {
        ws = new WebSocket(wsUrl);

        ws.onopen = () => {
            console.log('WebSocket connected');
            updateConnectionStatus(true);
        };

        ws.onmessage = (event) => {
            try {
                const msg = JSON.parse(event.data);
                handleWSMessage(msg);
            } catch (e) {
                console.error('Failed to parse WebSocket message:', e);
            }
        };

        ws.onclose = () => {
            console.log('WebSocket disconnected');
            updateConnectionStatus(false);
            // Reconnect after 3 seconds
            if (wsReconnectTimeout) clearTimeout(wsReconnectTimeout);
            wsReconnectTimeout = setTimeout(() => initWebSocket(), 3000);
        };

        ws.onerror = (error) => {
            console.error('WebSocket error:', error);
            updateConnectionStatus(false);
        };
    } catch (e) {
        console.error('Failed to create WebSocket:', e);
        updateConnectionStatus(false);
    }
}

/**
 * Update connection status indicator
 */
function updateConnectionStatus(connected) {
    const statusEl = document.getElementById('connection-status');
    const dotEl = document.getElementById('connection-dot');
    if (statusEl) {
        statusEl.textContent = connected ? 'Connected' : 'Offline';
    }
    if (dotEl) {
        if (connected) {
            dotEl.classList.remove('disconnected');
        } else {
            dotEl.classList.add('disconnected');
        }
    }
}

/**
 * Handle WebSocket messages
 */
function handleWSMessage(msg) {
    console.log('WebSocket message:', msg);

    // Only refresh if wallet is unlocked
    if (!walletApp.isUnlocked()) return;

    switch (msg.type) {
        case 'balance_update':
            // Refresh balance display
            refreshBalance();
            break;
        case 'transaction_update':
            // Refresh both balance and transactions
            refreshBalance();
            refreshTransactions();
            break;
        case 'connected':
            console.log('WebSocket connection confirmed');
            break;
    }
}

/**
 * WalletApp class - manages wallet operations
 */
class WalletApp {
    constructor() {
        this.initialized = false;
    }

    /**
     * Initialize the wallet app by loading the WASM module
     */
    async init() {
        if (this.initialized) return;

        try {
            console.log('Loading WASM module...');
            wasmModule = await import('/wasm/coins_wallet_wasm.js');
            await wasmModule.default();
            console.log('WASM module loaded successfully');
            this.initialized = true;
        } catch (error) {
            console.error('Failed to load WASM module:', error);
            throw new Error('Failed to initialize wallet: WASM module could not be loaded');
        }
    }

    /**
     * Check if a wallet exists in localStorage
     */
    hasWallet() {
        return localStorage.getItem(STORAGE_KEY) !== null;
    }

    /**
     * Derive an encryption key from a password using PBKDF2
     * @param {string} password - The password to derive from
     * @param {Uint8Array} salt - The salt for key derivation
     * @returns {Promise<CryptoKey>} The derived AES-GCM key
     */
    async deriveKey(password, salt) {
        const encoder = new TextEncoder();
        const passwordKey = await crypto.subtle.importKey(
            'raw',
            encoder.encode(password),
            'PBKDF2',
            false,
            ['deriveKey']
        );

        return crypto.subtle.deriveKey(
            {
                name: 'PBKDF2',
                salt: salt,
                iterations: PBKDF2_ITERATIONS,
                hash: 'SHA-256'
            },
            passwordKey,
            { name: 'AES-GCM', length: 256 },
            false,
            ['encrypt', 'decrypt']
        );
    }

    /**
     * Encrypt data using AES-GCM
     * @param {Uint8Array} data - The data to encrypt
     * @param {string} password - The password for encryption
     * @returns {Promise<string>} Base64-encoded encrypted data (salt + iv + ciphertext)
     */
    async encryptData(data, password) {
        const salt = crypto.getRandomValues(new Uint8Array(SALT_SIZE));
        const iv = crypto.getRandomValues(new Uint8Array(IV_SIZE));
        const key = await this.deriveKey(password, salt);

        const ciphertext = await crypto.subtle.encrypt(
            { name: 'AES-GCM', iv: iv },
            key,
            data
        );

        // Combine salt + iv + ciphertext
        const combined = new Uint8Array(SALT_SIZE + IV_SIZE + ciphertext.byteLength);
        combined.set(salt, 0);
        combined.set(iv, SALT_SIZE);
        combined.set(new Uint8Array(ciphertext), SALT_SIZE + IV_SIZE);

        // Encode as base64 for storage
        return btoa(String.fromCharCode(...combined));
    }

    /**
     * Decrypt data using AES-GCM
     * @param {string} encryptedBase64 - Base64-encoded encrypted data
     * @param {string} password - The password for decryption
     * @returns {Promise<Uint8Array>} The decrypted data
     */
    async decryptData(encryptedBase64, password) {
        const combined = Uint8Array.from(atob(encryptedBase64), c => c.charCodeAt(0));

        const salt = combined.slice(0, SALT_SIZE);
        const iv = combined.slice(SALT_SIZE, SALT_SIZE + IV_SIZE);
        const ciphertext = combined.slice(SALT_SIZE + IV_SIZE);

        const key = await this.deriveKey(password, salt);

        const decrypted = await crypto.subtle.decrypt(
            { name: 'AES-GCM', iv: iv },
            key,
            ciphertext
        );

        return new Uint8Array(decrypted);
    }

    /**
     * Create a new wallet with password encryption
     * @param {string} password - The password to encrypt the wallet
     */
    async createWallet(password) {
        if (!this.initialized) {
            await this.init();
        }

        // Generate new key pair
        const walletKey = new wasmModule.WalletKey();
        const secretKeyBytes = walletKey.secret_key_bytes();

        // Encrypt the secret key
        const encryptedKey = await this.encryptData(new Uint8Array(secretKeyBytes), password);

        // Store in localStorage
        localStorage.setItem(STORAGE_KEY, encryptedKey);

        // Keep the key in memory
        currentKey = walletKey;

        // Store decrypted key in sessionStorage for persistence across page refreshes
        const secretKeyHex = Array.from(secretKeyBytes).map(b => b.toString(16).padStart(2, '0')).join('');
        sessionStorage.setItem(SESSION_KEY, secretKeyHex);

        console.log('Wallet created successfully');
        console.log('Public key:', walletKey.public_key_hex());

        return walletKey.public_key_hex();
    }

    /**
     * Import an existing wallet from a secret key hex string
     * @param {string} secretKeyHex - The secret key as 64 hex characters
     * @param {string} password - The password to encrypt the wallet
     * @returns {Promise<string>} The public key hex
     */
    async importWallet(secretKeyHex, password) {
        if (!this.initialized) {
            await this.init();
        }

        // Validate secret key format: must be 64 hex characters
        if (!/^[a-fA-F0-9]{64}$/.test(secretKeyHex)) {
            throw new Error('Invalid secret key: must be 64 hexadecimal characters');
        }

        // Convert hex string to bytes
        const secretKeyBytes = new Uint8Array(32);
        for (let i = 0; i < 32; i++) {
            secretKeyBytes[i] = parseInt(secretKeyHex.substr(i * 2, 2), 16);
        }

        // Try to create WalletKey from bytes
        let walletKey;
        try {
            walletKey = wasmModule.WalletKey.from_bytes(secretKeyBytes);
        } catch (error) {
            console.error('Failed to create key from bytes:', error);
            throw new Error('Invalid secret key: could not create wallet key');
        }

        // Encrypt the secret key
        const encryptedKey = await this.encryptData(secretKeyBytes, password);

        // Store in localStorage
        localStorage.setItem(STORAGE_KEY, encryptedKey);

        // Keep the key in memory
        currentKey = walletKey;

        // Store decrypted key in sessionStorage for persistence across page refreshes
        sessionStorage.setItem(SESSION_KEY, secretKeyHex);

        console.log('Wallet imported successfully');
        console.log('Public key:', walletKey.public_key_hex());

        return walletKey.public_key_hex();
    }

    /**
     * Unlock an existing wallet with password
     * @param {string} password - The password to decrypt the wallet
     * @returns {Promise<string>} The public key hex
     */
    async unlockWallet(password) {
        if (!this.initialized) {
            await this.init();
        }

        const encryptedKey = localStorage.getItem(STORAGE_KEY);
        if (!encryptedKey) {
            throw new Error('No wallet found');
        }

        try {
            const decryptedBytes = await this.decryptData(encryptedKey, password);
            const walletKey = wasmModule.WalletKey.from_bytes(decryptedBytes);

            // Keep the key in memory
            currentKey = walletKey;

            // Store decrypted key in sessionStorage for persistence across page refreshes
            const secretKeyHex = Array.from(decryptedBytes).map(b => b.toString(16).padStart(2, '0')).join('');
            sessionStorage.setItem(SESSION_KEY, secretKeyHex);

            console.log('Wallet unlocked successfully');
            console.log('Public key:', walletKey.public_key_hex());

            return walletKey.public_key_hex();
        } catch (error) {
            console.error('Failed to unlock wallet:', error);
            throw new Error('Wrong password or corrupted wallet data');
        }
    }

    /**
     * Lock the wallet (clear key from memory and sessionStorage)
     */
    lockWallet() {
        currentKey = null;
        sessionStorage.removeItem(SESSION_KEY);
        console.log('Wallet locked');
    }

    /**
     * Get the current wallet key (must be unlocked)
     * @returns {WalletKey|null} The current key or null if locked
     */
    getCurrentKey() {
        return currentKey;
    }

    /**
     * Check if wallet is unlocked
     */
    isUnlocked() {
        return currentKey !== null;
    }

    /**
     * Get the current public key (if unlocked)
     * @returns {string|null} Public key hex or null if locked
     */
    getPublicKey() {
        if (!currentKey) return null;
        return currentKey.public_key_hex();
    }

    /**
     * Try to restore wallet session from sessionStorage
     * @returns {Promise<boolean>} True if session was restored, false otherwise
     */
    async restoreSession() {
        if (!this.initialized) {
            await this.init();
        }

        // Check if session key exists
        const sessionKeyHex = sessionStorage.getItem(SESSION_KEY);
        if (!sessionKeyHex) {
            return false;
        }

        try {
            // Validate format
            if (!/^[a-fA-F0-9]{64}$/.test(sessionKeyHex)) {
                console.warn('Invalid session key format, clearing');
                sessionStorage.removeItem(SESSION_KEY);
                return false;
            }

            // Convert hex to bytes
            const secretKeyBytes = new Uint8Array(32);
            for (let i = 0; i < 32; i++) {
                secretKeyBytes[i] = parseInt(sessionKeyHex.substr(i * 2, 2), 16);
            }

            // Restore the wallet key
            const walletKey = wasmModule.WalletKey.from_bytes(secretKeyBytes);
            currentKey = walletKey;

            console.log('Session restored successfully');
            console.log('Public key:', walletKey.public_key_hex());

            return true;
        } catch (error) {
            console.error('Failed to restore session:', error);
            sessionStorage.removeItem(SESSION_KEY);
            return false;
        }
    }

    /**
     * Fetch account balance from the indexer
     * @returns {Promise<Object|null>} Account data or null if not found
     */
    async refreshBalance() {
        if (!currentKey) {
            throw new Error('Wallet is locked');
        }

        const pk = currentKey.public_key_hex();
        const response = await fetch(`/api/account/${pk}`);

        if (response.status === 404) {
            // Account not found - this is normal for new wallets
            return null;
        }

        if (!response.ok) {
            throw new Error(`Failed to fetch account: ${response.status}`);
        }

        return await response.json();
    }

    /**
     * Fetch transaction history from the indexer
     * @returns {Promise<Array|null>} Array of transactions or null if not found
     */
    async fetchTransactions() {
        if (!currentKey) {
            throw new Error('Wallet is locked');
        }

        const pk = currentKey.public_key_hex();
        const response = await fetch(`/api/account/${pk}/transactions`);

        if (response.status === 404) {
            // Account not found - no transactions
            return null;
        }

        if (!response.ok) {
            throw new Error(`Failed to fetch transactions: ${response.status}`);
        }

        return await response.json();
    }

    /**
     * Get the secret key from localStorage by decrypting with password
     * @param {string} password - The password to decrypt the wallet
     * @returns {Promise<string>} The secret key hex
     */
    async getSecretKeyHex(password) {
        if (!this.initialized) {
            await this.init();
        }

        const encryptedKey = localStorage.getItem(STORAGE_KEY);
        if (!encryptedKey) {
            throw new Error('No wallet found');
        }

        try {
            const decryptedBytes = await this.decryptData(encryptedKey, password);
            // Convert bytes to hex
            const hexKey = Array.from(decryptedBytes).map(b => b.toString(16).padStart(2, '0')).join('');
            return hexKey;
        } catch (error) {
            console.error('Failed to decrypt wallet:', error);
            throw new Error('Wrong password or corrupted wallet data');
        }
    }

    /**
     * Send a transaction
     * @param {string} recipientPk - Recipient public key hex (64 chars)
     * @param {number} amount - Amount to send
     * @param {number} tokenId - Token ID (default 0 for native)
     * @param {number} fee - Transaction fee
     * @returns {Promise<Object>} Transaction result
     */
    async sendTransaction(recipientPk, amount, tokenId = 0, fee = 1) {
        if (!currentKey) {
            throw new Error('Wallet is locked');
        }

        // Validate recipient public key (should be 64 hex chars)
        if (!/^[a-fA-F0-9]{64}$/.test(recipientPk)) {
            throw new Error('Invalid recipient public key: must be 64 hex characters');
        }

        // Validate amount
        if (!Number.isInteger(amount) || amount <= 0) {
            throw new Error('Amount must be a positive integer');
        }

        // Validate fee
        if (!Number.isInteger(fee) || fee < 1 || fee > 255) {
            throw new Error('Fee must be between 1 and 255');
        }

        // Fetch account to get sender_id and nonce
        const account = await this.refreshBalance();
        if (account === null) {
            throw new Error('Account not found. You need to receive tokens first before sending.');
        }

        const senderId = account.id;
        const nonce = account.nonce;

        console.log('Building transaction:', { senderId, recipientPk, tokenId, amount, fee, nonce });

        // Build the transaction using WASM
        const txBytes = wasmModule.build_transaction(
            senderId,
            recipientPk,
            tokenId,
            amount,
            fee
        );

        // Build the signing message (tx || nonce)
        const signingMessage = wasmModule.build_signing_message(txBytes, nonce);

        // Sign the message
        const signature = currentKey.sign(signingMessage);

        // Convert tx bytes to hex
        const txHex = Array.from(txBytes).map(b => b.toString(16).padStart(2, '0')).join('');

        console.log('Submitting transaction:', { tx: txHex, signature });

        // Submit to the publisher via wallet API
        const response = await fetch('/api/tx/submit', {
            method: 'POST',
            headers: {
                'Content-Type': 'application/json',
            },
            body: JSON.stringify({
                tx: txHex,
                signature: signature,
            }),
        });

        const responseText = await response.text();

        if (!response.ok) {
            throw new Error(`Transaction failed: ${responseText}`);
        }

        console.log('Transaction submitted successfully:', responseText);
        return { success: true, message: responseText };
    }
}

// Global wallet app instance
const walletApp = new WalletApp();

// Screen management
function showScreen(screenId) {
    document.querySelectorAll('.screen').forEach(screen => {
        screen.classList.remove('active');
    });
    const screen = document.getElementById(screenId);
    if (screen) {
        screen.classList.add('active');
    }
}

// Show error message
function showError(elementId, message) {
    const element = document.getElementById(elementId);
    if (element) {
        element.textContent = message;
        element.style.display = 'block';
    }
}

// Hide error message
function hideError(elementId) {
    const element = document.getElementById(elementId);
    if (element) {
        element.style.display = 'none';
    }
}

// Update wallet display with public key
function updateWalletDisplay(publicKey) {
    const pkElement = document.getElementById('wallet-public-key');
    if (pkElement) {
        pkElement.textContent = publicKey;
    }
}

// Update balance display with account data
function updateBalanceDisplay(account) {
    const balanceTokensEl = document.getElementById('balance-tokens');
    const balanceAccountEl = document.getElementById('balance-account');
    const balanceEl = document.getElementById('balance-display');
    const tokenBalancesCard = document.getElementById('token-balances-card');

    // Greek letters for token display
    const greekLetters = ['α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ'];
    const greekNames = ['Alpha', 'Beta', 'Gamma', 'Delta', 'Epsilon', 'Zeta', 'Eta', 'Theta', 'Iota', 'Kappa'];

    if (account === null) {
        // Account not found - new wallet
        if (balanceTokensEl) {
            balanceTokensEl.innerHTML = `
                <div class="balance-token-item">
                    <div class="balance-token-amount">0</div>
                    <div class="balance-token-name">New wallet</div>
                </div>
            `;
        }
        if (balanceAccountEl) balanceAccountEl.textContent = '';
        if (tokenBalancesCard) tokenBalancesCard.style.display = 'none';
        // Reset carousel state for empty wallet
        window.carouselState.tokenIds = ['0'];
        window.carouselState.balances = { '0': 0 };
        return;
    }

    // Get balances and filter out zero balances, sort by token_id
    const balances = account.balances || {};
    const tokenIds = Object.keys(balances)
        .filter(id => (balances[id] || 0) > 0)
        .sort((a, b) => parseInt(a) - parseInt(b));

    // Store in carousel state for send form
    window.carouselState.tokenIds = tokenIds;
    window.carouselState.balances = balances;

    // Find the token(s) with the largest balance
    let largestBalance = 0;
    tokenIds.forEach((id) => {
        if ((balances[id] || 0) > largestBalance) {
            largestBalance = balances[id];
        }
    });

    // Find all indices with the largest balance
    const largestIndices = [];
    tokenIds.forEach((id, index) => {
        if ((balances[id] || 0) === largestBalance) {
            largestIndices.push(index);
        }
    });

    // Among those with largest balance, pick the one closest to the middle
    // where |left - right| is closest to 0
    const middleIndex = (tokenIds.length - 1) / 2;
    let largestBalanceIndex = largestIndices[0];
    let closestToMiddle = Math.abs(largestIndices[0] - middleIndex);

    for (const idx of largestIndices) {
        const distFromMiddle = Math.abs(idx - middleIndex);
        if (distFromMiddle < closestToMiddle) {
            closestToMiddle = distFromMiddle;
            largestBalanceIndex = idx;
        }
    }

    // Update account ID
    if (balanceAccountEl) balanceAccountEl.textContent = `Account #${account.id}`;

    // Build horizontally scrollable token balances
    if (balanceTokensEl) {
        let html = '';
        for (const tokenId of tokenIds) {
            const balance = balances[tokenId] || 0;
            const idx = parseInt(tokenId);
            const letter = greekLetters[idx] || `#${tokenId}`;
            const name = greekNames[idx] || `Token ${tokenId}`;
            html += `
                <div class="balance-token-item" data-token-id="${tokenId}">
                    <div class="balance-token-amount">${balance.toLocaleString()}</div>
                    <div class="balance-token-name">${letter} ${name}</div>
                </div>
            `;
        }
        balanceTokensEl.innerHTML = html;

        // Initialize carousel with the largest balance centered
        setTimeout(() => {
            if (typeof window.initBalanceCarousel === 'function') {
                window.initBalanceCarousel(largestBalanceIndex);
            }
        }, 50);
    }

    // Hide the separate token balances card since we show them in the hero now
    if (tokenBalancesCard) tokenBalancesCard.style.display = 'none';
}

// Show send status message
function showSendStatus(message, isError = false) {
    const statusEl = document.getElementById('send-status');
    if (statusEl) {
        statusEl.textContent = message;
        statusEl.className = `notification mt-3 ${isError ? 'is-danger' : 'is-success'}`;
        statusEl.style.display = 'block';
    }
}

// Hide send status message
function hideSendStatus() {
    const statusEl = document.getElementById('send-status');
    if (statusEl) {
        statusEl.style.display = 'none';
    }
}

// Update transaction history display
function updateTransactionHistory(transactions) {
    const historyEl = document.getElementById('transaction-history');
    if (!historyEl) return;

    if (transactions === null || transactions.length === 0) {
        historyEl.innerHTML = '<p class="has-text-grey">No transactions yet</p>';
        return;
    }

    // Get current public key for display
    const currentPk = walletApp.getPublicKey();

    // Greek letters for token display
    const greekLetters = ['α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ'];
    const greekNames = ['Alpha', 'Beta', 'Gamma', 'Delta', 'Epsilon', 'Zeta', 'Eta', 'Theta', 'Iota', 'Kappa'];

    // Build transaction list HTML
    let html = '';
    for (let i = 0; i < transactions.length; i++) {
        const tx = transactions[i];
        const isOutgoing = tx.direction === 'outgoing';
        const directionClass = isOutgoing ? 'has-text-danger' : 'has-text-success';
        const directionIcon = isOutgoing ? '-' : '+';
        const directionLabel = isOutgoing ? 'Sent' : 'Received';

        // Format counterparty address (truncate for display)
        const counterpartyPk = isOutgoing ? tx.recipient_pk : tx.sender_pk;
        const counterpartyShort = counterpartyPk.substring(0, 8) + '...' + counterpartyPk.substring(56);

        // Format token display
        const tokenIdx = tx.token_id;
        const tokenLetter = greekLetters[tokenIdx] || `#${tokenIdx}`;
        const tokenName = greekNames[tokenIdx] || `Token ${tokenIdx}`;

        // Format status
        let statusHtml;
        if (tx.finalized) {
            statusHtml = '<span class="tag is-success is-light">Finalized</span>';
        } else if (tx.confirmations > 0) {
            statusHtml = `<span class="tag is-warning is-light">${tx.confirmations} conf</span>`;
        } else {
            statusHtml = '<span class="tag is-info is-light">Pending</span>';
        }

        html += `
            <div class="transaction-item clickable" data-tx-index="${i}">
                <div class="columns is-mobile is-vcentered mb-0">
                    <div class="column">
                        <p class="has-text-weight-semibold ${directionClass}">
                            ${directionIcon} ${tx.amount.toLocaleString()} ${tokenLetter} ${tokenName}
                        </p>
                        <p class="is-size-7 has-text-grey">
                            ${directionLabel} ${isOutgoing ? 'to' : 'from'}
                            <span class="is-family-monospace">${counterpartyShort}</span>
                        </p>
                    </div>
                    <div class="column is-narrow has-text-right">
                        ${statusHtml}
                    </div>
                </div>
            </div>
        `;
    }

    historyEl.innerHTML = html;

    // Store transactions for detail view
    window.currentTransactions = transactions;

    // Add click handlers
    historyEl.querySelectorAll('.transaction-item.clickable').forEach(item => {
        item.addEventListener('click', () => {
            const index = parseInt(item.getAttribute('data-tx-index'));
            showTransactionDetail(window.currentTransactions[index]);
        });
    });
}

// Show transaction detail modal
function showTransactionDetail(tx) {
    const modal = document.getElementById('tx-detail-modal');
    if (!modal || !tx) return;

    const isOutgoing = tx.direction === 'outgoing';
    const counterpartyPk = isOutgoing ? tx.recipient_pk : tx.sender_pk;

    // Greek letters for token display
    const greekLetters = ['α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ'];
    const greekNames = ['Alpha', 'Beta', 'Gamma', 'Delta', 'Epsilon', 'Zeta', 'Eta', 'Theta', 'Iota', 'Kappa'];
    const tokenIdx = tx.token_id;
    const tokenName = `${greekLetters[tokenIdx] || '#'} ${greekNames[tokenIdx] || `Token ${tokenIdx}`}`;

    // Update modal content
    const typeEl = document.getElementById('tx-detail-type');
    if (typeEl) {
        typeEl.textContent = isOutgoing ? 'Sent' : 'Received';
        typeEl.className = `tx-detail-value ${isOutgoing ? 'is-danger' : 'is-success'}`;
    }

    const amountEl = document.getElementById('tx-detail-amount');
    if (amountEl) {
        amountEl.textContent = `${isOutgoing ? '-' : '+'}${tx.amount.toLocaleString()}`;
        amountEl.className = `tx-detail-value ${isOutgoing ? 'is-danger' : 'is-success'}`;
    }

    const tokenEl = document.getElementById('tx-detail-token');
    if (tokenEl) tokenEl.textContent = tokenName;

    const feeEl = document.getElementById('tx-detail-fee');
    if (feeEl) feeEl.textContent = tx.fee > 0 ? tx.fee.toString() : '0';

    // Status
    const statusEl = document.getElementById('tx-detail-status');
    if (statusEl) {
        if (tx.finalized) {
            statusEl.innerHTML = '<span class="tag is-success">Finalized</span>';
        } else if (tx.confirmations > 0) {
            statusEl.innerHTML = `<span class="tag is-warning">${tx.confirmations} confirmations (${tx.confirmations_remaining} more needed)</span>`;
        } else {
            statusEl.innerHTML = '<span class="tag is-info">Pending</span>';
        }
    }

    // Block height (if available)
    const blockRow = document.getElementById('tx-detail-block-row');
    const blockEl = document.getElementById('tx-detail-block');
    if (tx.block_height !== undefined && tx.block_height !== null) {
        if (blockRow) blockRow.style.display = 'flex';
        if (blockEl) blockEl.textContent = `#${tx.block_height}`;
    } else {
        if (blockRow) blockRow.style.display = 'none';
    }

    // Counterparty address
    const labelEl = document.getElementById('tx-detail-counterparty-label');
    if (labelEl) labelEl.textContent = isOutgoing ? 'To' : 'From';

    const addressTextEl = document.getElementById('tx-detail-counterparty-text');
    if (addressTextEl) addressTextEl.textContent = counterpartyPk;

    // Explorer link
    const explorerLink = document.getElementById('tx-detail-explorer-link');
    if (explorerLink) {
        // Link to the counterparty's account in the explorer
        explorerLink.href = `http://localhost:3000/#/account/${counterpartyPk}`;
    }

    // Show modal
    modal.classList.add('is-active');
}

// Refresh transaction history from API
async function refreshTransactions() {
    const historyEl = document.getElementById('transaction-history');

    if (!walletApp.isUnlocked()) {
        if (historyEl) historyEl.innerHTML = '<p class="has-text-danger">Wallet is locked</p>';
        return;
    }

    try {
        if (historyEl) historyEl.innerHTML = '<p class="has-text-grey">Loading...</p>';

        const transactions = await walletApp.fetchTransactions();
        updateTransactionHistory(transactions);
    } catch (error) {
        console.error('Failed to refresh transactions:', error);
        if (historyEl) {
            historyEl.innerHTML = `<p class="has-text-danger">Error: ${error.message}</p>`;
        }
    }
}

// Send transaction from form
async function sendTransaction() {
    const sendBtn = document.getElementById('send-tx-btn');
    const recipientEl = document.getElementById('send-recipient');
    const amountEl = document.getElementById('send-amount');
    const feeEl = document.getElementById('send-fee');

    hideSendStatus();

    // Get token from carousel selection
    const selectedIndex = window.carouselState.selectedIndex;
    const tokenIds = window.carouselState.tokenIds;
    const tokenId = tokenIds.length > 0 ? parseInt(tokenIds[selectedIndex] || '0', 10) : 0;

    // Get form values
    const recipient = recipientEl.value.trim();
    const amount = parseInt(amountEl.value, 10);
    const fee = parseInt(feeEl.value || '1', 10);

    // Basic validation
    if (!recipient) {
        showSendStatus('Please enter a recipient public key', true);
        return;
    }

    if (isNaN(amount) || amount <= 0) {
        showSendStatus('Please enter a valid amount', true);
        return;
    }

    try {
        sendBtn.classList.add('is-loading');

        const result = await walletApp.sendTransaction(recipient, amount, tokenId, fee);

        showSendStatus(`Transaction submitted successfully!`);

        // Clear form
        recipientEl.value = '';
        amountEl.value = '';

        // Refresh balance and transactions after successful send
        await refreshBalance();
        await refreshTransactions();

    } catch (error) {
        console.error('Send transaction failed:', error);
        showSendStatus(error.message, true);
    } finally {
        sendBtn.classList.remove('is-loading');
    }
}

// Refresh balance from API
async function refreshBalance() {
    const balanceEl = document.getElementById('balance-display');
    const refreshBtn = document.getElementById('refresh-balance-btn');

    if (!walletApp.isUnlocked()) {
        if (balanceEl) balanceEl.innerHTML = '<p class="has-text-danger">Wallet is locked</p>';
        return;
    }

    try {
        if (refreshBtn) refreshBtn.classList.add('is-loading');
        if (balanceEl) balanceEl.innerHTML = '<p class="has-text-grey">Loading...</p>';

        const account = await walletApp.refreshBalance();
        updateBalanceDisplay(account);
    } catch (error) {
        console.error('Failed to refresh balance:', error);
        if (balanceEl) {
            balanceEl.innerHTML = `<p class="has-text-danger">Error: ${error.message}</p>`;
        }
    } finally {
        if (refreshBtn) refreshBtn.classList.remove('is-loading');
    }
}

// Initialize when DOM is loaded
document.addEventListener('DOMContentLoaded', async () => {
    console.log('Coins Wallet initializing...');

    // Initialize WebSocket connection
    initWebSocket();

    // Initialize WASM module
    try {
        await walletApp.init();
    } catch (error) {
        console.error('Failed to initialize wallet:', error);
        // Continue anyway - error will be shown when user tries to create/unlock
    }

    // Try to restore session from sessionStorage
    const sessionRestored = await walletApp.restoreSession();

    if (sessionRestored) {
        // Session restored - show wallet screen directly
        console.log('Session restored, showing wallet screen');
        const publicKey = walletApp.getPublicKey();
        updateWalletDisplay(publicKey);
        showScreen('wallet-screen');
        // Auto-refresh balance and transactions
        await refreshBalance();
        await refreshTransactions();
    } else {
        // No session - check if wallet exists in localStorage
        const hasWallet = walletApp.hasWallet();

        if (hasWallet) {
            showScreen('unlock-screen');
        } else {
            showScreen('create-screen');
        }
    }

    // Navigation links
    const showUnlockLink = document.getElementById('show-unlock-link');
    if (showUnlockLink) {
        showUnlockLink.addEventListener('click', () => showScreen('unlock-screen'));
    }

    const showCreateLink = document.getElementById('show-create-link');
    if (showCreateLink) {
        showCreateLink.addEventListener('click', () => showScreen('create-screen'));
    }

    const showImportLink = document.getElementById('show-import-link');
    if (showImportLink) {
        showImportLink.addEventListener('click', () => showScreen('import-screen'));
    }

    const showCreateFromImportLink = document.getElementById('show-create-from-import-link');
    if (showCreateFromImportLink) {
        showCreateFromImportLink.addEventListener('click', () => showScreen('create-screen'));
    }

    // Create wallet button
    const createBtn = document.getElementById('create-wallet-btn');
    if (createBtn) {
        createBtn.addEventListener('click', async () => {
            const password = document.getElementById('create-password').value;
            const confirmPassword = document.getElementById('create-password-confirm').value;

            // Validate passwords match
            if (password !== confirmPassword) {
                alert('Passwords do not match');
                return;
            }

            // Validate password not empty
            if (!password) {
                alert('Please enter a password');
                return;
            }

            try {
                createBtn.classList.add('is-loading');
                const publicKey = await walletApp.createWallet(password);
                updateWalletDisplay(publicKey);
                showScreen('wallet-screen');
                // Auto-refresh balance and transactions after creation
                await refreshBalance();
                await refreshTransactions();
            } catch (error) {
                alert('Failed to create wallet: ' + error.message);
            } finally {
                createBtn.classList.remove('is-loading');
            }
        });
    }

    // Import wallet button
    const importBtn = document.getElementById('import-wallet-btn');
    if (importBtn) {
        importBtn.addEventListener('click', async () => {
            const secretKeyHex = document.getElementById('import-secret-key').value.trim();
            const password = document.getElementById('import-password').value;
            const confirmPassword = document.getElementById('import-password-confirm').value;
            hideError('import-error');

            // Validate secret key is not empty
            if (!secretKeyHex) {
                showError('import-error', 'Please enter a secret key');
                return;
            }

            // Validate passwords match
            if (password !== confirmPassword) {
                showError('import-error', 'Passwords do not match');
                return;
            }

            // Validate password not empty
            if (!password) {
                showError('import-error', 'Please enter a password');
                return;
            }

            try {
                importBtn.classList.add('is-loading');
                const publicKey = await walletApp.importWallet(secretKeyHex, password);
                updateWalletDisplay(publicKey);
                showScreen('wallet-screen');
                // Auto-refresh balance and transactions after import
                await refreshBalance();
                await refreshTransactions();
            } catch (error) {
                showError('import-error', error.message);
            } finally {
                importBtn.classList.remove('is-loading');
            }
        });
    }

    // Unlock wallet button
    const unlockBtn = document.getElementById('unlock-wallet-btn');
    if (unlockBtn) {
        unlockBtn.addEventListener('click', async () => {
            const password = document.getElementById('unlock-password').value;
            hideError('unlock-error');

            if (!password) {
                showError('unlock-error', 'Please enter your password');
                return;
            }

            try {
                unlockBtn.classList.add('is-loading');
                const publicKey = await walletApp.unlockWallet(password);
                updateWalletDisplay(publicKey);
                showScreen('wallet-screen');
                // Auto-refresh balance and transactions after unlock
                await refreshBalance();
                await refreshTransactions();
            } catch (error) {
                showError('unlock-error', error.message);
            } finally {
                unlockBtn.classList.remove('is-loading');
            }
        });
    }

    // Lock wallet button
    const lockBtn = document.getElementById('lock-wallet-btn');
    if (lockBtn) {
        lockBtn.addEventListener('click', () => {
            walletApp.lockWallet();
            document.getElementById('unlock-password').value = '';
            showScreen('unlock-screen');
        });
    }

    // Copy public key - click on address area
    const copyPkBtn = document.getElementById('copy-pk-btn');
    if (copyPkBtn) {
        copyPkBtn.addEventListener('click', () => {
            const pkElement = document.getElementById('wallet-public-key');
            const feedbackElement = document.getElementById('copy-feedback');
            if (pkElement) {
                navigator.clipboard.writeText(pkElement.textContent).then(() => {
                    copyPkBtn.classList.add('copied');
                    if (feedbackElement) feedbackElement.textContent = 'copied!';
                    setTimeout(() => {
                        copyPkBtn.classList.remove('copied');
                        if (feedbackElement) feedbackElement.textContent = 'tap to copy';
                    }, 2000);
                });
            }
        });
    }

    // Quick amount selection
    const quickAmounts = document.querySelectorAll('.quick-amount');
    const amountInput = document.getElementById('send-amount');

    quickAmounts.forEach(btn => {
        btn.addEventListener('click', () => {
            const amount = parseInt(btn.getAttribute('data-amount'), 10);
            const maxBalance = window.carouselState.balances[window.carouselState.tokenIds[window.carouselState.selectedIndex]] || 0;
            const cappedAmount = Math.min(amount, maxBalance);
            if (amountInput) {
                amountInput.value = cappedAmount;
                amountInput.classList.add('scrolling');
                setTimeout(() => amountInput.classList.remove('scrolling'), 150);
            }
            quickAmounts.forEach(b => b.classList.remove('active'));
            btn.classList.add('active');
        });
    });

    // Helper to get max balance for selected token
    function getSelectedTokenMaxBalance() {
        const selectedIndex = window.carouselState.selectedIndex;
        const tokenIds = window.carouselState.tokenIds;
        const balances = window.carouselState.balances;
        if (tokenIds.length === 0) return 0;
        const tokenId = tokenIds[selectedIndex];
        return balances[tokenId] || 0;
    }

    // Scroll to adjust amount
    if (amountInput) {
        let scrollTimeout;
        amountInput.addEventListener('wheel', (e) => {
            e.preventDefault();

            const maxBalance = getSelectedTokenMaxBalance();
            const currentVal = parseInt(amountInput.value.replace(/,/g, '')) || 0;
            // Determine step size based on current value
            let step = 1;
            if (currentVal >= 10000) step = 1000;
            else if (currentVal >= 1000) step = 100;
            else if (currentVal >= 100) step = 10;

            // Scroll up = increase, scroll down = decrease
            const delta = e.deltaY < 0 ? step : -step;
            const newVal = Math.max(0, Math.min(maxBalance, currentVal + delta));

            amountInput.value = newVal;
            amountInput.classList.add('scrolling');

            // Clear active quick amounts
            quickAmounts.forEach(b => b.classList.remove('active'));

            // Remove scrolling class after a delay
            clearTimeout(scrollTimeout);
            scrollTimeout = setTimeout(() => {
                amountInput.classList.remove('scrolling');
            }, 150);
        }, { passive: false });

        // Clear active state and cap at max when manually editing
        amountInput.addEventListener('input', () => {
            quickAmounts.forEach(b => b.classList.remove('active'));
            // Cap value at max balance
            const maxBalance = getSelectedTokenMaxBalance();
            const currentVal = parseInt(amountInput.value.replace(/,/g, '')) || 0;
            if (currentVal > maxBalance) {
                amountInput.value = maxBalance;
            }
        });

        // Only allow numbers
        amountInput.addEventListener('keypress', (e) => {
            if (!/[0-9]/.test(e.key) && e.key !== 'Backspace' && e.key !== 'Delete') {
                e.preventDefault();
            }
        });
    }

    // Refresh balance button
    const refreshBtn = document.getElementById('refresh-balance-btn');
    if (refreshBtn) {
        refreshBtn.addEventListener('click', refreshBalance);
    }

    // Send transaction button
    const sendBtn = document.getElementById('send-tx-btn');
    if (sendBtn) {
        sendBtn.addEventListener('click', sendTransaction);
    }

    // Backup wallet button - shows modal
    const backupBtn = document.getElementById('backup-wallet-btn');
    if (backupBtn) {
        backupBtn.addEventListener('click', () => {
            const modal = document.getElementById('backup-modal');
            if (modal) {
                modal.classList.add('is-active');
                // Reset to password step
                document.getElementById('backup-password-step').style.display = 'block';
                document.getElementById('backup-key-step').style.display = 'none';
                document.getElementById('backup-password').value = '';
                document.getElementById('backup-secret-key').value = '';
                hideError('backup-password-error');
            }
        });
    }

    // Close backup modal buttons
    const closeBackupModal = document.getElementById('close-backup-modal');
    const closeBackupBtn = document.getElementById('close-backup-btn');
    if (closeBackupModal) {
        closeBackupModal.addEventListener('click', () => {
            const modal = document.getElementById('backup-modal');
            if (modal) modal.classList.remove('is-active');
        });
    }
    if (closeBackupBtn) {
        closeBackupBtn.addEventListener('click', () => {
            const modal = document.getElementById('backup-modal');
            if (modal) modal.classList.remove('is-active');
        });
    }

    // Close modal when clicking background
    const modalBackground = document.querySelector('#backup-modal .modal-background');
    if (modalBackground) {
        modalBackground.addEventListener('click', () => {
            const modal = document.getElementById('backup-modal');
            if (modal) modal.classList.remove('is-active');
        });
    }

    // Confirm password and show secret key
    const confirmBackupBtn = document.getElementById('confirm-backup-btn');
    if (confirmBackupBtn) {
        confirmBackupBtn.addEventListener('click', async () => {
            const password = document.getElementById('backup-password').value;
            hideError('backup-password-error');

            if (!password) {
                showError('backup-password-error', 'Please enter your password');
                return;
            }

            try {
                confirmBackupBtn.classList.add('is-loading');
                const secretKeyHex = await walletApp.getSecretKeyHex(password);

                // Show the secret key
                document.getElementById('backup-secret-key').value = secretKeyHex;

                // Switch to key display step
                document.getElementById('backup-password-step').style.display = 'none';
                document.getElementById('backup-key-step').style.display = 'block';
            } catch (error) {
                showError('backup-password-error', error.message);
            } finally {
                confirmBackupBtn.classList.remove('is-loading');
            }
        });
    }

    // Copy secret key to clipboard
    const copySecretKeyBtn = document.getElementById('copy-secret-key-btn');
    if (copySecretKeyBtn) {
        copySecretKeyBtn.addEventListener('click', () => {
            const secretKeyField = document.getElementById('backup-secret-key');
            if (secretKeyField) {
                navigator.clipboard.writeText(secretKeyField.value).then(() => {
                    copySecretKeyBtn.textContent = 'Copied!';
                    copySecretKeyBtn.classList.remove('is-primary');
                    copySecretKeyBtn.classList.add('is-success');
                    setTimeout(() => {
                        copySecretKeyBtn.textContent = 'Copy to Clipboard';
                        copySecretKeyBtn.classList.remove('is-success');
                        copySecretKeyBtn.classList.add('is-primary');
                    }, 2000);
                });
            }
        });
    }

    // Transaction detail modal handlers
    const txDetailModal = document.getElementById('tx-detail-modal');
    const closeTxDetailModal = document.getElementById('close-tx-detail-modal');
    const closeTxDetailBtn = document.getElementById('close-tx-detail-btn');
    const txDetailBackground = txDetailModal?.querySelector('.modal-background');

    function closeTxModal() {
        if (txDetailModal) txDetailModal.classList.remove('is-active');
    }

    if (closeTxDetailModal) closeTxDetailModal.addEventListener('click', closeTxModal);
    if (closeTxDetailBtn) closeTxDetailBtn.addEventListener('click', closeTxModal);
    if (txDetailBackground) txDetailBackground.addEventListener('click', closeTxModal);

    // Copy counterparty address in transaction detail
    const txDetailCounterparty = document.getElementById('tx-detail-counterparty');
    if (txDetailCounterparty) {
        txDetailCounterparty.addEventListener('click', () => {
            const addressText = document.getElementById('tx-detail-counterparty-text');
            const feedbackEl = document.getElementById('tx-detail-copy-feedback');
            if (addressText) {
                navigator.clipboard.writeText(addressText.textContent).then(() => {
                    txDetailCounterparty.classList.add('copied');
                    if (feedbackEl) feedbackEl.textContent = 'copied!';
                    setTimeout(() => {
                        txDetailCounterparty.classList.remove('copied');
                        if (feedbackEl) feedbackEl.textContent = 'tap to copy';
                    }, 2000);
                });
            }
        });
    }

    // Carousel for balance tokens with morphing sizes
    const balanceScroll = document.getElementById('balance-scroll');
    const balanceTokens = document.getElementById('balance-tokens');

    if (balanceScroll && balanceTokens) {
        let isDragging = false;
        let startX = 0;
        let dragOffset = 0;

        function updateTokenSizes(offset) {
            const items = balanceTokens.querySelectorAll('.balance-token-item');
            const positions = Array.from(items).map(item => item.offsetLeft);
            const widths = Array.from(items).map(item => item.offsetWidth);

            // Use the balance-hero box as the clipping boundary
            const balanceHero = balanceScroll.closest('.balance-hero');
            const heroRect = balanceHero ? balanceHero.getBoundingClientRect() : null;
            const scrollRect = balanceScroll.getBoundingClientRect();
            const containerWidth = heroRect ? (heroRect.right - scrollRect.left) : balanceScroll.clientWidth;
            const containerCenter = balanceScroll.clientWidth / 2;

            // Find which item is closest to center during drag
            let closestIndex = 0;
            let closestDist = Infinity;

            for (let i = 0; i < positions.length; i++) {
                const itemCenter = positions[i] + offset + (widths[i] / 2);
                const dist = Math.abs(itemCenter - containerCenter);
                if (dist < closestDist) {
                    closestDist = dist;
                    closestIndex = i;
                }
            }

            items.forEach((item, index) => {
                const itemPos = positions[index] || 0;
                const itemWidth = widths[index] || 100;

                // Check if item would be clipped at right edge
                const visibleLeft = itemPos + offset;
                const visibleRight = visibleLeft + itemWidth;
                const isClippedRight = visibleRight > containerWidth;

                if (isClippedRight) {
                    item.style.visibility = 'hidden';
                } else {
                    item.style.visibility = 'visible';
                }

                const amountEl = item.querySelector('.balance-token-amount');
                const nameEl = item.querySelector('.balance-token-name');
                if (!amountEl || !nameEl) return;

                // Calculate distance from center for smooth size transition
                const itemCenter = itemPos + offset + (itemWidth / 2);
                const distFromCenter = Math.abs(itemCenter - containerCenter);
                const normalizedDist = distFromCenter / 150; // Normalize for smooth transition

                if (index === closestIndex && normalizedDist < 0.5) {
                    // Centered item - large and accented
                    const scale = 1 - normalizedDist * 0.4; // Slight size reduction as it moves off center
                    amountEl.style.fontSize = `${2.5 * scale}rem`;
                    amountEl.style.color = 'var(--coins-accent)';
                    nameEl.style.fontSize = `${1 * scale}rem`;
                    nameEl.style.color = 'var(--coins-accent)';
                    item.style.opacity = '1';
                } else {
                    // Other items - uniform smaller size
                    amountEl.style.fontSize = '1.25rem';
                    amountEl.style.color = 'var(--coins-text)';
                    nameEl.style.fontSize = '0.75rem';
                    nameEl.style.color = 'var(--coins-text-secondary)';
                    item.style.opacity = '0.7';
                }
            });
        }

        function getItemPositions() {
            const items = balanceTokens.querySelectorAll('.balance-token-item');
            return Array.from(items).map(item => item.offsetLeft);
        }

        function getItemWidths() {
            const items = balanceTokens.querySelectorAll('.balance-token-item');
            return Array.from(items).map(item => item.offsetWidth);
        }

        function snapToNearest() {
            const positions = getItemPositions();
            const widths = getItemWidths();
            if (positions.length === 0) return;

            const containerWidth = balanceScroll.clientWidth;
            const containerCenter = containerWidth / 2;

            // Find which item's center is closest to the container center
            let closestIndex = 0;
            let closestDist = Infinity;

            for (let i = 0; i < positions.length; i++) {
                // Item center position in viewport = positions[i] + currentOffset + width/2
                const itemCenter = positions[i] + window.carouselState.currentOffset + (widths[i] / 2);
                const dist = Math.abs(itemCenter - containerCenter);
                if (dist < closestDist) {
                    closestDist = dist;
                    closestIndex = i;
                }
            }

            // Calculate offset to center this item
            const selectedPos = positions[closestIndex];
            const selectedWidth = widths[closestIndex];
            const centerOffset = containerCenter - selectedPos - (selectedWidth / 2);

            window.carouselState.currentOffset = centerOffset;
            window.carouselState.selectedIndex = closestIndex;

            balanceTokens.style.transform = `translateX(${centerOffset}px)`;
            updateCarouselSizes(closestIndex);
        }

        function handleDragStart(clientX) {
            isDragging = true;
            startX = clientX;
            dragOffset = window.carouselState.currentOffset;
            balanceScroll.classList.add('grabbing');
            balanceTokens.classList.add('dragging');
        }

        function handleDragMove(clientX) {
            if (!isDragging) return;

            const diff = clientX - startX;
            const positions = getItemPositions();
            const widths = getItemWidths();
            const containerWidth = balanceScroll.clientWidth;
            const containerCenter = containerWidth / 2;

            // Calculate offset limits for centered carousel
            // maxOffset: offset to center the first item (index 0)
            // minOffset: offset to center the last item
            const firstItemCenter = positions[0] + (widths[0] / 2);
            const lastItemCenter = positions[positions.length - 1] + (widths[widths.length - 1] / 2);
            const maxOffset = containerCenter - firstItemCenter;
            const minOffset = containerCenter - lastItemCenter;

            // Allow slight overscroll (50px) for bounce effect, but will snap back
            window.carouselState.currentOffset = Math.max(minOffset - 50, Math.min(maxOffset + 50, dragOffset + diff));
            balanceTokens.style.transform = `translateX(${window.carouselState.currentOffset}px)`;
            updateTokenSizes(window.carouselState.currentOffset);
        }

        function handleDragEnd() {
            if (!isDragging) return;
            isDragging = false;
            balanceScroll.classList.remove('grabbing');
            balanceTokens.classList.remove('dragging');
            snapToNearest();
        }

        // Mouse events
        balanceScroll.addEventListener('mousedown', (e) => {
            e.preventDefault();
            handleDragStart(e.clientX);
        });

        document.addEventListener('mousemove', (e) => {
            handleDragMove(e.clientX);
        });

        document.addEventListener('mouseup', () => {
            handleDragEnd();
        });

        // Touch events
        balanceScroll.addEventListener('touchstart', (e) => {
            handleDragStart(e.touches[0].clientX);
        }, { passive: true });

        balanceScroll.addEventListener('touchmove', (e) => {
            handleDragMove(e.touches[0].clientX);
        }, { passive: true });

        balanceScroll.addEventListener('touchend', () => {
            handleDragEnd();
        });

        // Note: sizes are initialized by initBalanceCarousel() called from updateBalanceDisplay()
    }

    console.log('Coins Wallet initialized');
});

// Balance carousel state and initialization
window.carouselState = {
    currentOffset: 0,
    itemWidth: 120,
    selectedIndex: 0,
    tokenIds: [],      // Array of token IDs in carousel order
    balances: {}       // Map of token_id -> balance
};

window.initBalanceCarousel = function(initialIndex = 0) {
    const balanceScroll = document.getElementById('balance-scroll');
    const balanceTokens = document.getElementById('balance-tokens');
    if (!balanceTokens || !balanceScroll) return;

    const items = balanceTokens.querySelectorAll('.balance-token-item');
    if (items.length === 0) return;

    // Store the selected index
    window.carouselState.selectedIndex = initialIndex;

    // First, set sizes based on selection
    items.forEach((item, index) => {
        const amountEl = item.querySelector('.balance-token-amount');
        const nameEl = item.querySelector('.balance-token-name');
        if (!amountEl || !nameEl) return;

        if (index === initialIndex) {
            // Selected item - large and accented
            amountEl.style.fontSize = '2.5rem';
            amountEl.style.color = 'var(--coins-accent)';
            nameEl.style.fontSize = '1rem';
            nameEl.style.color = 'var(--coins-accent)';
            item.style.opacity = '1';
        } else {
            // Other items - uniform smaller
            amountEl.style.fontSize = '1.25rem';
            amountEl.style.color = 'var(--coins-text)';
            nameEl.style.fontSize = '0.75rem';
            nameEl.style.color = 'var(--coins-text-secondary)';
            item.style.opacity = '0.7';
        }
        item.style.visibility = 'visible';
    });

    // Force a reflow to get accurate measurements after size changes
    balanceTokens.offsetHeight;

    // Now calculate positions with correct sizes
    const containerWidth = balanceScroll.clientWidth;
    const positions = Array.from(items).map(item => item.offsetLeft);
    const itemWidths = Array.from(items).map(item => item.offsetWidth);

    // Center position: item center should be at container center
    const selectedPos = positions[initialIndex] || 0;
    const selectedWidth = itemWidths[initialIndex] || 100;
    const centerOffset = (containerWidth / 2) - selectedPos - (selectedWidth / 2);

    window.carouselState.currentOffset = centerOffset;
    balanceTokens.style.transform = `translateX(${centerOffset}px)`;
};

function updateCarouselSizes(selectedIndex) {
    const balanceTokens = document.getElementById('balance-tokens');
    if (!balanceTokens) return;

    const items = balanceTokens.querySelectorAll('.balance-token-item');
    items.forEach((item, index) => {
        const amountEl = item.querySelector('.balance-token-amount');
        const nameEl = item.querySelector('.balance-token-name');
        if (!amountEl || !nameEl) return;

        if (index === selectedIndex) {
            // Selected/centered item - large and accented
            amountEl.style.fontSize = '2.5rem';
            amountEl.style.color = 'var(--coins-accent)';
            nameEl.style.fontSize = '1rem';
            nameEl.style.color = 'var(--coins-accent)';
            item.style.opacity = '1';
        } else {
            // Other items - uniform smaller size
            amountEl.style.fontSize = '1.25rem';
            amountEl.style.color = 'var(--coins-text)';
            nameEl.style.fontSize = '0.75rem';
            nameEl.style.color = 'var(--coins-text-secondary)';
            item.style.opacity = '0.7';
        }
        item.style.visibility = 'visible';
    });

    // Validate send amount against new selection's max balance
    const sendAmountEl = document.getElementById('send-amount');
    if (sendAmountEl) {
        const tokenIds = window.carouselState.tokenIds;
        const balances = window.carouselState.balances;
        if (tokenIds.length > 0) {
            const tokenId = tokenIds[selectedIndex];
            const maxBalance = balances[tokenId] || 0;
            const currentVal = parseInt(sendAmountEl.value.replace(/,/g, '')) || 0;
            if (currentVal > maxBalance) {
                sendAmountEl.value = maxBalance;
            }
        }
    }
}

// Export for use by other scripts (future stories)
window.walletApp = walletApp;
window.showScreen = showScreen;
window.refreshBalance = refreshBalance;
window.refreshTransactions = refreshTransactions;
window.sendTransaction = sendTransaction;
