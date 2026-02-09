// Coins Wallet - Main Entry Point (ES6 Module)
// Browser-based wallet with WASM BLS signing

// Core
import { walletApp } from './core/WalletApp.js';
import { carouselState, receiveState, lastBalanceJSON, setLastBalanceJSON } from './core/state.js';

// UI
import { showScreen, showError, hideError, updateWalletDisplay } from './ui/screens.js';

// Services
import { initWebSocket, checkServiceConnections, setRefreshCallbacks } from './services/websocket.js';
import { startBalancePolling, stopBalancePolling, setPollingCallbacks } from './services/polling.js';

// Features
import { initAddressBook } from './features/addressBook.js';
import { initInvoice } from './features/invoice.js';
import { setPendingRenderCallback, refreshPendingTransactions } from './features/pending.js';
import { renderTransactionHistory, refreshTransactions, initTransactionDetailModal } from './features/transactions.js';
import { sendTransaction, setSendRefreshCallback, initSendForm } from './features/send.js';
import { initSendReceiveTabs, initReceiveTab } from './features/receive.js';

// Components
import {
    initBalanceCarousel,
    updateBalanceDisplay,
    initCarouselDragHandlers,
    setUpdateSendTokenInfoCallback
} from './components/BalanceCarousel.js';
import { initDateDrum, setDateDrumRenderCallback } from './components/DateDrumPicker.js';
import { updateSendTokenInfo, repositionTokenSuffix, getSelectedTokenMaxBalance } from './components/TokenSelector.js';
import { initRecipientSuggestions } from './components/RecipientInput.js';

// ========================================
// Refresh Functions
// ========================================

/**
 * Refresh balance from API
 */
