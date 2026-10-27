# Orestra CLI

Native Oreslang command sources live under `src/` and `bin/`. The native
cluster HTTP client is not yet implemented.

Functional local bootstrap utilities are implemented in Rust and remain
**offline only**:
```sh
cargo run --bin orestra-local -- plan examples/nodes.json --cpu-millis 200 --memory-bytes 268435456
cargo test --all-targets
```
The plan command does not reserve resources; results are advisory only.

## Kubernetes import subset

`cargo run --bin orestra-local -- import-k8s deployment.json` converts a
deliberately small JSON subset of `apps/v1 Deployment` to an Orestra
Deployment. Unsupported or security-sensitive pod/container fields fail
closed. This is a migration helper, not Kubernetes API compatibility.

Python source files are not accepted in this repository.
