# Reference and provenance

Read-only inspection found delta-bot's default/current reference branch to be master at **8d453717a323a9df30da4864dc18e97e74d63732**. The local target repository contained only .git and its HEAD referenced master. No applicable local AGENTS.md was found at the workspace or inspected ancestors. The reference tree had no AGENTS.md or license file; GitHub returned no recognized license.

Inspected:
- [Workspace manifest](https://github.com/robertprp/delta-bot/blob/8d453717a323a9df30da4864dc18e97e74d63732/Cargo.toml)
- [Service registration](https://github.com/robertprp/delta-bot/blob/8d453717a323a9df30da4864dc18e97e74d63732/service/src/services.rs)
- [Task manager](https://github.com/robertprp/delta-bot/blob/8d453717a323a9df30da4864dc18e97e74d63732/service/src/core/tasks/manager.rs)
- [Chain provider](https://github.com/robertprp/delta-bot/blob/8d453717a323a9df30da4864dc18e97e74d63732/service/src/chain/provider.rs)
- [App runner](https://github.com/robertprp/delta-bot/blob/8d453717a323a9df30da4864dc18e97e74d63732/service/src/core/app/runner.rs)

Only structural responsibilities were followed. No reference source, betting logic, keys, configuration serialization or dependency lockfile was copied. New implementation code is MIT; original synthetic fixtures are CC0-1.0. Deployment facts are attributed to the linked registry.

Semantic/API references inspected:
- [Geth subscriptions](https://geth.ethereum.org/docs/interacting-with-geth/rpc/pubsub), [txpool](https://geth.ethereum.org/docs/interacting-with-geth/rpc/ns-txpool)
- [Base Flashblocks](https://blog.base.dev/flashblocks-deep-dive)
- [Nitro execution/sequencing](https://docs.arbitrum.io/how-arbitrum-works/inside-arbitrum-nitro)
- [BSC APIs](https://docs.bnbchain.org/bnb-smart-chain/developers/json_rpc/bsc-api-list/)
- [Tornado Withdrawal ABI](https://github.com/tornadocash/tornado-core/blob/master/contracts/Tornado.sol)
- [Tornado deployment metadata](https://github.com/tornadocash/tornado-cli/blob/master/config.js)
- [REVM 19.5 source/API](https://docs.rs/crate/revm/19.5.0/source/src/evm.rs), [features](https://docs.rs/crate/revm/19.5.0/source/Cargo.toml)
- [Current REVM](https://docs.rs/revm/latest/revm/) and [OP-REVM](https://docs.rs/op-revm/latest/op_revm/) were inspected. The older REVM pin here is deliberately confined to a Cancun fixture backend, not claimed current-chain fidelity.
- [Axum 0.8.4](https://docs.rs/axum/0.8.4/axum/), [SQLx 0.8.6](https://docs.rs/sqlx/0.8.6/sqlx/)
- [Flashbots private submission](https://github.com/flashbots/flashbots-docs/blob/main/docs/flashbots-protect/additional-documentation/eth-sendPrivateTransaction.mdx)
- [Defimon engineering account](https://www.quicknode.com/blog/real-time-defi-exploit-detection)

The supplied Defimon WebSocket documentation URL could not be retrieved. No Defimon schema compatibility is claimed. This project uses its own versioned event schema.
