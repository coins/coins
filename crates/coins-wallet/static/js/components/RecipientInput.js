// Recipient input with autocomplete suggestions

import { currentTransactions } from '../core/state.js';
import { escapeHtml } from '../utils/dom.js';

/**
 * Initialize recipient input suggestions
 */
export function initRecipientSuggestions() {
    const input = document.getElementById('send-recipient');
    const dropdown = document.getElementById('recipient-suggestions');
    if (!input || !dropdown) return;

    function buildCandidates(query) {
        const q = query.toLowerCase();
        const results = [];
        const seen = new Set();

        // Previous recipients from transaction history
        const txs = currentTransactions || [];
        for (const tx of txs) {
            if (tx.direction === 'outgoing' && tx.recipient_pk) {
                const addr = tx.recipient_pk;
                if (seen.has(addr)) continue;
                if (addr.toLowerCase().includes(q)) {
                    seen.add(addr);
                    results.push({ label: 'Previous recipient', address: addr });
                }
            }
        }

        return results.slice(0, 5);
    }

    function renderSuggestions(candidates) {
        if (candidates.length === 0) {
            dropdown.classList.remove('open');
            dropdown.innerHTML = '';
            return;
        }
        dropdown.innerHTML = candidates.map(c => `
            <div class="recipient-suggestion" data-address="${escapeHtml(c.address)}">
                <span class="suggestion-label">${escapeHtml(c.label)}</span>
                <span class="suggestion-address">${c.address.slice(0, 12)}...${c.address.slice(-6)}</span>
            </div>
        `).join('');
        dropdown.classList.add('open');

        dropdown.querySelectorAll('.recipient-suggestion').forEach(el => {
            el.addEventListener('mousedown', (e) => {
                e.preventDefault();
                input.value = el.getAttribute('data-address');
                dropdown.classList.remove('open');
                dropdown.innerHTML = '';
            });
        });
    }

    input.addEventListener('input', () => {
        const val = input.value.trim();
        if (val.length < 1) {
            dropdown.classList.remove('open');
            dropdown.innerHTML = '';
            return;
        }
        renderSuggestions(buildCandidates(val));
    });

    input.addEventListener('blur', () => {
        setTimeout(() => {
            dropdown.classList.remove('open');
            dropdown.innerHTML = '';
        }, 150);
    });

    input.addEventListener('keydown', (e) => {
        if (e.key === 'Escape') {
            dropdown.classList.remove('open');
            dropdown.innerHTML = '';
        }
    });
}
