---
paths:
  - "crates/graph/**"
---

# Resolution and graph analysis

Follow the [graph guide](../../crates/graph/AGENTS.md),
[detection reference](../../docs/reference/detection-internals.md), and
[architecture invariants](../../docs/architecture-invariants.md).

Preserve stable file identity, workspace ownership, explicit resolver fallbacks
and re-export provenance. Measure traversal work at the operation itself when
a test claims to protect its cost; a result variant is not an observation of
how much work ran.
