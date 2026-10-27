# Pure scheduler and controller algorithm contracts

Scheduling feasibility uses a subtract-first guard, not `used + requested`,
to avoid addition overflow when quantities exceed remaining capacity.
Final reservations must be committed transactionally by the state backend,
as a pure feasibility function cannot prevent races.

Packaging CPU/memory ratios into an integer score requires overflow controls:
production callers should use checked wide integer operations, rejecting or
saturating calculations when a product like `resource * 1000` exceeds signed
range. Tests must cover 64-bit extremes and equal-score tie breaks.

Deployment budgets are similarly overflow-sensitive. Avoid blindly adding
`desired + surge` when both may be near the integer maximum. All future
resource accounting must use Oreslang's checked arithmetic or explicit
preconditions.

No wall-clock liveness test is sufficient to establish that a node has
terminated a process after network partition.
