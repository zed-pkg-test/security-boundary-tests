# Orestra Drivers

Oreslang core source modules for runtime, CNI, CSI and driver ABI admission.
A versioned driver protocol and machine-readable plugin manifests define
future OCI, CNI and CSI integration.

**Important:** All provided driver manifests are `status: planned`.
There is no OCI executor or CNI/CSI binary implementation here yet. The
manifest validator and executable process-lifecycle oracle are Rust
development/test tools, not the runtime. The process oracle invokes only its
configured executable directly and never invokes a shell.

Run the contract suite with `cargo test --all-targets` and validate manifests
with `cargo run -- manifests`.

See docs/SECURITY.md for requirements before permitting host operations.
