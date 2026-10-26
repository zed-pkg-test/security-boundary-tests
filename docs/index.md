# Orestra engineering documentation

Orestra is a pre-alpha orchestration system designed to be implemented in
Oreslang. The design draws useful API/controller separation from Kubernetes but
does not claim Kubernetes compatibility yet.

## Start here
- [Architecture](architecture.md): authority boundaries and processes
- [Control plane](control-plane.md): reconcilers and actor lifecycle
- [Scheduling](scheduling.md): placement and reservations
- [State](state.md): transactions, leases and watch consistency
- [Nodes and workloads](node-lifecycle.md): desired vs observed execution
- [Security](security.md): multi-tenancy and host isolation
- [Operations](operations.md): bootstrapping and failure recovery
- [API compatibility](api-compatibility.md): OCI, CRI, CNI, CSI, K8s adapters
- [Testing](testing.md): conformance and fault injection
- [Milestones](milestones.md): actionable implementation gates

- [Audit report (October 19, 2026)](AUDIT_2026-10-19.md)
- [Release gates and integration versions](RELEASE_GATES.md)
