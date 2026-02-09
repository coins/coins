// WalletApp class - manages wallet operations

import {
    STORAGE_KEY,
    SESSION_KEY,
    NONCE_KEY,
    PBKDF2_ITERATIONS,
    SALT_SIZE,
    IV_SIZE
} from './constants.js';

import {
    wasmModule,
    currentKey,
    setWasmModule,
    setCurrentKey
} from './state.js';

/**
 * WalletApp class - manages wallet operations
 */
export class WalletApp {
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
            const module = await import('/wasm/coins_wallet_wasm.js');
            await module.default();
            setWasmModule(module);
            console.log('WASM module loaded successfully');
            this.initialized = true;
        } catch (error) {
            console.error('Failed to load WASM module:', error);
            throw new Error('Failed to initialize wallet: WASM module could not be loaded');
        }
    }

    /**
     * Get the WASM module
     */
    getWasmModule() {
        return wasmModule;
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
        setCurrentKey(walletKey);

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
        setCurrentKey(walletKey);

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
            setCurrentKey(walletKey);

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
        setCurrentKey(null);
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
            setCurrentKey(walletKey);

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
export const walletApp = new WalletApp();
