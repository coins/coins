// Balance carousel component with drag/snap and morphing sizes

import { carouselState } from '../core/state.js';
import { greekLetters, greekNames } from '../utils/tokens.js';

// Store update callback
let updateSendTokenInfoCallback = null;
let resizeTimeout = null;

/**
 * Set callback for updating send token info
 * @param {Function} callback
 */
export function setUpdateSendTokenInfoCallback(callback) {
    updateSendTokenInfoCallback = callback;
}

/**
 * Initialize resize handler to re-center carousel on window resize
 */
export function initCarouselResizeHandler() {
    window.addEventListener('resize', () => {
        clearTimeout(resizeTimeout);
        resizeTimeout = setTimeout(() => {
            recenterCarousel();
        }, 100);
    });
}

/**
 * Re-center the carousel on the currently selected item
 */
export function recenterCarousel() {
    const balanceScroll = document.getElementById('balance-scroll');
    const balanceTokens = document.getElementById('balance-tokens');
    if (!balanceScroll || !balanceTokens) return;

    const items = balanceTokens.querySelectorAll('.balance-token-item');
    if (items.length === 0) return;

    const selectedIndex = carouselState.selectedIndex || 0;
    if (selectedIndex >= items.length) return;

    const containerWidth = balanceScroll.clientWidth;
    const positions = Array.from(items).map(item => item.offsetLeft);
    const itemWidths = Array.from(items).map(item => item.offsetWidth);

    const selectedPos = positions[selectedIndex] || 0;
    const selectedWidth = itemWidths[selectedIndex] || 100;
    const centerOffset = (containerWidth / 2) - selectedPos - (selectedWidth / 2);

    carouselState.currentOffset = centerOffset;

    // Apply without transition for immediate response
    balanceTokens.style.transition = 'none';
    balanceTokens.style.transform = `translateX(${centerOffset}px)`;

    // Re-enable transition after reflow
    requestAnimationFrame(() => {
        balanceTokens.style.transition = '';
    });
}

/**
 * Initialize the balance carousel
 * @param {number} initialIndex - Index to center initially
 */
export function initBalanceCarousel(initialIndex = 0) {
    const balanceScroll = document.getElementById('balance-scroll');
    const balanceTokens = document.getElementById('balance-tokens');
    if (!balanceTokens || !balanceScroll) return;

    const items = balanceTokens.querySelectorAll('.balance-token-item');
    if (items.length === 0) return;

    // Store the selected index
    carouselState.selectedIndex = initialIndex;

    // First, set sizes based on selection
    items.forEach((item, index) => {
        const amountEl = item.querySelector('.balance-token-amount');
        const nameEl = item.querySelector('.balance-token-name');
        if (!amountEl || !nameEl) return;

        if (index === initialIndex) {
            amountEl.style.fontSize = '2.5rem';
            amountEl.style.color = 'var(--coins-accent)';
            nameEl.style.fontSize = '1rem';
            nameEl.style.color = 'var(--coins-accent)';
            item.style.opacity = '1';
        } else {
            amountEl.style.fontSize = '1.25rem';
            amountEl.style.color = 'var(--coins-text)';
            nameEl.style.fontSize = '0.75rem';
            nameEl.style.color = 'var(--coins-text-secondary)';
            item.style.opacity = '0.7';
        }
        item.style.visibility = 'visible';
    });

    // Force a reflow to get accurate measurements
    balanceTokens.offsetHeight;

    // Calculate positions with correct sizes
    const containerWidth = balanceScroll.clientWidth;
    const positions = Array.from(items).map(item => item.offsetLeft);
    const itemWidths = Array.from(items).map(item => item.offsetWidth);

    // Center position
    const selectedPos = positions[initialIndex] || 0;
    const selectedWidth = itemWidths[initialIndex] || 100;
    const centerOffset = (containerWidth / 2) - selectedPos - (selectedWidth / 2);

    carouselState.currentOffset = centerOffset;
    balanceTokens.style.transform = `translateX(${centerOffset}px)`;
}

