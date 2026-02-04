
- WebSocket refresh causes visual flash: the balances tab redraws on every WebSocket
update even when nothing changed. If data hasn't changed, skip the re-render so the
user doesn't see the page flash/reload.

- Send box token amount selector redesign: move the token indicator from the top-left
corner to after the amount input as a dimmed currency suffix (same style as the "10",
"100", ... quick-amount buttons). The "Max" button should match that format and sit
inline with the quick-amount buttons. Show the available balance in the amount selector
after the currency label (currency in accent color, available amount in default color)
and remove it from the top-right corner where it currently is.

- Recipient field fuzzy search and contacts: add fuzzy search that matches against
addresses we've previously sent to, known account numbers (e.g. typing "3" suggests
account 3), and saved contact names. After a successful send and the coins animation
finishes, prompt the user to save the recipient as a contact (with a name input). Only
show this prompt when the address is not already in contacts.

- Receive box layout: move the QR code below the fields. Place the Amount, Token,
Expires, and Memo fields next to each other horizontally if possible.
