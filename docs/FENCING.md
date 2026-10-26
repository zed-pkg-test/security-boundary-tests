# Unknown is not stopped

A partitioned node may continue running its previously assigned workloads.
This oracle therefore treats `Unknown` workload instances as unresolved
execution claims and does not allocate replacements until a verified external
fencing or stop acknowledgment is recorded.

`confirm_fenced(node, fence_proven=True)` is a **test-only assertion** that
the external fencing mechanism succeeded. Passing `True` is not an actual
fencing operation. Production Orestra must implement independently verifiable
fencing and avoid accepting arbitrary caller assertions.

Node rejoin by name does not revive an old unknown workload generation.
Storage attachment and single-writer network identities also need fencing.
This conservative design can temporarily sacrifice availability to preserve
safety. Losing heartbeats alone is never adequate proof of stopped processes.
