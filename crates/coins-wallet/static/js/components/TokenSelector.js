// Token selector dropdown component

import { carouselState } from '../core/state.js';
import { greekLetters, greekNames, getTokenDisplayInfo } from '../utils/tokens.js';

/**
 * Update the token info display in the send box and rebuild quick-select amounts
 */
export function updateSendTokenInfo() {
    const tokenSuffixEl = document.getElementById('send-token-suffix');
    const quickSelectEl = document.getElementById('send-quick-select');

    const { selectedIndex, tokenIds, balances } = carouselState;
    if (tokenIds.length === 0) {
        if (tokenSuffixEl) tokenSuffixEl.textContent = '--';
        if (quickSelectEl) quickSelectEl.innerHTML = '';
        return;
    }

    const tokenId = tokenIds[selectedIndex] || '0';
    const idx = parseInt(tokenId);
    const letter = greekLetters[idx] || `#${tokenId}`;
    const name = greekNames[idx] || `Token ${tokenId}`;
    const balance = balances[tokenId] || 0;
    const fee = parseInt(document.getElementById('send-fee')?.value || '1', 10);
    const maxSendable = Math.max(0, balance - fee);

    if (tokenSuffixEl) tokenSuffixEl.textContent = `${letter} ${name}`;

    // Rebuild quick-select: only show preset amounts < balance, plus maxSendable
    if (quickSelectEl) {
        const presets = [10, 100, 1000, 10000];
        const validPresets = presets.filter(v => v < balance);

        let html = '';
        for (const v of validPresets) {
            const label = v >= 1000 ? (v / 1000) + 'k' : v;
            html += `<span class="quick-amount" data-amount="${v}">${label}</span>`;
        }
        if (maxSendable > 0) {
            const maxLabel = maxSendable.toLocaleString();
            html += `<span class="quick-amount" data-amount="${maxSendable}">${maxLabel}</span>`;
        }
        quickSelectEl.innerHTML = html;

        // Wire up click handlers
        const amountInput = document.getElementById('send-amount');
        quickSelectEl.querySelectorAll('.quick-amount').forEach(btn => {
            btn.addEventListener('click', () => {
                const amount = parseInt(btn.getAttribute('data-amount'), 10);
                if (amountInput) {
                    amountInput.value = amount;
                    amountInput.classList.add('scrolling');
                    setTimeout(() => amountInput.classList.remove('scrolling'), 150);
                }
                quickSelectEl.querySelectorAll('.quick-amount').forEach(b => b.classList.remove('active'));
                btn.classList.add('active');
                if (window.repositionTokenSuffix) window.repositionTokenSuffix();
            });
        });
    }

    // Reposition suffix after token name change
    if (window.repositionTokenSuffix) requestAnimationFrame(window.repositionTokenSuffix);
}

/**
 * Update the spacer to match input text - CSS handles all positioning
 */
export function repositionTokenSuffix() {
    const input = document.getElementById('send-amount');
    const spacer = document.getElementById('send-amount-spacer');
    if (!input || !spacer) return;

    // Just sync the spacer text with the input - CSS grid/flexbox handles positioning
    const text = input.value || input.placeholder || '0';
    spacer.textContent = text;
}

/**
 * Get max balance for selected token
 * @returns {number}
 */
export function getSelectedTokenMaxBalance() {
    const selectedIndex = carouselState.selectedIndex;
    const tokenIds = carouselState.tokenIds;
    const balances = carouselState.balances;
    if (tokenIds.length === 0) return 0;
    const tokenId = tokenIds[selectedIndex];
    return balances[tokenId] || 0;
}
