# ADR 0001: Actors own mutable state, Tasks run bounded work
Status: Accepted for initial architecture.

Controller actors own their mutable caches and queues. Long-lived per-resource
actors are not required; keys can be partitioned across a bounded pool.
Scheduling filtering/scoring is pure work that may run on scoped Tasks.
The authoritative store serializes commits; no mutex protecting an in-memory
node cache may be treated as a cross-process reservation. A controller
must recover from its supervisor restarting it without corrupting state.
