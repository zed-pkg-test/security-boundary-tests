# Offline planning safety

Offline fit checks are advisory. They never reserve or bind workloads.
Untrusted node inventories must have unique names, real booleans, signed
64-bit nonnegative integer resource quantities, and coherent used/capacity
accounting. Invalid inventory fails closed instead of yielding candidates.

The CLI intentionally keeps its `lint` subcommand a limited shape check,
not a complete JSON Schema implementation. Use `orestra-api` contract
validation for full schema correctness. Production mutations must be
server-authorized, version-guarded and fenced.
