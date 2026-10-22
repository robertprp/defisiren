# Synthetic fixtures

These are original, isolated test inputs, dedicated under CC0-1.0. No historical exploit dataset or licensed reference source was copied.

demo.jsonl contains ordered Accepted::Block records. Its pool event address and ABI follow the attributed deployment registry, but **the blocks, transaction hashes, log and wallet history are synthetic**, not observations from Ethereum.

harmful.json and protected.json use identical strict snapshots. The first executes a helper deployment and a vulnerable protocol call. The second inserts an owner-only pause before the harmful call. Runtime bytecode is hand assembled; no Solidity compiler was run.

The protocol supports owner(), pause() and an intentionally unsafe default withdrawal branch. The owner getter/authorization address is the fixture owner. These programs exist only to validate isolated analysis and defensive policy behavior.

manifest.json records chain interpretation, block/transaction order and expected results. policy.json and action.json are fixture-only authorization inputs at a fixed synthetic clock; do not enroll them on a live chain.

Missing coverage includes real native-client differential fixtures, production-scale arrival recordings, verified canonical bridge message pairs, historical exploit labels and representative benign arbitrage/liquidation traces. No measured precision/recall is claimed.
