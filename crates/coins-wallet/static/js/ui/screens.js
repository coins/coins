// Screen navigation and display management

/**
 * Show a specific screen by ID, hiding all others
 * @param {string} screenId - The ID of the screen to show
 */
export function showScreen(screenId) {
    document.querySelectorAll('.screen').forEach(screen => {
        screen.classList.remove('active');
    });
    const screen = document.getElementById(screenId);
    if (screen) {
        screen.classList.add('active');
    }
}

/**
 * Show an error message in an element
 * @param {string} elementId - The ID of the error element
 * @param {string} message - The error message to display
 */
export function showError(elementId, message) {
    const element = document.getElementById(elementId);
    if (element) {
        element.textContent = message;
        element.style.display = 'block';
    }
}

/**
 * Hide an error message element
 * @param {string} elementId - The ID of the error element to hide
 */
export function hideError(elementId) {
    const element = document.getElementById(elementId);
    if (element) {
        element.style.display = 'none';
    }
}

/**
 * Update the wallet display with the public key
 * @param {string} publicKey - The wallet's public key
 */
export function updateWalletDisplay(publicKey) {
    const pkElement = document.getElementById('wallet-public-key');
    if (pkElement) {
        pkElement.textContent = publicKey;
    }
}
