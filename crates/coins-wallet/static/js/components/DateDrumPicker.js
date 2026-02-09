// Date drum picker component

import { ACTIVITY_PAGE_SIZE } from '../core/constants.js';
import { dateDrumState, activityFilters, setActivityDisplayCount } from '../core/state.js';

// Store render callback
let renderCallback = null;

/**
 * Set callback for rendering transactions
 * @param {Function} callback
 */
export function setDateDrumRenderCallback(callback) {
    renderCallback = callback;
}

/**
 * Initialize the date drum picker
 */
export function initDateDrum() {
    const drum = document.getElementById('date-drum');
    const track = document.getElementById('date-drum-track');
    if (!drum || !track) return;

    const items = track.querySelectorAll('.date-drum-item');
    if (items.length === 0) return;

    dateDrumState.itemHeight = items[0].offsetHeight || 22;

    let isDragging = false;
    let startY = 0;
    let dragOffset = 0;

    updateDrumActive(dateDrumState.selectedIndex);

    function updateDrumActive(index) {
        items.forEach((item, i) => {
            item.classList.toggle('active', i === index);
        });
    }

    function snapToIndex(index) {
        index = Math.max(0, Math.min(items.length - 1, index));
        dateDrumState.selectedIndex = index;
        dateDrumState.currentOffset = -index * dateDrumState.itemHeight;
        track.style.transform = 'translateY(' + dateDrumState.currentOffset + 'px)';
        updateDrumActive(index);

        const dayValue = items[index].dataset.days;
        if (dayValue === 'custom') {
            activityFilters.dateDays = 'custom';
            openDateRangePicker();
        } else {
            activityFilters.dateDays = dayValue;
            activityFilters.customFrom = null;
            activityFilters.customTo = null;
            updateCustomDrumLabel(null, null);
            closeDateRangePicker();
            setActivityDisplayCount(ACTIVITY_PAGE_SIZE);
            if (renderCallback) renderCallback();
        }
    }

    function snapToNearest() {
        const index = Math.round(-dateDrumState.currentOffset / dateDrumState.itemHeight);
        snapToIndex(index);
    }

    function handleDragStart(clientY) {
        isDragging = true;
        startY = clientY;
        dragOffset = dateDrumState.currentOffset;
        drum.classList.add('grabbing');
        track.classList.add('dragging');
    }

    function handleDragMove(clientY) {
        if (!isDragging) return;
        const diff = clientY - startY;
        const maxOffset = 0;
        const minOffset = -(items.length - 1) * dateDrumState.itemHeight;
        dateDrumState.currentOffset = Math.max(minOffset - 20, Math.min(maxOffset + 20, dragOffset + diff));
        track.style.transform = 'translateY(' + dateDrumState.currentOffset + 'px)';

        const previewIndex = Math.round(-dateDrumState.currentOffset / dateDrumState.itemHeight);
        updateDrumActive(Math.max(0, Math.min(items.length - 1, previewIndex)));
    }

    function handleDragEnd() {
        if (!isDragging) return;
        isDragging = false;
        drum.classList.remove('grabbing');
        track.classList.remove('dragging');
        snapToNearest();
    }

    // Mouse events
    drum.addEventListener('mousedown', function(e) {
        e.preventDefault();
        handleDragStart(e.clientY);
    });
    document.addEventListener('mousemove', function(e) {
        handleDragMove(e.clientY);
    });
    document.addEventListener('mouseup', function() {
        handleDragEnd();
    });

    // Touch events
    drum.addEventListener('touchstart', function(e) {
        handleDragStart(e.touches[0].clientY);
    }, { passive: true });
    drum.addEventListener('touchmove', function(e) {
        handleDragMove(e.touches[0].clientY);
    }, { passive: true });
    drum.addEventListener('touchend', function() {
        handleDragEnd();
    });

    // Click on drum opens date range picker
    drum.addEventListener('click', function(e) {
        if (Math.abs(dateDrumState.currentOffset - dragOffset) < 3) {
            openDateRangePicker();
        }
    });

    // Date range picker buttons
    const applyBtn = document.getElementById('date-range-apply');
    const cancelBtn = document.getElementById('date-range-cancel');
    if (applyBtn) {
        applyBtn.addEventListener('click', applyDateRange);
    }
    if (cancelBtn) {
        cancelBtn.addEventListener('click', cancelDateRange);
    }
}

