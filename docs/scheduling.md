# Scheduling and resource reservation

## Placement pipeline
Filter by node readiness, taints/tolerations, affinity, runtime support, CPU,
memory and resource claims. Score feasible nodes using deterministic integer
criteria, tie-breaking by stable Node UID. Hard constraints are checked **again**
at commit time.

## Authoritative transaction

```text
transaction(reserve(workload, candidate, schedulerEpoch)):
  assert currentLeadershipEpoch == schedulerEpoch
  assert workload.uid == expectedUid
  assert workload.specVersion == expectedVersion
  assert workload.binding is empty
  assert node.uid == expectedNodeUid && node.ready
  assert free(node, existingReservations) >= request(workload)
  create reservation with unique idempotency key
  set workload.binding = node.uid
  commit
```

All checks and writes must share one transactional linearization point.
Scanning a read-only snapshot, or using a lock local to one scheduler actor,
cannot prevent overcommit across scheduling replicas.

## Fair-share, preemption and gang allocation
Add fair queues after basic correctness. Preemption must select victims using
policy, not blindly kill. Gang admission must reserve all members atomically or
roll back with consistent recovery; otherwise support only independent jobs.

## Resource model
CPU uses millicores, memory bytes and explicit extended resources. Quantity
conversion and integer arithmetic must reject overflow rather than wrap.
Future: NUMA, huge pages, accelerators, topology spread and network bandwidth.
