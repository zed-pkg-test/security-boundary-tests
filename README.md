# Orestra State

Transactional resource-state contracts, fencing epochs and ordered watches.

This repository contains Oreslang pure helpers and a **Rust conformance oracle** for deterministic unit tests. The oracle is not a distributed state service and must never be deployed as one.

Run `cargo test --all-targets` to test CAS, create-only writes, watch compaction, copy isolation, lease turnover, reservation atomicity, WAL recovery, quorum behavior, and expired-epoch rejection. Run `cargo run --bin bounded-fencing` for the previously landed bounded fencing explorer and `cargo run --bin check-fencing` for the migrated finite epoch/commit checker. Both are bounded models, not proofs of distributed consensus or production fencing safety.

See `spec/CONSISTENCY.md` for production Raft, durability and security gates.

## WAL oracle coordination and crash behavior

The Rust journal reference takes a Linux/POSIX `flock` on a stable `.lock` file for each mutation, reloads the complete WAL under the lock, and serializes the next checkpoint image. This prevents two cooperating reference store instances from overwriting each other's committed writes. It is **not** a network-distributed lock, consensus protocol, multi-node HA state service, or production-ready storage implementation.

Recovery rejects revision regressions and decreasing leadership counters; a valid-checksum incomplete final frame is removed before further append operations. Completed frames with invalid checksums still fail closed. Durable acknowledgment requires fsync of the staging file, atomic rename, and fsync of the containing directory. If durability after rename cannot be established, the instance refuses further reads and writes until reopened and reconciled; clients must not assume the unacknowledged operation did not take effect.

Reference get/list/watch/revision calls replay the WAL while holding the same file lock, so cooperating single-host readers see the latest completed reference transaction. This is expensive and does not create distributed linearizability, a replicated watch stream, or multi-node HA. Storage paths and hosts are assumed trusted. The journal's SHA-256 detects accidental changes, not tampering by an attacker who can rewrite frames and checksums.

Python source files are not accepted in this repository.
