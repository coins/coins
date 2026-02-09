// Receive panel functionality

import { walletApp } from '../core/WalletApp.js';
import { carouselState, receiveState } from '../core/state.js';
import { greekLetters, greekNames, getTokenDisplayInfo } from '../utils/tokens.js';
import { generateCoinsUri } from './invoice.js';

/**
 * Initialize send/receive tab toggle (flip animation)
 */
export function initSendReceiveTabs() {
    const flipCard = document.getElementById('sr-flip-card');
    const toggleFront = document.getElementById('sr-toggle-front');
    const toggleBack = document.getElementById('sr-toggle-back');
    if (!flipCard) return;

    function toggleFlip() {
        receiveState.isReceiveMode = !receiveState.isReceiveMode;

        if (receiveState.isReceiveMode) {
            // Switch to Receive
            const needsInit = !receiveState.tokenSelectorReady;
            if (needsInit) {
                initReceiveTokenSelector();
                receiveState.tokenSelectorReady = true;
            }

            updateReceiveQR();

            // Position suffix before flip animation starts (while back face is measured)
            repositionReceiveTokenSuffix();

            requestAnimationFrame(() => {
                flipCard.classList.add('flipped');
            });
        } else {
            // Switch to Send
            // Close token dropdown if open
            const tokenSelect = document.getElementById('receive-token-select');
            if (tokenSelect) tokenSelect.classList.remove('open');

            flipCard.classList.add('flipping-to-front');
            flipCard.classList.remove('flipped');

            // Remove transition class after animation completes
            setTimeout(() => {
                flipCard.classList.remove('flipping-to-front');
            }, 350);
        }
    }

    if (toggleFront) toggleFront.addEventListener('click', toggleFlip);
    if (toggleBack) toggleBack.addEventListener('click', toggleFlip);
}

/**
 * Initialize receive tab controls
 */
export function initReceiveTab() {
    // QR click-to-copy
    const qrContainer = document.getElementById('receive-qr-container');
    if (qrContainer) {
        qrContainer.addEventListener('click', () => {
            const uri = buildReceiveUri();
            if (!uri) return;
            navigator.clipboard.writeText(uri).then(() => {
                qrContainer.classList.add('copied');
                setTimeout(() => qrContainer.classList.remove('copied'), 1500);
            });
        });
    }

    // Amount input
    const amountInput = document.getElementById('receive-amount');
    const receiveQuickBtns = document.querySelectorAll('.receive-quick-amount');

    function updateReceiveQuickActive(val) {
        receiveQuickBtns.forEach(b => {
            b.classList.toggle('active', b.dataset.amount === val);
        });
    }

    if (amountInput) {
        amountInput.addEventListener('focus', () => {
            if (amountInput.value === 'Any') {
                amountInput.value = '';
                amountInput.inputMode = 'numeric';
            }
            requestAnimationFrame(repositionReceiveTokenSuffix);
        });
        amountInput.addEventListener('blur', () => {
            if (!amountInput.value.trim()) {
                amountInput.value = 'Any';
                amountInput.inputMode = '';
                updateReceiveQuickActive('any');
                onReceiveFieldChange();
            }
            requestAnimationFrame(repositionReceiveTokenSuffix);
        });
        amountInput.addEventListener('input', () => {
            amountInput.value = amountInput.value.replace(/[^0-9]/g, '');
            updateReceiveQuickActive(amountInput.value);
            onReceiveFieldChange();
            requestAnimationFrame(repositionReceiveTokenSuffix);
        });
    }

    // Quick amount buttons
    receiveQuickBtns.forEach(btn => {
        btn.addEventListener('click', () => {
            if (!amountInput) return;
            if (btn.dataset.amount === 'any') {
                amountInput.value = 'Any';
                amountInput.inputMode = '';
            } else {
                amountInput.value = btn.dataset.amount;
                amountInput.inputMode = 'numeric';
            }
            updateReceiveQuickActive(btn.dataset.amount);
            onReceiveFieldChange();
            requestAnimationFrame(repositionReceiveTokenSuffix);
        });
    });

    // Expiry chips
    initReceiveExpiryChips();

    // Memo chip → simple toggle
    const memoBadge = document.getElementById('receive-memo-badge');
    const memoInputWrap = document.getElementById('receive-memo-input');
    const memoInput = document.getElementById('receive-memo');

    if (memoBadge && memoInputWrap && memoInput) {
        memoBadge.addEventListener('click', () => {
            const isActive = memoBadge.classList.contains('active');
            memoBadge.classList.toggle('active', !isActive);
            memoInputWrap.classList.toggle('visible', !isActive);
            if (!isActive) {
                memoInput.focus({ preventScroll: true });
            }
        });

        memoInput.addEventListener('input', () => {
            onReceiveFieldChange();
        });
    }

    // Custom datetime change
    const customDatetime = document.getElementById('receive-custom-datetime');
    if (customDatetime) {
        customDatetime.addEventListener('change', () => {
            onReceiveFieldChange();
        });
    }
}

