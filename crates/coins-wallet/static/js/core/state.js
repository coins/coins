// Centralized state management for the wallet application

// WASM module reference
export let wasmModule = null;

// Current wallet key (in memory while unlocked)
export let currentKey = null;

// WebSocket connection
export let ws = null;
export let wsReconnectTimeout = null;

// Cache for WebSocket refresh deduplication
export let lastBalanceJSON = null;
export let lastTransactionsJSON = null;
export let lastPendingJSON = null;

// Balance polling interval
export let balancePollInterval = null;

// Pending transaction state
export let pendingCountdownInterval = null;
export let pendingCountdownSecs = null;
export let pendingStatusPollInterval = null;

// Balance carousel state
export const carouselState = {
    currentOffset: 0,
    itemWidth: 120,
    selectedIndex: 0,
    tokenIds: [],      // Array of token IDs in carousel order
    balances: {}       // Map of token_id -> balance
};

// Receive panel state
export const receiveState = {
    qrDebounceTimer: null,
    tokenSelectorReady: false,
    selectedTokenId: 0,
    isReceiveMode: false
};

// Date drum picker state
export const dateDrumState = {
    currentOffset: 0,
    selectedIndex: 0,
    itemHeight: 22
};

// Activity filter state
export const activityFilters = {
    tokenIds: null, // null = all tokens, Set of enabled token IDs when filtering
    dateDays: 'all', // 'all', number of days, or 'custom'
    customFrom: null, // Date string (yyyy-mm-dd) for custom range start
    customTo: null,   // Date string (yyyy-mm-dd) for custom range end
};

// Pagination state for activity list
export let activityDisplayCount = 20;

// Store pending and confirmed transactions
export let pendingTransactions = [];
export let apiPendingTransactions = [];
export let currentTransactions = [];

// Setters for module-level state that needs to be updated
export function setWasmModule(module) {
    wasmModule = module;
}

export function setCurrentKey(key) {
    currentKey = key;
}

export function setWs(socket) {
    ws = socket;
}

export function setWsReconnectTimeout(timeout) {
    wsReconnectTimeout = timeout;
}

export function setLastBalanceJSON(json) {
    lastBalanceJSON = json;
}

export function setLastTransactionsJSON(json) {
    lastTransactionsJSON = json;
}

export function setLastPendingJSON(json) {
    lastPendingJSON = json;
}

export function setBalancePollInterval(interval) {
    balancePollInterval = interval;
}

export function setPendingCountdownInterval(interval) {
    pendingCountdownInterval = interval;
}

export function setPendingCountdownSecs(secs) {
    pendingCountdownSecs = secs;
}

export function setPendingStatusPollInterval(interval) {
    pendingStatusPollInterval = interval;
}

export function setActivityDisplayCount(count) {
    activityDisplayCount = count;
}

export function setPendingTransactions(txs) {
    pendingTransactions = txs;
}

export function setApiPendingTransactions(txs) {
    apiPendingTransactions = txs;
}

export function setCurrentTransactions(txs) {
    currentTransactions = txs;
}

// Clear caches (used after sending transaction)
export function clearRefreshCaches() {
    lastBalanceJSON = null;
    lastTransactionsJSON = null;
    lastPendingJSON = null;
}
