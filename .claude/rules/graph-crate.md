---
paths:
  - "crates/graph/**"
---

# Graph constraints

- Read `docs/development/repo-map.md`, `docs/reference/detection-internals.md`,
  and the graph crate source for the affected stage.
- Preserve stable file identity, deterministic graph traversal, workspace
  boundaries, re-export propagation, and cycle reporting.
- Keep resolution conservative across package managers and platform-specific
  extensions.
- Add focused fixtures and real-project evidence for behavior changes.
