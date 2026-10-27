# Watch protocol and atomic multi-object transactions

WatchEvent.revision identifies the committed transaction; more than one event
may have that revision. A client that resumes using the last seen revision
can silently skip the remainder of the transaction.

Initial list returns a consistent snapshot revision R, and the client starts
WatchRequest(after=R). Every event supplies an opaque `resume_cursor`. On
disconnect, resume with WatchRequest(after_event_cursor=last_committed_cursor)
instead of sending the last event's revision. Both inputs are mutually
exclusive. On RESET_REQUIRED (compacted/expired cursor), obtain a new
consistent list and begin again.

Production cursor security requirements: stable across failover, scoped to
authorization context, bounded in size, integrity protected, and not accepted
across namespace/tenant visibility changes without a fresh list.

Event processing must tolerate duplicates. A client should not acknowledge
a cursor until the resource event has been durably applied to its local view
or it can recompute that view from a relist.

## Identity-bound continuation and bounds

The loopback API reference now HMAC-binds continuation cursors to the
authenticated principal **and its current namespace/verb grants**, in addition
to the watch/list operation, namespace, kind and revision. A token issued to
one bearer identity must fail with RESET_REQUIRED when replayed by another
identity, even if both have access to the same namespace. Changes to the
principal's RBAC grants also invalidate previously issued cursors.

Watch requests return at most 100 events per call (default 100, configurable
with `limit=1..100`). To consume a larger history, resume after the last
event's `resumeCursor`; never jump to that event's revision alone.
A cursor and an explicit `after` parameter are mutually exclusive.
Invalid/future watch revisions and oversized cursors fail closed with HTTP 410.

These protections are **single-process test-oracle behavior**, not distributed
compaction, stable cross-HA-failover cursors, or production identity attestation.
The reference cursor signing key is a fixed test secret; production must use
separately provisioned, rotated cluster keys and an authenticated principal
identity derived from a real transport/mTLS/OIDC boundary.

## Reference event-retention compaction

The in-memory HTTP reference now retains at most 256 watch events by default.
When history is discarded, a client attempting to resume before the
compaction watermark receives HTTP 410 and **must** obtain a fresh list
snapshot. The current reference store produces exactly one event per
revision; future atomic multi-object transactions must compact complete
revisions, never leave partial event ordinal histories. Production will
require durable compaction watermarks and failover-safe snapshot semantics.

The reference HTTP server now fails closed if asked to bind on an interface
other than 127.0.0.1. Token-only HTTP without TLS is not safe to expose to
other machines.
