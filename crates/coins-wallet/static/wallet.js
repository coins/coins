// Coins Wallet - Browser-based wallet with WASM BLS signing
// Uses Web Crypto API for AES-GCM encryption with PBKDF2 key derivation
// Includes WebSocket support for real-time updates

const STORAGE_KEY = 'coins_wallet_key';
const SESSION_KEY = 'coins_wallet_session_key';
const NONCE_KEY = 'coins_wallet_nonce';
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

// Cache for WebSocket refresh deduplication (skip re-render when data unchanged)
let lastBalanceJSON = null;
let lastTransactionsJSON = null;
let lastPendingJSON = null;

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
 * Check connection status for indexer, publisher, and explorer
 */
async function checkServiceConnections() {
    const indexerDot = document.getElementById('indexer-status-dot');
    const indexerText = document.getElementById('indexer-status-text');
    const publisherDot = document.getElementById('publisher-status-dot');
    const publisherText = document.getElementById('publisher-status-text');
    const explorerDot = document.getElementById('explorer-status-dot');
    const explorerText = document.getElementById('explorer-status-text');
    const mainDot = document.getElementById('connection-dot');

    let indexerConnected = false;
    let publisherConnected = false;
    let explorerConnected = false;

    // Check indexer health
    try {
        const resp = await fetch('/api/account/health-check-dummy', {
            method: 'GET',
            signal: AbortSignal.timeout(3000)
        });
        // Even a 404 means the indexer is reachable
        indexerConnected = true;
        if (indexerText) indexerText.textContent = 'Connected';
    } catch (e) {
        if (indexerText) indexerText.textContent = 'Offline';
    }
    if (indexerDot) {
        indexerDot.classList.toggle('connected', indexerConnected);
        indexerDot.classList.toggle('disconnected', !indexerConnected);
    }

    // Check publisher status
    try {
        const resp = await fetch('/api/publisher/status', {
            signal: AbortSignal.timeout(3000)
        });
        if (resp.ok) {
            const status = await resp.json();
            publisherConnected = true;
            if (publisherText) {
                const nextLoop = status.secs_until_next_loop;
                if (nextLoop !== null) {
                    publisherText.textContent = `Next batch: ${nextLoop}s`;
                } else {
                    publisherText.textContent = 'Connected';
                }
            }
        } else {
            if (publisherText) publisherText.textContent = 'Offline';
        }
    } catch (e) {
        if (publisherText) publisherText.textContent = 'Offline';
    }
    if (publisherDot) {
        publisherDot.classList.toggle('connected', publisherConnected);
        publisherDot.classList.toggle('disconnected', !publisherConnected);
    }

    // Check explorer health
    try {
        const resp = await fetch('/api/explorer/health', {
            signal: AbortSignal.timeout(3000)
        });
        explorerConnected = resp.ok;
        if (explorerText) explorerText.textContent = explorerConnected ? 'Connected' : 'Offline';
    } catch (e) {
        if (explorerText) explorerText.textContent = 'Offline';
    }
    if (explorerDot) {
        explorerDot.classList.toggle('connected', explorerConnected);
        explorerDot.classList.toggle('disconnected', !explorerConnected);
    }

    // Update main dot based on all services
    if (mainDot) {
        const allConnected = indexerConnected && publisherConnected && explorerConnected;
        mainDot.classList.toggle('disconnected', !allConnected);
    }
}

