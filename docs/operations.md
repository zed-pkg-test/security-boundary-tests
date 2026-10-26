# Bootstrap and operations

## Local development
1. Compile the Oreslang runtime and run backend semantic tests.
2. Build the pure Orestra modules.
3. Start a local authoritative API + state service once implemented.
4. Enroll a node-agent using a deliberately short-lived development identity.
5. Apply one Workload, observe assignment, then exercise deletion.
6. Inject a node failure; verify NotReady is distinct from physical stopping.

**There is not yet an executable end-to-end cluster bootstrap command.**
Current repositories contain pure Oreslang modules and test oracles only.

## Production prerequisites
At least three independent control-plane state replicas for quorum,
persistent storage with backups, secure PKI issuance/rotation, API admission
and RBAC, CNI/CSI integration, host runtime isolation and disaster recovery.

## Day-two concerns
Certificate expiration, quota, time skew, lease renewal, resource eviction,
orphaned sandboxes, stale finalizers, node drain, upgrades, capacity planning,
store compaction, restore dry runs and observability cardinality.

## Safe upgrades
Stage schema conversion, dual-read or explicit migrations, version-skew gates
and verified downgrade path. Controller generations must not apply outdated
schema writes after leadership changes.
