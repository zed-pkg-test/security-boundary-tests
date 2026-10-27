# CLI behavior

Long-term executable: `orestra` written in Oreslang. Current CLI has a native
help-only entrypoint, pure command helper modules and a Python **offline
bootstrap tool**. The Python tool is not the production cluster CLI.

Planned verbs:
- `get KIND [NAME] --namespace N --output json|table`
- `describe KIND NAME`
- `apply -f file.json --field-manager NAME`
- `delete KIND NAME --resource-version VERSION`
- `events --watch --since REVISION`
- `logs workload/name --container NAME --follow`
- `plan -f workload.json` (advisory only)

All mutating verbs must use conditional resourceVersion or server-side field
management, bounded deadlines, TLS verification and an explicit context.
Exit codes: 0 success, 1 network/unknown, 2 invalid input, 3 authorization,
4 not found, 5 conflict. Never print credential or secret contents by default.
