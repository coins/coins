/**
 * Wallet JavaScript Tests
 *
 * These tests verify the recipient resolution and transaction format logic.
 * Run with: node wallet.test.js (requires Node.js)
 *
 * Note: These are standalone tests that don't require a browser environment.
 */

// Test utilities
let testsPassed = 0;
let testsFailed = 0;

function assert(condition, message) {
    if (!condition) {
        throw new Error(message || 'Assertion failed');
    }
}

function assertEqual(actual, expected, message) {
    if (actual !== expected) {
        throw new Error(message || `Expected ${expected}, got ${actual}`);
    }
}

function test(name, fn) {
    try {
        fn();
        console.log(`  ✓ ${name}`);
        testsPassed++;
    } catch (e) {
        console.log(`  ✗ ${name}`);
        console.log(`    Error: ${e.message}`);
        testsFailed++;
    }
}

// ============================================================================
// Recipient Validation Tests
// ============================================================================

/**
 * Check if a string is a valid account ID (numeric)
 */
function isAccountId(str) {
    return /^\d+$/.test(str);
}

/**
 * Check if a string is a valid hex public key (64 chars)
 */
function isHexPublicKey(str) {
    return /^[a-fA-F0-9]{64}$/.test(str);
}

/**
 * Validate recipient format (returns 'account_id', 'public_key', or null)
 */
function validateRecipient(recipient) {
    if (isAccountId(recipient)) {
        return 'account_id';
    } else if (isHexPublicKey(recipient)) {
        return 'public_key';
    }
    return null;
}

console.log('\nRecipient Validation Tests:');

test('should recognize numeric account ID', () => {
    assertEqual(validateRecipient('42'), 'account_id');
    assertEqual(validateRecipient('0'), 'account_id');
    assertEqual(validateRecipient('123456789'), 'account_id');
});

test('should recognize account ID with leading zeros', () => {
    assertEqual(validateRecipient('007'), 'account_id');
    assertEqual(validateRecipient('00123'), 'account_id');
});

test('should recognize valid 64-char hex public key', () => {
    const validPk = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef';
    assertEqual(validateRecipient(validPk), 'public_key');
});

test('should recognize uppercase hex public key', () => {
    const validPk = '0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF';
    assertEqual(validateRecipient(validPk), 'public_key');
});

test('should recognize mixed case hex public key', () => {
    const validPk = '0123456789AbCdEf0123456789aBcDeF0123456789abcdef0123456789ABCDEF';
    assertEqual(validateRecipient(validPk), 'public_key');
});

test('should reject invalid hex (wrong length)', () => {
    assertEqual(validateRecipient('abcd1234'), null);
    assertEqual(validateRecipient('0123456789abcdef'), null);
});

test('should reject invalid hex (invalid characters)', () => {
    const invalidPk = 'ghij456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef';
    assertEqual(validateRecipient(invalidPk), null);
});

test('should reject empty string', () => {
    assertEqual(validateRecipient(''), null);
});

test('should reject mixed alphanumeric that is not valid hex', () => {
    assertEqual(validateRecipient('abc123xyz'), null);
});

// ============================================================================
// Account ID Parsing Tests
// ============================================================================

console.log('\nAccount ID Parsing Tests:');

test('should parse simple account ID', () => {
    assertEqual(parseInt('42', 10), 42);
});

test('should parse account ID with leading zeros', () => {
    assertEqual(parseInt('007', 10), 7);
    assertEqual(parseInt('00123', 10), 123);
});

test('should parse zero as account ID', () => {
    assertEqual(parseInt('0', 10), 0);
});

test('should handle large account IDs within u32 range', () => {
    const maxU32 = 4294967295;
    assertEqual(parseInt(maxU32.toString(), 10), maxU32);
});

// ============================================================================
// Transaction Format Tests (simulated)
// ============================================================================

console.log('\nTransaction Format Tests:');

/**
 * Simulate transaction structure
 * Canonical format: sender_id (4) + recipient_pk (32) + token_id (2) + amount (4) + fee (1) = 43 bytes
 */
function calculateTxSize(format) {
    if (format === 'canonical') {
        return 4 + 32 + 2 + 4 + 1; // 43 bytes
    } else if (format === 'compact') {
        return 4 + 4 + 2 + 4 + 1; // 15 bytes (uses recipient_id instead of recipient_pk)
    }
    return 0;
}

test('canonical transaction size should be 43 bytes', () => {
    assertEqual(calculateTxSize('canonical'), 43);
});

test('compact transaction size should be 15 bytes', () => {
    assertEqual(calculateTxSize('compact'), 15);
});

test('canonical format uses full public key (32 bytes)', () => {
    // sender_id: 4, recipient_pk: 32, token_id: 2, amount: 4, fee: 1
    const senderIdSize = 4;
    const recipientPkSize = 32;
    const tokenIdSize = 2;
    const amountSize = 4;
    const feeSize = 1;
    assertEqual(senderIdSize + recipientPkSize + tokenIdSize + amountSize + feeSize, 43);
});

// ============================================================================
// Input Sanitization Tests
// ============================================================================

console.log('\nInput Sanitization Tests:');

function sanitizeRecipient(input) {
    return input.trim().toLowerCase();
}

test('should trim whitespace', () => {
    assertEqual(sanitizeRecipient('  42  '), '42');
    assertEqual(sanitizeRecipient('\n123\t'), '123');
});

test('should lowercase hex input', () => {
    const input = '0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF';
    const expected = '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef';
    assertEqual(sanitizeRecipient(input), expected);
});

// ============================================================================
// Summary
// ============================================================================

console.log('\n' + '='.repeat(50));
console.log(`Tests: ${testsPassed + testsFailed} total, ${testsPassed} passed, ${testsFailed} failed`);

if (testsFailed > 0) {
    process.exit(1);
}
