# Schema and Admission

The API uses JSON Schema 2020-12. Tests now use the full versioned
validator rather than checking only required top-level fields. Unknown
fields and invalid types, names, quantities and service ports are rejected.

The Python validator is test infrastructure; production admission must be
implemented with validated Oreslang APIs. Only bundled, trusted schemas
are accepted; never resolve client-controlled remote schemas or $refs.
Schema validation is not RBAC, quota enforcement, CAS or sandboxing.
Only deploy after those checks are enforced by the actual API server.
