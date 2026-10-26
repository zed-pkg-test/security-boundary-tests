# Durable store, leases and ordered watches

## API invariants
- Resource UID is stable across object updates; name can be reused.
- resourceVersion is opaque; CAS compares for equality, not ordering.
- Generation changes when desired spec changes; status writers cannot silently
  modify spec.
- List has a consistent snapshot revision R. Watch strictly after R includes
  later committed mutations, possibly duplicate notifications on reconnect.
- A watch cursor older than compaction returns RESET_REQUIRED, never a
  plausible but incomplete stream.

## Persistence
The production store requires WAL checksums, fsync policy, verified restart,
snapshot integrity checks, replica recovery, leader transfer, membership change
handling and backups with point-in-time restore tests.

## Fencing
Leadership must be checked by the authoritative state machine in the same
transaction as mutation. An expired application-local timer is not enough.
An epoch must never roll back after a restore or cluster reconfiguration.

Node-side effects also need agent epochs and external fencing if single-writer
constraints matter; quorum consensus alone cannot kill a partitioned process.

## Bootstrap mode
A single process in-memory oracle can validate API semantics, but must not
present itself as durable or HA. An explicitly labeled single-node development
backend may use SQLite with transaction boundaries before Raft is available.
