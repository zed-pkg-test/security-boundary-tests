# Native Oreslang production boundary

All Orestra control-plane/runtime authority is native Oreslang.

HTML/CSS/JavaScript in `assets/` is browser presentation only. It may render
or fetch data, but it is not allowed to implement authoritative State,
scheduling, admission, fencing, reservation, node-agent, driver, API-server or
security semantics.

CI now uses a positive allowlist for any authoritative/runtime directory:
tracked entries there must be regular `.ores` blobs. Host-language source,
extensionless executables, symlinks and submodules fail closed. Host-language
code elsewhere is allowed only in browser assets or explicit test/tool/docs
trees.

If this repository gains a server, API, State service, agent, driver, scheduler
or other runtime component, that component must be native Oreslang and pass
native compiler conformance. Browser assets remain presentation-only and are
never a security boundary.
