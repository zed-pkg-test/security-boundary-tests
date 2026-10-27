# Orestra API

Versioned resource contracts for Orestra's declarative orchestration plane.

See [schemas](schemas/v1alpha1), [gRPC contracts](proto/orestra/v1alpha1/control.proto), [OpenAPI](openapi/orestra-v1alpha1.yaml), and [compatibility rules](docs/COMPATIBILITY.md).

Run `cargo run --bin validate-api-contracts` for JSON Schema Draft 2020-12
validation and `cargo test --all-targets` for the executable contract suite.
Contracts are alpha and MUST NOT be advertised as Kubernetes API compatible.

## Executable protocol reference

`reference/http_api.rs` is a test-only localhost HTTP service used by CI to
exercise authentication, namespace RBAC, schema admission, leader fencing,
exact CAS, bounded pagination and integrity-protected watch resume cursors over
a real socket. It is **not** the production Oreslang/mTLS API server.

## P0: server-owned leadership fencing

The Rust HTTP reference does not accept a client-controlled
`X-Leader-Epoch` header as proof of write authority. Incoming PUT and DELETE
requests carrying this header fail closed. A server instance snapshots its
locally established epoch when created, and the state authority verifies that
epoch on every write. After leadership turns over, the old instance cannot
mutate the store even if clients know the current epoch.

This is only a single-process reference contract. Production Orestra must bind
API serving authority to an actual authenticated, quorum-backed leader lease
and prevent old leaders from writing after term changes. Internal state/store
transactions must evaluate fencing epochs atomically with resource writes.
Client identity and authorization never confer controller/leader authority.

The leadership counter in the reference state authority increases
monotonically. Older epochs cannot be set back to reactivate previously fenced
API workers. Production leadership remains dependent on quorum and durable
persisted epochs, not this in-memory Rust reference.

Python source files are not accepted in this repository.
