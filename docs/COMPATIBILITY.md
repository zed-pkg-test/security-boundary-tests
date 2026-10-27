# v1alpha1 compatibility contract

Objects use a stable UID distinct from their namespace/name, a monotonic generation for user-intent changes, and an opaque resourceVersion for CAS.

## Write semantics
- Creates are conditional on absence; updates require an exact resourceVersion.
- All writes that act under a leader lease carry a fencing epoch validated by the authoritative state backend.
- Status and spec are logically separate writers; field ownership rules precede server-side apply.
- Deletion is intent-first; finalizers and termination are not instantaneous.

## List/watch semantics
- List returns a snapshot revision; watch streams changes strictly after it.
- A compacted revision returns RESET_REQUIRED; consumers relist and retry.
- Events may repeat, coalesce or arrive after a reconnect. Reconcilers must be idempotent.
- Persistent identity is UID, not name; names may be reused after deletion.

## Security
- Never accept the caller's claimed identity without authentication.
- Secrets are encrypted at rest with external key management and excluded from normal list responses.
- All authorization checks apply to list, get, mutate and watch.
