# Engineering execution plan

## M0 source foundation
- [x] Versioned resource schemas and transport contracts
- [x] Oreslang pure scheduling/controller predicates
- [x] Reference CAS/watch and scheduler simulation
- [x] Reference fault tests and CI definitions
- [ ] Cross-backend Oreslang compiler execution verified

## M1 first local live cluster
- [ ] Oreslang HTTP API listener + admission/RBAC
- [ ] Local transactional persistent store backend
- [ ] Oreslang scheduler actor + work queue
- [ ] Oreslang deployment controller actor
- [ ] Linux OCI runtime node adapter and lifecycle
- [ ] One-command launch and real 1-node smoke test

## M2 multi-node
- [ ] Authenticated agent registration, watch/relist reconnect
- [ ] Node leases, versioned reservations, failed-node handling
- [ ] Rolling updates, services and readiness probing
- [ ] Three-node failure/recovery suite

## M3 production HA
- [ ] Proven replicated Raft storage and WAL/snapshots
- [ ] Stale-leader and external-effects fencing
- [ ] Key management, secure RBAC/admission and audit
- [ ] Backup/restore and skew/partition chaos tests

## M4 ecosystem
- [ ] OCI/CRI/CNI/CSI compatibility and conformance
- [ ] Persistent workloads, autoscale, upgrades and extensibility
- [ ] Scheduling plug-ins and multi-cluster federation
