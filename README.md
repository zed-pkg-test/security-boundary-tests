# Orestra Conformance

Reproducible failure-oriented conformance scenarios and executable local reference models. All tracked reference and verifier source in this repository is Rust; Orestra itself targets **Oreslang**.

Run `cargo test --all-targets`. The separately hardened trusted-plan runner remains in `tools/trusted-plan` and is tested with its own Cargo manifest.

Current scope includes deterministic reconciliation, bin packing, capacity invariants, fencing epochs, unknown node workloads, scale up/down, a 500-step seeded state-machine fault campaign, native-source runner boundary tests, and private exact-tree carrier provenance/authorization tests. See `docs/TEST_MATRIX.md` for unimplemented live cluster and security coverage.

Tracked Python source files are not accepted. The carrier keeps its source-free pre-check before checkout and executes repository verifier/test-plan logic only after authorization.
