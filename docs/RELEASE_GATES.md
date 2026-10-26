# Release readiness and test pyramid

| Gate | Pre-alpha source | M1 single-node | M2 multi-node | Production |
|---|---:|---:|---:|---:|
| Schemas/negative tests | required | required | required | required |
| Pure controller/scheduler tests | required | required | required | required |
| GraalVM compiled runtime | pending | required | required | required |
| LLVM pure-source parity | pending | required | required | required |
| Live authenticated API | pending | required | required | required |
| Real OCI image lifecycle | pending | required | required | required |
| Transactional persistent KV | oracle only | required | required | required |
| Authenticated node heartbeat | pending | required | required | required |
| Network/storage driver integration | planned | selected | required | required |
| Replicated Raft control plane | planned | no | required | required |
| Partition/fencing chaos | reference only | simulation | required | required |
| RBAC, secret policy, audit, backup | planned | development | pre-release | required |

Do not promote a release past a required gate when runtime evidence is
missing, even if other repository CI checks are green.

## Multi-object events
List revision and watch event cursor are different concepts.
The state oracle, API protobuf schema, and SDK agree that a transaction
may emit two events with the same revision. Resume by individual event
cursor; relist after compaction.

## Pinned source set

See [integration lock](../integration/compatibility.lock.json) for exact
cross-repository commit SHAs. This lock does not pin the documentation
repository itself, avoiding a self-referential hash. Release artifacts must
also identify compiler SHA and execution backend; those are not verified yet.
