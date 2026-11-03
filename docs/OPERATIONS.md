# Operator setup

Read STATUS.md for the current verification scope before operating. The procedures below include checks that still require external services.

## Build and validation

Use the pinned Rust toolchain. Generate and review Cargo.lock, then retain it in version control. Perform dependency/advisory review; do not treat exact direct pins as a resolved dependency tree.

Suggested verification sequence:
```text
cargo generate-lockfile
cargo fmt --all
cargo check --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo test --test database db_atomic -- --ignored --test-threads=1
```

Database tests require a disposable isolated PostgreSQL database. They mutate canonical history and retention floors. Never point tests or synthetic replay at a live monitor's database.

The CI workflow includes formatting, lint, offline tests, explicit PostgreSQL tests, an isolated demo database, microbenchmark and dependency checks. It has not run.

## Offline demonstration

Start PostgreSQL separately or from docker/compose.yaml, set DATABASE_URL, and use a clean database plus config/offline.toml. No EVM nodes, monitoring keys, Telegram or LLM are needed.

```text
chain-siren --config config/offline.toml demo
```

The demo is written to replay three synthetic blocks: a relayed withdrawal, a watched-wallet deployment, then a harmful protected-protocol interaction. It starts an authenticated local WebSocket client/server, evaluates harmful and owner-pause REVM sequences in child processes, validates the fixture policy, publishes a simulation revision, and waits for that revision over WebSocket. It uses a fixed synthetic clock. It neither signs nor submits any live action.

The fixture pause policy is enabled only as input to isolated validation. The policy installer rejects enabled production policies. Fixture chain IDs, hashes and apparent block heights are synthetic; see fixtures/manifest.json.

## Commands

| Command | Behavior |
|---|---|
| run | Recover journal, start supervised configured-chain ingestion, API and optional integrations |
| doctor | Probe configured providers; exit nonzero for inaccessible/unverified providers |
| backfill --chain ID --from N --to N | Bounded whole-block range through the durable pipeline, at most 100000 blocks per invocation |
| replay FILE | Replay checked JSONL through journal/database; Telegram disabled for those events |
| analyze-tx --chain ID HASH | Native included call/state evidence; pending speculative analysis unsupported |
| analyze-contract --chain ID ADDRESS --block-hash HASH | Hash-bound code scan and implementation-slot candidate |
| watch --chain ID ADDRESS --hours 168 | Store operator wallet watch |
| policy FILE | Install a disabled, versioned response policy for review |
| api-key --name NAME | Generate 256 random bits; print key once; persist SHA-256 only, 90-day expiry |
| review UUID --reason TEXT | Append an explicitly operator-reviewed revision; this is not deterministic proof |
| benchmark --iterations N | Repeated-envelope decode/dedup/cheap-screen microbenchmark |
| demo | Isolated synthetic database/WS/REVM response demonstration |

