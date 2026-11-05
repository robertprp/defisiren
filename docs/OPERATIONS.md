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

## Provider prerequisites and configuration

Use environment-variable references from config/example.toml. Never log endpoint credentials or serialize whole configuration. No monitor private key is required. Four chain nodes or RPC subscriptions are external prerequisites.

Configure start_block explicitly for each deployment. Example Base/Arbitrum start at zero to avoid inventing a coverage start; scanning from genesis can be very expensive. Changing a start height changes historical coverage and must be recorded. Ethereum/BSC example starts align with configured pool registry entries.

Required RPC methods: eth_chainId, eth_blockNumber, eth_getBlockByNumber with full transactions, eth_getTransactionReceipt. Pool probes also need eth_getCode and eth_call. Native analysis needs debug_traceTransaction with callTracer and preferably prestateTracer diff mode, and sufficient history for the queried block. Pending subscriptions need a genuine eth_subscribe newPendingTransactions source plus transaction hydration. Lack of archive or trace access is degradation, not zero state.

Missing chain credentials leave that chain absent from active handles and unhealthy in readiness. A failed pool ABI/code probe blocks that chain's canonical loop. Capability probing repeats about every minute, between bounded catch-up turns. RPC retries are bounded with jitter; pending disconnects report unrecoverable ephemeral gaps. Canonical RPC fills missed included blocks.

Mode observe is the default. Mode simulate also runs the native evidence enrichment worker. execute_authorized is rejected until the missing execution service and conformance checks are implemented.

## API and TLS

Use an Authorization: Bearer header; keys in query strings are unsupported. Browser-only clients needing authentication headers require an appropriate backend/session gateway. Do not put a permanent key into a WebSocket URL.

Serve external traffic through the Caddy TLS profile or another maintained TLS proxy. Compose exposes the raw API only on loopback. Restrict metrics and health routes to operators. The in-process server is HTTP and does not terminate TLS.

Use /v1/chains to inspect degradation; /health/ready is a readiness summary, not proof of complete trace/finality coverage. /ws/findings and /ws/reviewed take after, chain_id and severity filters. Save the cursor after application processing and tolerate event-ID redelivery.

## Failure runbook

- Provider outage: inspect per-chain capability/probe age and errors, restore access, resume from committed cursors. Never substitute an endpoint with a different chain/state.
- Gap/reorg: preserve evidence, inspect parent hashes and canonical branch. Recovery beyond max_reorg_depth currently needs an operator-reviewed clean checkpoint; automatic deep recovery is absent.
- Database outage: accepted journal records remain; restore PostgreSQL and restart for idempotent replay. A durability-health failure remains latched until restart.
- Disk full/checksum failure: stop ingestion, preserve files/database and expand storage or perform audited recovery. Post-commit reclamation does not discard uncommitted frames; preserve the journal during failed recovery.
- Slow consumers: reconnect with the last processed cursor. HTTP 410 requires historical REST inspection and a fresh stream baseline.
- Simulation failure: retain observational findings and unresolved dependencies. Never execute on an unsupported or inconclusive result.
- Response incident: production execution is disabled. Do not remove that gate to bypass missing signer/nonce/role/proof checks.
- Telegram failure: restore credentials/destination, inspect delivery_outbox attempts/next_attempt. Duplicate notifications after a crash are possible.

## Retention and capacity

No automatic retention worker is shipped. Recommended policy pending implementation: outbox 7 days, canonical replay window at least max_reorg_depth plus verified safe checkpoint, routine observations 7 days, incident revisions and response audit 90 days or protocol requirements, large evidence 30 days. Do not delete referenced evidence or canonical snapshots needed for rewind.

Journal caps default to 256 MiB live / 16 MiB offline. Committed journal frames are reclaimed atomically at half the cap, and after successful startup recovery. Acceptance stops after database/journal failure; uncommitted frames cannot be reclaimed. Crash-injection validation remains outstanding. Back up PostgreSQL and evidence consistently.

Resource settings bound RPC concurrency, input sizes, candidate trace count, profile count, hop/fanout and shutdown time. Optional analysis can be skipped at trace-budget saturation, which is counted. The implementation does not yet provide protected-protocol reserved queues or measured sustained-load capacity.

## Ordered feeds and bridges

Set BASE_FLASHBLOCKS_WS_URL to a Base endpoint supporting eth_subscribe with ["newFlashblockTransactions", true]. The adapter persists the raw message before normalization. It retains tentative positions with no canonical block hash. Canonical inclusion or absence at the tentative height emits a separate preconfirmation correction.

