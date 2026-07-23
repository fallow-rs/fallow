---
paths:
  - "crates/core/src/analyze/**"
  - "crates/extract/src/visitor/**"
  - "crates/graph/src/graph/**"
  - "crates/graph/src/resolve/**"
---

# Detection constraints

- Read `docs/reference/detection-internals.md` for the affected analyzer stage.
- Fix the earliest incorrect pipeline layer.
- Preserve production, workspace, changed-file, suppression, baseline, audit,
  cache, LSP, MCP, and output-format parity.
- Add a minimal regression fixture and verify behavior on a real consumer
  project.
- Update durable reference knowledge when an invariant changes.
