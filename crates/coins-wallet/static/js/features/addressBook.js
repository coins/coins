// Address book functionality

import { ADDRESSBOOK_KEY } from '../core/constants.js';
import { escapeHtml } from '../utils/dom.js';

/**
 * Get address book contacts from localStorage
 * @returns {Array<{label: string, address: string}>} Array of contacts
 */
export function getAddressBook() {
    try {
        return JSON.parse(localStorage.getItem(ADDRESSBOOK_KEY)) || [];
    } catch {
        return [];
    }
}

/**
 * Save address book contacts to localStorage
 * @param {Array<{label: string, address: string}>} contacts - Array of contacts
 */
export function saveAddressBook(contacts) {
    localStorage.setItem(ADDRESSBOOK_KEY, JSON.stringify(contacts));
}

/**
 * Render the address book list
 * @param {string} filter - Optional filter string
 */
export function renderAddressBook(filter = '') {
    const listEl = document.getElementById('addressbook-list');
    if (!listEl) return;

    let contacts = getAddressBook();
    if (filter) {
        const f = filter.toLowerCase();
        contacts = contacts.filter(c =>
            c.label.toLowerCase().includes(f) ||
            c.address.toLowerCase().includes(f)
        );
    }

    if (contacts.length === 0) {
        listEl.innerHTML = `<div class="addressbook-empty">${filter ? 'No matching contacts' : 'No contacts yet'}</div>`;
        return;
    }

    listEl.innerHTML = contacts.map((c, i) => `
        <div class="addressbook-contact" data-index="${i}" data-address="${c.address}">
            <div class="addressbook-contact-info">
                <div class="addressbook-contact-label">${escapeHtml(c.label)}</div>
                <div class="addressbook-contact-addr">${c.address}</div>
            </div>
            <div class="addressbook-contact-actions">
                <button class="button is-danger is-light is-small addressbook-delete-btn" data-index="${i}" title="Delete">Del</button>
            </div>
        </div>
    `).join('');

    // Wire up select handlers (click on contact info)
    listEl.querySelectorAll('.addressbook-contact-info').forEach(el => {
        el.addEventListener('click', () => {
            const contact = el.closest('.addressbook-contact');
            const addr = contact.getAttribute('data-address');
            const recipientInput = document.getElementById('send-recipient');
            if (recipientInput) recipientInput.value = addr;
            document.getElementById('addressbook-modal').classList.remove('is-active');
        });
    });

    // Wire up delete handlers
    listEl.querySelectorAll('.addressbook-delete-btn').forEach(btn => {
        btn.addEventListener('click', (e) => {
            e.stopPropagation();
            const idx = parseInt(btn.getAttribute('data-index'), 10);
            const contacts = getAddressBook();
            contacts.splice(idx, 1);
            saveAddressBook(contacts);
            renderAddressBook(document.getElementById('addressbook-search')?.value || '');
        });
    });
}

/**
 * Initialize address book modal and event handlers
 */
export function initAddressBook() {
    // Open button
    const openBtn = document.getElementById('open-addressbook-btn');
    if (openBtn) {
        openBtn.addEventListener('click', () => {
            document.getElementById('addressbook-modal').classList.add('is-active');
            renderAddressBook();
        });
    }

    // Close buttons
    const closeModal = document.getElementById('close-addressbook-modal');
    const closeBtn = document.getElementById('close-addressbook-btn');
    const modalBg = document.querySelector('#addressbook-modal .modal-background');
    [closeModal, closeBtn, modalBg].forEach(el => {
        if (el) el.addEventListener('click', () => {
            document.getElementById('addressbook-modal').classList.remove('is-active');
        });
    });

    // Search
    const searchInput = document.getElementById('addressbook-search');
    if (searchInput) {
        searchInput.addEventListener('input', () => {
            renderAddressBook(searchInput.value);
        });
    }

    // Add contact
    const addBtn = document.getElementById('addressbook-add-btn');
    if (addBtn) {
        addBtn.addEventListener('click', () => {
            const labelInput = document.getElementById('addressbook-new-label');
            const addrInput = document.getElementById('addressbook-new-address');
            const errorEl = document.getElementById('addressbook-error');

            const label = labelInput.value.trim();
            const address = addrInput.value.trim();

            if (!label || !address) {
                if (errorEl) {
                    errorEl.textContent = 'Both label and address are required';
                    errorEl.style.display = 'block';
                }
                return;
            }

            if (!/^[0-9a-fA-F]+$/.test(address)) {
                if (errorEl) {
                    errorEl.textContent = 'Address must be a hex string';
                    errorEl.style.display = 'block';
                }
                return;
            }

            const contacts = getAddressBook();
            contacts.push({ label, address });
            saveAddressBook(contacts);

            labelInput.value = '';
            addrInput.value = '';
            if (errorEl) errorEl.style.display = 'none';
            renderAddressBook();
        });
    }
}
