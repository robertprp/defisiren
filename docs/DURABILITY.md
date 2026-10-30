# Durability, recovery and streams

An accepted input becomes locally durable only after its length/checksum/payload frame has been appended and fsync succeeds. A partially written final frame is truncated during exclusive startup recovery. A checksum mismatch in a complete frame is an error, not discarded evidence.

A finding becomes stream-visible only when its incident revision and outbox row commit in PostgreSQL. Local-journal acceptance is not a finding-publication acknowledgment. Canonical cursor, canonical branch, serialized detector state, observations, funding edges, profiles and findings commit in the same transaction. SQL errors retain the journal input for restart replay.

Each applied input has a SHA-256 identity recorded transactionally; replaying acknowledged input is idempotent. A per-chain transaction advisory lock serializes canonical application. The initial deployment is intentionally one ingestion writer per database, with an additional exclusive file lock. It is not a distributed ingestion cluster.

Outbox cursors are allocated while holding a stream-head row lock until commit. This avoids the sequence-allocation/commit-order race where clients could skip an earlier uncommitted row. Finding revisions and duplicate delivery are different concepts. Event IDs are stable per incident/revision. Reviewed events have an independent stream cursor.

WebSocket authentication uses a bearer header. Each client reads at most 64 rows at a time, sends with a five-second deadline and receives heartbeat cursors. Filtered-out events still advance the resume cursor. A connection expires after fifteen minutes to require fresh authentication. Reconnect with the last fully processed cursor; deduplicate by event_id. There is no end-to-end exactly-once claim.

PostgreSQL floor values enforce cursor-expired HTTP 410 before upgrade. A running stream disconnects if its floor overtakes the cursor. Automated retention is not implemented.

Reorg recovery can rewind to a stored ancestor within max_reorg_depth. Orphan branch observations and funding edges lose canonical status; contract snapshots are removed; engine state comes from the ancestor snapshot; affected current findings emit retractions. Multi-branch incident restoration and deep recovery need further work.

On disk exhaustion, journal corruption or write/fsync failure, readiness becomes unhealthy and ingestion terminates or fails closed. Never remove or overwrite the journal to hide the error. Provision disk, preserve the original journal and database, repair only with an audited recovery procedure. Automatic rotation/compaction is missing; the journal cap is therefore an operational limit, not a sustainable retention policy.

Shutdown stops producers, drains bounded pending work, waits for workers, then flushes the journal. Deadline expiration aborts remaining tasks; unfinished transactions roll back and durable inputs remain replayable.

Filesystem evidence objects are compressed JSON addressed by SHA-256 of uncompressed data. Atomic rename plus directory fsync precedes publishing their references. Only native asynchronous enrichment currently uses the external store.

Telegram uses locked outbox rows and persisted attempts/backoff. It cannot guarantee exactly-once delivery because Telegram offers no idempotency key; a send/commit crash can repeat a notification. Offline replay findings are marked non-deliverable to Telegram by default.

Journal reclamation occurs only while holding the ingestion writer lock and after synchronous PostgreSQL commit of every retained frame. Startup recovery runs inside single-flight initialization before any producer receives handles. An atomic empty-file replacement and parent-directory fsync leave either an idempotently replayable old file or an empty checkpoint. Any apply failure latches acceptance off until recovery; this prevents skipping a failed frame during reclamation. Database applied-journal IDs remain for idempotency. These crash boundaries are authored but not fault-tested.