/**
 * Update carousel item sizes based on selection
 * @param {number} selectedIndex - Currently selected item index
 */
export function updateCarouselSizes(selectedIndex) {
    const balanceTokens = document.getElementById('balance-tokens');
    if (!balanceTokens) return;

    const items = balanceTokens.querySelectorAll('.balance-token-item');
    items.forEach((item, index) => {
        const amountEl = item.querySelector('.balance-token-amount');
        const nameEl = item.querySelector('.balance-token-name');
        if (!amountEl || !nameEl) return;

        if (index === selectedIndex) {
            amountEl.style.fontSize = '2.5rem';
            amountEl.style.color = 'var(--coins-accent)';
            nameEl.style.fontSize = '1rem';
            nameEl.style.color = 'var(--coins-accent)';
            item.style.opacity = '1';
        } else {
            amountEl.style.fontSize = '1.25rem';
            amountEl.style.color = 'var(--coins-text)';
            nameEl.style.fontSize = '0.75rem';
            nameEl.style.color = 'var(--coins-text-secondary)';
            item.style.opacity = '0.7';
        }
        item.style.visibility = 'visible';
    });

    // Validate send amount against new selection's max balance
    const sendAmountEl = document.getElementById('send-amount');
    if (sendAmountEl) {
        const tokenIds = carouselState.tokenIds;
        const balances = carouselState.balances;
        if (tokenIds.length > 0) {
            const tokenId = tokenIds[selectedIndex];
            const maxBalance = balances[tokenId] || 0;
            const currentVal = parseInt(sendAmountEl.value.replace(/,/g, '')) || 0;
            if (currentVal > maxBalance) {
                sendAmountEl.value = maxBalance;
            }
        }
    }

    // Update send token info display
    if (updateSendTokenInfoCallback) updateSendTokenInfoCallback();
}

/**
 * Initialize carousel drag handlers
 */
