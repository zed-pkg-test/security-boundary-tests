# Implementation status and explicit gaps

Working sources here implement Oreslang-side pure placement, replica and
lifecycle decisions and a smoke example; the system is **not a runnable
Kubernetes replacement** yet. Do not confuse a spec with an implementation.

Required dependencies and integrations:
- Oreslang compiler/backend stable enough to build multiple `.ores` modules
  on Java/GraalVM and LLVM using the same syntax.
- HTTP/2 or REST API listener, TLS/mTLS, schema validation and RBAC.
- Durable transactional KV implementation with CAS, ordered watch and epochs.
- OCI/CRI runtime adapter, process isolation, CNI and CSI boundaries.
- State reconciliation, restart recovery, scheduler loops and health signaling.
- Cluster fault tests including partitions, clock skew, duplicate watch events,
  filesystem failure, and process termination.

Release gates: deterministic conformance, multi-node integration, security
review, OSS dependency/OCI image provenance and upgrade/rollback tests.
