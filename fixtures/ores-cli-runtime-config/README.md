# ORES CLI runtime-config acceptance corpus

This corpus is intentionally owned by a public `*-test` organization so `ORESoftware/ores-cli` can consume it as external acceptance evidence without requiring access to the private ores-cli repository from this test repository.

Each case is a self-contained repository-shaped fixture. `expected.json` records the expected `oresc audit repo` runtime-consumption evidence class and finding code. The local Python tests guard the semantic shape of each case so a fixture cannot drift into a different class silently.

The corpus contains no credentials and does not execute provider or production resources.