Set ARB_SEQUENCER_RELAY_WS_URL to an **uncompressed Nitro feed relay**, not a JSON-RPC WebSocket. The decoder handles version-1 broadcasts and L2 signed messages/batches. It stores unsupported/system messages as raw evidence and hydrates transaction hashes from native RPC. Feed signatures are not independently validated; RPC association is not cryptographic validation of sequencer ordering. Disconnects and hydration failures increment gap counters. These paths have synthetic parsing tests but no live-provider conformance results.

Bridge support is deliberately narrow: direct native Base Portal version-0 deposits and Arbitrum Inbox kind-12 deposits originating on Ethereum. Both canonical halves are required. Matching uses Base sourceHash or the derived Arbitrum typed-deposit transaction hash, then checks recipient/value consistency. Other bridge/token/withdrawal paths remain unsupported. Funding propagation requires existing source funding evidence, dust/hop/time/fanout limits and separate destination-chain identities. A source rollback retracts the bridge incident immediately; destination watch state is reconciled on its next canonical application.

Native safe/finalized tags are checked against ingested canonical hashes before promotion; block depth is not converted into finality. Unavailable tags remain unsupported. Pending observations expire after pending_ttl_seconds (default 900); expiry means watch timeout, not proof that the transaction can never be included.

## RPC limits and console output

Console logs default to compact text on stderr. Set SIREN_LOG_FORMAT=pretty for multiline output or json for log collectors; RUST_LOG controls verbosity. Progress is logged periodically, recovery is explicit, and repeated failures are coalesced. Secrets and raw provider error messages are excluded.

rpc_requests_per_second (default 5) paces HTTP request starts across all chain pools in the process. HTTP 429 and RPC rate-limit responses establish a shared cooldown; numeric Retry-After is honored, otherwise 30 seconds is used. This request budget is not a compute-unit budget and cannot guarantee compatibility with a provider plan. See https://www.alchemy.com/docs/reference/throughput.

Chain-ID probes are single-flight per endpoint and cached for 60 seconds. Native trace requests still validate the specified block hash before and after execution. Blocks prefer eth_getBlockReceipts with position/count/hash checks and use bounded individual receipts when the method is explicitly unsupported. Rate limiting does not trigger an expensive individual-receipt fallback.

The local configuration uses 2 requests/second, 2 concurrent operations per chain, 5-second polls, 1 candidate trace per block, and no pending subscriptions. This reduces coverage and can produce canonical lag; it is a development profile, not a demonstrated real-time four-chain capacity. Existing durable cursors always resume. start_at_head=true selects the current head only when that chain has no cursor; it does not erase historical data or claim historical funding coverage.

Doctor distinguishes reachable RPC endpoints from complete integration verification; successful probes alone no longer set live_integration_verified=true.

## REST replay and incident history

Authenticated `GET /v1/findings` and `GET /v1/reviewed` provide bounded polling of the same streams as the WebSocket endpoints. Both accept `after` (exclusive stream cursor, default 0), `limit` (1–256, default 100), and optional exact `chain_id` and `severity` filters. Unknown parameters, negative cursors and invalid limits return 400. Cursors belong to their individual stream.

Responses contain `stream`, `findings`, `next_after`, `scanned`, and `page_full`. Persist `next_after` only after processing the page. Filtering happens within the scanned page: an empty findings list can still advance `next_after`. When `page_full` is true, immediately fetch another page; it does not guarantee more records. Otherwise poll again later. A cursor older than the retention floor returns 410.

`GET /v1/incidents/{id}/revisions?after=0&limit=100` returns revisions in ascending order, including retractions. Its `after` and `next_after` values are **incident revision numbers**, independent of stream cursors. The response contains `incident_id`, `revisions`, `next_after`, and `page_full`. An unknown incident returns 404; an existing incident with no later revisions returns an empty page.

Run the HTTP integration test against a fresh disposable database (it inserts fixtures and changes retention floors):

```sh
cargo test -p siren-service rest_replay_and_revision_history -- --ignored
```

## Triage dashboard and registry update

Rebuild and restart the monitor to activate chain-aware envelope recognition and
the triaged incident API. Migration 0005 preserves the original records while
archiving exact-match legacy envelope noise; it is safe to apply on normal startup
after the manual November 5 cleanup. See [the triage report](INCIDENT_TRIAGE.md).

The new dashboard starts disconnected. Connect an API key to see live data, or
explicitly select the synthetic demo. Needs review and Watch activity are separate
views; unsupported formats belong to Diagnostics. The summary counters cover all
stored records; charts cover only the selected category's loaded window.

The expanded [pool registry](POOL_REGISTRY.md) takes effect for subsequent ingestion.
Historical watches are not automatically backfilled. Source-linked identities do
not turn downstream funding intermediaries into registered pool addresses.
