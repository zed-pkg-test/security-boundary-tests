# Orestra SDK

The Oreslang-native client library for Orestra resources.

Current code includes capped backoff, version-aware mutation guards, watch
cursor validation and canonical CPU/memory quantities, plus a runnable Oreslang
smoke example for a compatible backend.

**Status:** Early source foundation. No HTTP transport, generated RPC client or
real cluster connectivity yet. See spec/CLIENT_CONTRACT.md before adding them.

The source check is now Rust-first: run `cargo test --offline` and
`cargo run --offline -- --root .`. This only checks module structure,
balanced brace counts, and rejects standalone `&` or `|` in Oreslang source;
it is **not** a compiler or borrow-checker admission proof.
