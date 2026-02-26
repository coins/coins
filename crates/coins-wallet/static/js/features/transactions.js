// Transaction history rendering and filtering

import { walletApp } from '../core/WalletApp.js';
import {
    ACTIVITY_PAGE_SIZE
} from '../core/constants.js';
import {
    pendingTransactions,
    apiPendingTransactions,
    currentTransactions,
    activityFilters,
    activityDisplayCount,
    lastTransactionsJSON,
    setPendingTransactions,
    setApiPendingTransactions,
    setCurrentTransactions,
    setActivityDisplayCount,
    setLastTransactionsJSON
} from '../core/state.js';
import { greekLetters, greekNames } from '../utils/tokens.js';
import { stopPendingCountdown, getPendingCountdownSecs } from './pending.js';

/**
 * Apply activity filters to confirmed transactions
 * @param {Array} confirmedTxs - Array of confirmed transactions
 * @returns {Array} Filtered transactions
 */
export function applyActivityFilters(confirmedTxs) {
    let filtered = confirmedTxs;
    const { tokenIds, dateDays, customFrom, customTo } = activityFilters;

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

/**
 * Render transaction history including pending transactions
 */
export function renderTransactionHistory() {
    const historyEl = document.getElementById('transaction-history');
    if (!historyEl) return;

    const confirmedTxs = currentTransactions || [];
    const localPendingTxs = pendingTransactions || [];
    const apiPendingTxs = apiPendingTransactions || [];
    const currentPk = walletApp.getPublicKey();
    const pendingCountdownSecs = getPendingCountdownSecs();

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
    setPendingTransactions(stillLocalPending);

    // Filter out API pending transactions that are confirmed (finalized ones)
    const stillApiPending = apiPendingTxs.filter(pending => {
        // Keep if not finalized
        if (!pending.finalized) return true;

        // Check if in confirmed txs
        const isConfirmed = confirmedTxs.some(confirmed =>
            confirmed.sender_pk === pending.sender_pk &&
            confirmed.nonce === pending.nonce
        );
        return !isConfirmed;
    });
    setApiPendingTransactions(stillApiPending);

    // Stop polling if no more pending transactions
    if (stillLocalPending.length === 0 && stillApiPending.length === 0) {
        stopPendingCountdown();
    }

    if (confirmedTxs.length === 0 && stillLocalPending.length === 0 && stillApiPending.length === 0) {
        historyEl.innerHTML = '<p class="has-text-grey">No transactions yet</p>';
        return;
    }

    let html = '';

    // 1. Add local pending transactions (just submitted, not yet in API)
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
    const filteredTxs = applyActivityFilters(confirmedTxs);

    // Apply pagination
    const pendingCount = stillLocalPending.length + stillApiPending.length;
    const confirmedLimit = activityDisplayCount - pendingCount;
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
            setActivityDisplayCount(activityDisplayCount + ACTIVITY_PAGE_SIZE);
            renderTransactionHistory();
        });
    }

    // Add click handlers for confirmed transactions
    historyEl.querySelectorAll('.transaction-item.clickable[data-tx-index]').forEach(item => {
        item.addEventListener('click', () => {
            const index = parseInt(item.getAttribute('data-tx-index'), 10);
            if (currentTransactions && currentTransactions[index]) {
                showTransactionDetail(currentTransactions[index]);
            }
        });
    });
}

/**
 * Show transaction detail modal
 * @param {Object} tx - Transaction data
 */
export function showTransactionDetail(tx) {
    const modal = document.getElementById('tx-detail-modal');
    if (!modal || !tx) return;

    const isOutgoing = tx.direction === 'outgoing';
    const counterpartyPk = isOutgoing ? tx.recipient_pk : tx.sender_pk;

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

    const nonceEl = document.getElementById('tx-detail-nonce');
    if (nonceEl) nonceEl.textContent = tx.nonce !== undefined ? tx.nonce.toString() : '--';

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

    // Bitcoin block height
    const blockRow = document.getElementById('tx-detail-block-row');
    const blockEl = document.getElementById('tx-detail-block');
    if (tx.btc_height && tx.btc_height > 0) {
        if (blockRow) blockRow.style.display = 'flex';
        if (blockEl) blockEl.textContent = `#${tx.btc_height}`;
    } else {
        if (blockRow) blockRow.style.display = 'none';
    }

    // Bitcoin txid
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
        explorerLink.href = `http://localhost:3000/#/account/${counterpartyPk}`;
    }

    // Show modal
    modal.classList.add('is-active');
}

/**
 * Update transaction history from API data
 * @param {Array|null} transactions - Array of transactions
 */