export function initCarouselDragHandlers() {
    const balanceScroll = document.getElementById('balance-scroll');
    const balanceTokens = document.getElementById('balance-tokens');
    if (!balanceScroll || !balanceTokens) return;

    let isDragging = false;
    let startX = 0;
    let dragOffset = 0;

    function updateTokenSizes(offset) {
        const items = balanceTokens.querySelectorAll('.balance-token-item');
        const positions = Array.from(items).map(item => item.offsetLeft);
        const widths = Array.from(items).map(item => item.offsetWidth);

        const balanceHero = balanceScroll.closest('.balance-hero');
        const heroRect = balanceHero ? balanceHero.getBoundingClientRect() : null;
        const scrollRect = balanceScroll.getBoundingClientRect();
        const containerWidth = heroRect ? (heroRect.right - scrollRect.left) : balanceScroll.clientWidth;
        const containerCenter = balanceScroll.clientWidth / 2;

        let closestIndex = 0;
        let closestDist = Infinity;

        for (let i = 0; i < positions.length; i++) {
            const itemCenter = positions[i] + offset + (widths[i] / 2);
            const dist = Math.abs(itemCenter - containerCenter);
            if (dist < closestDist) {
                closestDist = dist;
                closestIndex = i;
            }
        }

        items.forEach((item, index) => {
            const itemPos = positions[index] || 0;
            const itemWidth = widths[index] || 100;

            const visibleLeft = itemPos + offset;
            const visibleRight = visibleLeft + itemWidth;
            const isClippedRight = visibleRight > containerWidth;

            item.style.visibility = isClippedRight ? 'hidden' : 'visible';

            const amountEl = item.querySelector('.balance-token-amount');
            const nameEl = item.querySelector('.balance-token-name');
            if (!amountEl || !nameEl) return;

            const itemCenter = itemPos + offset + (itemWidth / 2);
            const distFromCenter = Math.abs(itemCenter - containerCenter);
            const normalizedDist = distFromCenter / 150;

            if (index === closestIndex && normalizedDist < 0.5) {
                const scale = 1 - normalizedDist * 0.4;
                amountEl.style.fontSize = `${2.5 * scale}rem`;
                amountEl.style.color = 'var(--coins-accent)';
                nameEl.style.fontSize = `${1 * scale}rem`;
                nameEl.style.color = 'var(--coins-accent)';
                item.style.opacity = '1';
            } else {
                amountEl.style.fontSize = '1.25rem';
                amountEl.style.color = 'var(--coins-text)';
                nameEl.style.fontSize = '0.75rem';
                nameEl.style.color = 'var(--coins-text-secondary)';
                item.style.opacity = '0.7';
            }
        });
    }

    function getItemPositions() {
        const items = balanceTokens.querySelectorAll('.balance-token-item');
        return Array.from(items).map(item => item.offsetLeft);
    }

    function getItemWidths() {
        const items = balanceTokens.querySelectorAll('.balance-token-item');
        return Array.from(items).map(item => item.offsetWidth);
    }

    function snapToNearest() {
        const positions = getItemPositions();
        const widths = getItemWidths();
        if (positions.length === 0) return;

        const containerWidth = balanceScroll.clientWidth;
        const containerCenter = containerWidth / 2;

        let closestIndex = 0;
        let closestDist = Infinity;

        for (let i = 0; i < positions.length; i++) {
            const itemCenter = positions[i] + carouselState.currentOffset + (widths[i] / 2);
            const dist = Math.abs(itemCenter - containerCenter);
            if (dist < closestDist) {
                closestDist = dist;
                closestIndex = i;
            }
        }

        const selectedPos = positions[closestIndex];
        const selectedWidth = widths[closestIndex];
        const centerOffset = containerCenter - selectedPos - (selectedWidth / 2);

        carouselState.currentOffset = centerOffset;
        carouselState.selectedIndex = closestIndex;

        balanceTokens.style.transform = `translateX(${centerOffset}px)`;
        updateCarouselSizes(closestIndex);
    }

    function handleDragStart(clientX) {
        isDragging = true;
        startX = clientX;
        dragOffset = carouselState.currentOffset;
        balanceScroll.classList.add('grabbing');
        balanceTokens.classList.add('dragging');
    }

    function handleDragMove(clientX) {
        if (!isDragging) return;

        const diff = clientX - startX;
        const positions = getItemPositions();
        const widths = getItemWidths();
        const containerWidth = balanceScroll.clientWidth;
        const containerCenter = containerWidth / 2;

        const firstItemCenter = positions[0] + (widths[0] / 2);
        const lastItemCenter = positions[positions.length - 1] + (widths[widths.length - 1] / 2);
        const maxOffset = containerCenter - firstItemCenter;
        const minOffset = containerCenter - lastItemCenter;

        carouselState.currentOffset = Math.max(minOffset - 50, Math.min(maxOffset + 50, dragOffset + diff));
        balanceTokens.style.transform = `translateX(${carouselState.currentOffset}px)`;
        updateTokenSizes(carouselState.currentOffset);
    }

    function handleDragEnd() {
        if (!isDragging) return;
        isDragging = false;
        balanceScroll.classList.remove('grabbing');
        balanceTokens.classList.remove('dragging');
        snapToNearest();
    }

    // Mouse events
    balanceScroll.addEventListener('mousedown', (e) => {
        e.preventDefault();
        handleDragStart(e.clientX);
    });

    document.addEventListener('mousemove', (e) => {
        handleDragMove(e.clientX);
    });

    document.addEventListener('mouseup', () => {
        handleDragEnd();
    });

    // Touch events
    balanceScroll.addEventListener('touchstart', (e) => {
        handleDragStart(e.touches[0].clientX);
    }, { passive: true });

    balanceScroll.addEventListener('touchmove', (e) => {
        handleDragMove(e.touches[0].clientX);
    }, { passive: true });

    balanceScroll.addEventListener('touchend', () => {
        handleDragEnd();
    });
}

