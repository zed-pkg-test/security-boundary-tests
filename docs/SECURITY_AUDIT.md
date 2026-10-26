# Driver security audit

Earlier manifest checks used Python `assert`, which `python -O` removes.
Current checks raise explicit ValueError and are exercised with optimized
and regular Python. Driver manifests are declarative descriptors only:
their implementation field is an allowlisted symbolic registry key, never
a filesystem path or command. The validator rejects extra fields, duplicate
policy entries, invalid ABI, mismatched runtime contract, and unverified
driver readiness claims.

These controls validate metadata, **not an executable driver**. Production
driver execution requires signed artifact verification, secure IPC
authentication, per-plugin authority limits, host syscall restrictions,
rollback and device/volume fencing.
