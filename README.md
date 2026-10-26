# Orestra Console

Responsive static dashboard with resource inventory, health summaries, search,
filters, refresh and a same-origin API proxy mode. It does not require a JS
framework or build tool.

```sh
python3 -m http.server 8080
# Open http://localhost:8080
```

The default dashboard is explicitly labeled **sample data**. There is no
functional live Orestra API yet. See docs/INTEGRATION.md for the future
proxy contract and authentication requirements.

## Rust-first static asset gate

Run `cargo test` and `cargo run -- --root .` to check demo JSON node/workload phases,
accessible element markers, and absence of `innerHTML` in the checked JS asset.
The existing Node/npm checks remain in CI. These static string checks **do not**
prove browser accessibility, safe DOM behavior, or a functioning Orestra API.
