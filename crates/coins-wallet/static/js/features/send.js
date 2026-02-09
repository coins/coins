// Send transaction functionality

import { walletApp } from '../core/WalletApp.js';
import { carouselState, clearRefreshCaches } from '../core/state.js';
import { showSendStatus, hideSendStatus, showSendAnimation } from '../ui/notifications.js';
import { getAddressBook, saveAddressBook } from './addressBook.js';
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

        // Store recipient before clearing form
        const sentTo = recipient;

        // Clear form
        recipientEl.value = '';
        amountEl.value = '';

        // Refresh balance
        if (refreshBalanceCallback) await refreshBalanceCallback();

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

/**
 * Initialize send form handlers
 */
export function initSendForm() {
    const sendBtn = document.getElementById('send-tx-btn');
    if (sendBtn) {
        sendBtn.addEventListener('click', sendTransaction);
    }
}
