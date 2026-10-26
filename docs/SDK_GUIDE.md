# SDK usage and boundaries

This repository defines pure Oreslang helper routines and the client contract.
Network transport bindings, protobuf generation, TLS and complete typed API
models will follow `orestra-api` versioned contracts.

CPU quantities are millicores; memory quantities are bytes. Negative or zero
requests fail closed. The retry policy uses capped exponential backoff with
additional per-request jitter to be added by the caller once a suitable secure
entropy source exists. Never silently loop forever on 429/503; honor deadlines.

The SDK must operate under Oreslang's `core / alloc / stdlib` split and never
require operating-system capabilities merely to construct value objects.

## Resource versions and watch cursors

ResourceVersion is an **opaque string**, not a sequential integer. CAS must
compare nonempty values for equality; clients must not sort, increment or
parse them. List-watch starts after a snapshot version; watch resumption
uses a per-event opaque token, because one transaction may emit multiple
events with a shared revision. See orestra-api/docs/WATCH_PROTOCOL.md.
