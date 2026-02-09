// Notification and status message handling

/**
 * Show send status message (typically for errors)
 * @param {string} message - The message to display
 * @param {boolean} isError - Whether this is an error message
 */
export function showSendStatus(message, isError = false) {
    const statusEl = document.getElementById('send-status');
    if (statusEl) {
        statusEl.textContent = message;
        statusEl.className = `notification mt-3 ${isError ? 'is-danger' : 'is-success'}`;
        statusEl.style.display = 'block';
    }
}

/**
 * Hide the send status message
 */
export function hideSendStatus() {
    const statusEl = document.getElementById('send-status');
    if (statusEl) {
        statusEl.style.display = 'none';
    }
}

/**
 * Show the send animation overlay
 */
export function showSendAnimation() {
    const sendCard = document.getElementById('send-card');
    if (!sendCard) return;

    // Make sure the card has position relative for absolute overlay
    sendCard.style.position = 'relative';

    // Create overlay with coin stack animation
    const overlay = document.createElement('div');
    overlay.className = 'send-animation-overlay';
    overlay.id = 'send-animation-overlay';
    overlay.innerHTML = `
        <div class="coin-stack">
            <div class="coin coin-dropping">◎</div>
            <div class="coin coin-3">◎</div>
            <div class="coin coin-2">◎</div>
            <div class="coin coin-1">◎</div>
        </div>
        <div class="send-animation-text">Sending...</div>
    `;

    sendCard.appendChild(overlay);

    // Remove after animation completes
    setTimeout(() => {
        overlay.style.opacity = '0';
        overlay.style.transition = 'opacity 0.2s ease-out';
        setTimeout(() => {
            overlay.remove();
        }, 200);
    }, 1000);
}

/**
 * Update connection status indicator
 * @param {boolean} connected - Whether the connection is active
 */
export function updateConnectionStatus(connected) {
    const statusEl = document.getElementById('connection-status');
    const dotEl = document.getElementById('connection-dot');
    if (statusEl) {
        statusEl.textContent = connected ? 'Connected' : 'Offline';
    }
    if (dotEl) {
        if (connected) {
            dotEl.classList.remove('disconnected');
        } else {
            dotEl.classList.add('disconnected');
        }
    }
}
