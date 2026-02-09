// Balance and transaction polling

import { BALANCE_POLL_INTERVAL_MS } from '../core/constants.js';
import { walletApp } from '../core/WalletApp.js';
import { balancePollInterval, setBalancePollInterval } from '../core/state.js';

// Store callback references
let refreshBalanceCallback = null;
let refreshTransactionsCallback = null;

/**
 * Set the refresh callbacks for polling
 * @param {Function} balanceCallback - Function to refresh balance
 * @param {Function} transactionsCallback - Function to refresh transactions
 */
export function setPollingCallbacks(balanceCallback, transactionsCallback) {
    refreshBalanceCallback = balanceCallback;
    refreshTransactionsCallback = transactionsCallback;
}

/**
 * Start polling for balance updates
 */
export function startBalancePolling() {
    stopBalancePolling();
    const interval = setInterval(async () => {
        if (walletApp.isUnlocked()) {
            if (refreshBalanceCallback) await refreshBalanceCallback();
            if (refreshTransactionsCallback) await refreshTransactionsCallback();
        } else {
            stopBalancePolling();
        }
    }, BALANCE_POLL_INTERVAL_MS);
    setBalancePollInterval(interval);
}

/**
 * Stop polling for balance updates
 */
export function stopBalancePolling() {
    if (balancePollInterval) {
        clearInterval(balancePollInterval);
        setBalancePollInterval(null);
    }
}
