# Watch resumption must not lose part of a transaction

One transaction may mutate multiple resources with a **single revision**.
A revision-only watch resume token can silently lose later events from that
revision if the stream disconnects after delivering only the first event.

`MemoryStore.watch_after(snapshot_revision)` begins from a consistent list
snapshot. Every emitted `Event` has an individual `cursor` of
`revision:ordinal`, and `watch_resume(event_cursor)` resumes *after that
particular event*, even when subsequent events share its commit revision.
Compacted or unknown event cursors trigger RESET_REQUIRED and relisting.

The `revision:ordinal` format is test-only. Production cursors should be
opaque/versioned, authenticated to prevent cross-tenant replay, bounded in
length, and checked against the durable event log. Prefer monotonic global
event sequence numbers or a transaction index+offset pair, with stable replay
semantics after restart. A status bookmark cursor is separate from a list
snapshot revision.
