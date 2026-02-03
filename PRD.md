# Product Requirements Documents

## PRD #1: Token Info & Max Selection in Send Box

**Summary:** Enhance the send box amount selector to display information about the currently selected token and enforce a maximum selection limit.

**Problem:** Users currently lack visibility into which token they're sending and how much they can send, which could lead to errors or confusion during transaction creation.

**Requirements:**
1. Display the selected token's name/ID and relevant metadata (e.g., balance) in or near the amount selector
2. Show the user's maximum available balance for the selected token
3. Provide a "Max" button or similar affordance to auto-fill the maximum sendable amount
4. Validate that the entered amount does not exceed the available balance
5. Style consistently with the existing wallet UI

**Acceptance Criteria:**
- Selected token info is visible when composing a send transaction
- User can see their max available balance for that token
- User can quickly select the max amount
- Amount entry is validated against the max

---

## PRD #2: Explorer Style Alignment with Wallet

**Summary:** Restyle the explorer page to match the modern visual style of the wallet page, ensuring a consistent look and feel across the application.

**Problem:** The explorer and wallet pages currently have divergent visual styles, creating an inconsistent user experience.

**Requirements:**
1. Audit the wallet page's current design language (colors, typography, spacing, component styles, layout patterns)
2. Apply the same shared style system to the explorer page
3. Ensure shared components (buttons, cards, tables, headers) use identical styling
4. Maintain all existing explorer functionality — this is a visual-only change
5. Responsive behavior should match the wallet page's approach

**Acceptance Criteria:**
- Explorer page visually matches the wallet page's design language
- No explorer functionality is lost or broken
- Shared UI components are reused where possible (leveraging any existing shared style modules)
- Works correctly at the same breakpoints as the wallet page

---

## PRD #3: Activity Pagination (Load More)

**Summary:** Add incremental loading to the Activity box to limit the number of entries shown at once, improving performance and usability.

**Problem:** When the activity list grows large, showing all entries at once hurts readability and potentially performance. Users need a way to browse through history in manageable chunks.

**Requirements:**
1. Limit the activity list to a configurable number of entries per page (e.g., 20 or 25)
2. Provide a "Load more" button at the bottom that appends the next batch of entries
3. Show the current loaded count relative to total entries
4. Maintain sort order (most recent first) across loaded batches
5. Style the "Load more" button consistently with the wallet UI

**Acceptance Criteria:**
- Activity list shows only N entries initially
- User can load older entries via "Load more" button
- "Load more" button is styled consistently with the rest of the wallet
- Performance is acceptable even with a large transaction history

---

## PRD #4: Broadcast Timer Format Fix

**Summary:** Fix the broadcast countdown timer formatting and improve its end-state behavior.

**Problem:** The timer currently displays "Broadcast in8s" (missing space) and shows "0s" before transitioning to the next state, which looks unpolished.

**Requirements:**
1. Add a space between "in" and the number: "Broadcast in 8s" (not "Broadcast in8s")
2. Stop the countdown at 1s — do not display "0s"
3. When the countdown would reach 0, transition to displaying "Broadcasting" (or the next appropriate state)
4. Ensure the transition from countdown to "Broadcasting" is smooth (no flicker or missing frame)

**Acceptance Criteria:**
- Timer reads "Broadcast in Xs" with proper spacing for all values
- Timer counts down to "Broadcast in 1s", then switches to "Broadcasting"
- "0s" is never shown to the user
- State transition is visually clean

---

## PRD #5: Transaction Information Page Fix

**Summary:** Fix the broken transaction detail page that should appear when clicking a transaction in the activity box, and add the Bitcoin block timestamp.