// Check connections when hovering over status indicator
document.addEventListener('DOMContentLoaded', () => {
    const statusIndicator = document.getElementById('status-indicator');
    if (statusIndicator) {
        statusIndicator.addEventListener('mouseenter', () => {
            checkServiceConnections();
        });
    }
});

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
        case 'pending_txs_update':
            // Refresh pending transaction statuses
            refreshPendingTransactions();
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

        // Initialize nonce to 0 for new wallet
        this.setLocalNonce(0);

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

        // Fetch account from API to get the current nonce
        try {
            const pk = walletKey.public_key_hex();
            const response = await fetch(`/api/account/${pk}`);
            if (response.ok) {
                const account = await response.json();
                this.setLocalNonce(account.nonce || 0);
            } else {
                // Account doesn't exist yet, start at 0
                this.setLocalNonce(0);
            }
        } catch (error) {
            console.warn('Could not fetch account nonce during import:', error);
            this.setLocalNonce(0);
        }

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
     * Get the local nonce from localStorage
     * @returns {number} The stored nonce or 0 if not set
     */
    getLocalNonce() {
        const stored = localStorage.getItem(NONCE_KEY);
        return stored !== null ? parseInt(stored, 10) : 0;
    }

    /**
     * Set the local nonce in localStorage
     * @param {number} nonce - The nonce value to store
     */
    setLocalNonce(nonce) {
        localStorage.setItem(NONCE_KEY, nonce.toString());
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
     * Fetch pending transactions from the explorer
     * @returns {Promise<Array>} Array of pending transactions with status
     */
    async fetchPendingTransactions() {
        if (!currentKey) {
            return [];
        }

        // We need the account ID to filter pending transactions
        try {
            const account = await this.refreshBalance();
            if (!account) {
                return [];
            }

            const senderId = account.id;
            const response = await fetch(`/api/pending-transactions?sender_id=${senderId}`);

            if (!response.ok) {
                console.warn('Failed to fetch pending transactions:', response.status);
                return [];
            }

            return await response.json();
        } catch (error) {
            console.warn('Error fetching pending transactions:', error);
            return [];
        }
    }

    /**
     * Resync nonce from the API
     * Useful when local nonce gets out of sync with on-chain state
     * @returns {Promise<number>} The resynced nonce
     */
    async resyncNonce() {
        const account = await this.refreshBalance();
        const nonce = account ? account.nonce : 0;
        this.setLocalNonce(nonce);
        return nonce;
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
     * @param {string} recipient - Recipient: account ID (number) or public key hex (64 chars)
     * @param {number} amount - Amount to send
     * @param {number} tokenId - Token ID (default 0 for native)
     * @param {number} fee - Transaction fee
     * @returns {Promise<Object>} Transaction result
     */
    async sendTransaction(recipient, amount, tokenId = 0, fee = 1) {
        if (!currentKey) {
            throw new Error('Wallet is locked');
        }

        // Resolve recipient: can be account ID (number) or public key (64 hex chars)
        let recipientPk;
        if (/^\d+$/.test(recipient)) {
            // It's an account ID - fetch the account to get the public key
            const accountId = parseInt(recipient, 10);
            const response = await fetch(`/api/account/by-id/${accountId}`);
            if (response.ok) {
                const account = await response.json();
                recipientPk = account.pk;
            } else if (response.status === 404) {
                throw new Error(`Account #${accountId} not found`);
            } else {
                throw new Error(`Error fetching account #${accountId}: ${response.status}`);
            }
        } else if (/^[a-fA-F0-9]{64}$/.test(recipient)) {
            // It's a public key
            recipientPk = recipient;
        } else {
            throw new Error('Invalid recipient: must be account ID (number) or 64 hex characters');
        }

        // Validate amount
        if (!Number.isInteger(amount) || amount <= 0) {
            throw new Error('Amount must be a positive integer');
        }

        // Validate fee
        if (!Number.isInteger(fee) || fee < 1 || fee > 255) {
            throw new Error('Fee must be between 1 and 255');
        }

        // Fetch account to get sender_id
        const account = await this.refreshBalance();
        if (account === null) {
            throw new Error('Account not found. You need to receive tokens first before sending.');
        }

        const senderId = account.id;

        // Get nonce from localStorage
        let nonce = this.getLocalNonce();

        // If nonce isn't set in localStorage yet (existing wallet from before this change),
        // initialize from account
        if (localStorage.getItem(NONCE_KEY) === null) {
            nonce = account.nonce || 0;
        }

        // Increment and store for next transaction
        this.setLocalNonce(nonce + 1);

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
            // On failure, decrement the nonce since transaction wasn't accepted
            this.setLocalNonce(nonce);
            throw new Error(`Transaction failed: ${responseText}`);
        }

        console.log('Transaction submitted successfully:', responseText);
        return { success: true, message: responseText, usedNonce: nonce, signature: signature };
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

    // Update send token info display
    updateSendTokenInfo();
}

// Show send status message (only for errors now)
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

// Show send animation overlay
function showSendAnimation() {
    const sendCard = document.getElementById('send-card');
    if (!sendCard) return;

    // Make sure the card has position relative for absolute overlay
    sendCard.style.position = 'relative';

    // Create overlay with coin stack animation
    const overlay = document.createElement('div');
    overlay.className = 'send-animation-overlay';
    overlay.id = 'send-animation-overlay';
    overlay.innerHTML = `
        <div class="coin-stack">
            <div class="coin coin-dropping">◎</div>
            <div class="coin coin-3">◎</div>
            <div class="coin coin-2">◎</div>
            <div class="coin coin-1">◎</div>
        </div>
        <div class="send-animation-text">Sending...</div>
    `;

    sendCard.appendChild(overlay);

    // Remove after animation completes
    setTimeout(() => {
        overlay.style.opacity = '0';
        overlay.style.transition = 'opacity 0.2s ease-out';
        setTimeout(() => {
            overlay.remove();
        }, 200);
    }, 1000);
}

// Update transaction history display
// Balance polling interval (poll every 30s when wallet is unlocked)
const BALANCE_POLL_INTERVAL_MS = 30000;
let balancePollInterval = null;

function startBalancePolling() {
    stopBalancePolling();
    balancePollInterval = setInterval(async () => {
        if (walletApp.isUnlocked()) {
            await refreshBalance();
            await refreshTransactions();
        } else {
            stopBalancePolling();
        }
    }, BALANCE_POLL_INTERVAL_MS);
}

function stopBalancePolling() {
    if (balancePollInterval) {
        clearInterval(balancePollInterval);
        balancePollInterval = null;
    }
}

// Store pending transactions that haven't been confirmed yet
window.pendingTransactions = window.pendingTransactions || [];

// Store pending transactions fetched from API (explorer)
window.apiPendingTransactions = window.apiPendingTransactions || [];

// Pagination state for activity list
const ACTIVITY_PAGE_SIZE = 20;
window.activityDisplayCount = window.activityDisplayCount || ACTIVITY_PAGE_SIZE;

// Activity filter state
window.activityFilters = window.activityFilters || {
    tokenIds: null, // null = all tokens, Set of enabled token IDs when filtering
    dateDays: 'all', // 'all', number of days, or 'custom'
    customFrom: null, // Date string (yyyy-mm-dd) for custom range start
    customTo: null,   // Date string (yyyy-mm-dd) for custom range end
};

// Date drum picker state
window.dateDrumState = {
    currentOffset: 0,
    selectedIndex: 0,
    itemHeight: 22
};

// Countdown timer for pending transactions
let pendingCountdownInterval = null;
let pendingCountdownSecs = null;

// Polling interval for pending tx status (after countdown reaches 0)
let pendingStatusPollInterval = null;

// Start countdown timer for pending transactions
function startPendingCountdown() {
    // Clear any existing interval
    if (pendingCountdownInterval) {
        clearInterval(pendingCountdownInterval);
        pendingCountdownInterval = null;
    }

    // If no pending transactions, don't start countdown
    if (!window.pendingTransactions || window.pendingTransactions.length === 0) {
        return;
    }

    // Fetch initial publisher status
    fetchPublisherCountdown();

    // Update every second
    pendingCountdownInterval = setInterval(() => {
        if (pendingCountdownSecs !== null && pendingCountdownSecs > 1) {
            pendingCountdownSecs--;
            updatePendingCountdownDisplay();
        } else {
            // Countdown reached 1 or below - transition to "Broadcasting"
            pendingCountdownSecs = 0;
            updatePendingCountdownDisplay();
            // Start polling for actual status from API
            startPendingStatusPolling();
        }
    }, 1000);
}

// Start polling for pending transaction status from API
function startPendingStatusPolling() {
    // Stop the countdown interval since we're now polling
    if (pendingCountdownInterval) {
        clearInterval(pendingCountdownInterval);
        pendingCountdownInterval = null;
    }

    // Clear any existing poll interval
    if (pendingStatusPollInterval) {
        clearInterval(pendingStatusPollInterval);
        pendingStatusPollInterval = null;
    }

    // Fetch immediately
    refreshPendingTransactions();

    // Poll every 3 seconds
    pendingStatusPollInterval = setInterval(async () => {
        await refreshPendingTransactions();

        // If no more local pending or API pending transactions, stop polling
        if ((!window.pendingTransactions || window.pendingTransactions.length === 0) &&
            (!window.apiPendingTransactions || window.apiPendingTransactions.length === 0)) {
            stopPendingStatusPolling();
        }
    }, 3000);
}

// Stop polling for pending transaction status
function stopPendingStatusPolling() {
    if (pendingStatusPollInterval) {
        clearInterval(pendingStatusPollInterval);
        pendingStatusPollInterval = null;
    }
}

// Refresh pending transactions from API and re-render
async function refreshPendingTransactions() {
    try {
        const pendingFromAPI = await walletApp.fetchPendingTransactions();
        const json = JSON.stringify(pendingFromAPI);
        if (json === lastPendingJSON) return;
        lastPendingJSON = json;
        window.apiPendingTransactions = pendingFromAPI || [];
        renderTransactionHistory();
    } catch (error) {
        console.warn('Error refreshing pending transactions:', error);
    }
}

// Fetch countdown from publisher
async function fetchPublisherCountdown() {
    try {
        const resp = await fetch('/api/publisher/status', {
            signal: AbortSignal.timeout(3000)
        });
        if (resp.ok) {
            const status = await resp.json();
            if (status.secs_until_next_loop !== null) {
                pendingCountdownSecs = status.secs_until_next_loop;
                updatePendingCountdownDisplay();
            }
        }
    } catch (e) {
        console.warn('Could not fetch publisher status for countdown:', e);
    }
}

// Update the countdown display in pending transactions
function updatePendingCountdownDisplay() {
    const pendingTags = document.querySelectorAll('.pending-tag');
    pendingTags.forEach(el => {
        if (pendingCountdownSecs !== null && pendingCountdownSecs > 0) {
            el.innerHTML = `Broadcast in <span class="pending-countdown">${pendingCountdownSecs}s</span>`;
        } else {
            el.innerHTML = 'Broadcasting';
            el.classList.remove('is-warning');
            el.classList.add('is-info');
        }
    });
}

// Stop countdown timer and status polling
function stopPendingCountdown() {
    if (pendingCountdownInterval) {
        clearInterval(pendingCountdownInterval);
        pendingCountdownInterval = null;
    }
    if (pendingStatusPollInterval) {
        clearInterval(pendingStatusPollInterval);
        pendingStatusPollInterval = null;
    }
    pendingCountdownSecs = null;
}

// Add a pending transaction to the activity list
function addPendingTransaction(tx) {
    const currentPk = walletApp.getPublicKey();

    // Add to pending list with timestamp
    window.pendingTransactions.unshift({
        ...tx,
        sender_pk: currentPk,
        pending: true,
        timestamp: Date.now()
    });

    // Re-render the transaction history with the pending transaction at the top
    renderTransactionHistory();

    // Start countdown timer
    startPendingCountdown();
}

// Render transaction history including pending transactions
function renderTransactionHistory() {
    const historyEl = document.getElementById('transaction-history');
    if (!historyEl) return;

    const confirmedTxs = window.currentTransactions || [];
    const localPendingTxs = window.pendingTransactions || [];
    const apiPendingTxs = window.apiPendingTransactions || [];
    const currentPk = walletApp.getPublicKey();

    // Filter out local pending transactions that are now:
    // 1. In API pending (we'll show API version instead)
    // 2. Confirmed in the indexer
    const stillLocalPending = localPendingTxs.filter(pending => {
        // Check if in API pending (match by sender_pk + nonce)
        const inApiPending = apiPendingTxs.some(api =>
            api.sender_pk === pending.sender_pk &&
            api.nonce === pending.nonce
        );
        if (inApiPending) return false;

        // Check if confirmed
        const isConfirmed = confirmedTxs.some(confirmed =>
            confirmed.sender_pk === pending.sender_pk &&
            confirmed.nonce === pending.nonce &&
            confirmed.direction === pending.direction
        );
        return !isConfirmed;
    });
    window.pendingTransactions = stillLocalPending;

    // Filter out API pending transactions that are confirmed (finalized ones)
    // Note: API pending txs with status 'unconfirmed' have confirmations but aren't in our confirmed list yet
    const stillApiPending = apiPendingTxs.filter(pending => {
        // Keep if not finalized (unconfirmed/broadcasting/publishing are all pending states)
        if (!pending.finalized) return true;

        // Check if in confirmed txs
        const isConfirmed = confirmedTxs.some(confirmed =>
            confirmed.sender_pk === pending.sender_pk &&
            confirmed.nonce === pending.nonce
        );
        return !isConfirmed;
    });
    window.apiPendingTransactions = stillApiPending;

    // Stop polling if no more pending transactions
    if (stillLocalPending.length === 0 && stillApiPending.length === 0) {
        stopPendingCountdown();
    }

    if (confirmedTxs.length === 0 && stillLocalPending.length === 0 && stillApiPending.length === 0) {
        historyEl.innerHTML = '<p class="has-text-grey">No transactions yet</p>';
        return;
    }

    const greekLetters = ['α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ'];
    const greekNames = ['Alpha', 'Beta', 'Gamma', 'Delta', 'Epsilon', 'Zeta', 'Eta', 'Theta', 'Iota', 'Kappa'];

    let html = '';

    // 1. Add local pending transactions (just submitted, not yet in API)
    // These show "Publishing in Xs" with countdown
    for (const tx of stillLocalPending) {
        const tokenIdx = tx.token_id;
        const tokenLetter = greekLetters[tokenIdx] || `#${tokenIdx}`;
        const tokenName = greekNames[tokenIdx] || `Token ${tokenIdx}`;
        const counterpartyShort = tx.recipient_pk.substring(0, 8) + '...' + tx.recipient_pk.substring(56);

        let tagHtml;
        if (pendingCountdownSecs !== null && pendingCountdownSecs > 0) {
            tagHtml = `<span class="tag is-warning is-light pending-tag">Broadcast in <span class="pending-countdown">${pendingCountdownSecs}s</span></span>`;
        } else {
            tagHtml = `<span class="tag is-info is-light pending-tag">Broadcasting</span>`;
        }
        html += `
            <div class="transaction-item clickable pending-tx">
                <div class="columns is-mobile is-vcentered mb-0">
                    <div class="column">
                        <p class="has-text-weight-semibold has-text-danger">
                            - ${tx.amount.toLocaleString()} ${tokenLetter} ${tokenName}
                        </p>
                        <p class="is-size-7 has-text-grey">
                            Sending to
                            <span class="is-family-monospace">${counterpartyShort}</span>
                        </p>
                    </div>
                    <div class="column is-narrow has-text-right">
                        ${tagHtml}
                    </div>
                </div>
            </div>
        `;
    }

    // 2. Add API pending transactions (from explorer)
    // These show actual status: Publishing, Broadcasting, or Unconfirmed with confirmations
    for (const tx of stillApiPending) {
        const tokenIdx = tx.token_id;
        const tokenLetter = greekLetters[tokenIdx] || `#${tokenIdx}`;
        const tokenName = greekNames[tokenIdx] || `Token ${tokenIdx}`;
        const counterpartyShort = tx.recipient_pk.substring(0, 8) + '...' + tx.recipient_pk.substring(56);

        let statusHtml;
        if (tx.status === 'publishing') {
            statusHtml = '<span class="tag is-warning is-light">Publishing</span>';
        } else if (tx.status === 'broadcasting') {
            statusHtml = '<span class="tag is-info is-light">Broadcasting</span>';
        } else if (tx.status === 'unconfirmed') {
            if (tx.confirmations > 0) {
                statusHtml = `<span class="tag is-warning is-light">${tx.confirmations} conf</span>`;
            } else {
                statusHtml = '<span class="tag is-info is-light">Unconfirmed</span>';
            }
        } else {
            statusHtml = '<span class="tag is-info is-light">Pending</span>';
        }

        html += `
            <div class="transaction-item clickable pending-tx">
                <div class="columns is-mobile is-vcentered mb-0">
                    <div class="column">
                        <p class="has-text-weight-semibold has-text-danger">
                            - ${tx.amount.toLocaleString()} ${tokenLetter} ${tokenName}
                        </p>
                        <p class="is-size-7 has-text-grey">
                            Sending to
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

    // 3. Add confirmed transactions (newest first, reversed order)
    // Apply filters
    const filteredTxs = applyActivityFilters(confirmedTxs);

    // Apply pagination: only show up to activityDisplayCount confirmed items
    const pendingCount = stillLocalPending.length + stillApiPending.length;
    const confirmedLimit = window.activityDisplayCount - pendingCount;
    const totalConfirmed = filteredTxs.length;
    let confirmedShown = 0;

    for (let i = filteredTxs.length - 1; i >= 0 && confirmedShown < confirmedLimit; i--) {
        confirmedShown++;
        const tx = filteredTxs[i];
        const isOutgoing = tx.direction === 'outgoing';
        const directionClass = isOutgoing ? 'has-text-danger' : 'has-text-success';
        const directionIcon = isOutgoing ? '-' : '+';
        const directionLabel = isOutgoing ? 'Sent' : 'Received';

        const counterpartyPk = isOutgoing ? tx.recipient_pk : tx.sender_pk;
        const counterpartyShort = counterpartyPk.substring(0, 8) + '...' + counterpartyPk.substring(56);

        const tokenIdx = tx.token_id;
        const tokenLetter = greekLetters[tokenIdx] || `#${tokenIdx}`;
        const tokenName = greekNames[tokenIdx] || `Token ${tokenIdx}`;

        let statusHtml;
        if (tx.finalized) {
            statusHtml = '<span class="tag is-success is-light">Finalized</span>';
        } else if (tx.confirmations > 0) {
            statusHtml = `<span class="tag is-warning is-light">${tx.confirmations} conf</span>`;
        } else {
            // Confirmed but 0 confirmations - show "Unconfirmed"
            statusHtml = '<span class="tag is-info is-light">Unconfirmed</span>';
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

    // "Load more" button if there are more confirmed transactions to show
    const totalDisplayed = pendingCount + confirmedShown;
    const totalAll = pendingCount + totalConfirmed;
    if (confirmedShown < totalConfirmed) {
        html += `
            <div class="activity-load-more">
                <button class="button is-light is-fullwidth is-small" id="load-more-activity">
                    Load more
                </button>
                <p class="activity-count">Showing ${totalDisplayed} of ${totalAll}</p>
            </div>
        `;
    }

    historyEl.innerHTML = html;

    // Wire up "Load more" button
    const loadMoreBtn = document.getElementById('load-more-activity');
    if (loadMoreBtn) {
        loadMoreBtn.addEventListener('click', () => {
            window.activityDisplayCount += ACTIVITY_PAGE_SIZE;
            renderTransactionHistory();
        });
    }

    // Add click handlers for confirmed transactions
    historyEl.querySelectorAll('.transaction-item.clickable[data-tx-index]').forEach(item => {
        item.addEventListener('click', () => {
            const index = parseInt(item.getAttribute('data-tx-index'), 10);
            if (window.currentTransactions && window.currentTransactions[index]) {
                showTransactionDetail(window.currentTransactions[index]);
            }
        });
    });
}

function updateTransactionHistory(transactions) {
    // Store transactions for later use
    window.currentTransactions = transactions || [];

    // Update token filter options based on available tokens
    updateTokenFilterOptions(transactions || []);

    // Use the render function that handles both pending and confirmed
    renderTransactionHistory();
}

function updateTokenFilterOptions(transactions) {
    const container = document.getElementById('activity-token-chips');
    if (!container) return;

    const tokenSet = new Set();
    for (const tx of transactions) {
        tokenSet.add(tx.token_id);
    }

    const sorted = [...tokenSet].sort((a, b) => a - b);
    if (sorted.length <= 1) {
        container.innerHTML = '';
        return;
    }

    const activeSet = window.activityFilters.tokenIds;
    let html = '';
    for (const tid of sorted) {
        const idx = parseInt(tid);
        const letter = greekLetters[idx] || `#${tid}`;
        const name = greekNames[idx] || `Token ${tid}`;
        const isActive = !activeSet || activeSet.has(idx);
        html += `<button class="token-chip${isActive ? ' active' : ''}" data-token-id="${idx}">${letter} ${name}</button>`;
    }
    container.innerHTML = html;

    container.querySelectorAll('.token-chip').forEach(chip => {
        chip.addEventListener('click', () => {
            const tid = parseInt(chip.dataset.tokenId);
            toggleTokenFilter(tid, sorted.map(t => parseInt(t)));
        });
    });
}

function toggleTokenFilter(tid, allTokenIds) {
    let activeSet = window.activityFilters.tokenIds;

    if (!activeSet) {
        // Currently showing all — turn off the clicked token
        activeSet = new Set(allTokenIds);
        activeSet.delete(tid);
    } else if (activeSet.has(tid)) {
        // Token is on — turn it off
        activeSet.delete(tid);
    } else {
        // Token is off — turn it on
        activeSet.add(tid);
    }

    // If all are selected (or none), reset to null (show all)
    if (activeSet.size === 0 || activeSet.size === allTokenIds.length) {
        activeSet = null;
    }

    window.activityFilters.tokenIds = activeSet;

    // Update chip visuals directly
    const container = document.getElementById('activity-token-chips');
    if (container) {
        container.querySelectorAll('.token-chip').forEach(chip => {
            const chipTid = parseInt(chip.dataset.tokenId);
            const isActive = !activeSet || activeSet.has(chipTid);
            chip.classList.toggle('active', isActive);
        });
    }

    window.activityDisplayCount = ACTIVITY_PAGE_SIZE;
    renderTransactionHistory();
}

// Apply activity filters to confirmed transactions
function applyActivityFilters(confirmedTxs) {
    let filtered = confirmedTxs;
    const { tokenIds, dateDays, customFrom, customTo } = window.activityFilters;

    // Token filter (multi-select)
    if (tokenIds) {
        filtered = filtered.filter(tx => tokenIds.has(tx.token_id));
    }

    // Check if transactions have real timestamps from the backend
    const hasTimestamps = confirmedTxs.some(tx => tx.timestamp > 0);

    // Date filter — uses tx.timestamp when available, falls back to block height
    if (dateDays === 'custom' && (customFrom || customTo)) {
        if (hasTimestamps) {
            if (customFrom) {
                const fromTs = new Date(customFrom).getTime() / 1000;
                filtered = filtered.filter(tx => tx.timestamp >= fromTs);
            }
            if (customTo) {
                const toDate = new Date(customTo);
                toDate.setDate(toDate.getDate() + 1);
                const toTs = toDate.getTime() / 1000;
                filtered = filtered.filter(tx => tx.timestamp < toTs);
            }
        } else {
            // Fallback: estimate from block heights (~144 blocks/day)
            let maxHeight = 0;
            for (const tx of confirmedTxs) {
                if (tx.btc_height > maxHeight) maxHeight = tx.btc_height;
            }
            const now = new Date();
            const blocksPerDay = 144;
            if (customFrom) {
                const daysAgo = Math.max(0, (now - new Date(customFrom)) / 86400000);
                filtered = filtered.filter(tx => tx.btc_height >= maxHeight - Math.ceil(daysAgo * blocksPerDay));
            }
            if (customTo) {
                const toDate = new Date(customTo);
                toDate.setDate(toDate.getDate() + 1);
                const daysAgo = Math.max(0, (now - toDate) / 86400000);
                filtered = filtered.filter(tx => tx.btc_height <= maxHeight - Math.ceil(daysAgo * blocksPerDay));
            }
        }
    } else if (dateDays !== 'all' && dateDays !== 'custom') {
        const days = parseInt(dateDays, 10);
        if (hasTimestamps) {
            const cutoffTs = (Date.now() / 1000) - (days * 86400);
            filtered = filtered.filter(tx => tx.timestamp >= cutoffTs);
        } else {
            // Fallback: estimate from block heights (~144 blocks/day)
            let maxHeight = 0;
            for (const tx of confirmedTxs) {
                if (tx.btc_height > maxHeight) maxHeight = tx.btc_height;
            }
            const cutoff = maxHeight - (days * 144);
            filtered = filtered.filter(tx => tx.btc_height >= cutoff);
        }
    }

    return filtered;
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

    // Nonce
    const nonceEl = document.getElementById('tx-detail-nonce');
    if (nonceEl) nonceEl.textContent = tx.nonce !== undefined ? tx.nonce.toString() : '--';

    // Status
    const statusEl = document.getElementById('tx-detail-status');
    if (statusEl) {
        if (tx.finalized) {
            statusEl.innerHTML = '<span class="tag is-success">Finalized</span>';
        } else if (tx.confirmations > 0) {
            statusEl.innerHTML = `<span class="tag is-warning">${tx.confirmations} confirmation${tx.confirmations !== 1 ? 's' : ''} (${tx.confirmations_remaining} more needed)</span>`;
        } else {
            statusEl.innerHTML = '<span class="tag is-info">Pending</span>';
        }
    }

    // Bitcoin block height (if available)
    const blockRow = document.getElementById('tx-detail-block-row');
    const blockEl = document.getElementById('tx-detail-block');
    if (tx.btc_height && tx.btc_height > 0) {
        if (blockRow) blockRow.style.display = 'flex';
        if (blockEl) blockEl.textContent = `#${tx.btc_height}`;
    } else {
        if (blockRow) blockRow.style.display = 'none';
    }

    // Bitcoin txid (if available)
    const btcTxidRow = document.getElementById('tx-detail-btctxid-row');
    const btcTxidEl = document.getElementById('tx-detail-btctxid');
    if (tx.btc_txid) {
        if (btcTxidRow) btcTxidRow.style.display = 'flex';
        if (btcTxidEl) btcTxidEl.textContent = tx.btc_txid;
    } else {
        if (btcTxidRow) btcTxidRow.style.display = 'none';
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
        const transactions = await walletApp.fetchTransactions();
        const json = JSON.stringify(transactions);
        if (json === lastTransactionsJSON) return;
        lastTransactionsJSON = json;
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

        // Clear refresh caches so next WS update always re-renders
        lastBalanceJSON = null;
        lastTransactionsJSON = null;
        lastPendingJSON = null;

        // Show send animation instead of status message
        showSendAnimation();

        // Add pending transaction to activity for UI display
        addPendingTransaction({
            recipient_pk: recipient,
            amount: amount,
            token_id: tokenId,
            fee: fee,
            direction: 'outgoing',
            nonce: result.usedNonce
        });

        // Store recipient before clearing form
        const sentTo = recipient;

        // Clear form
        recipientEl.value = '';
        amountEl.value = '';

        // Refresh balance (transactions will include the pending one we just added)
        await refreshBalance();

        // After animation (1200ms), prompt to save contact if not already in address book
        setTimeout(() => {
            const contacts = getAddressBook();
            const alreadySaved = contacts.some(c => c.address === sentTo);
            if (!alreadySaved) {
                const statusEl = document.getElementById('send-status');
                if (statusEl) {
                    const shortAddr = sentTo.length > 16
                        ? sentTo.slice(0, 8) + '...' + sentTo.slice(-6)
                        : sentTo;
                    statusEl.innerHTML = `
                        <div class="save-contact-prompt">
                            <span>Save <strong>${shortAddr}</strong> as contact?</span>
                            <div style="display:flex;gap:0.5rem;margin-top:0.5rem">
                                <input class="input is-small" type="text" id="save-contact-name"
                                       placeholder="Contact name" style="flex:1">
                                <button class="button is-primary is-small" id="save-contact-btn">Save</button>
                                <button class="button is-light is-small" id="dismiss-contact-btn">Dismiss</button>
                            </div>
                        </div>`;
                    statusEl.className = 'notification mt-3 is-info';
                    statusEl.style.display = 'block';

                    document.getElementById('save-contact-btn')?.addEventListener('click', () => {
                        const name = document.getElementById('save-contact-name')?.value.trim();
                        if (name) {
                            const contacts = getAddressBook();
                            contacts.push({ label: name, address: sentTo });
                            saveAddressBook(contacts);
                        }
                        statusEl.style.display = 'none';
                    });
                    document.getElementById('dismiss-contact-btn')?.addEventListener('click', () => {
                        statusEl.style.display = 'none';
                    });
                }
            }
        }, 1200);

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

        const account = await walletApp.refreshBalance();
        const json = JSON.stringify(account);
        if (json === lastBalanceJSON) return;
        lastBalanceJSON = json;
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
        startBalancePolling();
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
                startBalancePolling();
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
                startBalancePolling();
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
                startBalancePolling();
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
            stopBalancePolling();
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

    // Position token suffix right after the amount digits, baseline-aligned
    // Uses a temporary inline probe to find the real rendered baseline.
    function repositionTokenSuffix() {
        const input = document.getElementById('send-amount');
        const measure = document.getElementById('send-amount-measure');
        const suffix = document.getElementById('send-token-suffix');
        if (!input || !measure || !suffix) return;

        // Horizontal: place suffix right after the centered text
        const text = input.value || input.placeholder || '';
        measure.textContent = text;
        const inputRect = input.getBoundingClientRect();
        const textWidth = measure.offsetWidth;
        const center = inputRect.left + inputRect.width / 2;
        const rowRect = input.closest('.amount-row').getBoundingClientRect();
        const leftPos = center + textWidth / 2 - rowRect.left;
        suffix.style.left = leftPos + 'px';

        // Vertical: baseline-align using a hidden inline-flex container
        // with both font sizes, letting the browser compute the exact alignment.
        const inputStyle = getComputedStyle(input);
        const helper = document.createElement('div');
        helper.style.cssText = 'position:absolute;top:-9999px;left:0;display:inline-flex;align-items:baseline;';
        const bigSpan = document.createElement('span');
        bigSpan.style.cssText = `font-size:${inputStyle.fontSize};font-weight:${inputStyle.fontWeight};font-family:${inputStyle.fontFamily};line-height:${inputStyle.lineHeight};padding:${inputStyle.padding};`;
        bigSpan.textContent = text || '0';
        const smallSpan = document.createElement('span');
        const suffixStyle = getComputedStyle(suffix);
        smallSpan.style.cssText = `font-size:${suffixStyle.fontSize};font-weight:${suffixStyle.fontWeight};font-family:${suffixStyle.fontFamily};line-height:1;`;
        smallSpan.textContent = suffix.textContent;
        helper.appendChild(bigSpan);
        helper.appendChild(smallSpan);
        document.body.appendChild(helper);
        const delta = smallSpan.getBoundingClientRect().top - bigSpan.getBoundingClientRect().top;
        document.body.removeChild(helper);

        suffix.style.top = delta + 'px';
    }

    const sendAmountInput = document.getElementById('send-amount');
    if (sendAmountInput) {
        sendAmountInput.addEventListener('input', repositionTokenSuffix);
        // Initial position after layout
        requestAnimationFrame(repositionTokenSuffix);
    }
    // Expose for use by updateSendTokenInfo
    window.repositionTokenSuffix = repositionTokenSuffix;

    // Send amount input — scroll to adjust & manual input cap
    const amountInput = document.getElementById('send-amount');

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
            const quickSelectEl = document.getElementById('send-quick-select');
            if (quickSelectEl) quickSelectEl.querySelectorAll('.quick-amount').forEach(b => b.classList.remove('active'));

            if (window.repositionTokenSuffix) window.repositionTokenSuffix();

            // Remove scrolling class after a delay
            clearTimeout(scrollTimeout);
            scrollTimeout = setTimeout(() => {
                amountInput.classList.remove('scrolling');
            }, 150);
        }, { passive: false });

        // Clear active state and cap at max when manually editing
        amountInput.addEventListener('input', () => {
            const quickSelectEl = document.getElementById('send-quick-select');
            if (quickSelectEl) quickSelectEl.querySelectorAll('.quick-amount').forEach(b => b.classList.remove('active'));
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

    // Resync nonce button
    const resyncNonceBtn = document.getElementById('resync-nonce-btn');
    if (resyncNonceBtn) {
        resyncNonceBtn.addEventListener('click', async () => {
            const originalText = resyncNonceBtn.textContent;
            resyncNonceBtn.textContent = 'Syncing...';
            resyncNonceBtn.style.pointerEvents = 'none';
            try {
                const nonce = await walletApp.resyncNonce();
                resyncNonceBtn.textContent = `Synced (${nonce})`;
                setTimeout(() => {
                    resyncNonceBtn.textContent = originalText;
                    resyncNonceBtn.style.pointerEvents = '';
                }, 2000);
            } catch (error) {
                console.error('Failed to resync nonce:', error);
                resyncNonceBtn.textContent = 'Failed';
                setTimeout(() => {
                    resyncNonceBtn.textContent = originalText;
                    resyncNonceBtn.style.pointerEvents = '';
                }, 2000);
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

    // Date drum picker initialization
    initDateDrum();

    // Initialize invoice/request payment
    initInvoice();

    // Initialize send/receive tabs and receive tab
    initSendReceiveTabs();
    initReceiveTab();

    // Initialize recipient suggestions and address book
    initRecipientSuggestions();
    initAddressBook();

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

    // Update send token info display
    updateSendTokenInfo();
}

// ========================================
// Date Drum Picker
// ========================================
function initDateDrum() {
    const drum = document.getElementById('date-drum');
    const track = document.getElementById('date-drum-track');
    if (!drum || !track) return;

    const items = track.querySelectorAll('.date-drum-item');
    if (items.length === 0) return;

    const state = window.dateDrumState;
    state.itemHeight = items[0].offsetHeight || 22;

    let isDragging = false;
    let startY = 0;
    let dragOffset = 0;
    let prevSelectedIndex = state.selectedIndex;

    // Set initial active state
    updateDrumActive(state.selectedIndex);

    function updateDrumActive(index) {
        items.forEach((item, i) => {
            item.classList.toggle('active', i === index);
        });
    }

    function snapToIndex(index) {
        index = Math.max(0, Math.min(items.length - 1, index));
        state.selectedIndex = index;
        state.currentOffset = -index * state.itemHeight;
        track.style.transform = 'translateY(' + state.currentOffset + 'px)';
        updateDrumActive(index);

        // Update activity filter
        const dayValue = items[index].dataset.days;
        if (dayValue === 'custom') {
            window.activityFilters.dateDays = 'custom';
            openDateRangePicker();
        } else {
            window.activityFilters.dateDays = dayValue;
            window.activityFilters.customFrom = null;
            window.activityFilters.customTo = null;
            updateCustomDrumLabel(null, null);
            closeDateRangePicker();
            window.activityDisplayCount = ACTIVITY_PAGE_SIZE;
            renderTransactionHistory();
        }
    }

    function snapToNearest() {
        const index = Math.round(-state.currentOffset / state.itemHeight);
        snapToIndex(index);
    }

    function handleDragStart(clientY) {
        isDragging = true;
        startY = clientY;
        dragOffset = state.currentOffset;
        prevSelectedIndex = state.selectedIndex;
        drum.classList.add('grabbing');
        track.classList.add('dragging');
    }

    function handleDragMove(clientY) {
        if (!isDragging) return;
        var diff = clientY - startY;
        var maxOffset = 0;
        var minOffset = -(items.length - 1) * state.itemHeight;
        state.currentOffset = Math.max(minOffset - 20, Math.min(maxOffset + 20, dragOffset + diff));
        track.style.transform = 'translateY(' + state.currentOffset + 'px)';

        // Preview which item would be selected
        var previewIndex = Math.round(-state.currentOffset / state.itemHeight);
        previewIndex = Math.max(0, Math.min(items.length - 1, previewIndex));
        updateDrumActive(previewIndex);
    }

    function handleDragEnd() {
        if (!isDragging) return;
        isDragging = false;
        drum.classList.remove('grabbing');
        track.classList.remove('dragging');
        snapToNearest();
    }

    // Mouse events
    drum.addEventListener('mousedown', function(e) {
        e.preventDefault();
        handleDragStart(e.clientY);
    });
    document.addEventListener('mousemove', function(e) {
        handleDragMove(e.clientY);
    });
    document.addEventListener('mouseup', function() {
        handleDragEnd();
    });

    // Touch events
    drum.addEventListener('touchstart', function(e) {
        handleDragStart(e.touches[0].clientY);
    }, { passive: true });
    drum.addEventListener('touchmove', function(e) {
        handleDragMove(e.touches[0].clientY);
    }, { passive: true });
    drum.addEventListener('touchend', function() {
        handleDragEnd();
    });

    // Click on drum opens date range picker (if not dragging)
    drum.addEventListener('click', function(e) {
        if (Math.abs(state.currentOffset - dragOffset) < 3) {
            // Tap without drag - open date range picker
            openDateRangePicker();
        }
    });

    // Date range picker buttons
    var applyBtn = document.getElementById('date-range-apply');
    var cancelBtn = document.getElementById('date-range-cancel');
    if (applyBtn) {
        applyBtn.addEventListener('click', function() {
            applyDateRange();
        });
    }
    if (cancelBtn) {
        cancelBtn.addEventListener('click', function() {
            cancelDateRange();
        });
    }
}

function openDateRangePicker() {
    var picker = document.getElementById('date-range-picker');
    if (!picker) return;

    // Set default dates if not already set
    var fromInput = document.getElementById('date-range-from');
    var toInput = document.getElementById('date-range-to');
    if (toInput && !toInput.value) {
        toInput.value = new Date().toISOString().split('T')[0];
    }
    if (fromInput && !fromInput.value) {
        var d = new Date();
        d.setDate(d.getDate() - 30);
        fromInput.value = d.toISOString().split('T')[0];
    }

    picker.style.display = '';
}

function closeDateRangePicker() {
    var picker = document.getElementById('date-range-picker');
    if (picker) picker.style.display = 'none';
}

function formatShortDate(dateStr) {
    if (!dateStr) return '';
    var parts = dateStr.split('-');
    // mm/dd from yyyy-mm-dd
    return parseInt(parts[1], 10) + '/' + parseInt(parts[2], 10);
}

function updateCustomDrumLabel(fromVal, toVal) {
    var track = document.getElementById('date-drum-track');
    if (!track) return;
    var items = track.querySelectorAll('.date-drum-item');
    for (var i = 0; i < items.length; i++) {
        if (items[i].dataset.days === 'custom') {
            if (fromVal || toVal) {
                var label = formatShortDate(fromVal) + ' – ' + formatShortDate(toVal);
                items[i].textContent = label;
            } else {
                items[i].textContent = 'Custom...';
            }
            break;
        }
    }
}

function applyDateRange() {
    var fromInput = document.getElementById('date-range-from');
    var toInput = document.getElementById('date-range-to');
    var fromVal = fromInput ? fromInput.value : null;
    var toVal = toInput ? toInput.value : null;

    // Snap drum to "Custom..." item and update its label
    var track = document.getElementById('date-drum-track');
    if (track) {
        var items = track.querySelectorAll('.date-drum-item');
        for (var i = 0; i < items.length; i++) {
            if (items[i].dataset.days === 'custom') {
                window.dateDrumState.selectedIndex = i;
                window.dateDrumState.currentOffset = -i * window.dateDrumState.itemHeight;
                track.style.transform = 'translateY(' + window.dateDrumState.currentOffset + 'px)';
                items.forEach(function(el, idx) {
                    el.classList.toggle('active', idx === i);
                });
                break;
            }
        }
    }

    updateCustomDrumLabel(fromVal, toVal);

    window.activityFilters.dateDays = 'custom';
    window.activityFilters.customFrom = fromVal || null;
    window.activityFilters.customTo = toVal || null;
    window.activityDisplayCount = ACTIVITY_PAGE_SIZE;
    closeDateRangePicker();
    renderTransactionHistory();
}

function cancelDateRange() {
    closeDateRangePicker();
    // If we were on custom and cancelled, revert to previous non-custom selection
    if (window.activityFilters.dateDays === 'custom' && !window.activityFilters.customFrom && !window.activityFilters.customTo) {
        // Snap back to "All time"
        var track = document.getElementById('date-drum-track');
        if (track) {
            window.dateDrumState.selectedIndex = 0;
            window.dateDrumState.currentOffset = 0;
            track.style.transform = 'translateY(0px)';
            var items = track.querySelectorAll('.date-drum-item');
            items.forEach(function(el, idx) {
                el.classList.toggle('active', idx === 0);
            });
        }
        updateCustomDrumLabel(null, null);
        window.activityFilters.dateDays = 'all';
        window.activityDisplayCount = ACTIVITY_PAGE_SIZE;
        renderTransactionHistory();
    }
}

// Greek letters/names for token display
const greekLetters = ['α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ'];
const greekNames = ['Alpha', 'Beta', 'Gamma', 'Delta', 'Epsilon', 'Zeta', 'Eta', 'Theta', 'Iota', 'Kappa'];

// Update the token info display in the send box and rebuild quick-select amounts
function updateSendTokenInfo() {
    const tokenSuffixEl = document.getElementById('send-token-suffix');
    const quickSelectEl = document.getElementById('send-quick-select');

    const { selectedIndex, tokenIds, balances } = window.carouselState;
    if (tokenIds.length === 0) {
        if (tokenSuffixEl) tokenSuffixEl.textContent = '--';
        if (quickSelectEl) quickSelectEl.innerHTML = '';
        return;
    }

    const tokenId = tokenIds[selectedIndex] || '0';
    const idx = parseInt(tokenId);
    const letter = greekLetters[idx] || `#${tokenId}`;
    const name = greekNames[idx] || `Token ${tokenId}`;
    const balance = balances[tokenId] || 0;
    const fee = parseInt(document.getElementById('send-fee')?.value || '1', 10);
    const maxSendable = Math.max(0, balance - fee);

    if (tokenSuffixEl) tokenSuffixEl.textContent = `${letter} ${name}`;

    // Rebuild quick-select: only show preset amounts < balance, plus maxSendable
    if (quickSelectEl) {
        const presets = [10, 100, 1000, 10000];
        const validPresets = presets.filter(v => v < balance);

        let html = '';
        for (const v of validPresets) {
            const label = v >= 1000 ? (v / 1000) + 'k' : v;
            html += `<span class="quick-amount" data-amount="${v}">${label}</span>`;
        }
        // Always show max (balance - fee) if positive
        if (maxSendable > 0) {
            const maxLabel = maxSendable.toLocaleString();
            html += `<span class="quick-amount" data-amount="${maxSendable}">${maxLabel}</span>`;
        }
        quickSelectEl.innerHTML = html;

        // Wire up click handlers on newly created buttons
        const amountInput = document.getElementById('send-amount');
        quickSelectEl.querySelectorAll('.quick-amount').forEach(btn => {
            btn.addEventListener('click', () => {
                const amount = parseInt(btn.getAttribute('data-amount'), 10);
                if (amountInput) {
                    amountInput.value = amount;
                    amountInput.classList.add('scrolling');
                    setTimeout(() => amountInput.classList.remove('scrolling'), 150);
                }
                quickSelectEl.querySelectorAll('.quick-amount').forEach(b => b.classList.remove('active'));
                btn.classList.add('active');
                if (window.repositionTokenSuffix) window.repositionTokenSuffix();
            });
        });
    }

    // Reposition suffix after token name change
    if (window.repositionTokenSuffix) requestAnimationFrame(window.repositionTokenSuffix);
}

// ========================================
// Invoice / Payment Request
// ========================================

function parseCoinsUri(uri) {
    if (!uri.startsWith('coins://pay')) return null;
    const rest = uri.slice('coins://pay'.length);
    if (!rest.startsWith('?')) return null;
    const query = rest.slice(1);
    const params = {};
    for (const pair of query.split('&')) {
        if (!pair) continue;
        const eq = pair.indexOf('=');
        if (eq < 0) continue;
        const key = pair.slice(0, eq);
        const value = pair.slice(eq + 1);
        params[key] = decodeURIComponent(value);
    }
    if (!params.addr) return null;
    return {
        addr: params.addr,
        amount: params.amount ? parseInt(params.amount, 10) : null,
        token_id: params.token ? parseInt(params.token, 10) : null,
        memo: params.memo || null,
        expires: params.expires ? parseInt(params.expires, 10) : null,
    };
}

function generateCoinsUri(addr, amount, token_id, memo, expires) {
    let uri = `coins://pay?addr=${addr}`;
    if (amount) uri += `&amount=${amount}`;
    if (token_id !== undefined && token_id !== null) uri += `&token=${token_id}`;
    if (memo) uri += `&memo=${encodeURIComponent(memo)}`;
    if (expires) uri += `&expires=${expires}`;
    return uri;
}

function initInvoice() {
    // Request payment button
    const reqBtn = document.getElementById('request-payment-btn');
    if (reqBtn) {
        reqBtn.addEventListener('click', () => {
            document.getElementById('invoice-create-form').style.display = 'block';
            document.getElementById('invoice-result').style.display = 'none';
            document.getElementById('invoice-modal').classList.add('is-active');
        });
    }

    // Close modal
    ['close-invoice-modal', 'close-invoice-btn'].forEach(id => {
        const el = document.getElementById(id);
        if (el) el.addEventListener('click', () => {
            document.getElementById('invoice-modal').classList.remove('is-active');
        });
    });
    const invoiceBg = document.querySelector('#invoice-modal .modal-background');
    if (invoiceBg) invoiceBg.addEventListener('click', () => {
        document.getElementById('invoice-modal').classList.remove('is-active');
    });

    // Generate invoice
    const genBtn = document.getElementById('generate-invoice-btn');
    if (genBtn) {
        genBtn.addEventListener('click', () => {
            const pk = walletApp.getPublicKey();
            if (!pk) return;

            const amountVal = document.getElementById('invoice-amount').value;
            const tokenVal = document.getElementById('invoice-token').value;
            const memoVal = document.getElementById('invoice-memo').value.trim();

            const amount = amountVal ? parseInt(amountVal, 10) : null;
            const token = tokenVal ? parseInt(tokenVal, 10) : null;

            const uri = generateCoinsUri(pk, amount, token, memoVal || null);

            // Show result
            document.getElementById('invoice-create-form').style.display = 'none';
            document.getElementById('invoice-result').style.display = 'block';
            document.getElementById('invoice-uri-text').value = uri;

            // Generate QR code
            const canvas = document.getElementById('invoice-qr');
            if (canvas && typeof QRCode !== 'undefined') {
                QRCode.toCanvas(canvas, uri, {
                    width: 200,
                    margin: 2,
                    color: { dark: '#0f172a', light: '#f1f5f9' }
                });
            }
        });
    }

    // Copy URI
    const copyBtn = document.getElementById('copy-invoice-uri-btn');
    if (copyBtn) {
        copyBtn.addEventListener('click', () => {
            const text = document.getElementById('invoice-uri-text').value;
            navigator.clipboard.writeText(text).then(() => {
                copyBtn.textContent = 'Copied!';
                setTimeout(() => { copyBtn.textContent = 'Copy URI'; }, 2000);
            });
        });
    }

    // Create another
    const newBtn = document.getElementById('invoice-new-btn');
    if (newBtn) {
        newBtn.addEventListener('click', () => {
            document.getElementById('invoice-create-form').style.display = 'block';
            document.getElementById('invoice-result').style.display = 'none';
        });
    }

    // Detect coins:// URI paste in recipient field
    const recipientInput = document.getElementById('send-recipient');
    if (recipientInput) {
        recipientInput.addEventListener('input', () => {
            const val = recipientInput.value.trim();
            if (val.startsWith('coins://pay')) {
                const inv = parseCoinsUri(val);
                if (inv) {
                    recipientInput.value = inv.addr;
                    if (inv.amount) {
                        const amountInput = document.getElementById('send-amount');
                        if (amountInput) amountInput.value = inv.amount;
                    }
                    if (inv.expires) {
                        const now = Math.floor(Date.now() / 1000);
                        if (now > inv.expires) {
                            showSendStatus('Warning: This invoice has expired', true);
                        }
                    }
                }
            }
        });
    }
}

// ========================================
// Send / Receive Flip Toggle
// ========================================

window.receiveState = {
    qrDebounceTimer: null,
    tokenSelectorReady: false,
    selectedTokenId: 0,
    isReceiveMode: false
};

function initSendReceiveTabs() {
    const flipCard = document.getElementById('sr-flip-card');
    const toggleFront = document.getElementById('sr-toggle-front');
    const toggleBack = document.getElementById('sr-toggle-back');
    const faceFront = flipCard ? flipCard.querySelector('.sr-face-front') : null;
    const faceBack = flipCard ? flipCard.querySelector('.sr-face-back') : null;
    if (!flipCard) return;

    // Set flip card height to match the currently visible face
    function setHeightForFace(face) {
        if (!face) return;
        // Temporarily ensure face is measurable
        const origPos = face.style.position;
        const origVis = face.style.visibility;
        const origTransform = face.style.transform;
        face.style.position = 'relative';
        face.style.visibility = 'visible';
        face.style.transform = 'none';

        // Force reflow
        void face.offsetHeight;

        const height = face.offsetHeight;

        // Restore
        face.style.position = origPos;
        face.style.visibility = origVis;
        face.style.transform = origTransform;

        flipCard.style.minHeight = height + 'px';
    }

    // Initial height for front face
    setTimeout(() => setHeightForFace(faceFront), 100);

    function toggleFlip() {
        window.receiveState.isReceiveMode = !window.receiveState.isReceiveMode;

        if (window.receiveState.isReceiveMode) {
            // Switch to Receive
            // Initialize token selector FIRST if needed (affects height)
            var needsInit = !window.receiveState.tokenSelectorReady;
            if (needsInit) {
                initReceiveTokenSelector();
                window.receiveState.tokenSelectorReady = true;
            }

            // Render QR first (sets canvas dimensions, affects height)
            updateReceiveQR();

            // Wait for DOM to update, then measure and flip
            requestAnimationFrame(() => {
                setHeightForFace(faceBack);
                flipCard.classList.add('flipped');
            });
        } else {
            // Switch to Send - flip and shrink together
            flipCard.classList.remove('flipped');
            setHeightForFace(faceFront);
        }
    }

    if (toggleFront) toggleFront.addEventListener('click', toggleFlip);
    if (toggleBack) toggleBack.addEventListener('click', toggleFlip);
}

function initReceiveTab() {
    // QR click-to-copy
    const qrContainer = document.getElementById('receive-qr-container');
    if (qrContainer) {
        qrContainer.addEventListener('click', () => {
            const uri = buildReceiveUri();
            if (!uri) return;
            navigator.clipboard.writeText(uri).then(() => {
                qrContainer.classList.add('copied');
                setTimeout(() => qrContainer.classList.remove('copied'), 1500);
            });
        });
    }

    // Amount input
    const amountInput = document.getElementById('receive-amount');
    const receiveQuickBtns = document.querySelectorAll('.receive-quick-amount');

    function updateReceiveQuickActive(val) {
        receiveQuickBtns.forEach(b => {
            b.classList.toggle('active', b.dataset.amount === val);
        });
    }

    if (amountInput) {
        // Clear "Any" on focus so user can type
        amountInput.addEventListener('focus', () => {
            if (amountInput.value === 'Any') {
                amountInput.value = '';
                amountInput.inputMode = 'numeric';
            }
        });
        // If user empties field and blurs, restore "Any"
        amountInput.addEventListener('blur', () => {
            if (!amountInput.value.trim()) {
                amountInput.value = 'Any';
                amountInput.inputMode = '';
                updateReceiveQuickActive('any');
                onReceiveFieldChange();
            }
        });
        amountInput.addEventListener('input', () => {
            amountInput.value = amountInput.value.replace(/[^0-9]/g, '');
            updateReceiveQuickActive(amountInput.value);
            onReceiveFieldChange();
        });
    }

    // Quick amount buttons
    receiveQuickBtns.forEach(btn => {
        btn.addEventListener('click', () => {
            if (!amountInput) return;
            if (btn.dataset.amount === 'any') {
                amountInput.value = 'Any';
                amountInput.inputMode = '';
            } else {
                amountInput.value = btn.dataset.amount;
                amountInput.inputMode = 'numeric';
            }
            updateReceiveQuickActive(btn.dataset.amount);
            onReceiveFieldChange();
        });
    });

    // Expiry chips
    initReceiveExpiryChips();

    // Memo badge → expand, with close button
    const memoBadge = document.getElementById('receive-memo-badge');
    const memoInputWrap = document.getElementById('receive-memo-input');
    const memoInput = document.getElementById('receive-memo');
    const memoCount = document.getElementById('receive-memo-count');
    const memoClose = document.getElementById('receive-memo-close');

    if (memoBadge && memoInputWrap && memoInput) {
        memoBadge.addEventListener('click', () => {
            memoBadge.style.display = 'none';
            memoInputWrap.classList.add('visible');
            memoInput.focus();
        });

        memoInput.addEventListener('input', () => {
            if (memoCount) {
                var len = memoInput.value.length;
                memoCount.textContent = len + ' / 255';
                memoCount.classList.toggle('warn', len > 230);
            }
            onReceiveFieldChange();
        });

        if (memoClose) {
            memoClose.addEventListener('click', () => {
                memoInput.value = '';
                if (memoCount) {
                    memoCount.textContent = '0 / 255';
                    memoCount.classList.remove('warn');
                }
                memoInputWrap.classList.remove('visible');
                memoBadge.style.display = '';
                onReceiveFieldChange();
            });
        }
    }

    // Custom datetime change
    const customDatetime = document.getElementById('receive-custom-datetime');
    if (customDatetime) {
        customDatetime.addEventListener('change', () => {
            onReceiveFieldChange();
        });
    }
}

function initReceiveExpiryChips() {
    const chips = document.querySelectorAll('.receive-expiry-chip');
    const customExpiry = document.getElementById('receive-custom-expiry');
    if (!chips.length) return;

    chips.forEach(chip => {
        chip.addEventListener('click', () => {
            chips.forEach(c => c.classList.remove('active'));
            chip.classList.add('active');

            if (chip.dataset.seconds === 'custom') {
                if (customExpiry) customExpiry.classList.add('visible');
            } else {
                if (customExpiry) customExpiry.classList.remove('visible');
            }
            onReceiveFieldChange();
        });
    });
}

function tokenDisplayInfo(id) {
    var idx = parseInt(id, 10);
    var letter = greekLetters[idx];
    var name = greekNames[idx];
    if (letter && name) return { icon: letter, name: name };
    return { icon: '#', name: String(idx) };
}

function initReceiveTokenSelector() {
    var selectEl = document.getElementById('receive-token-select');
    var btn = document.getElementById('receive-token-btn');
    var dropdown = document.getElementById('receive-token-dropdown');
    var searchInput = document.getElementById('receive-token-search');
    var listEl = document.getElementById('receive-token-list');
    var iconEl = document.getElementById('receive-token-icon');
    var labelEl = document.getElementById('receive-token-label');
    if (!selectEl || !btn || !dropdown || !listEl) return;

    // Build token list: owned tokens first, then the rest of 0-255
    var ownedSet = new Set();
    var ownedTokens = (window.carouselState.tokenIds || []).map(function(tid) {
        var id = parseInt(tid, 10);
        ownedSet.add(id);
        return id;
    });
    var otherTokens = [];
    for (var i = 0; i < 256; i++) {
        if (!ownedSet.has(i)) otherTokens.push(i);
    }

    var allTokens = ownedTokens.concat(otherTokens);
    var dividerAfter = ownedTokens.length > 0 ? ownedTokens.length - 1 : -1;

    function renderList(filter) {
        var html = '';
        var f = (filter || '').toLowerCase();
        var insertedDivider = false;
        for (var j = 0; j < allTokens.length; j++) {
            var tid = allTokens[j];
            var info = tokenDisplayInfo(tid);
            var searchText = (info.icon + ' ' + info.name + ' ' + tid).toLowerCase();
            if (f && searchText.indexOf(f) === -1) continue;

            if (!insertedDivider && dividerAfter >= 0 && j > dividerAfter) {
                html += '<div class="receive-token-divider"></div>';
                insertedDivider = true;
            }

            var selectedClass = tid === window.receiveState.selectedTokenId ? ' selected' : '';
            var ownedLabel = ownedSet.has(tid) ? '<span class="tok-owned">owned</span>' : '';
            html += '<div class="receive-token-option' + selectedClass + '" data-token-id="' + tid + '">'
                + '<span class="tok-icon">' + info.icon + '</span>'
                + '<span class="tok-name">' + info.name + '</span>'
                + ownedLabel
                + '</div>';
        }
        if (!html) {
            html = '<div style="padding:0.5rem 0.75rem;color:var(--coins-text-muted);font-size:0.75rem;">No tokens found</div>';
        }
        listEl.innerHTML = html;
    }

    function selectToken(tid) {
        window.receiveState.selectedTokenId = tid;
        var info = tokenDisplayInfo(tid);
        if (iconEl) iconEl.textContent = info.icon;
        if (labelEl) labelEl.textContent = info.name;
        closeDropdown();
        onReceiveFieldChange();
    }

    function openDropdown() {
        selectEl.classList.add('open');
        renderList('');
        if (searchInput) { searchInput.value = ''; searchInput.focus(); }
    }

    function closeDropdown() {
        selectEl.classList.remove('open');
    }

    // Toggle on button click
    btn.addEventListener('click', function() {
        if (selectEl.classList.contains('open')) {
            closeDropdown();
        } else {
            openDropdown();
        }
    });

    // Search filtering
    if (searchInput) {
        searchInput.addEventListener('input', function() {
            renderList(searchInput.value);
        });
    }

    // Delegate clicks on token options
    listEl.addEventListener('click', function(e) {
        var option = e.target.closest('.receive-token-option');
        if (option && option.dataset.tokenId !== undefined) {
            selectToken(parseInt(option.dataset.tokenId, 10));
        }
    });

    // Close on click outside
    document.addEventListener('click', function(e) {
        if (!selectEl.contains(e.target)) {
            closeDropdown();
        }
    });

    // Set initial selection — first owned token or token 0
    var initialToken = ownedTokens.length > 0 ? ownedTokens[0] : 0;
    selectToken(initialToken);
}

function onReceiveFieldChange() {
    clearTimeout(window.receiveState.qrDebounceTimer);
    window.receiveState.qrDebounceTimer = setTimeout(() => {
        updateReceiveQR();
    }, 150);
}

function buildReceiveUri() {
    const pk = walletApp.getPublicKey();
    if (!pk) return null;

    // Amount
    const amountEl = document.getElementById('receive-amount');
    const amount = amountEl ? (parseInt(amountEl.value, 10) || null) : null;

    // Token from selector
    var tokenId = window.receiveState.selectedTokenId;

    // Expiry from chips
    let expires = null;
    const activeChip = document.querySelector('.receive-expiry-chip.active');
    if (activeChip) {
        const seconds = activeChip.dataset.seconds;
        if (seconds === 'custom') {
            const customInput = document.getElementById('receive-custom-datetime');
            if (customInput && customInput.value) {
                expires = Math.floor(new Date(customInput.value).getTime() / 1000);
            }
        } else if (seconds && parseInt(seconds, 10) > 0) {
            expires = Math.floor(Date.now() / 1000) + parseInt(seconds, 10);
        }
    }

    // Memo
    const memoEl = document.getElementById('receive-memo');
    const memo = memoEl ? memoEl.value.trim() || null : null;

    return generateCoinsUri(pk, amount, tokenId, memo, expires);
}

function updateReceiveQR() {
    const canvas = document.getElementById('receive-qr-canvas');
    if (!canvas || typeof QRCode === 'undefined') return;

    const uri = buildReceiveUri();
    if (!uri) return;

    QRCode.toCanvas(canvas, uri, {
        width: 200,
        margin: 2,
        color: { dark: '#0f172a', light: '#f1f5f9' }
    });
}

// ========================================
// Address Book
// ========================================
const ADDRESSBOOK_KEY = 'coins_addressbook';

function getAddressBook() {
    try {
        return JSON.parse(localStorage.getItem(ADDRESSBOOK_KEY)) || [];
    } catch {
        return [];
    }
}

function saveAddressBook(contacts) {
    localStorage.setItem(ADDRESSBOOK_KEY, JSON.stringify(contacts));
}

function renderAddressBook(filter = '') {
    const listEl = document.getElementById('addressbook-list');
    if (!listEl) return;

    let contacts = getAddressBook();
    if (filter) {
        const f = filter.toLowerCase();
        contacts = contacts.filter(c =>
            c.label.toLowerCase().includes(f) ||
            c.address.toLowerCase().includes(f)
        );
    }

    if (contacts.length === 0) {
        listEl.innerHTML = `<div class="addressbook-empty">${filter ? 'No matching contacts' : 'No contacts yet'}</div>`;
        return;
    }

    listEl.innerHTML = contacts.map((c, i) => `
        <div class="addressbook-contact" data-index="${i}" data-address="${c.address}">
            <div class="addressbook-contact-info">
                <div class="addressbook-contact-label">${escapeHtml(c.label)}</div>
                <div class="addressbook-contact-addr">${c.address}</div>
            </div>
            <div class="addressbook-contact-actions">
                <button class="button is-danger is-light is-small addressbook-delete-btn" data-index="${i}" title="Delete">Del</button>
            </div>
        </div>
    `).join('');

    // Wire up select handlers (click on contact info)
    listEl.querySelectorAll('.addressbook-contact-info').forEach(el => {
        el.addEventListener('click', () => {
            const contact = el.closest('.addressbook-contact');
            const addr = contact.getAttribute('data-address');
            const recipientInput = document.getElementById('send-recipient');
            if (recipientInput) recipientInput.value = addr;
            document.getElementById('addressbook-modal').classList.remove('is-active');
        });
    });

    // Wire up delete handlers
    listEl.querySelectorAll('.addressbook-delete-btn').forEach(btn => {
        btn.addEventListener('click', (e) => {
            e.stopPropagation();
            const idx = parseInt(btn.getAttribute('data-index'), 10);
            const contacts = getAddressBook();
            contacts.splice(idx, 1);
            saveAddressBook(contacts);
            renderAddressBook(document.getElementById('addressbook-search')?.value || '');
        });
    });
}

function escapeHtml(str) {
    const div = document.createElement('div');
    div.textContent = str;
    return div.innerHTML;
}

function initRecipientSuggestions() {
    const input = document.getElementById('send-recipient');
    const dropdown = document.getElementById('recipient-suggestions');
    if (!input || !dropdown) return;

    function buildCandidates(query) {
        const q = query.toLowerCase();
        const results = [];
        const seen = new Set();

        // 1. Saved contacts
        const contacts = getAddressBook();
        for (const c of contacts) {
            if (c.label.toLowerCase().includes(q) || c.address.toLowerCase().includes(q)) {
                if (!seen.has(c.address)) {
                    seen.add(c.address);
                    results.push({ label: c.label, address: c.address });
                }
            }
        }

        // 2. Previous recipients from transaction history
        const txs = window.currentTransactions || [];
        for (const tx of txs) {
            if (tx.direction === 'outgoing' && tx.recipient_pk) {
                const addr = tx.recipient_pk;
                if (seen.has(addr)) continue;
                if (addr.toLowerCase().includes(q)) {
                    seen.add(addr);
                    results.push({ label: 'Previous recipient', address: addr });
                }
            }
        }

        return results.slice(0, 5);
    }

    function renderSuggestions(candidates) {
        if (candidates.length === 0) {
            dropdown.classList.remove('open');
            dropdown.innerHTML = '';
            return;
        }
        dropdown.innerHTML = candidates.map(c => `
            <div class="recipient-suggestion" data-address="${escapeHtml(c.address)}">
                <span class="suggestion-label">${escapeHtml(c.label)}</span>
                <span class="suggestion-address">${c.address.slice(0, 12)}...${c.address.slice(-6)}</span>
            </div>
        `).join('');
        dropdown.classList.add('open');

        dropdown.querySelectorAll('.recipient-suggestion').forEach(el => {
            el.addEventListener('mousedown', (e) => {
                e.preventDefault();
                input.value = el.getAttribute('data-address');
                dropdown.classList.remove('open');
                dropdown.innerHTML = '';
            });
        });
    }

    input.addEventListener('input', () => {
        const val = input.value.trim();
        if (val.length < 1) {
            dropdown.classList.remove('open');
            dropdown.innerHTML = '';
            return;
        }
        renderSuggestions(buildCandidates(val));
    });

    input.addEventListener('blur', () => {
        // Slight delay so mousedown on suggestion registers
        setTimeout(() => {
            dropdown.classList.remove('open');
            dropdown.innerHTML = '';
        }, 150);
    });

    input.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') {
            dropdown.classList.remove('open');
            dropdown.innerHTML = '';
        }
    });
}

function initAddressBook() {
    // Open button
    const openBtn = document.getElementById('open-addressbook-btn');
    if (openBtn) {
        openBtn.addEventListener('click', () => {
            document.getElementById('addressbook-modal').classList.add('is-active');
            renderAddressBook();
        });
    }

    // Close buttons
    const closeModal = document.getElementById('close-addressbook-modal');
    const closeBtn = document.getElementById('close-addressbook-btn');
    const modalBg = document.querySelector('#addressbook-modal .modal-background');
    [closeModal, closeBtn, modalBg].forEach(el => {
        if (el) el.addEventListener('click', () => {
            document.getElementById('addressbook-modal').classList.remove('is-active');
        });
    });

    // Search
    const searchInput = document.getElementById('addressbook-search');
    if (searchInput) {
        searchInput.addEventListener('input', () => {
            renderAddressBook(searchInput.value);
        });
    }

    // Add contact
    const addBtn = document.getElementById('addressbook-add-btn');
    if (addBtn) {
        addBtn.addEventListener('click', () => {
            const labelInput = document.getElementById('addressbook-new-label');
            const addrInput = document.getElementById('addressbook-new-address');
            const errorEl = document.getElementById('addressbook-error');

            const label = labelInput.value.trim();
            const address = addrInput.value.trim();

            if (!label || !address) {
                if (errorEl) {
                    errorEl.textContent = 'Both label and address are required';
                    errorEl.style.display = 'block';
                }
                return;
            }

            if (!/^[0-9a-fA-F]+$/.test(address)) {
                if (errorEl) {
                    errorEl.textContent = 'Address must be a hex string';
                    errorEl.style.display = 'block';
                }
                return;
            }

            const contacts = getAddressBook();
            contacts.push({ label, address });
            saveAddressBook(contacts);

            labelInput.value = '';
            addrInput.value = '';
            if (errorEl) errorEl.style.display = 'none';
            renderAddressBook();
        });
    }
}

// Export for use by other scripts (future stories)
window.walletApp = walletApp;
window.showScreen = showScreen;
window.refreshBalance = refreshBalance;
window.refreshTransactions = refreshTransactions;
window.sendTransaction = sendTransaction;
