---
paths:
  - "crates/core/src/analyze/**"
  - "crates/extract/src/visitor/**"
  - "crates/graph/src/graph/**"
  - "crates/graph/src/resolve/**"
---

# Detection changes

Use the [detection reference](../../docs/reference/detection-internals.md) for
pipeline ownership, accuracy invariants, resolution fallbacks and validation.
Use the [extraction reference](../../docs/reference/extract-internals.md) for
syntax, source mapping and cache requirements, and the
[plugin reference](../../docs/reference/plugin-internals.md) for framework
activation and config-derived credit.

Follow the affected crate's `AGENTS.md`. Fix the earliest incorrect pipeline
stage, keep advisory detection conservative and validate through the real
production path. Consult maintained references and current source/tests for
supported syntax; do not duplicate module inventories or versioned capability
catalogues here.
