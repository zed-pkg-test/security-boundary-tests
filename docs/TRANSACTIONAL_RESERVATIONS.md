# Transactional placement oracle

`MemoryStore.reserve` binds a pending workload and writes its capacity reservation
at one committed revision under the reference-store lock. This is a **single-process
oracle**; it makes no persistence or network consensus guarantees.

Preconditions include workload UID/revision/phase, node UID/readiness, existing
reservation capacity, unique reservation key, and authoritative lease token.
Admission failure must leave both workload and reservation unchanged.

The production Oreslang store must implement the same behavior as one
replicated linearizable transaction. Test cases must cover competing schedulers,
expired leader epochs, node deletion/replacement, retries, reservation release,
quota enforcement, process crash during commit, and durability after ACK.

## P0 authority preconditions (reference oracle)

The `reserve` operation requires a caller-provided **node resource version**
(`node_version`) in addition to the workload version, immutable node/workload
UIDs, active lease epoch and positive integer resource quantities. Stale node
versions fail rather than allowing placement against a changed node.

The canonical reservation key is exactly `Reservation/<namespace>/<name>`,
where `<namespace>/<name>` matches `Workload/<namespace>/<name>`.
Arbitrary reservation keys must not evade resource accounting. Every new
reservation records its node key, UID and observed resource version.

If a node name is reused with a new UID while a reservation for the old UID
remains, the oracle rejects further placements on that name. Operators must
not interpret a new Node identity or a lost heartbeat as physical termination.
Production must gate removal of old reservations on independently verified
stop/fence evidence, and commit all checks atomically in durable replicated
state (not this Python test model).
