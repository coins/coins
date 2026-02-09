// WebSocket connection management

import { walletApp } from '../core/WalletApp.js';
import { ws, wsReconnectTimeout, setWs, setWsReconnectTimeout } from '../core/state.js';
import { updateConnectionStatus } from '../ui/notifications.js';

// Import these lazily to avoid circular dependencies
let refreshBalance, refreshTransactions, refreshPendingTransactions;

export function setRefreshCallbacks(balance, transactions, pending) {
    refreshBalance = balance;
    refreshTransactions = transactions;
    refreshPendingTransactions = pending;
}

/**
 * Initialize WebSocket connection for real-time updates
 */
export function initWebSocket() {
    if (ws && ws.readyState === WebSocket.OPEN) return;

    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const wsUrl = `${protocol}//${window.location.host}/ws`;

    try {
        const socket = new WebSocket(wsUrl);
        setWs(socket);

        socket.onopen = () => {
            console.log('WebSocket connected');
            updateConnectionStatus(true);
        };

        socket.onmessage = (event) => {
            try {
                const msg = JSON.parse(event.data);
                handleWSMessage(msg);
            } catch (e) {
                console.error('Failed to parse WebSocket message:', e);
            }
        };

        socket.onclose = () => {
            console.log('WebSocket disconnected');
            updateConnectionStatus(false);
            // Reconnect after 3 seconds
            if (wsReconnectTimeout) clearTimeout(wsReconnectTimeout);
            setWsReconnectTimeout(setTimeout(() => initWebSocket(), 3000));
        };

        socket.onerror = (error) => {
            console.error('WebSocket error:', error);
            updateConnectionStatus(false);
        };
    } catch (e) {
        console.error('Failed to create WebSocket:', e);
        updateConnectionStatus(false);
    }
}

/**
 * Handle incoming WebSocket messages
 * @param {Object} msg - The parsed message object
 */
function handleWSMessage(msg) {
    console.log('WebSocket message:', msg);

    // Only refresh if wallet is unlocked
    if (!walletApp.isUnlocked()) return;

    switch (msg.type) {
        case 'balance_update':
            // Refresh balance display
            if (refreshBalance) refreshBalance();
            break;
        case 'transaction_update':
            // Refresh both balance and transactions
            if (refreshBalance) refreshBalance();
            if (refreshTransactions) refreshTransactions();
            break;
        case 'pending_txs_update':
            // Refresh pending transaction statuses
            if (refreshPendingTransactions) refreshPendingTransactions();
            break;
        case 'connected':
            console.log('WebSocket connection confirmed');
            break;
    }
}

/**
 * Check connection status for indexer, publisher, and explorer
 */
export async function checkServiceConnections() {
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