/**
 * Open the date range picker
 */
export function openDateRangePicker() {
    const picker = document.getElementById('date-range-picker');
    if (!picker) return;

    const fromInput = document.getElementById('date-range-from');
    const toInput = document.getElementById('date-range-to');
    if (toInput && !toInput.value) {
        toInput.value = new Date().toISOString().split('T')[0];
    }
    if (fromInput && !fromInput.value) {
        const d = new Date();
        d.setDate(d.getDate() - 30);
        fromInput.value = d.toISOString().split('T')[0];
    }

    picker.style.display = '';
}

/**
 * Close the date range picker
 */
export function closeDateRangePicker() {
    const picker = document.getElementById('date-range-picker');
    if (picker) picker.style.display = 'none';
}

/**
 * Format a short date
 * @param {string} dateStr - Date string in yyyy-mm-dd format
 * @returns {string} Formatted date like "mm/dd"
 */
function formatShortDate(dateStr) {
    if (!dateStr) return '';
    const parts = dateStr.split('-');
    return parseInt(parts[1], 10) + '/' + parseInt(parts[2], 10);
}

/**
 * Update the custom drum label
 * @param {string|null} fromVal - From date
 * @param {string|null} toVal - To date
 */
export function updateCustomDrumLabel(fromVal, toVal) {
    const track = document.getElementById('date-drum-track');
    if (!track) return;
    const items = track.querySelectorAll('.date-drum-item');
    for (let i = 0; i < items.length; i++) {
        if (items[i].dataset.days === 'custom') {
            if (fromVal || toVal) {
                const label = formatShortDate(fromVal) + ' – ' + formatShortDate(toVal);
                items[i].textContent = label;
            } else {
                items[i].textContent = 'Custom...';
            }
            break;
        }
    }
}

/**
 * Apply the date range filter
 */
export function applyDateRange() {
    const fromInput = document.getElementById('date-range-from');
    const toInput = document.getElementById('date-range-to');
    const fromVal = fromInput ? fromInput.value : null;
    const toVal = toInput ? toInput.value : null;

    // Snap drum to "Custom..." item
    const track = document.getElementById('date-drum-track');
    if (track) {
        const items = track.querySelectorAll('.date-drum-item');
        for (let i = 0; i < items.length; i++) {
            if (items[i].dataset.days === 'custom') {
                dateDrumState.selectedIndex = i;
                dateDrumState.currentOffset = -i * dateDrumState.itemHeight;
                track.style.transform = 'translateY(' + dateDrumState.currentOffset + 'px)';
                items.forEach(function(el, idx) {
                    el.classList.toggle('active', idx === i);
                });
                break;
            }
        }
    }

    updateCustomDrumLabel(fromVal, toVal);

    activityFilters.dateDays = 'custom';
    activityFilters.customFrom = fromVal || null;
    activityFilters.customTo = toVal || null;
    setActivityDisplayCount(ACTIVITY_PAGE_SIZE);
    closeDateRangePicker();
    if (renderCallback) renderCallback();
}

/**
 * Cancel the date range picker
 */
export function cancelDateRange() {
    closeDateRangePicker();
    if (activityFilters.dateDays === 'custom' && !activityFilters.customFrom && !activityFilters.customTo) {
        const track = document.getElementById('date-drum-track');
        if (track) {
            dateDrumState.selectedIndex = 0;
            dateDrumState.currentOffset = 0;
            track.style.transform = 'translateY(0px)';
            const items = track.querySelectorAll('.date-drum-item');
            items.forEach(function(el, idx) {
                el.classList.toggle('active', idx === 0);
            });
        }
        updateCustomDrumLabel(null, null);
        activityFilters.dateDays = 'all';
        setActivityDisplayCount(ACTIVITY_PAGE_SIZE);
        if (renderCallback) renderCallback();
    }
}
