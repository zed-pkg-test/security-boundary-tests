# Native Oreslang production policy

Orestra production/runtime behavior is implemented in native Oreslang.

Shipping product/runtime code must be native `.ores` source. CI uses a
positive allowlist for production roots, rejects extensionless/host-language
runtime files, and rejects symlink or submodule entries. Native product source
may not reach into test/reference/tool/model trees.

This conformance repository has one explicit additional non-production
exception: `source/` may contain an exact Java/C Oreslang compiler/runtime
carrier mirror used only to obtain funded provenance/test evidence. That mirror
is compiler evidence, not Orestra product code and must never be packaged as an
Orestra runtime.

Python or other host languages remain allowed only in CI/test/tool/reference
trees. They cannot become scheduler, State, API, agent, driver, CLI or SDK
runtime authority.

Organization-level qualification separately SHA-verifies and parse/type-checks
every shipping Orestra `.ores` blob with an exact-tree funded Oreslang
compiler carrier.