/**
 * Initialize receive expiry chips
 */
function initReceiveExpiryChips() {
    // Exclude memo chip from expiry chips
    const chips = document.querySelectorAll('.receive-expiry-chip:not(.receive-memo-chip)');
    const customExpiry = document.getElementById('receive-custom-expiry');
    if (!chips.length) return;

    chips.forEach(chip => {
        chip.addEventListener('click', () => {
            chips.forEach(c => c.classList.remove('active'));
            chip.classList.add('active');

            if (chip.dataset.seconds === 'custom') {
                if (customExpiry) customExpiry.classList.add('visible');
            } else {
                if (customExpiry) customExpiry.classList.remove('visible');
            }
            onReceiveFieldChange();
        });
    });
}

/**
 * Position receive token suffix right after the amount digits
 */
export function repositionReceiveTokenSuffix() {
    const input = document.getElementById('receive-amount');
    const measure = document.getElementById('receive-amount-measure');
    const suffix = document.getElementById('receive-token-suffix');
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
 * Initialize receive token selector dropdown
 */
export function initReceiveTokenSelector() {
    const selectEl = document.getElementById('receive-token-select');
    const dropdown = document.getElementById('receive-token-dropdown');
    const searchInput = document.getElementById('receive-token-search');
    const listEl = document.getElementById('receive-token-list');
    const suffixEl = document.getElementById('receive-token-suffix');
    if (!selectEl || !dropdown || !listEl || !suffixEl) return;

    // Build token list: owned tokens first, then the rest of 0-255
    const ownedSet = new Set();
    const ownedTokens = (carouselState.tokenIds || []).map(tid => {
        const id = parseInt(tid, 10);
        ownedSet.add(id);
        return id;
    });
    const otherTokens = [];
    for (let i = 0; i < 256; i++) {
        if (!ownedSet.has(i)) otherTokens.push(i);
    }

    const allTokens = ownedTokens.concat(otherTokens);
    const dividerAfter = ownedTokens.length > 0 ? ownedTokens.length - 1 : -1;

    function renderList(filter) {
        let html = '';
        const f = (filter || '').toLowerCase();
        let insertedDivider = false;
        for (let j = 0; j < allTokens.length; j++) {
            const tid = allTokens[j];
            const info = getTokenDisplayInfo(tid);
            const searchText = (info.icon + ' ' + info.name + ' ' + tid).toLowerCase();
            if (f && searchText.indexOf(f) === -1) continue;

            if (!insertedDivider && dividerAfter >= 0 && j > dividerAfter) {
                html += '<div class="receive-token-divider"></div>';
                insertedDivider = true;
            }

            const selectedClass = tid === receiveState.selectedTokenId ? ' selected' : '';
            const ownedLabel = ownedSet.has(tid) ? '<span class="tok-owned">owned</span>' : '';
            html += '<div class="receive-token-option' + selectedClass + '" data-token-id="' + tid + '">'
                + '<span class="tok-icon">' + info.icon + '</span>'
                + '<span class="tok-name">' + info.name + '</span>'
                + ownedLabel
                + '</div>';
        }
        if (!html) {
            html = '<div style="padding:0.5rem 0.75rem;color:var(--coins-text-muted);font-size:0.75rem;">No tokens found</div>';
        }
        listEl.innerHTML = html;
    }

    function selectToken(tid) {
        receiveState.selectedTokenId = tid;
        const info = getTokenDisplayInfo(tid);
        if (suffixEl) suffixEl.textContent = `${info.icon} ${info.name}`;
        closeDropdown();
        onReceiveFieldChange();
        requestAnimationFrame(repositionReceiveTokenSuffix);
    }

    function openDropdown() {
        renderList('');
        selectEl.classList.add('open');
        if (searchInput) {
            searchInput.value = '';
            setTimeout(() => searchInput.focus({ preventScroll: true }), 50);
        }
    }

    function closeDropdown() {
        if (!selectEl.classList.contains('open')) return;
        selectEl.classList.remove('open');
    }

    // Click on token suffix opens the dropdown
    suffixEl.addEventListener('click', function(e) {
        e.stopPropagation();
        if (selectEl.classList.contains('open')) {
            closeDropdown();
        } else {
            openDropdown();
        }
    });

    if (searchInput) {
        searchInput.addEventListener('input', function() {
            renderList(searchInput.value);
        });
    }

    listEl.addEventListener('click', function(e) {
        const option = e.target.closest('.receive-token-option');
        if (option && option.dataset.tokenId !== undefined) {
            selectToken(parseInt(option.dataset.tokenId, 10));
        }
    });

    document.addEventListener('click', function(e) {
        if (!selectEl.contains(e.target) && e.target !== suffixEl) {
            closeDropdown();
        }
    });

    // Set initial selection
    const initialToken = ownedTokens.length > 0 ? ownedTokens[0] : 0;
    selectToken(initialToken);

    // Initial suffix positioning
    requestAnimationFrame(repositionReceiveTokenSuffix);
}

/**
 * Handle receive field change (debounced QR update)
 */
function onReceiveFieldChange() {
    clearTimeout(receiveState.qrDebounceTimer);
    receiveState.qrDebounceTimer = setTimeout(() => {
        updateReceiveQR();
    }, 150);
}

/**
 * Build the receive URI from current form state
 * @returns {string|null} The generated URI or null
 */
export function buildReceiveUri() {
    const pk = walletApp.getPublicKey();
    if (!pk) return null;

    // Amount
    const amountEl = document.getElementById('receive-amount');
    const amount = amountEl ? (parseInt(amountEl.value, 10) || null) : null;

    // Token from selector
    const tokenId = receiveState.selectedTokenId;

    // Expiry from chips
    let expires = null;
    const activeChip = document.querySelector('.receive-expiry-chip.active');
    if (activeChip) {
        const seconds = activeChip.dataset.seconds;
        if (seconds === 'custom') {
            const customInput = document.getElementById('receive-custom-datetime');
            if (customInput && customInput.value) {
                expires = Math.floor(new Date(customInput.value).getTime() / 1000);
            }
        } else if (seconds && parseInt(seconds, 10) > 0) {
            expires = Math.floor(Date.now() / 1000) + parseInt(seconds, 10);
        }
    }

    // Memo
    const memoEl = document.getElementById('receive-memo');
    const memo = memoEl ? memoEl.value.trim() || null : null;

    return generateCoinsUri(pk, amount, tokenId, memo, expires);
}

/**
 * Update the receive QR code
 */
export function updateReceiveQR() {
    const canvas = document.getElementById('receive-qr-canvas');
    if (!canvas || typeof QRCode === 'undefined') return;

    const uri = buildReceiveUri();
    if (!uri) return;

    QRCode.toCanvas(canvas, uri, {
        width: 200,
        margin: 2,
        color: { dark: '#0f172a', light: '#f1f5f9' }
    });
}