/**
 * Update balance display with account data
 * @param {Object|null} account - Account data or null for new wallet
 */
export function updateBalanceDisplay(account) {
    const balanceTokensEl = document.getElementById('balance-tokens');
    const balanceAccountEl = document.getElementById('balance-account');
    const balanceEl = document.getElementById('balance-display');
    const tokenBalancesCard = document.getElementById('token-balances-card');

    if (account === null) {
        if (balanceTokensEl) {
            balanceTokensEl.innerHTML = `
                <div class="balance-token-item">
                    <div class="balance-token-amount">0</div>
                    <div class="balance-token-name">New wallet</div>
                </div>
            `;
            setTimeout(() => {
                initBalanceCarousel(0);
            }, 50);
        }
        if (balanceAccountEl) balanceAccountEl.textContent = '';
        if (tokenBalancesCard) tokenBalancesCard.style.display = 'none';
        carouselState.tokenIds = ['0'];
        carouselState.balances = { '0': 0 };
        return;
    }

    const balances = account.balances || {};
    const tokenIds = Object.keys(balances)
        .filter(id => (balances[id] || 0) > 0)
        .sort((a, b) => parseInt(a) - parseInt(b));

    carouselState.tokenIds = tokenIds;
    carouselState.balances = balances;

    if (balanceAccountEl) balanceAccountEl.textContent = `Account #${account.id}`;

    // Handle no balances case
    if (tokenIds.length === 0) {
        if (balanceTokensEl) {
            balanceTokensEl.innerHTML = `
                <div class="balance-empty-message">
                    <div class="balance-empty-text">No balances yet</div>
                </div>
            `;
            balanceTokensEl.style.transform = '';
        }
        if (tokenBalancesCard) tokenBalancesCard.style.display = 'none';
        if (updateSendTokenInfoCallback) updateSendTokenInfoCallback();
        return;
    }

    // Find the token with the largest balance
    let largestBalance = 0;
    tokenIds.forEach((id) => {
        if ((balances[id] || 0) > largestBalance) {
            largestBalance = balances[id];
        }
    });

    const largestIndices = [];
    tokenIds.forEach((id, index) => {
        if ((balances[id] || 0) === largestBalance) {
            largestIndices.push(index);
        }
    });

    const middleIndex = (tokenIds.length - 1) / 2;
    let largestBalanceIndex = largestIndices[0];
    let closestToMiddle = Math.abs(largestIndices[0] - middleIndex);

    for (const idx of largestIndices) {
        const distFromMiddle = Math.abs(idx - middleIndex);
        if (distFromMiddle < closestToMiddle) {
            closestToMiddle = distFromMiddle;
            largestBalanceIndex = idx;
        }
    }

    if (balanceTokensEl) {
        let html = '';
        for (const tokenId of tokenIds) {
            const balance = balances[tokenId] || 0;
            const idx = parseInt(tokenId);
            const letter = greekLetters[idx] || `#${tokenId}`;
            const name = greekNames[idx] || `Token ${tokenId}`;
            html += `
                <div class="balance-token-item" data-token-id="${tokenId}">
                    <div class="balance-token-amount">${balance.toLocaleString()}</div>
                    <div class="balance-token-name">${letter} ${name}</div>
                </div>
            `;
        }
        balanceTokensEl.innerHTML = html;

        // Set selected index immediately so token info is in sync
        carouselState.selectedIndex = largestBalanceIndex;

        setTimeout(() => {
            initBalanceCarousel(largestBalanceIndex);
        }, 50);
    }

    if (tokenBalancesCard) tokenBalancesCard.style.display = 'none';

    if (updateSendTokenInfoCallback) updateSendTokenInfoCallback();
}
