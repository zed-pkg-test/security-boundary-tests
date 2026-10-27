# ADR 0002: Placement is a conditional transaction
Status: Accepted for initial architecture.

Use a stable Workload UID, Node UID, leader epoch and monotonic resourceVersion.
A successful proposal atomically creates a Reservation and binds the workload,
checking node allocatable capacity **including outstanding reservations**,
scheduler lease validity, and workload still Pending.
Retry on conflict; never invent a successful scheduling result.
Never call a speculative `fits` predicate a hard admission guarantee.
