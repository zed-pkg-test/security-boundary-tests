# Native Oreslang production policy

Orestra production/runtime behavior is implemented in native Oreslang.

Shipping control-plane, State, scheduler, agent, driver, SDK, CLI, API and
service code must be native `.ores` source. CI uses a positive allowlist:
tracked entries in production roots must be regular Oreslang source blobs.
Extensionless executables, host-language source, symlinks and submodules do not
qualify merely because they are placed in a runtime directory.

The policy also scans the whole repository for host-language source. Python,
JavaScript/TypeScript, Java/Kotlin, Go, Rust, C/C++, .NET, Ruby, Perl, PHP,
Lua, Swift and shell source are permitted only in explicitly non-production
trees such as tests, CI/build tooling, documentation, formal models and
reference/oracle fixtures. New top-level runtime directories cannot bypass the
rule.

Native production source may not import or otherwise reach into
`reference/`, `tests/`, `tools/`, `scripts/`, `formal/` or `model/`.

Non-Oreslang code is permitted only for tests, CI/build tooling,
schema/code-generation inputs, documentation, formal verification, and
explicitly labeled reference/oracle fixtures. Those files are never runtime
authority and must not be wired into production entry points.

A host-language reference implementation cannot become the Orestra API server,
CLI transport, scheduler, State authority, node agent, driver runtime or SDK
transport. Production behavior must first exist and pass native Oreslang
conformance.

Organization-level qualification additionally SHA-verifies and
parse/type-checks every shipping `.ores` blob with the funded exact-tree
Oreslang compiler carrier. This prevents host-language content renamed to
`.ores` from satisfying the path policy.

Repository-local Rust documentation verification lives under `tools/`, outside production source roots.
