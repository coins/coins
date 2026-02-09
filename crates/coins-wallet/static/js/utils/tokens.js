// Token display utilities

// Greek letters for token display
export const greekLetters = ['α', 'β', 'γ', 'δ', 'ε', 'ζ', 'η', 'θ', 'ι', 'κ'];
export const greekNames = ['Alpha', 'Beta', 'Gamma', 'Delta', 'Epsilon', 'Zeta', 'Eta', 'Theta', 'Iota', 'Kappa'];

/**
 * Get display information for a token ID
 * @param {number|string} id - Token ID
 * @returns {{icon: string, name: string}} Token display info
 */
export function getTokenDisplayInfo(id) {
    const idx = parseInt(id, 10);
    const letter = greekLetters[idx];
    const name = greekNames[idx];
    if (letter && name) {
        return { icon: letter, name: name };
    }
    return { icon: '#', name: String(idx) };
}

/**
 * Get full token label (icon + name) for a token ID
 * @param {number|string} tokenId - Token ID
 * @returns {string} Formatted token label like "α Alpha"
 */
export function getTokenLabel(tokenId) {
    const info = getTokenDisplayInfo(tokenId);
    return `${info.icon} ${info.name}`;
}
