# Compatibility strategy

Orestra-native v1alpha1 API is separate from Kubernetes APIs. A Workload is
**not** presently a Pod, and a generic OpenAPI shape is not kube-apiserver
compatibility.

Initial format: canonical JSON with versioned schemas and Protobuf transport
definitions. Importing Kubernetes YAML is a **translation project**, including
defaulting, schema conversion, status fields, event semantics, service account
permissions and admission behavior.

External standards:
- OCI runtime spec: launch real container bundles with audited runtime integration.
- CRI: potential kubelet-compatible backend adapter.
- CNI: network attach/detach interface, not a substitute for network policy.
- CSI: volume controller/node plugin interface with attachment fencing.
- Gateway API: optional API conversion/implementation.

Conformance claims must identify exact versions and pass official tests where
available. Do not advertise drop-in Kubernetes compatibility ahead of that.
