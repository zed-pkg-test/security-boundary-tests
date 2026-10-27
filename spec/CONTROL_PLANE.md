# Orestra control plane: minimal viable design

## Trust boundaries
1. Only the state backend is authoritative about resource versions and lease epochs.
2. API workers authenticate callers, authorize every operation, validate schemas,
   and write using conditional transactions.
3. Controllers consume at-least-once watch hints. Every handler relists/reads
   authoritative state before issuing version-guarded mutations.
4. Node agents authenticate with unique renewable identities; stale node epochs
   must not authorize commands.
5. OCI runtimes remain separate processes with OS-level sandboxing; actor heaps
   alone do **not** sandbox untrusted container workloads.

## Lifecycle
- Desired objects: Deployment, Workload, Node, Reservation, Lease.
- Workloads move Pending -> Assigned -> Running -> Succeeded|Failed.
- Deletion sets intent, transitions to Terminating, then removes finalizers.
- A workload is never placed twice by a legitimate scheduler commit;
  a partitioned worker may still run a stale copy until externally fenced.
- A scheduler may calculate proposals speculatively, but reservation and
  workload binding commit atomically with preconditions on Node UID, Node
  generation, Workload UID and version, free capacity and scheduler epoch.

## One-node prototype
Run an API service, single-process state backend, scheduler actor, deployment
controller actor, and node-agent as separate Oreslang modules. The language
routines in src/ currently provide **pure core logic only**; OS/process/HTTP
wiring is a separate milestone tracked in docs/IMPLEMENTATION_GAPS.md.

## Failure taxonomy
- Watch disconnect: relist, resume from returned revision.
- Scheduler crash: leadership re-acquired at higher fencing epoch.
- API restart: state remains durable; clients retry idempotently with UID keys.
- Node heartbeat timeout: NotReady; delay eviction using policy and fencing.
- Storage quorum loss: stop mutating; never silently fail open.
