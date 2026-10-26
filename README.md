# Orestra Documentation

Engineering documentation for the Oreslang-native orchestration project.
Start at [docs/index.md](docs/index.md).

Documents cover controller supervision, scheduling atomicity, storage
consistency, node lifecycle, multi-tenancy, compatibility, operations,
verification and release gates. Milestones distinguish implemented source from
future integrations.

Use the Rust checker instead of Python scripts:

```sh
cargo test
cargo run -- all --root .
cargo run -- links --root .
cargo run -- lock --root .
```

These checks verify local Markdown file destinations and the integration
lock's declared revision SHA **shape**. They do not fetch remote repositories
or establish executable Oreslang compiler/runtime evidence.
