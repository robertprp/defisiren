# Tornado Cash pool coverage

Registry review: 2026-11-05. `config/tornado.v1.json` is shared by Rust ingestion
and the dashboard's chain-scoped labels. A pool address is never treated as a label
on another chain merely because the address bytes match.

## Sources

- [Official CLI registry, pinned commit 378bab8f](https://github.com/tornadocash/tornado-cli/blob/378bab8fbe0a101585170433dea7303f54b8f130/config.js)
- [Official Classic UI registry, pinned commit 6e01a677](https://github.com/tornadocash/tornado-classic-ui/blob/6e01a677aebd4b202d96f57162efc5147fb1c7db/networkConfig.js)

All twelve native-pool addresses below agree between these two upstream records.
This is source verification, not a new independent onchain audit. The existing
monitor startup gate checks that each configured pool has code and that its
`denomination()` matches before ingestion proceeds.

| Chain | Asset | 0.1 | 1 | 10 | 100 |
|---|---|---|---|---|---|
| Ethereum (1) | ETH | `0x12d66f87a04a9e220743712ce6d9bb1b5616b8fc` | `0x47ce0c6ed5b0ce3d3a51fdb1c52dc66a7c3c2936` | `0x910cbd523d972eb0a6f4cae4618ad62622b39dbf` | `0xa160cdab225685da1d56aa342ad8841c3b53f291` |
| BNB Chain (56) | BNB | `0x84443cfd09a48af6ef360c6976c5392ac5023a1f` | `0xd47438c816c9e7f2e2888e060936a499af9582b3` | `0x330bdfade01ee9bf63c209ee33102dd334618e0a` | `0x1e34a77868e19a6647b1f2f47b51ed72dede95dd` |
| Arbitrum (42161) | ETH | `0x84443cfd09a48af6ef360c6976c5392ac5023a1f` | `0xd47438c816c9e7f2e2888e060936a499af9582b3` | `0x330bdfade01ee9bf63c209ee33102dd334618e0a` | `0x1e34a77868e19a6647b1f2f47b51ed72dede95dd` |
| Base (8453) | ETH | Unverified | Unverified | Unverified | Unverified |

The checked official sources do not list Base. Search did not establish a verified
Classic deployment there; no address has been invented or copied from another
chain. This is a coverage limitation, not proof that no community fork exists.

The existing Ethereum 100 DAI pool is retained as a thirteenth registry entry.
Expanding other ERC-20 denominations is outside this native-pool update.

## Heights and historical coverage

Ethereum and BNB creation heights retain the per-pool metadata from the pinned
CLI registry. Arbitrum's identical `3300000` values are represented explicitly as
`scan_start_block`, with `deployment_block: null`; the prior implementation had
correctly noted that these were not independently verified deployment heights.
Withdrawal screening uses the recorded creation height when supplied, otherwise
the conservative upstream scan boundary. It does not assert history before it.

Adding pools affects subsequent ingestion after rebuilding/restarting the Rust
service. It does not backfill historical withdrawals or rewrite existing funding
edges. Existing block/journal deduplication makes naive replay unsuitable for
filling historical registry gaps. A separate reconciliation/backfill workflow is
needed to recover previously missed withdrawals safely.

## Interpreting funding addresses

- A `withdrawal` evidence record from a matching chain/pool is a direct pool
  withdrawal; the UI labels its denomination and asset.
- A `transfer_provenance` record is a downstream transfer from a watched wallet.
  Its immediate `from` address often is not a pool. The UI calls it an intermediary
  rather than assigning an unsupported Tornado Cash label.
- A registered pool withdrawal is neutral provenance, not proof of abuse.
