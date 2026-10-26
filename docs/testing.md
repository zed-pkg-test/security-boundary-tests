# Verification strategy

## Layers
- Pure Oreslang unit tests: placement math, FSM transitions, quantities.
- Reference models: deterministic state CAS/watch, scheduler/convergence.
- Backend parity: compile identical `.ores` inputs with GraalVM and LLVM.
- API integration: auth, version conflicts, watches, list/compaction.
- OCI integration: Linux namespaces/cgroups, verified process lifecycle.
- Distributed chaos: crashes, partitions, disk full, slow clocks, rotation.
- Security: adversarial tenant and driver interactions.

## Formal models
Use TLA+ to model invariants around Raft terms, lease fencing, reservation
atomicity and workload generations. Couple them with executable refinement
tests. Test linearizability using histories of concurrent operations, not
just a sequential unit test.

## CI
Cross-repo check matrices should pin commit SHAs of versioned API and SDK
contracts, not float against unrelated default branches. Never mirror
private sources into a different organization without explicit authorization
and preflight destination verification.

## Release gates
No blocking correctness defects; no successful stale-epoch commits;
deterministic reproducible CI; stress/load benchmarks with defensible
resource limits; signed artifacts; migration and backup restore proven.
