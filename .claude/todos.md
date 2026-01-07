# TODOs

- run tests on signet [in progress]
- run tests on mainnet
- build a web-client

### Compression
- compactTx (recipient id instead of pubkey)
- compact aggregator key in subblock header
- allow for annex subblocks
- get rid of nonce?

### More features
- handle reorgs
- support for multiple tokens
- swap TXs?
- mint coins 
	- by burning BTC? 
	- by time-locking BTC?
- Tokens: USDT, USDC, XAUT, ETH ? 
- Ethereum Bridge
- Lending/borrowing 
- AMM 
- Bitcoin Naming System
- Instant finality? 



## Done 
- rename aggregator to "publisher"
- clean everything "Esplora" from the entire project 
- fix crypto library
- subchain should contain a genesis height to have a start for scanning
- compress subchain serialization (reduce to params + list of 64 byte sigs)


## Don't do
- indexer should use ZeroMQ?
