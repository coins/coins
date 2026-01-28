// Coins Wallet - Browser-based wallet with WASM BLS signing
// This file will be fully implemented in US-007 through US-013

console.log('Coins Wallet loading...');

// Screen management
function showScreen(screenId) {
    document.querySelectorAll('.screen').forEach(screen => {
        screen.classList.remove('active');
    });
    const screen = document.getElementById(screenId);
    if (screen) {
        screen.classList.add('active');
    }
}

// Initialize when DOM is loaded
document.addEventListener('DOMContentLoaded', () => {
    console.log('Coins Wallet initialized');

    // Check if wallet exists in localStorage
    const hasWallet = localStorage.getItem('coins_wallet_key') !== null;

    if (hasWallet) {
        showScreen('unlock-screen');
    } else {
        showScreen('create-screen');
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

    // Placeholder click handlers (will be implemented in later stories)
    const createBtn = document.getElementById('create-wallet-btn');
    if (createBtn) {
        createBtn.addEventListener('click', () => {
            console.log('Create wallet clicked - will be implemented in US-007');
        });
    }

    const unlockBtn = document.getElementById('unlock-wallet-btn');
    if (unlockBtn) {
        unlockBtn.addEventListener('click', () => {
            console.log('Unlock wallet clicked - will be implemented in US-008');
        });
    }

    const lockBtn = document.getElementById('lock-wallet-btn');
    if (lockBtn) {
        lockBtn.addEventListener('click', () => {
            showScreen('unlock-screen');
        });
    }
});
