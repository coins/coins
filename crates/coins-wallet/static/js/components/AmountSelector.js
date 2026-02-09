// Reusable AmountSelector component for send/receive panels

import { greekLetters, greekNames } from '../utils/tokens.js';

/**
 * AmountSelector - A reusable amount input component with quick select buttons
 */
export class AmountSelector {
    /**
     * Create an AmountSelector instance
     * @param {Object} options - Configuration options
     * @param {string} options.inputId - Amount input element ID
     * @param {string} options.measureId - Hidden measure span for positioning
     * @param {string} options.suffixId - Token suffix element ID
     * @param {string} options.quickSelectId - Quick select buttons container ID
     * @param {Function} options.getMaxBalance - Function returning max balance
     * @param {Function} options.getTokenName - Function returning token display name
     * @param {Function} [options.onChange] - Optional callback on value change
     * @param {number[]} [options.presets] - Quick select amounts (default: [10, 100, 1000, 10000])
     * @param {boolean} [options.showMaxButton] - Show max balance button (default: true)
     */
    constructor(options) {
        this.inputId = options.inputId;
        this.measureId = options.measureId;
        this.suffixId = options.suffixId;
        this.quickSelectId = options.quickSelectId;
        this.getMaxBalance = options.getMaxBalance;
        this.getTokenName = options.getTokenName;
        this.onChange = options.onChange || null;
        this.presets = options.presets || [10, 100, 1000, 10000];
        this.showMaxButton = options.showMaxButton !== false;

        this.scrollTimeout = null;

        this.init();
    }

    /**
     * Get the input element
     */
    get input() {
        return document.getElementById(this.inputId);
    }

    /**
     * Get the measure element
     */
    get measure() {
        return document.getElementById(this.measureId);
    }

    /**
     * Get the suffix element
     */
    get suffix() {
        return document.getElementById(this.suffixId);
    }

    /**
     * Get the quick select container
     */
    get quickSelect() {
        return document.getElementById(this.quickSelectId);
    }

    /**
     * Initialize the component
     */
    init() {
        const input = this.input;
        if (!input) return;

        // Scroll to adjust amount
        input.addEventListener('wheel', (e) => this.handleWheel(e), { passive: false });

        // Manual input handling
        input.addEventListener('input', () => this.handleInput());

        // Only allow numbers
        input.addEventListener('keypress', (e) => {
            if (!/[0-9]/.test(e.key) && e.key !== 'Backspace' && e.key !== 'Delete') {
                e.preventDefault();
            }
        });

        // Initial position
        requestAnimationFrame(() => this.repositionSuffix());
    }

    /**
     * Handle mouse wheel events for scroll-to-adjust
     * @param {WheelEvent} e
     */
    handleWheel(e) {
        e.preventDefault();

        const input = this.input;
        if (!input) return;

        const maxBalance = this.getMaxBalance();
        const currentVal = parseInt(input.value.replace(/,/g, '')) || 0;

        // Determine step size based on current value
        let step = 1;
        if (currentVal >= 10000) step = 1000;
        else if (currentVal >= 1000) step = 100;
        else if (currentVal >= 100) step = 10;

        // Scroll up = increase, scroll down = decrease
        const delta = e.deltaY < 0 ? step : -step;
        const newVal = Math.max(0, Math.min(maxBalance, currentVal + delta));

        input.value = newVal;
        input.classList.add('scrolling');

        // Clear active quick amounts
        this.clearQuickSelectActive();
        this.repositionSuffix();

        if (this.onChange) this.onChange(newVal);

        // Remove scrolling class after a delay
        clearTimeout(this.scrollTimeout);
        this.scrollTimeout = setTimeout(() => {
            input.classList.remove('scrolling');
        }, 150);
    }

    /**
     * Handle manual input
     */
    handleInput() {
        const input = this.input;
        if (!input) return;

        this.clearQuickSelectActive();

        // Cap value at max balance
        const maxBalance = this.getMaxBalance();
        const currentVal = parseInt(input.value.replace(/,/g, '')) || 0;
        if (currentVal > maxBalance) {
            input.value = maxBalance;
        }

        this.repositionSuffix();

        if (this.onChange) this.onChange(parseInt(input.value) || 0);
    }

    /**
     * Clear active state from quick select buttons
     */
    clearQuickSelectActive() {
        const qs = this.quickSelect;
        if (qs) {
            qs.querySelectorAll('.quick-amount').forEach(b => b.classList.remove('active'));
        }
    }

    /**
     * Position token suffix right after the amount digits, baseline-aligned
     */
    repositionSuffix() {
        const input = this.input;
        const measure = this.measure;
        const suffix = this.suffix;
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

        // Vertical: baseline-align using a hidden inline-flex container
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
     * Update the quick select buttons based on current balance
     * @param {number} balance - Current token balance
     * @param {number} fee - Fee to subtract from max
     */
    updateQuickSelect(balance, fee = 1) {
        const qs = this.quickSelect;
        if (!qs) return;

        const maxSendable = Math.max(0, balance - fee);
        const validPresets = this.presets.filter(v => v < balance);

        let html = '';
        for (const v of validPresets) {
            const label = v >= 1000 ? (v / 1000) + 'k' : v;
            html += `<span class="quick-amount" data-amount="${v}">${label}</span>`;
        }

        // Always show max if positive
        if (this.showMaxButton && maxSendable > 0) {
            const maxLabel = maxSendable.toLocaleString();
            html += `<span class="quick-amount" data-amount="${maxSendable}">${maxLabel}</span>`;
        }

        qs.innerHTML = html;

        // Wire up click handlers
        const input = this.input;
        qs.querySelectorAll('.quick-amount').forEach(btn => {
            btn.addEventListener('click', () => {
                const amount = parseInt(btn.getAttribute('data-amount'), 10);
                if (input) {
                    input.value = amount;
                    input.classList.add('scrolling');
                    setTimeout(() => input.classList.remove('scrolling'), 150);
                }
                qs.querySelectorAll('.quick-amount').forEach(b => b.classList.remove('active'));
                btn.classList.add('active');
                this.repositionSuffix();
                if (this.onChange) this.onChange(amount);
            });
        });
    }

    /**
     * Update the token suffix display
     * @param {string} tokenName - Token display name
     */
    updateTokenSuffix(tokenName) {
        const suffix = this.suffix;
        if (suffix) {
            suffix.textContent = tokenName;
            requestAnimationFrame(() => this.repositionSuffix());
        }
    }

    /**
     * Get current value
     * @returns {number}
     */
    getValue() {
        const input = this.input;
        return input ? (parseInt(input.value.replace(/,/g, '')) || 0) : 0;
    }

    /**
     * Set value
     * @param {number} value
     */
    setValue(value) {
        const input = this.input;
        if (input) {
            input.value = value;
            this.repositionSuffix();
        }
    }

    /**
     * Clear the input
     */
    clear() {
        const input = this.input;
        if (input) {
            input.value = '';
            this.clearQuickSelectActive();
            this.repositionSuffix();
        }
    }
}

// Export a factory function for easier creation
export function createAmountSelector(options) {
    return new AmountSelector(options);
}