export function updateTransactionHistory(transactions) {
    setCurrentTransactions(transactions || []);
    updateTokenFilterOptions(transactions || []);
    renderTransactionHistory();
}

/**
 * Initialize drag scrolling for token chips container
 * @param {HTMLElement} container - The token chips container
 */
function initTokenChipsDragScroll(container) {
    if (!container || container.dataset.dragInitialized) return;
    container.dataset.dragInitialized = 'true';

    let isDragging = false;
    let hasDragged = false;
    let startX = 0;
    let scrollLeft = 0;

    // Mouse events
    container.addEventListener('mousedown', (e) => {
        isDragging = true;
        hasDragged = false;
        startX = e.pageX - container.offsetLeft;
        scrollLeft = container.scrollLeft;
        container.classList.add('dragging');
    });

    document.addEventListener('mousemove', (e) => {
        if (!isDragging) return;
        const x = e.pageX - container.offsetLeft;
        const diff = Math.abs(x - startX);
        if (diff > 5) {
            hasDragged = true;
            e.preventDefault();
        }
        const walk = (x - startX) * 1.5;
        container.scrollLeft = scrollLeft - walk;
    });

    document.addEventListener('mouseup', () => {
        if (isDragging) {
            isDragging = false;
            container.classList.remove('dragging');
        }
    });

    // Prevent click on chips if we dragged
    container.addEventListener('click', (e) => {
        if (hasDragged) {
            e.stopPropagation();
            e.preventDefault();
            hasDragged = false;
        }
    }, true);

    // Touch events (native scroll works, but add class for visual feedback)
    container.addEventListener('touchstart', () => {
        container.classList.add('dragging');
    }, { passive: true });

    container.addEventListener('touchend', () => {
        container.classList.remove('dragging');
    });
}

/**
 * Update token filter options based on available tokens
 * @param {Array} transactions - Array of transactions
 */
export function updateTokenFilterOptions(transactions) {
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

    const activeSet = activityFilters.tokenIds;
    let html = '';
    for (const tid of sorted) {
        const idx = parseInt(tid);
        const letter = greekLetters[idx] || `#${tid}`;
        const name = greekNames[idx] || `Token ${tid}`;
        const isActive = !activeSet || activeSet.has(idx);
        html += `<button class="token-chip${isActive ? ' active' : ''}" data-token-id="${idx}">${letter} ${name}</button>`;
    }
    container.innerHTML = html;

    // Initialize drag scrolling
    initTokenChipsDragScroll(container);

    container.querySelectorAll('.token-chip').forEach(chip => {
        chip.addEventListener('click', () => {
            const tid = parseInt(chip.dataset.tokenId);
            toggleTokenFilter(tid, sorted.map(t => parseInt(t)));
        });
    });
}

/**
 * Toggle a token filter
 * @param {number} tid - Token ID to toggle
 * @param {Array<number>} allTokenIds - All available token IDs
 */
export function toggleTokenFilter(tid, allTokenIds) {
    let activeSet = activityFilters.tokenIds;

    if (!activeSet) {
        activeSet = new Set(allTokenIds);
        activeSet.delete(tid);
    } else if (activeSet.has(tid)) {
        activeSet.delete(tid);
    } else {
        activeSet.add(tid);
    }

    // Only reset to null (show all) when all tokens are selected
    if (activeSet.size === allTokenIds.length) {
        activeSet = null;
    }

    activityFilters.tokenIds = activeSet;

    const container = document.getElementById('activity-token-chips');
    if (container) {
        container.querySelectorAll('.token-chip').forEach(chip => {
            const chipTid = parseInt(chip.dataset.tokenId);
            const isActive = !activeSet || activeSet.has(chipTid);
            chip.classList.toggle('active', isActive);
        });
    }

    setActivityDisplayCount(ACTIVITY_PAGE_SIZE);
    renderTransactionHistory();
}

/**
 * Refresh transactions from API
 */
export async function refreshTransactions() {
    const historyEl = document.getElementById('transaction-history');

    if (!walletApp.isUnlocked()) {
        if (historyEl) historyEl.innerHTML = '<p class="has-text-danger">Wallet is locked</p>';
        return;
    }

    try {
        const transactions = await walletApp.fetchTransactions();
        const json = JSON.stringify(transactions);
        if (json === lastTransactionsJSON) return;
        setLastTransactionsJSON(json);
        updateTransactionHistory(transactions);
    } catch (error) {
        console.error('Failed to refresh transactions:', error);
        if (historyEl) {
            historyEl.innerHTML = `<p class="has-text-danger">Error: ${error.message}</p>`;
        }
    }
}

/**
 * Initialize transaction detail modal handlers
 */
export function initTransactionDetailModal() {
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

    // Copy counterparty address
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
}
