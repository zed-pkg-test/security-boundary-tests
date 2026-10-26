# Architectural overview

## Processes and ownership

The API accepts declarative resources and forwards conditional mutations to a
durable state engine. Scheduler and controllers are supervised Oreslang actors.
Node agents execute **authorized** workloads via OCI host adapters. Data-plane
networking does not depend on the API server after configuration.

```text
              clients, GitOps, CLI
                      |
          API + admission + RBAC
                      |
           transactional state <------+
               /        \             |
      watch/relist      watch/relist   |
            |              |          |
   controllers          scheduler ----+
            |              |
            +----- desired commands --+
                        |
                   node agents
                        |
            OCI runtime / CNI / CSI
```

### Source of truth

Desired spec and scheduled binding live in the durable store. Agent-reported
status is an observation, never proof of physical cleanup. Watch notifications
are hints. APIs and controllers must relist after compaction and retry with
CAS on conflict.

### Oreslang language integration

Use actors for ownership and supervision, Tasks for bounded parallel work,
immutable shared snapshots within authorized domains and per-actor arenas
where useful. `core` stays freestanding. Networking, persistence and host
isolation live in optional `stdlib` packages and drivers.

### Non-goals for pre-alpha

Not a drop-in Kubernetes API; not hard realtime; not a custom container
runtime; not cross-cloud federation; not a Kubernetes-compatible CNI engine.
