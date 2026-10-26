# Node and workload execution lifecycle

## Node join
Agent obtains a unique identity and authenticated certificate, receives
capabilities, observes runtime devices, and reports capacity. Node readiness
requires fresh liveness plus suitable local runtime/network/storage state.

## Workload scheduling
Pending -> Assigned after authoritative transaction. Agent checks UID,
generation, node epoch and idempotency key, prepares sandbox/volumes/network,
then starts via OCI. Status becomes Running only after observing runtime state.
Deployment controller reconciles desired replicas, not node agent.

## Node loss
Heartbeat loss transitions Node to NotReady/Unknown. Do not assume processes
terminated; a disconnected node may still be serving traffic. Service routing
can remove endpoints, but exclusive volumes need storage-level fencing before
reattachment to a new node.

## Deletion
Set deletion intent and deadline; stop sandbox, drain network, unmount volumes,
acknowledge cleanup, remove finalizers. Timeouts enter an operator-visible
stuck/unknown state, never a silent success.

## Security
Kernel namespaces, cgroups, seccomp, LSM policy and appropriately restricted
runtime credentials are needed. Oreslang actor isolation does not substitute
for container OS isolation.
