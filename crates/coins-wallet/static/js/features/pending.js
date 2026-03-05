// Pending transaction management

import { walletApp } from '../core/WalletApp.js';
import {
    pendingTransactions,
    apiPendingTransactions,
    pendingCountdownInterval,
    pendingCountdownSecs,
    pendingStatusPollInterval,
    lastPendingJSON,
    setPendingTransactions,
    setApiPendingTransactions,
    setPendingCountdownInterval,
    setPendingCountdownSecs,
    setPendingStatusPollInterval,
    setLastPendingJSON
} from '../core/state.js';

// Store callback for rendering
let renderCallback = null;

/**
 * Set the render callback for transaction history
 * @param {Function} callback - Function to call to render transaction history
 */
export function setPendingRenderCallback(callback) {
    renderCallback = callback;
}

/**
 * Start countdown timer for pending transactions
 */
export function startPendingCountdown() {
    // Clear any existing interval
    if (pendingCountdownInterval) {
        clearInterval(pendingCountdownInterval);
        setPendingCountdownInterval(null);
    }

    // If no pending transactions, don't start countdown
    if (!pendingTransactions || pendingTransactions.length === 0) {
        return;
    }

    // Set an optimistic default immediately so renders see "Broadcasting"
    // before the async fetch resolves with the real value
    if (pendingCountdownSecs === null || pendingCountdownSecs <= 0) {
        setPendingCountdownSecs(30);
    }

    // Fetch real value from publisher (will update in-place)
    fetchPublisherCountdown();

    // Update every second
    const interval = setInterval(() => {
        if (pendingCountdownSecs !== null && pendingCountdownSecs > 1) {
            setPendingCountdownSecs(pendingCountdownSecs - 1);
            updatePendingCountdownDisplay();
        } else {
            // Countdown reached 0 - publisher loop should run now
            setPendingCountdownSecs(0);
            updatePendingCountdownDisplay();
            // Start polling for actual status from API
            startPendingStatusPolling();
        }
    }, 1000);
    setPendingCountdownInterval(interval);
}

/**
 * Start polling for pending transaction status from API
 */
export function startPendingStatusPolling() {
    // Stop the countdown interval since we're now polling
    if (pendingCountdownInterval) {
        clearInterval(pendingCountdownInterval);
        setPendingCountdownInterval(null);
    }

    // Clear any existing poll interval
    if (pendingStatusPollInterval) {
        clearInterval(pendingStatusPollInterval);
        setPendingStatusPollInterval(null);
    }

    // Fetch immediately
    refreshPendingTransactions();

    // Poll every 3 seconds
    const interval = setInterval(async () => {
        await refreshPendingTransactions();

        // If no more local pending or API pending transactions, stop polling
        if ((!pendingTransactions || pendingTransactions.length === 0) &&
            (!apiPendingTransactions || apiPendingTransactions.length === 0)) {
            stopPendingStatusPolling();
        }
    }, 3000);
    setPendingStatusPollInterval(interval);
}

/**
 * Stop polling for pending transaction status
 */
export function stopPendingStatusPolling() {
    if (pendingStatusPollInterval) {
        clearInterval(pendingStatusPollInterval);
        setPendingStatusPollInterval(null);
    }
}

/**
 * Refresh pending transactions from API and re-render
 */
export async function refreshPendingTransactions() {
    try {
        const pendingFromAPI = await walletApp.fetchPendingTransactions();
        const json = JSON.stringify(pendingFromAPI);
        if (json === lastPendingJSON) return;
        setLastPendingJSON(json);
        setApiPendingTransactions(pendingFromAPI || []);
        if (renderCallback) renderCallback();
    } catch (error) {
        console.warn('Error refreshing pending transactions:', error);
    }
}

/**
 * Fetch countdown from publisher
 */
export async function fetchPublisherCountdown() {
    try {
        const resp = await fetch('/api/publisher/status', {
            signal: AbortSignal.timeout(3000)
        });
        if (resp.ok) {
            const status = await resp.json();
            if (status.secs_until_next_loop !== null) {
                setPendingCountdownSecs(status.secs_until_next_loop);
                updatePendingCountdownDisplay();
            }
        }
    } catch (e) {
        console.warn('Could not fetch publisher status for countdown:', e);
    }
}

/**
 * Update the countdown display in pending transactions
 */
export function updatePendingCountdownDisplay() {
    const pendingTags = document.querySelectorAll('.pending-tag');
    pendingTags.forEach(el => {
        if (pendingCountdownSecs !== null && pendingCountdownSecs > 0) {
            el.innerHTML = `Publishing in <span class="pending-countdown">${pendingCountdownSecs}s</span>`;
            el.className = 'conf-badge publishing pending-tag';
        } else {
            el.innerHTML = 'Publishing';
            el.className = 'conf-badge publishing pending-tag';
        }
    });
}

/**
 * Stop countdown timer and status polling
 */
export function stopPendingCountdown() {
    if (pendingCountdownInterval) {
        clearInterval(pendingCountdownInterval);
        setPendingCountdownInterval(null);
    }
    if (pendingStatusPollInterval) {
        clearInterval(pendingStatusPollInterval);
        setPendingStatusPollInterval(null);
    }
    setPendingCountdownSecs(null);
}

/**
 * Add a pending transaction to the activity list
 * @param {Object} tx - Transaction data
 */
export function addPendingTransaction(tx) {
    const currentPk = walletApp.getPublicKey();

    // Add to pending list with timestamp
    const newPending = [{
        ...tx,
        sender_pk: currentPk,
        pending: true,
        timestamp: Date.now()
    }, ...pendingTransactions];
    setPendingTransactions(newPending);

    // Re-render the transaction history with the pending transaction at the top
    if (renderCallback) renderCallback();

    // Start countdown timer
    startPendingCountdown();
}

/**
 * Get current pending countdown seconds
 * @returns {number|null}
 */
export function getPendingCountdownSecs() {
    return pendingCountdownSecs;
}
