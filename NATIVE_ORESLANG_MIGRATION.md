# Orestra native Oreslang migration inventory

Orestra's production/runtime implementation is native Oreslang.

This inventory records the remaining non-Oreslang executable files in the
organization and why they are **not** part of the production runtime.

## Native production repositories

The shipping runtime/source directories in these repositories are Oreslang-only
at the time of this audit:

- `orestra/orestra`: control-plane scheduling/controllers/agent/API guards.
- `orestra/orestra-api`: native API admission guards; production HTTP server
  transport is not yet implemented and must be Oreslang before release.
- `orestra/orestra-cli`: native CLI command/status/format sources and entry
  point; live cluster transport is not yet implemented and must be Oreslang.
- `orestra/orestra-sdk`: native SDK sources; live transport must remain
  Oreslang.
- `orestra/orestra-state`: native admission/state/WAL/transaction milestones;
  Python model code is reference/oracle only.
- `orestra/orestra-drivers`: native driver admission/lifecycle guards; a real
  OCI/CNI/CSI executor is not yet implemented and must be Oreslang.
- `orestra/orestra-conformance`: no Orestra production runtime is shipped
  from this repository.

CI policy PRs reject host-language code under shipping runtime directories
(`src/`, `bin/`, `runtime/`, `native/`, `cmd/`).

## Remaining non-Oreslang executable files

These files are allowed only because they are outside the production/runtime
boundary:

### Test/reference/oracle code

- `orestra-api/reference/http_api.py`: executable HTTP protocol reference,
  never the production API server.
- `orestra-state/model/*.py`, `formal/*.py`, `tests/*.py`: oracle,
  differential, formal and regression testing only.
- `orestra-drivers/reference/process_runtime.py`: process-lifecycle test
  oracle only; never the production node runtime.
- `orestra-conformance/reference/cluster.py`: deterministic conformance oracle,
  never the production scheduler/state machine.
- Python/Rust files under `tests/`, `scripts/`, `tools/` are CI/build/test
  tooling only.

### Browser presentation

- `orestra-ui/assets/*.js` and `orestra-website/assets/*.js` are browser
  presentation/progressive-enhancement code only.
- They may not implement State authority, scheduling, admission, fencing,
  reservations, node-agent authority, driver/runtime authority, or security
  decisions. Those remain native Oreslang server responsibilities.

### Compiler carrier evidence

Some `orestra-conformance` branches mirror Java/C compiler/runtime source
under `source/` to obtain funded exact-tree execution evidence. That mirrored
compiler code is not an Orestra product implementation and must never be
packaged as Orestra runtime code.

## Release rule

A feature is not considered implemented in Orestra production merely because a
Python/Rust/JavaScript reference or test exists.

Before release, the behavior must:

1. exist in native `.ores` production source;
2. pass native compiler/backend conformance;
3. pass the relevant durable/fencing/security tests;
4. not depend on a host-language reference implementation at runtime.

Current known production gaps that still require native Oreslang implementation
include the full authenticated API server, native CLI live-cluster transport,
real OCI/CNI/CSI execution, general State object-map/WAL service, authenticated
workload termination/release, LLVM parity, and quorum-backed HA.
