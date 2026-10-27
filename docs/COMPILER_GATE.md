# Compiler parity gate — not yet complete

Initial Orestra CI validates repository structure/imports and executes
reference conformance oracles in other repositories. It **does not compile**
Oreslang sources on either backend. A green source CI result must not be
interpreted as compiler or native execution success.

The acceptance gate for Orestra milestone M1 requires:

1. Compile and run placement-smoke, edge-guards, workload-state-smoke,
   SDK smoke, and HTTP-health against a pinned GraalVM runtime commit.
2. Run identical pure source fixtures on LLVM and compare machine-readable
   outcomes. HTTP transport parity needs separate adapter conformance.
3. Verify that `http.listen` uses allowed NET capability, is loopback-only
   in development, and shuts down without leaked request actors.
4. Test negative placements, stale leader epochs, workload transition guards,
   cancellation and malformed API requests on the compiled implementation.
5. Only then mark a cluster executable/releasable.

Private cross-organization source checkout requires a deliberately provisioned
installation with repository-scoped read access, or an explicitly approved
source mirror. Do not silently copy private compiler trees or use unreviewed
wide-scope personal access tokens to make CI green.
