// Send transaction functionality

import { walletApp } from '../core/WalletApp.js';
import { carouselState, clearRefreshCaches } from '../core/state.js';
import { showSendStatus, hideSendStatus, showSendAnimation } from '../ui/notifications.js';
import { addPendingTransaction } from './pending.js';

// Store refresh callback
let refreshBalanceCallback = null;

/**
 * Set the refresh balance callback
 * @param {Function} callback
 */
export function setSendRefreshCallback(callback) {
    refreshBalanceCallback = callback;
}

/**
 * Send transaction from form
 */
export async function sendTransaction() {
    const sendBtn = document.getElementById('send-tx-btn');
    const recipientEl = document.getElementById('send-recipient');
    const amountEl = document.getElementById('send-amount');
    const feeEl = document.getElementById('send-fee');

    hideSendStatus();

    // Get token from carousel selection
    const selectedIndex = carouselState.selectedIndex;
    const tokenIds = carouselState.tokenIds;
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
        clearRefreshCaches();

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

        // Clear form
        recipientEl.value = '';
        amountEl.value = '';

        // Refresh balance
        if (refreshBalanceCallback) await refreshBalanceCallback();

    } catch (error) {
        console.error('Send transaction failed:', error);
        showSendStatus(error.message, true);
    } finally {
        sendBtn.classList.remove('is-loading');
    }
}

/**
 * Initialize send form handlers
 */
export function initSendForm() {
    const sendBtn = document.getElementById('send-tx-btn');
    if (sendBtn) {
        sendBtn.addEventListener('click', sendTransaction);
    }
}
