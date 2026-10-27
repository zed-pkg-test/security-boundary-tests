# M1 experimental native one-node bootstrap

`examples/native-bootstrap.ores` is a **source-level bootstrap attempt**:
it declares three independently spawned actors (state, scheduler, node
agent), awaits their startup barriers, and exposes a bounded loopback HTTP
listener through isolated pooled request handlers.

The actors currently only acknowledge simple `boot` and `shutdown` control
messages internally. They **do not** store objects, perform CAS, enforce lease
fencing, execute containers, supervise crashes, authenticate a node, or
exchange scheduler reservations. The experimental actor/HTTP fixture now has **targeted GraalVM/JDK 25
execution evidence**, described below. This does **not** make the actors
operational or establish LLVM actor/HTTP transport parity, native-image AOT,
replicated state or strong process fencing.

## Explicit readiness semantics

* `GET /healthz` responds HTTP 200 with a process-level liveness indicator.
* `GET /readyz` **always responds HTTP 503** until actual durable state,
  scheduler reservation, authenticated node-agent authority and recovery
  constraints are verified.
* `/v1alpha1/**` responds HTTP 401 and has no mutation, watch or list API.
* Unknown routes respond HTTP 404.

The listener binds only `127.0.0.1`; it accepts exactly 16 requests before
draining and requesting shutdown from all three actors. It prints the selected
port as its first stdout line. This is a finite harness fixture, not an API
daemon or production node. A functioning HTTP 200 endpoint is **not** cluster
readiness.

## Compile/execute gate

Run with an independently provisioned, pinned Oreslang compiler and executable
host; check the actual backend identity, matching compiled source tree,
runtime capability grants, and nonzero failure signals. Prefer the black-box
loopback HTTP integration test from `orestra-conformance/scripts/run_native_http.py`,
once it is installed, for both positive and negative requests. The source
checks in this repository are only structural and never substitute for the
actual compiled test.

## Verified targeted GraalVM guest execution (October 20, 2026)

The byte-identical source fixture was compiled/typechecked and executed as a
GraalVM Truffle guest under JDK 25 with the compiler's native carrier mode.
The JUnit integration test exercised all **16 real loopback HTTP requests**
(including health, denied readiness, unauthorized GET/PUT/DELETE and unknown
routes), validated exact JSON/status and drained the bounded actor runtime.

* Origin compiler PR: `ores-truffle-oreslang/oreslang-source.java#1711`, pinned
  commit `9eb092fbaa8939e8d21629e2dd8d2986c3de8966`.
* Fixture Git blob in both repositories: `4b3ea92ec1bf4edd335e04c5d927a4c4c43ed603`.
* Funded runner: `orestra/orestra-conformance`, [successful targeted run](https://github.com/orestra/orestra-conformance/actions/runs/39501208817),
  job `118898868482`.
* Maven Surefire: `OrestraBootstrapNativeHttpTest`, **1 run, 0 failures,
  0 errors, 0 skipped; BUILD SUCCESS**.
* The CI provenance gate compared all **103 original `src/main` Git blobs**
  byte-for-byte using Git SHA and verified the identical fixture, JUnit source,
  Maven manifest, carrier builder script and standard-library files.

**Scope boundary:** the carrier is still missing unrelated files from the
original compiler's full **763-blob** Git tree. The result establishes real
runtime execution of the exact compiler production-source subtree and fixture,
**not complete-Git-tree equivalence**, full compiler-suite regression coverage,
OCI/containerd behavior, native-image AOT, multi-node coordination or readiness.
The full-tree CI gate and negative distributed-fencing scenarios remain open.

## Subsequent implementation

Replace each boot-only actor with a real subsystem:
1. State actor — serialized authenticated CAS + transactional durable WAL,
   lease epochs, cursor-scoped ordered watches, corruption fail-closed.
2. Scheduler actor — snapshot proposal only; commit reservations in the
   authoritative store against exact UID/version/epoch/capacity preconditions.
3. Node agent — mTLS identity, epoch authorization, idempotent runtime commands,
   and externally verified OCI absence before resource release.
4. API ingress — authn, RBAC, admission, bounded body decoding and server-owned
   fencing epoch. Never accept leader authority from an arbitrary client header.
5. Crash/partition tests — prove unknown running workloads retain reservations
   until a trusted external fence receipt is committed.
