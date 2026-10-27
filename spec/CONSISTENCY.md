# Storage contract

The in-memory oracle in `model/store.py` is deterministic test support only.\n`model/journal.py` adds a single-process, fsync-backed crash-recovery oracle with\nchecksummed frames for restart and torn-tail tests; it is still not consensus.
It intentionally does **not** implement WAL, Raft, fsync, snapshots, encryption,
membership changes, clock-safe lease enforcement or a network API.

Production acceptance requirements:
1. Every mutation has one linearization point backed by replicated consensus.
2. Revision allocation, CAS, resource binding and reservations are transactional.
3. Leader fencing epochs never decrease, including across snapshot restore.
4. Watch revisions cannot silently skip durable committed changes; compacted
   readers receive RESET_REQUIRED and relist.
5. Lease timing MUST use an authority that prevents a paused or partitioned
   leader from committing stale effects. Local clock checks alone are insufficient.
6. Storage compaction preserves object state and monotonic revisions.
7. Recovery after crash or partial WAL tail never acknowledges lost writes.
8. Membership changes require a proven joint-consensus or equivalent protocol.
9. All reads/writes/watches enforce authenticated tenant and namespace scope.
10. Recovery, backup, encryption, repair and observability must be testable.

Oreslang ports:
- `src/epoch.ores`: epoch gating helpers
- `src/revision.ores`: revision and watch cursor predicates
- `src/raft_math.ores`: pure consensus math (not consensus)\n- `src/wal.ores`: pure recovery/durability invariants for the future native backend

Before promoting this subsystem, build production storage in Oreslang and
cross-check it against the reference oracle under deterministic fault injection.
