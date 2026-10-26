# Orestra client contract

1. Mutations require an explicit expected resourceVersion (create-only if none).
2. A 409 cannot be retried by replaying the stale body; re-read and recompute.
3. Mutating operations are only retried automatically if they have an idempotency key
   or strong, version-checked semantics.
4. Watch from an exact list snapshot revision, and relist on RESET_REQUIRED.
5. Every request carries authenticated identity; authorize per verb and namespace.
6. Requests have deadlines, cancellation and bounded retry budgets.
7. TLS peer identities are validated; do not disable verification to work around
   test/self-signed certificates in production.
8. Typed resource UIDs, names and versions are distinct; a reused name is not
   evidence of the original UID.
9. Errors are structured: code, retryable flag, machine-readable reason and
   human-readable message; do not leak sensitive payloads.
10. Clients must not assume a successful API write means a workload is Running.

## Watch recovery

Persist the latest **individual event resume token** after handling the event.
Do not resume from the transaction's revision: this may omit siblings of a
multi-object transaction. If RESET_REQUIRED, relist from a consistent snapshot
rather than silently skipping changes.
