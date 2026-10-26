# Connecting the UI

Serve files using a static HTTPS origin. Without query parameters the UI reads
`data/demo.json` and visibly labels itself SAMPLE DATA.

For a same-origin reverse proxy, open `/?api=/api`. The proxy should forward
`GET /api/v1alpha1/{Node,Workload}` and return `{"items":[...],"revision":"..."}`.
The production API is not yet implemented; this is an integration contract.

Do not put tokens, cluster credentials or external API URLs in query strings.
The proxy must authenticate the browser, authorize each list request, enforce
CSRF/origin policy and TLS. Do not expose administrator permissions or secrets
to the dashboard. No polling is enabled; users must explicitly refresh.
