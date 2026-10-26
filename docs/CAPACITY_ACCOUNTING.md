# Capacity accounting during uncertainty

A terminating workload may still consume resources until the node agent
confirms that its runtime process has stopped. An unknown workload may be
running after network partition or after its node rejoins. Therefore
capacity claims for **Running, Terminating, and Unknown** all count against
node capacity in the conformance model.

Releasing a Terminating claim requires `confirm_stopped(uid,
stopped_proven=True)` in this test-only oracle. Releasing Unknown claims
requires external verified fencing. These booleans simulate independently
verified actions; production must never accept an untrusted assertion of
proof.

Conservative accounting can reduce availability while fencing is unresolved,
but avoids admitting unrelated workloads into capacity already occupied by
uncertain processes. The invariant is not merely that *visible Running*
resources fit: all potentially occupied resources must fit.