**Problem:** Clicking on a transaction in the activity list does not currently show the transaction information page (it's broken). Additionally, when working, it should display the timestamp of the Bitcoin block containing the transaction.

**Requirements:**
1. Debug and fix the click handler / routing so that clicking a transaction in the activity box navigates to or opens its detail view
2. The transaction detail page should display all relevant transaction information (amount, token, sender/recipient, status, tx ID, etc.)
3. Add a field showing the timestamp of the Bitcoin block that the transaction was included in
4. Format the block timestamp in a human-readable format (e.g., "Jan 15, 2026, 3:42 PM" or similar)
5. Style the transaction detail page consistently with the wallet UI

**Acceptance Criteria:**
- Clicking a transaction in the activity list opens its detail view
- Transaction detail view shows all relevant transaction metadata
- Bitcoin block timestamp is displayed on the detail view
- Page is styled consistently with the rest of the wallet

---

## PRD #6: Address Book

**Summary:** Add an address book feature for the recipient address field in the send flow, allowing users to save, manage, and quickly select frequently-used addresses.

**Problem:** Users must manually enter or paste recipient addresses each time they send a transaction. This is error-prone and inefficient for repeat recipients.

**Requirements:**
1. Provide an address book UI accessible from or integrated into the recipient address field in the send box
2. Users can add a new contact (label/name + address)
3. Users can edit or delete existing contacts
4. Users can select a contact from the address book to auto-fill the recipient field
5. Address book data is persisted locally (localStorage or similar)
6. Search/filter within the address book when the list grows
7. Style in the same modern design language as the rest of the wallet page

**Acceptance Criteria:**
- Address book is accessible from the send flow's recipient field
- Users can CRUD (create, read, update, delete) contacts
- Selecting a contact fills the recipient address field
- Data persists across sessions
- UI matches the wallet's modern style

---

## PRD #7: Invoice / Payment Request Format

**Summary:** Create an invoice/payment request format that users can share with others to pre-fill transaction details, supporting both QR code and text-based sharing.

**Problem:** Currently there's no way for a user to request a specific payment from another user. Recipients must manually communicate amounts and token details out-of-band, which is error-prone.

### Invoice Schema

- **Required:** Recipient address
- **Optional:** Amount, Token ID, Memo/description, Expiration timestamp

### Format

Custom URI scheme using `coins://`:
- Example: `coins://pay?addr=abc123&amount=100&token=xyz&memo=Coffee&expires=1738600000`
- Parameters are URL-encoded, optional fields can be omitted

### Sharing

- Generate a QR code encoding the `coins://` URI
- Copyable text string of the URI (shareable like an account number / payment link)

### Receiving Side

- When a wallet scans a QR code or pastes a `coins://` URI, the send form is pre-filled with the decoded fields
- No invoice status tracking — it's a fire-and-forget pre-fill template

### Expiration

- Optional. If set by the creator, the wallet should warn or block usage of an expired invoice
- If not set, the invoice is valid indefinitely

### Cross-Client Compatibility

- The `coins://` URI format must be supported by both the web wallet UI and the `coins-client` CLI tool
- `coins-client` should be able to:
  - **Generate** an invoice from the command line (e.g., `coins-client invoice create --addr=... --amount=... --token=...`) and output the `coins://` URI string
  - **Parse** a `coins://` URI passed as an argument and pre-fill / execute a send transaction (e.g., `coins-client send --invoice "coins://pay?addr=..."`)
- The URI parsing/generation logic should live in shared code that both the web wallet and CLI can use

### Testing

- Write comprehensive tests for the invoice module covering:
  - URI generation: correct `coins://` URI output for all combinations of optional/required fields
  - URI parsing: correctly extracts all fields from a valid `coins://` URI
  - Missing optional fields: parsing succeeds with only the required address field
  - Expiration validation: expired invoices are correctly detected and flagged
  - Invalid URIs: malformed strings, missing required fields, bad parameter values are rejected with clear errors
  - Round-trip: generate an invoice -> serialize to URI -> parse back -> all fields match
  - Cross-client: same URI is parsed identically by both the web wallet and `coins-client` CLI
- Tests should be automated and runnable in CI

### UI

- "Request Payment" flow to create and share an invoice
- Scan/import flow to receive an invoice and pre-fill the send form
- Styled consistently with the wallet UI

**Acceptance Criteria:**
- User can create an invoice with at minimum their address
- Invoice is rendered as a QR code and a copyable `coins://` URI string
- Another user can scan/paste the invoice and get the send form pre-filled
- Expired invoices show a warning
- No payment status tracking needed
- Works in both web wallet and `coins-client` CLI
- Comprehensive test suite covers all invoice module requirements

---

## PRD #8: Activity Box Filters (Token + Date)

**Summary:** Add filtering capabilities to the activity box, allowing users to filter transactions by token and by date range.

**Problem:** As transaction history grows, users need ways to narrow down the activity list to find specific transactions — either by token type or time period.

### Token Filter

1. Add a filter control to the activity box that allows selecting a specific token
2. When a token is selected, only transactions involving that token are shown
3. Provide an "All tokens" option to clear the filter and show everything
4. The filter should show available tokens based on the user's transaction history

### Date Filter

1. Add a date range filter with preset options:
   - Last 7 days
   - Last 30 days
   - Last 90 days
   - Last 180 days
   - Last 1 year
   - All time
2. Optionally allow custom date range selection (start date / end date)
3. Token and date filters should work in combination (AND logic)

### UI

- Filter controls should be compact and not clutter the activity box header
- Active filters should be clearly indicated (e.g., highlighted filter chip or badge)
- Styled consistently with the wallet UI
- Filters should work with the "Load more" pagination from PRD #3

**Acceptance Criteria:**
- User can filter activity by a specific token
- User can filter activity by date range presets
- Filters combine (token AND date range)
- Active filters are visually indicated
- Clearing filters restores the full activity list
- Works correctly with "Load more" pagination

---

## Post-Completion: Development Documentation

After all 8 PRDs above are implemented, document the development setup in `CLAUDE.md`:
- How to start all services for mutinynet and regtest
- How to send transactions
- How to run the tests
