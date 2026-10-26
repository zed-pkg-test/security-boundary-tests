# Actual compiler/backend integration

`scripts/run_native_sources.py` runs four actual `.ores` source fixtures
against a **caller-provided real Oreslang runner** and requires explicit
sentinel results. It never pretends that the Python conformance model is the
Oreslang compiler. The script does not create a compiler, access another
private GitHub organization, or download opaque unpinned artifacts.

Example:
```sh
python3 scripts/run_native_sources.py --backend graalvm \
  --runner-json '["/absolute/path/to/oreslang-runner"]' \
  --orestra-root ../orestra --sdk-root ../orestra-sdk
```

The runner command must accept a source filename appended as its last arg.
If a backend requires flags, include them in the JSON array. To qualify a
release, run for each pinned GraalVM and LLVM compiler with their commit SHAs
recorded and examine the exact fixture and runtime output.

The harness has unit tests in CI, but **neither backend is automatically
compiled or exercised** by the current Orestra workflows. A zero-exit
structural check or a simulated runner is not acceptable native evidence.
