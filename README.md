# Orestra

**Oreslang-native orchestration control plane** (pre-alpha).

This is the primary source repository. Its initial implementation is pure
Oreslang scheduling, deployment, node-health and restart-policy logic, with
an architecture specification, smoke example and CI source checks. It is not
yet a networked cluster or Kubernetes replacement.

- `src/scheduler/placement.ores`: safe capacity checks and deterministic scoring
- `src/controllers/deployment.ores`: desired/observed replica reconciliation math
- `src/controllers/node.ores`: liveness predicates
- `src/controllers/leases.ores`: fail-closed fencing validation helpers
- `src/agent/restart.ores`: restart/relaunch predicates
- `spec/CONTROL_PLANE.md`: authority and failure boundaries
- `docs/IMPLEMENTATION_GAPS.md`: explicit work remaining

Run the Rust structural source/import gate using `cargo run -- --root .`;
run its fixture tests with `cargo test`. The checker rejects missing imports,
tabs, unbalanced brace counts, and insufficient source/example inventory.
It intentionally does **not** compile or execute Oreslang.

The Oreslang compiler is developed in
[ores-truffle-oreslang/oreslang-source.java](https://github.com/ores-truffle-oreslang/oreslang-source.java)
and its LLVM companion; run the smoke example with a compatible Oreslang
compiler build. The CI gate here is **structural only** until compiler
integration is configured.
