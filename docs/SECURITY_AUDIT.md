# Browser audit

Only canonical same-origin proxy paths are allowed; credentials must never be
placed in query strings. Resources are bounded, quantities checked and remote
text rendered via `textContent`. The browser is not an RBAC boundary:
the reverse proxy must authenticate and authorize all list operations.

Set a strict server CSP and frame-ancestors policy in deployment. A static
HTML CSP is defense in depth, not a substitute for response headers.
