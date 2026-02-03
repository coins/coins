- Add selected token info and max selection to send box amount selector
- Have explorer look same style as wallet
- Limit Activity to X entries per page and then have pages to click through or a load
more button
- Change timer format to have a space between in and the number of seconds -> currently looks like "Broadcast in8s" but should look like "Broadcast in 8s". Also stop at 1s and then go to next state and do not show 0s. Maybe at 0s show "Broadcasting".
- Fix the transaction information page not showing when clicking on a tx in the
activity box and include the timestamp of the bitcoin bock that this transation was in.
- Add an address book for the recipient Address field in the same modern style as the rest of the wallet page.
- Create an invoice format to be able to share to other users to prefill the amount and
token id and so on to send a tx. Ask the user what it should include and how it should work. QR code would be nice but also shareable via string so e.g. account number.
- Add a filter to activity box to show selected token only. and to filter by date interval or day. e.g. last 180 days. 1 year ago etc.

- After finishing all of the above todos take the claude session and ask it what it learned about setting up development (how to start all services and so on for mutinynet and regtest) and how to send transactions and run the tests and ask it to keep that in persistent storage (so i assume include in CLAUDE.md of the project)
