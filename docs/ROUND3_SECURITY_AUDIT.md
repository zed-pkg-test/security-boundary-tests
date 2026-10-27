# Orestra native-bootstrap / API boundary audit (2026-10-20)

Status: **pre-alpha, fail-closed scaffold**, not an operational Oreslang
orchestration cluster. This document records concrete controls and remaining
release blockers rather than conflating reference-model CI with native proof.

## P0 findings and dispositions

| Boundary | Failure mode | Mitigation / evidence | Remaining gap |
| --- | --- | --- | --- |
| Actor-owned memory | Mutable instance fields outside the only `state` schema can bypass critical state loans. | M1 actor bootstrap is now deliberately **stateless**; no per-actor fields or cross-actor mutable references. | Native state/scheduler/agent actors must later declare at most one `state { ... }` schema and access it only through verified critical handle_state / state loans. |
| Boot readiness | Actor `get_ready()` proves startup, not complete state recovery or successful `boot` message handling. | `/readyz` is unconditionally 503; `/healthz` means **process liveness only**. | Explicit authenticated recovery/lease, scheduler/agent handshakes and drain-safe readiness transition. |
| HTTP / authorization | Guessable client-supplied leader epoch or unprotected mutation routes. | API Python reference rejects `X-Leader-Epoch` writes and binds authority to server epoch; M1 mutation routes return 401 without state side effects. | Native authenticated API, per-verb RBAC, mTLS, cluster service identities, HTTP ingress limits and audit trail. |
| Leader handoff | A stale API worker resumes writes after leadership rollback. | Reference leader epoch is monotonic; cached prior-server epoch is fenced. | Durable quorum epoch authority and replicated atomic fencing transactions. |
| List / watch | A valid cursor stolen by a second principal or reused after grants change; infinite responses; skipped multi-event transactions. | HMAC-signed identity+RBAC-scoped cursors, max 100 watch entries per call, event-retention compaction with 410 relist, revision-only `after=R` strictly excludes all events of R. | Persistent multi-event ordinal watches, stable cursor signing keys, authenticated principals, replay-safe compaction across HA failover. |
| Runner / provenance | Shell/env injection, byte-swapped compiler or guest fixture, simulated success output. | Runner digest, source digest, sanitized launch environment, bounded process/output, 16-request black-box HTTP assertions. | Actual trusted GraalVM guest run + artifact/toolchain attestation; no LLVM actor HTTP parity yet. |
| Node partition | Heartbeat loss incorrectly releases capacity and duplicates still-running work. | Reservation/oracle gates hold Unknown and Terminating capacity, reject stale UID epochs. | Real cgroup/containerd process identity, externally verifiable fence receipts, quorum state commit and fault-injection. |

## Known native execution blocker

The Java/GraalVM compiler PR mirroring this bootstrap source is
`ores-truffle-oreslang/oreslang-source.java#1711`.
The mirrored Oreslang source Git blob is
`4b3ea92ec1bf4edd335e04c5d927a4c4c43ed603`.
The targeted GitHub Actions job did not return an executed test trace or
usable logs; its red result is **not** evidence that the Oreslang source failed
to compile. Do not mark any backend green without executed compiler/runtime
evidence matching the exact tested Git tree.

The Orestra structural CI validates braces, imports, fixture inventory only.
The Python API and HTTP probe tests are valid **reference oracle / harness**
evidence, not native compiled guest or distributed control-plane evidence.

## Release blockers and order

1. Run the exact-byte GraalVM fixture in a trusted funded Actions environment
   with an independent source/tree hash and a passing trace of all 16 requests.
2. Implement a native Oreslang state actor with a single state schema,
   critical state access, WAL durability, lease and CAS, and ordinal watch.
3. Connect a scheduler actor to **atomic authority-side** reservation commits;
   do not use scored snapshots as capacity authority.
4. Connect authenticated node agents to OCI/containerd, cgroups v2,
   persistent workload identity and **physical stop evidence**.
5. Introduce quorum replication, epoch-fenced failover, partition/crash
   testing, key rotation, source/OCI provenance and operational upgrades.
