// Coins Wallet - Browser-based wallet with WASM BLS signing
// Uses Web Crypto API for AES-GCM encryption with PBKDF2 key derivation

const STORAGE_KEY = 'coins_wallet_key';
const SESSION_KEY = 'coins_wallet_session_key';
const PBKDF2_ITERATIONS = 100000;
const SALT_SIZE = 16;
const IV_SIZE = 12;

// WASM module reference
let wasmModule = null;

// Current wallet key (in memory while unlocked)
let currentKey = null;

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
    const balanceEl = document.getElementById('balance-display');
    if (!balanceEl) return;

    if (account === null) {
        // Account not found - new wallet
        balanceEl.innerHTML = `
            <div class="notification is-info is-light">
                <p><strong>Account not found</strong></p>
                <p class="is-size-7">This is a new wallet. Your account will be created when you receive your first transaction.</p>
            </div>
        `;
        return;
    }

    // Display all token balances
    const balances = account.balances || {};
    const tokenIds = Object.keys(balances).sort((a, b) => parseInt(a) - parseInt(b));

    if (tokenIds.length === 0) {
        balanceEl.innerHTML = '<p class="has-text-grey">No tokens</p>';
        return;
    }

    let html = '<table class="table is-fullwidth is-hoverable"><thead><tr><th>Token ID</th><th class="has-text-right">Balance</th></tr></thead><tbody>';

    for (const tokenId of tokenIds) {
        const balance = balances[tokenId];
        const isNative = tokenId === '0';
        const label = isNative ? 'Native (0)' : tokenId;
        html += `<tr><td>${label}</td><td class="has-text-right"><strong>${balance.toLocaleString()}</strong></td></tr>`;
    }

    html += '</tbody></table>';

    // Also show nonce for reference
    html += `<p class="is-size-7 has-text-grey mt-2">Account ID: ${account.id} | Nonce: ${account.nonce}</p>`;

    balanceEl.innerHTML = html;
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

    // Build transaction list HTML
    let html = '';
    for (const tx of transactions) {
        const isOutgoing = tx.direction === 'outgoing';
        const directionClass = isOutgoing ? 'has-text-danger' : 'has-text-success';
        const directionIcon = isOutgoing ? '-' : '+';
        const directionLabel = isOutgoing ? 'Sent' : 'Received';

        // Format counterparty address (truncate for display)
        const counterpartyPk = isOutgoing ? tx.recipient_pk : tx.sender_pk;
        const counterpartyShort = counterpartyPk.substring(0, 8) + '...' + counterpartyPk.substring(56);

        // Format token display
        const tokenLabel = tx.token_id === 0 ? 'Native' : `Token ${tx.token_id}`;

        // Format status
        let statusHtml;
        if (tx.finalized) {
            statusHtml = '<span class="tag is-success is-light">Finalized</span>';
        } else if (tx.confirmations > 0) {
            statusHtml = `<span class="tag is-warning is-light">${tx.confirmations} conf (${tx.confirmations_remaining} more needed)</span>`;
        } else {
            statusHtml = '<span class="tag is-info is-light">Pending</span>';
        }

        html += `
            <div class="transaction-item">
                <div class="columns is-mobile is-vcentered mb-0">
                    <div class="column">
                        <p class="has-text-weight-semibold ${directionClass}">
                            ${directionIcon} ${tx.amount.toLocaleString()} ${tokenLabel}
                        </p>
                        <p class="is-size-7 has-text-grey">
                            ${directionLabel} ${isOutgoing ? 'to' : 'from'}
                            <span class="is-family-monospace">${counterpartyShort}</span>
                        </p>
                    </div>
                    <div class="column is-narrow has-text-right">
                        ${statusHtml}
                        ${tx.fee > 0 ? `<p class="is-size-7 has-text-grey">Fee: ${tx.fee}</p>` : ''}
                    </div>
                </div>
            </div>
        `;
    }

    historyEl.innerHTML = html;
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
    const tokenIdEl = document.getElementById('send-token-id');
    const feeEl = document.getElementById('send-fee');

    hideSendStatus();

    // Get form values
    const recipient = recipientEl.value.trim();
    const amount = parseInt(amountEl.value, 10);
    const tokenId = parseInt(tokenIdEl.value || '0', 10);
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

    // Copy public key button
    const copyPkBtn = document.getElementById('copy-pk-btn');
    if (copyPkBtn) {
        copyPkBtn.addEventListener('click', () => {
            const pkElement = document.getElementById('wallet-public-key');
            if (pkElement) {
                navigator.clipboard.writeText(pkElement.textContent).then(() => {
                    copyPkBtn.textContent = 'Copied!';
                    setTimeout(() => {
                        copyPkBtn.textContent = 'Copy';
                    }, 2000);
                });
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

    console.log('Coins Wallet initialized');
});

// Export for use by other scripts (future stories)
window.walletApp = walletApp;
window.showScreen = showScreen;
window.refreshBalance = refreshBalance;
window.refreshTransactions = refreshTransactions;
window.sendTransaction = sendTransaction;
