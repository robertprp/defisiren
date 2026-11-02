# Benchmark report

**No Rust benchmark, profiler or load test has been run. All acceptance targets are unverified.**

| Measurement | Requested budget | Result |
|---|---|---|
| Decode/dedup/cheap screen | >=50,000 envelopes/s; p99 <20 ms | Unmeasured |
| Durable raw finding publication | p95 <500 ms after complete input | Unmeasured |
| Warm bounded simulations | p95 <1 s | Unmeasured |
| Cold simulations | Report separately | Unmeasured |
| Traces/s and simulations/s | Corpus and concurrency documented | Unmeasured |
| Sustained ingestion | 2x busiest recorded ten-minute rate for 30 min | No recorded production dataset supplied |
| Memory growth / overload | Bounded under sustained replay | Unmeasured |
| Precision / recall | Explicitly labeled representative corpus | Synthetic test examples only; no reported statistic |
| Flamegraph | Profile measured bottlenecks | Not produced |

Reference host: 16 modern CPU cores, 64 GB RAM, local NVMe, excluding separately provisioned chain nodes. No run on that host was performed.

The benchmark command and benches/screen.rs measure only repeated synthetic JSON decoding, deduplication and candidate screening with no RPC, database, fsync, trace or simulation. They print actual elapsed time and basic host information when run. Repeated-envelope cache behavior is explicitly labeled and cannot establish the 50,000-envelope target on unique traffic.

Required before any performance claim: resolve/build release dependencies, capture CPU model/physical cores/RAM/storage/OS/compiler, dataset source and byte distribution, unique hashes, batch size, candidates, traces, cache hits, durability settings, source-arrival delays, fetch timings and queue saturation. Run separate cold/warm simulation corpora with gas, trace size, state reads, concurrency and timeouts. Add a production-size replay generator and durable latency histogram before testing the acceptance budgets.

Known throughput constraints include serialized journal/database application, per-block profile snapshot serialization/replacement, per-request chain-id validation, receipt-per-transaction fetching and sequential candidate traces. No full-pipeline throughput is inferred from the microbenchmark.

RPC usage is not measured. A rough request-accounting formula for the implemented path is 2*(1+N) requests per full block and N receipts, plus head/probe requests; each anchored trace uses chain-id, pre-hash, trace and post-hash calls. Failover multiplies attempts. This is a count model, not a measured cost or subscription recommendation.
