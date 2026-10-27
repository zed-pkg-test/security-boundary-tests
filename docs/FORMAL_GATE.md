# Lease fencing model checking

`formal/LeaseFencing.tla` defines a bounded state machine for leadership
epochs, commit actions and a history invariant that binds a committed write
to its current holder and epoch. `LeaseFencing.cfg` bounds epochs and history
length to allow termination in TLC. Run manually with TLA+ tools:

```sh
java -cp /path/to/tla2tools.jar tlc2.TLC -config formal/LeaseFencing.cfg formal/LeaseFencing.tla
```

`python3 formal/check_fencing.py` also exhaustively enumerates an independent
small state space in CI. This is only a **gate for the abstract reference model**,
not proof of real server implementation or system behavior. A complete proof
also needs timing, durability, snapshots, renewal, cross-resource transactions,
storage quorum partitions and external node-side fencing.

Test trace refinement against the Oreslang replicated store before claiming
any formally verified implementation.
