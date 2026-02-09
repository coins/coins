// Invoice and payment request functionality

import { walletApp } from '../core/WalletApp.js';
import { showSendStatus } from '../ui/notifications.js';

/**
 * Parse a coins:// URI into its components
 * @param {string} uri - The coins:// URI to parse
 * @returns {Object|null} Parsed invoice data or null if invalid
 */
export function parseCoinsUri(uri) {
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

/**
 * Generate a coins:// URI
 * @param {string} addr - Recipient address
 * @param {number|null} amount - Amount (optional)
 * @param {number|null} token_id - Token ID (optional)
 * @param {string|null} memo - Memo (optional)
 * @param {number|null} expires - Unix timestamp expiry (optional)
 * @returns {string} The generated URI
 */
export function generateCoinsUri(addr, amount, token_id, memo, expires) {
    let uri = `coins://pay?addr=${addr}`;
    if (amount) uri += `&amount=${amount}`;
    if (token_id !== undefined && token_id !== null) uri += `&token=${token_id}`;
    if (memo) uri += `&memo=${encodeURIComponent(memo)}`;
    if (expires) uri += `&expires=${expires}`;
    return uri;
}

/**
 * Initialize invoice modal and event handlers
 */
export function initInvoice() {
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
