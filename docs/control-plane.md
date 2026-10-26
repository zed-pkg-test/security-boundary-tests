# Reconcilers and supervisor design

Each controller reads desired and observed state, computes a plan without side
effects, and applies version-guarded changes. Reconciliation is at least once
and safely repeatable; every watch event is not a command.

## Controller algorithm
1. Watch relevant resource families; persist last observed revision only as a
   resume hint, not as authority.
2. Queue keys in a bounded, coalescing, rate-limited work queue.
3. Read current object(s) and validate UID/generation/deletion state.
4. Compute pure plan: no changes, create missing resource, update status, or
   request finalizer.
5. Commit plan using CAS + fencing epoch; conflict triggers refreshed read.
6. Acknowledge and reschedule if dependencies remain unsatisfied.

## Supervision
Use bounded actor pools, not an actor per object. Partition keys by consistent
hash; sequential handling per key avoids intra-process races but does not
replace CAS for inter-process safety. Restarted actors rebuild caches from
snapshots and watch cursors. Cancellation must release pending reservations and
propagate to subtasks.

## Fairness
Per-tenant quotas and scheduler admission are separate: queued work cannot
starve another tenant, and transient retries need exponential backoff. Infinite
hot loops on conflicts are forbidden.

## Side effects
External effects (OCI starts, DNS writes, volume mounts) require unique
idempotency keys, generation checks and cleanup workflows. There is no
cross-machine exactly-once execution guarantee.
