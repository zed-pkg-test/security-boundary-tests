# Conformance roadmap and threat-oriented test matrix

| Area | Test cases | Implemented here |
|---|---|---|
| Scheduler | no overcommit, integer deterministic tie breaks, no duplicate UID | reference |
| Controllers | replica drift, repeated reconcile, scale down | reference |
| Fencing | stale scheduler epoch denied | reference |
| Nodes | heartbeat timeout, unknown process vs confirmed terminated | reference (partial) |
| State | CAS, list/watch gaps, compaction, WAL crashes | test in orestra-state (WAL pending) |
| Runtime | OCI execution, namespace/cgroup isolation, restart loop | pending |
| Network | Service VIP, policy enforcement, CNI setup/rollback | pending |
| Security | RBAC, mTLS renewal, secrets exfiltration attempts | pending |
| Chaos | disk full, quorums, partitions, skew, kill -9 | pending |
| Lifecycle | safe upgrades, GC, finalizers, orphan cleanup | pending |

The random fault campaign has a deterministic seed and checks invariants on
every step; it does not implement a distributed failure model. Never describe
this suite as proving HA, linearizability, security or OCI functionality.

## Backend strategy
Run the same source-compatible Oreslang workloads against Java/GraalVM and
LLVM backends. A reference simulator helps check expected outcomes but does
not substitute for actual compiled execution.
