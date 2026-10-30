# Coverage and fidelity

## Chains

| Chain | Implemented input transport | Execution evidence | Important gap |
|---|---|---|---|
| Ethereum 1 | Canonical RPC blocks/receipts; configured pending WS | Native callTracer/prestateTracer for included transactions | No independent native differential validation or live speculative REVM |
| Base 8453 | Canonical RPC blocks/receipts; configured full Flashblocks subscription | Provider-native included tracing | Live Flashblocks conformance and OP-aware local execution absent |
| BSC 56 | Canonical RPC blocks/receipts; configured pending WS | BSC client's included tracing | BSC fork/system-call conformance absent |
| Arbitrum 42161 | Canonical RPC blocks/receipts; uncompressed sequencer relay framing plus RPC hydration | Nitro client's included tracing | Feed signature verification, system-message decoding and differential conformance absent |

Capabilities are reports of probes/configuration, not complete visibility. The type list describes known observation families, not local execution support. A successful historical-state probe establishes only that queried height. System contract inventories are not populated.

Private transactions may be first observed after inclusion. Base preconfirmation represents provisionally executed work; a Nitro sequencer feed represents already ordered work. Neither creates a promise of front-running through higher fees. Configured ordered feeds persist raw evidence before normalization; provider compatibility remains unverified.

## Detectors

Cheap screening runs for every normalized transaction. Trace selection covers top-level creation, protected targets/logs and privilege topics up to the per-block budget. It does not deliver complete internal-call coverage; untraced counts include noncandidates. Traces propagate ancestor reverts to children before value-flow accounting.

Findings are explainable observations:
- Privacy funding is informational, never exploit confirmation.
- Recent funding plus a creation creates a suspicious-deployment candidate.
- Known exact fingerprints are separate from normalized similarity.
- Privilege/governance logs are observed events, not proof of storage changes or malicious intent.
- Protected native outflow uses successful call values net of incoming call values. It is not a complete accounting invariant, especially for system transfers, fees, selfdestruct semantics or nonstandard assets.
- Repeated visits increase an outflow's priority only with outflow corroboration.

Missing current code, unknown calldata, untraced helper deployments and unknown counterparties stay unresolved. Allowlists only change known-admin context and do not suppress native budget findings.

