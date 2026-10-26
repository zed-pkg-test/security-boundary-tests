# Security model

## Trust tiers
1. Untrusted workloads: container/VM/WASM sandboxes. Never expose control
   plane actor pointers or host memory as an implicit capability.
2. Node agents: privileged, narrow RPC surface, authenticated/authorized.
3. Control plane: independent service identities, mTLS, policy-enforced APIs.
4. Durable store: private network, authenticated quorum members, encrypted
   backups and audited admin operations.

## Multi tenancy
Every object has a namespace/tenant identity enforced at storage-admission
boundaries, not by namespace string filtering in UI alone. A tenant may not
watch other tenants or guess UIDs to bypass authorization. Enforce quota at
the transaction commit boundary; admission-time quota checks alone can race.

## Secrets and provenance
Do not put credentials in manifests, logs, shell command lines or CI workflow
outputs. Container images should use immutable digests for reproducibility,
signature/attestation policies and content-addressed OCI layers.

## Required adversarial tests
Tenant breakout, unauthorized list/watch, stale leadership write, actor
capability escape, arbitrary host mount, Linux namespace traversal, plugin
path injection, symlink races, container image credential leakage, faulty
certificate rotation, and rollback to vulnerable code generations.

## Branch and CI
Require review and reproducible builds before production release; never use
untrusted pull-request code with privileged repo or cross-org publish tokens.