async function refreshBalance() {
    const balanceEl = document.getElementById('balance-display');
    const refreshBtn = document.getElementById('refresh-balance-btn');

    if (!walletApp.isUnlocked()) {
        if (balanceEl) balanceEl.innerHTML = '<p class="has-text-danger">Wallet is locked</p>';
        return;
    }

    try {
        if (refreshBtn) refreshBtn.classList.add('is-loading');

        const account = await walletApp.refreshBalance();
        const json = JSON.stringify(account);
        if (json === lastBalanceJSON) return;
        setLastBalanceJSON(json);
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

// ========================================
// Wire up callbacks between modules
// ========================================

// Set refresh callbacks for WebSocket
setRefreshCallbacks(refreshBalance, refreshTransactions, refreshPendingTransactions);

// Set refresh callbacks for polling
setPollingCallbacks(refreshBalance, refreshTransactions);

// Set pending transaction render callback
setPendingRenderCallback(renderTransactionHistory);

// Set send refresh callback
setSendRefreshCallback(refreshBalance);

// Set date drum render callback
setDateDrumRenderCallback(renderTransactionHistory);

// Set update send token info callback for carousel
setUpdateSendTokenInfoCallback(updateSendTokenInfo);

// ========================================
// Amount Input Initialization
// ========================================

function initAmountInput() {
    const sendAmountInput = document.getElementById('send-amount');
    if (sendAmountInput) {
        sendAmountInput.addEventListener('input', repositionTokenSuffix);
        requestAnimationFrame(repositionTokenSuffix);
    }
    // Expose for use by updateSendTokenInfo
    window.repositionTokenSuffix = repositionTokenSuffix;

    // Scroll to adjust amount
    const amountInput = document.getElementById('send-amount');
    if (amountInput) {
        let scrollTimeout;
        amountInput.addEventListener('wheel', (e) => {
            e.preventDefault();

            const maxBalance = getSelectedTokenMaxBalance();
            const currentVal = parseInt(amountInput.value.replace(/,/g, '')) || 0;
            let step = 1;
            if (currentVal >= 10000) step = 1000;
            else if (currentVal >= 1000) step = 100;
            else if (currentVal >= 100) step = 10;

            const delta = e.deltaY < 0 ? step : -step;
            const newVal = Math.max(0, Math.min(maxBalance, currentVal + delta));

            amountInput.value = newVal;
            amountInput.classList.add('scrolling');

            const quickSelectEl = document.getElementById('send-quick-select');
            if (quickSelectEl) quickSelectEl.querySelectorAll('.quick-amount').forEach(b => b.classList.remove('active'));

            repositionTokenSuffix();

            clearTimeout(scrollTimeout);
            scrollTimeout = setTimeout(() => {
                amountInput.classList.remove('scrolling');
            }, 150);
        }, { passive: false });

        amountInput.addEventListener('input', () => {
            const quickSelectEl = document.getElementById('send-quick-select');
            if (quickSelectEl) quickSelectEl.querySelectorAll('.quick-amount').forEach(b => b.classList.remove('active'));
            const maxBalance = getSelectedTokenMaxBalance();
            const currentVal = parseInt(amountInput.value.replace(/,/g, '')) || 0;
            if (currentVal > maxBalance) {
                amountInput.value = maxBalance;
            }
        });

        amountInput.addEventListener('keypress', (e) => {
            if (!/[0-9]/.test(e.key) && e.key !== 'Backspace' && e.key !== 'Delete') {
                e.preventDefault();
            }
        });
    }
}

// ========================================
// Modal Handlers
// ========================================

function initBackupModal() {
    const backupBtn = document.getElementById('backup-wallet-btn');
    if (backupBtn) {
        backupBtn.addEventListener('click', () => {
            const modal = document.getElementById('backup-modal');
            if (modal) {
                modal.classList.add('is-active');
                document.getElementById('backup-password-step').style.display = 'block';
                document.getElementById('backup-key-step').style.display = 'none';
                document.getElementById('backup-password').value = '';
                document.getElementById('backup-secret-key').value = '';
                hideError('backup-password-error');
            }
        });
    }

    const closeBackupModal = document.getElementById('close-backup-modal');
    const closeBackupBtn = document.getElementById('close-backup-btn');
    if (closeBackupModal) {
        closeBackupModal.addEventListener('click', () => {
            document.getElementById('backup-modal').classList.remove('is-active');
        });
    }
    if (closeBackupBtn) {
        closeBackupBtn.addEventListener('click', () => {
            document.getElementById('backup-modal').classList.remove('is-active');
        });
    }

    const modalBackground = document.querySelector('#backup-modal .modal-background');
    if (modalBackground) {
        modalBackground.addEventListener('click', () => {
            document.getElementById('backup-modal').classList.remove('is-active');
        });
    }

    const confirmBackupBtn = document.getElementById('confirm-backup-btn');
    if (confirmBackupBtn) {
        confirmBackupBtn.addEventListener('click', async () => {
            const password = document.getElementById('backup-password').value;
            hideError('backup-password-error');

            if (!password) {
                showError('backup-password-error', 'Please enter your password');
                return;
            }

            try {
                confirmBackupBtn.classList.add('is-loading');
                const secretKeyHex = await walletApp.getSecretKeyHex(password);
                document.getElementById('backup-secret-key').value = secretKeyHex;
                document.getElementById('backup-password-step').style.display = 'none';
                document.getElementById('backup-key-step').style.display = 'block';
            } catch (error) {
                showError('backup-password-error', error.message);
            } finally {
                confirmBackupBtn.classList.remove('is-loading');
            }
        });
    }

    const copySecretKeyBtn = document.getElementById('copy-secret-key-btn');
    if (copySecretKeyBtn) {
        copySecretKeyBtn.addEventListener('click', () => {
            const secretKeyField = document.getElementById('backup-secret-key');
            if (secretKeyField) {
                navigator.clipboard.writeText(secretKeyField.value).then(() => {
                    copySecretKeyBtn.textContent = 'Copied!';
                    copySecretKeyBtn.classList.remove('is-primary');
                    copySecretKeyBtn.classList.add('is-success');
                    setTimeout(() => {
                        copySecretKeyBtn.textContent = 'Copy to Clipboard';
                        copySecretKeyBtn.classList.remove('is-success');
                        copySecretKeyBtn.classList.add('is-primary');
                    }, 2000);
                });
            }
        });
    }
}

function initResyncNonce() {
    const resyncNonceBtn = document.getElementById('resync-nonce-btn');
    if (resyncNonceBtn) {
        resyncNonceBtn.addEventListener('click', async () => {
            const originalText = resyncNonceBtn.textContent;
            resyncNonceBtn.textContent = 'Syncing...';
            resyncNonceBtn.style.pointerEvents = 'none';
            try {
                const nonce = await walletApp.resyncNonce();
                resyncNonceBtn.textContent = `Synced (${nonce})`;
                setTimeout(() => {
                    resyncNonceBtn.textContent = originalText;
                    resyncNonceBtn.style.pointerEvents = '';
                }, 2000);
            } catch (error) {
                console.error('Failed to resync nonce:', error);
                resyncNonceBtn.textContent = 'Failed';
                setTimeout(() => {
                    resyncNonceBtn.textContent = originalText;
                    resyncNonceBtn.style.pointerEvents = '';
                }, 2000);
            }
        });
    }
}

function initCopyPublicKey() {
    const copyPkBtn = document.getElementById('copy-pk-btn');
    if (copyPkBtn) {
        copyPkBtn.addEventListener('click', () => {
            const pkElement = document.getElementById('wallet-public-key');
            const feedbackElement = document.getElementById('copy-feedback');
            if (pkElement) {
                navigator.clipboard.writeText(pkElement.textContent).then(() => {
                    copyPkBtn.classList.add('copied');
                    if (feedbackElement) feedbackElement.textContent = 'copied!';
                    setTimeout(() => {
                        copyPkBtn.classList.remove('copied');
                        if (feedbackElement) feedbackElement.textContent = 'tap to copy';
                    }, 2000);
                });
            }
        });
    }
}

// ========================================
// DOMContentLoaded
// ========================================

document.addEventListener('DOMContentLoaded', async () => {
    console.log('Coins Wallet initializing...');

    // Initialize WebSocket connection
    initWebSocket();

    // Check connections on hover
    const statusIndicator = document.getElementById('status-indicator');
    if (statusIndicator) {
        statusIndicator.addEventListener('mouseenter', () => {
            checkServiceConnections();
        });
    }

    // Initialize WASM module
    try {
        await walletApp.init();
    } catch (error) {
        console.error('Failed to initialize wallet:', error);
    }

    // Try to restore session from sessionStorage
    const sessionRestored = await walletApp.restoreSession();

    if (sessionRestored) {
        console.log('Session restored, showing wallet screen');
        const publicKey = walletApp.getPublicKey();
        updateWalletDisplay(publicKey);
        showScreen('wallet-screen');
        await refreshBalance();
        await refreshTransactions();
        startBalancePolling();
    } else {
        const hasWallet = walletApp.hasWallet();
        if (hasWallet) {
            showScreen('unlock-screen');
        } else {
            showScreen('create-screen');
        }
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

    const showImportLink = document.getElementById('show-import-link');
    if (showImportLink) {
        showImportLink.addEventListener('click', () => showScreen('import-screen'));
    }

    const showCreateFromImportLink = document.getElementById('show-create-from-import-link');
    if (showCreateFromImportLink) {
        showCreateFromImportLink.addEventListener('click', () => showScreen('create-screen'));
    }

    // Create wallet button
    const createBtn = document.getElementById('create-wallet-btn');
    if (createBtn) {
        createBtn.addEventListener('click', async () => {
            const password = document.getElementById('create-password').value;
            const confirmPassword = document.getElementById('create-password-confirm').value;

            if (password !== confirmPassword) {
                alert('Passwords do not match');
                return;
            }

            if (!password) {
                alert('Please enter a password');
                return;
            }

            try {
                createBtn.classList.add('is-loading');
                const publicKey = await walletApp.createWallet(password);
                updateWalletDisplay(publicKey);
                showScreen('wallet-screen');
                await refreshBalance();
                await refreshTransactions();
                startBalancePolling();
            } catch (error) {
                alert('Failed to create wallet: ' + error.message);
            } finally {
                createBtn.classList.remove('is-loading');
            }
        });
    }

    // Import wallet button
    const importBtn = document.getElementById('import-wallet-btn');
    if (importBtn) {
        importBtn.addEventListener('click', async () => {
            const secretKeyHex = document.getElementById('import-secret-key').value.trim();
            const password = document.getElementById('import-password').value;
            const confirmPassword = document.getElementById('import-password-confirm').value;
            hideError('import-error');

            if (!secretKeyHex) {
                showError('import-error', 'Please enter a secret key');
                return;
            }

            if (password !== confirmPassword) {
                showError('import-error', 'Passwords do not match');
                return;
            }

            if (!password) {
                showError('import-error', 'Please enter a password');
                return;
            }

            try {
                importBtn.classList.add('is-loading');
                const publicKey = await walletApp.importWallet(secretKeyHex, password);
                updateWalletDisplay(publicKey);
                showScreen('wallet-screen');
                await refreshBalance();
                await refreshTransactions();
                startBalancePolling();
            } catch (error) {
                showError('import-error', error.message);
            } finally {
                importBtn.classList.remove('is-loading');
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
                await refreshBalance();
                await refreshTransactions();
                startBalancePolling();
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
            stopBalancePolling();
            walletApp.lockWallet();
            document.getElementById('unlock-password').value = '';
            showScreen('unlock-screen');
        });
    }

    // Initialize components
    initCopyPublicKey();
    initAmountInput();
    initCarouselDragHandlers();
    initDateDrum();
    initInvoice();
    initSendReceiveTabs();
    initReceiveTab();
    initRecipientSuggestions();
    initAddressBook();
    initSendForm();
    initTransactionDetailModal();
    initBackupModal();
    initResyncNonce();

    // Refresh balance button
    const refreshBtn = document.getElementById('refresh-balance-btn');
    if (refreshBtn) {
        refreshBtn.addEventListener('click', refreshBalance);
    }

    console.log('Coins Wallet initialized');
});

// ========================================
// Global Exports (Compatibility)
// ========================================

window.walletApp = walletApp;
window.showScreen = showScreen;
window.refreshBalance = refreshBalance;
window.refreshTransactions = refreshTransactions;
window.sendTransaction = sendTransaction;
window.initBalanceCarousel = initBalanceCarousel;
window.carouselState = carouselState;
window.receiveState = receiveState;
