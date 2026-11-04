# Release status

Status: **incomplete; builds and offline/database tests pass; live integrations remain unverified**.

## Verification update — 2026-11-04

- `cargo test --workspace --locked --quiet`: 25 passed; three explicitly environment-dependent tests skipped.
- New authenticated HTTP replay/history test: passed separately against a fresh local PostgreSQL database. Covers stream pagination, filters that advance cursors, empty pages, authentication, invalid queries, revision order/retractions, missing incidents and expired cursors.
- Existing `db_atomic_replay_reorg_and_cursor_expiry`: passed separately against another fresh local PostgreSQL database, applying migrations and testing replay idempotency, reorgs, bridge rollback, pending lifecycle and authentication.
- `cargo clippy -p siren-service --lib --locked`: completed with existing warnings outside the new history module; strict lint cleanliness is still outstanding.
- New endpoints: `/v1/findings`, `/v1/reviewed`, `/v1/incidents/{id}/revisions`; see OPERATIONS.md and openapi.json.
- Live providers, containers, production-scale performance, crash recovery and production response execution remain unverified. The tables below preserve the original implementation assessment unless superseded by these results.

The initial authoring session did not run commands; the verification above supersedes that earlier execution status.

## Implemented source paths

| Area | Source implementation | Verification |
|---|---|---|
| Workspace | Root CLI; shared lib; service factory with single-flight initialization; typed Arc handles | Not compiled |
| Ingestion | Configured Ethereum/OP/BSC/Nitro RPC families; full blocks/receipts; pending WS for Ethereum/BSC; chain checks; bounded RPC responses; anchored native traces; Base full Flashblocks subscription; bounded Nitro signed-message/batch framing | No live provider tests |
| Durability | Framed fsync journal; exclusive file writer; streaming startup recovery; applied-journal idempotency; post-commit journal reclamation; atomic canonical cursor/engine/findings/outbox commits | PostgreSQL and crash tests unrun |
| Reorg | Bounded ancestor search; checkpoint engine rewind; orphaned observations/edges; finding retractions; preconfirmation corrections; hash-checked native safe/finalized promotion; pending TTL | Synthetic DB test unrun |
| Funding | Three sourced pools; successful Withdrawal recipient/relayer/fee decoding; bounded native transfer propagation; neutral watches | Offline tests written, unrun |
| Bridges | Base Portal native v0 and Arbitrum Inbox native kind-12 message IDs; independent counterpart arrival; canonical rollback and bounded watch propagation | Synthetic vectors and PostgreSQL tests written, unrun |
| Rules | Watched activity; funding/deployment/continuation; exact fingerprints; privilege events; native outflow budget; supported governance execution events | Limited synthetic tests |
| Analysis | PUSH-aware instruction scan; exact and normalized signatures; selector/address candidates; EIP-1167 and EIP-7702 designator recognition; EIP-1967 slot query | Offline tests written, unrun |
| Simulation | Strict complete-state synthetic Cancun REVM sequences; isolated CLI worker; native included call/prestate tracing; asynchronous revisions in simulate mode | No execution or differential validation |
| API | Bearer keys hashed at rest; DB rate limits; REST queries; per-stream cursors; WS replay/filter/heartbeat/send deadlines; manual reviewed revisions | Demo source and schema only |
| Response | Exact action policy validation; digest; role-result comparison; local pause proof; remote signing/relay auth interfaces; public/private submission transports | Not connected to a production executor |
| Operations | Compose, TLS proxy profile, Prometheus counters, doctor, CLI, CI workflow | Containers and CI unrun |

## Remaining requirements / release blockers

1. Complete workspace formatting/lint cleanup and dependency/advisory review. Cargo.lock is present, compilation/offline tests pass, and migrations ran in the two isolated PostgreSQL integration tests above. Broader fault and production migration coverage remains outstanding.
2. Validate the implemented Base Flashblocks and Nitro feed transports against live providers, add Nitro signature verification/system-message conformance, push block heads, a live source capability state machine and stronger provider/quorum/state validation. Pending hydration is currently serial. Historical archive probe is only one configured height.
3. Validate safe/finalized updates and pending expiration, add robust repeated-fork incident restoration, deep-reorg recovery from a verified safe checkpoint, database/evidence retention and comprehensive per-chain health. Same-nonce pending arrivals remain alternatives until canonical inclusion.
4. Validate the two implemented direct-native Ethereum–Base/Arbitrum bridge paths against chain-native receipts and SDK vectors. Persist independently evolving finality for both bridge halves. Broader ERC-20/withdrawal/retryable bridge variants remain unsupported.
5. Expand the verified pool registry with exact deployment metadata and pinned provenance. Arbitrum CLI registry lists an apparent scan-start height, which was not represented as a verified deployment block. Base has no configured verified pool. ERC-20 withdrawal fees are decoded; refunds and ERC-20 transfer-hop propagation need reconciled trace/state support.
6. Add incremental withdrawal-only backfill/coverage, historical incoming-activity and nonce-state acquisition, code/delegation refresh, freshness confidence calibration, per-asset dust thresholds and chain-scoped shared-account labels. The existing backfill processes whole canonical blocks.
7. Complete configurable versioned rule registry/scoring budgets, token conservation/mint/burn/flash-loan rules, decoded role/proxy storage validation, governance proposal intent and invariant policies. Current event/flow heuristics do not prove exploitation. Internal-only unknown-wallet behavior outside selected trace candidates is missed.
8. Add full runtime/constructor/factory analysis caching and dependency/call-graph discovery. Static PUSH20 extraction alone never establishes a victim. No general decompiler or source recovery is implemented.
9. Add live signed-envelope recovery and exact pending-state acquisition, ordered block-prefix replay, prefetch/read-through caches, EVM inspectors, validated OP execution, current fork support and chain-native differential fixtures. Local execution is intentionally restricted to explicit synthetic Cancun snapshots. Network errors never become empty state.
10. Complete the isolated production response service: reviewed action generation, independent role verification, proof freshness, decoded signed-envelope verification, nonce/digest reservation, durable attempts/receipts, endpoint probes, remote signer conformance and development signer. Existing transport interfaces are not a safe executor. execute_authorized configuration is rejected.
11. Add S3-compatible evidence storage, actual database/evidence retention workers, crash-testing of journal reclamation, outbox retry health, watched-address API completeness and richer metrics/traces/OTLP. Current filesystem evidence storage applies to asynchronous native enrichment.
12. Connect optional LLM integration to a bounded asynchronous queue and reviewed-feed workflow. Its strict opinion-output boundary exists, but no automatic LLM worker is enabled.
13. Complete production-size replay datasets, all requested fault/conformance cases, labeled benign/exploit precision/recall evaluation, slow-consumer and timeout integration tests, end-to-end execution verification, profiling and all performance acceptance runs.

These are unmet requirements of the requested single release, not claims that the requested features are optional or deferred by agreement.
