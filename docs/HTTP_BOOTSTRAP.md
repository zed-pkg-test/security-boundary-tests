# First native HTTP smoke service

`examples/http-health.ores` uses the actual Java/GraalVM native `http.listen`
server and pooled isolated actor request handlers as documented by the
Oreslang runtime. It handles 16 incoming requests then shuts down cleanly.

The server binds a loopback ephemeral port and prints that port on stdout.
A test harness should read the port and request `GET /healthz` to receive:
`{"status":"ok","component":"orestra"}`.
Unknown routes should return HTTP 404.

This is a limited ingress demonstration, not a production API. It has no
TLS, authentication, admission, persistence, schema handling, workload
scheduling or OCI execution. Never bind it to an untrusted network interface
or advertise it as a deployment-ready control plane.

The Java/GraalVM handler transfer API is documented in
`oreslang-source.java/docs/HTTP_SERVER.md`; LLVM currently uses a different
transport integration path, so backend parity must be tested independently.
