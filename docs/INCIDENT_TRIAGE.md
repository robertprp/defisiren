# Incident triage and noise cleanup

## Database inspection — 2026-11-05

Read-only queries against the database configured in `.env` found that over 99% of
stored incidents were informational `unsupported_envelope` diagnostics from
canonical RPC ingestion. No synthetic/replay source markers and no medium, high,
or critical findings were present in the inspected snapshot. The database was
actively ingesting, so totals changed during inspection.

The old detector classified every non-Ethereum transaction type above 2 as
unsupported. The affected records included normal Base deposits (126), Arbitrum
system/bridge transactions (100, 101, 104, 105, 106), and BNB/Arbitrum typed user
transactions. Recognition is now chain-specific. This change does not expand the
synthetic-only local execution backend or suppress other detectors on those
transactions.

Protocol references:

- [OP deposit transaction specification](https://specs.optimism.io/protocol/deposits.html)
- [Arbitrum RPC transaction types](https://developer.arbitrum.io/arbitrum-essentials/arbitrum-vs-ethereum/rpc-methods)
- [BNB transaction type definitions](https://github.com/bnb-chain/bsc/blob/master/core/types/transaction.go)

## Applied cleanup

Migration `0005_incident_triage.sql` was applied transactionally to the configured
database on November 5. It archived **21,675** exact-match zero-score diagnostics
into `incident_suppressions`. The resulting category snapshot was **79 watch
records** and **0 review candidates**. At that point the watches comprised 73
funding records and 6 deployments after funding; none of the six deployment
records matched a known exact malicious fingerprint.

Archiving is reversible classification, not deletion. Original incidents,
observations, revisions, funding edges, delivery-outbox entries and stream cursors
remain unchanged. Raw REST/WS replay still exposes historical records; consumers
that need triaged presentation should use the incident catalog. The migration is
idempotent and will also be recorded/applied by SQLx on the next normal startup.
The old running binary can continue emitting the old diagnostic until restarted;
startup archives matching intervening entries and the updated detector stops new
ones. No process was started or restarted during this change.

## Catalog API

`GET /v1/incidents?category=review&limit=256` filters **before** applying the limit.
Categories: `review`, `watch`, `diagnostics`, `retracted`, `archived`, `demo`.
`active` (the default) includes review and watch; `all` preserves access to every
category. Optional `chain_id` and `severity` filters are applied in SQL. Responses
include per-record `category` and `triage` metadata. Incident detail also includes
these fields, while immutable revision payloads remain unchanged.

`GET /v1/incidents/summary` reports category counts across the full database,
independent of the loaded 256-record window. Counts are not proof of exploits.

The UI opens disconnected, with no fabricated alerts. Demo data requires an
explicit choice and cannot silently replace live errors. Live mode defaults to
Needs review, with neutral funding watches in a separate tab. Evidence confidence
is explained independently from severity, and direct pool withdrawals are clearly
distinguished from downstream funding transfers.

## Reversibility

An operator can remove a specific `incident_suppressions` entry to unarchive its
original incident; that returns it to the diagnostics category. Do not delete or
renumber finding revisions or delivery cursors to clean a presentation feed.

Source-level regression tests were added but not run, respecting the request to
leave local execution to the operator. No dependencies were installed and no
servers were started for this change. Read-only queries verified the applied SQL
catalog and archive metadata; frontend and Rust builds remain to be run.
