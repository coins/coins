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
 * Position token suffix right after the amount digits
 */
export function repositionTokenSuffix() {
    const input = document.getElementById('send-amount');
    const measure = document.getElementById('send-amount-measure');
    const suffix = document.getElementById('send-token-suffix');
    if (!input || !measure || !suffix) return;

    // Horizontal: place suffix right after the centered text
    const text = input.value || input.placeholder || '';
    measure.textContent = text;
    const inputRect = input.getBoundingClientRect();
    const textWidth = measure.offsetWidth;
    const center = inputRect.left + inputRect.width / 2;
    const rowRect = input.closest('.amount-row').getBoundingClientRect();
    const leftPos = center + textWidth / 2 - rowRect.left;
    suffix.style.left = leftPos + 'px';

    // Vertical: baseline-align
    const inputStyle = getComputedStyle(input);
    const helper = document.createElement('div');
    helper.style.cssText = 'position:absolute;top:-9999px;left:0;display:inline-flex;align-items:baseline;';
    const bigSpan = document.createElement('span');
    bigSpan.style.cssText = `font-size:${inputStyle.fontSize};font-weight:${inputStyle.fontWeight};font-family:${inputStyle.fontFamily};line-height:${inputStyle.lineHeight};padding:${inputStyle.padding};`;
    bigSpan.textContent = text || '0';
    const smallSpan = document.createElement('span');
    const suffixStyle = getComputedStyle(suffix);
    smallSpan.style.cssText = `font-size:${suffixStyle.fontSize};font-weight:${suffixStyle.fontWeight};font-family:${suffixStyle.fontFamily};line-height:1;`;
    smallSpan.textContent = suffix.textContent;
    helper.appendChild(bigSpan);
    helper.appendChild(smallSpan);
    document.body.appendChild(helper);
    const delta = smallSpan.getBoundingClientRect().top - bigSpan.getBoundingClientRect().top;
    document.body.removeChild(helper);

    suffix.style.top = delta + 'px';
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
