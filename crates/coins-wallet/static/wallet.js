// Coins Wallet - Browser-based wallet with WASM BLS signing
// Uses Web Crypto API for AES-GCM encryption with PBKDF2 key derivation

const STORAGE_KEY = 'coins_wallet_key';
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

        console.log('Wallet created successfully');
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

            console.log('Wallet unlocked successfully');
            console.log('Public key:', walletKey.public_key_hex());

            return walletKey.public_key_hex();
        } catch (error) {
            console.error('Failed to unlock wallet:', error);
            throw new Error('Wrong password or corrupted wallet data');
        }
    }

    /**
     * Lock the wallet (clear key from memory)
     */
    lockWallet() {
        currentKey = null;
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

    // Check if wallet exists in localStorage
    const hasWallet = walletApp.hasWallet();

    if (hasWallet) {
        showScreen('unlock-screen');
    } else {
        showScreen('create-screen');
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
                // Auto-refresh balance after creation
                await refreshBalance();
            } catch (error) {
                alert('Failed to create wallet: ' + error.message);
            } finally {
                createBtn.classList.remove('is-loading');
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
                // Auto-refresh balance after unlock
                await refreshBalance();
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

    console.log('Coins Wallet initialized');
});

// Export for use by other scripts (future stories)
window.walletApp = walletApp;
window.showScreen = showScreen;
window.refreshBalance = refreshBalance;
