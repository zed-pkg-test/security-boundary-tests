# Drivers and host security

Actors and actor arenas do **not** provide kernel sandbox boundaries.
The node agent delegates container isolation to an audited OCI runtime on Linux.
The adapter must:
- Verify caller capability, node identity and generation before host actions.
- Authenticate the runtime endpoint; never accept arbitrary executable paths
  or unvalidated host file mounts from user-supplied YAML/JSON.
- Use cgroups v2, Linux namespaces, seccomp, capabilities and LSM profiles
  according to configurable security policy.
- Default deny privileged mode, host PID/network, host devices and root host
  filesystem mounts; require explicit policy exceptions and audit them.
- Make start/stop idempotent by workload UID + generation + node agent epoch.
- Treat reported process termination and detached network/volumes separately.
- Use an allowlisted binary/runtime registry, never shell interpolation.
- Enforce CNI DEL on rollback and CSI detach only after mount quiescence.
- Reject unsupported plugin ABI and authenticate plugin processes.

These are requirements, not completed features. OCI, CNI and CSI are
interfaces, not claims of compatible implementations.

## Process-oracle process-group hardening

The Rust test oracle starts each child in a dedicated POSIX process group.
Status and stop therefore treat the launcher PID and its descendants as one
physical lifetime: a reaped launcher is not sufficient evidence that the
sandbox stopped. Stop sends TERM and then KILL to the entire group and returns
`Stopped` only after the group can no longer be observed; uncertain group
absence fails closed.

This remains a development oracle, not containerd/OCI, cgroups, namespaces,
seccomp, or an authenticated external fencing receipt.
