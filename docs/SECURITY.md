# Website security and honest release claims

The static site uses an HTML-level CSP that restricts scripts, styles,
connections and images to the same origin, with no object embedding. Host
servers should additionally set response headers for CSP (including
frame-ancestors), HSTS, Referrer-Policy and X-Content-Type-Options.

Navigation CI checks relative targets and anchors, while JavaScript remains
framework-free. This is a pre-alpha engineering website: do not publish
throughput, performance, Kubernetes compatibility or production-readiness
claims without independently reproduced tests.

The source repositories are currently private and links require access.
Publishing the website does not automatically publish private Oreslang code.
