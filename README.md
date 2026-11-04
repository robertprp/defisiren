# chain-siren

Independent Rust EVM evidence-monitor implementation, following the root executable / `lib` / `service` boundaries of delta-bot.

**This is an incomplete implementation, not a production-ready MVP.** The workspace builds and its offline tests pass with the checked-in `Cargo.lock`. REST replay and revision history also have a PostgreSQL integration test. [Release status](docs/STATUS.md) lists verification scope and remaining gaps.

The source connects canonical block/receipt ingestion, selective native traces, Tornado withdrawal watches, deployment/privilege/native-flow screening, an fsync journal, PostgreSQL incident revisions and outbox, authenticated REST/WebSockets, and asynchronous native trace enrichment. A synthetic demo connects replay, a local REVM harmful sequence, an authorized pause model, a finding revision and WebSocket receipt.

Production response execution is disabled. The local demonstration never submits a transaction.

## Repository

- `src/`: executable and operator commands.
- `lib/`: typed lifecycle, evidence, simulation and policy entities.
- `service/`: startup factory, supervised pipeline and subsystem implementations.
- `migrations/`: SQLx/PostgreSQL schema.
- `config/`: environment-referenced four-chain and offline configurations.
- `fixtures/`: original synthetic datasets with explicit provenance.
- `tests/`, `benches/`: behavior/fault checks and a narrow microbenchmark.
- `docker/`, `docs/`: deployment configuration, API schema, operation and coverage.

See [setup and commands](docs/OPERATIONS.md), [fidelity and detector coverage](docs/COVERAGE.md), [durability](docs/DURABILITY.md), [benchmark status](docs/BENCHMARKS.md), and [reference inspection](docs/REFERENCE.md).
