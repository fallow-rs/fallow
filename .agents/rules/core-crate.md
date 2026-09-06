---
paths:
  - "crates/core/**"
---

# Core backend

Follow the [core guide](../../crates/core/AGENTS.md),
[detection reference](../../docs/reference/detection-internals.md), and
[plugin reference](../../docs/reference/plugin-internals.md).

Core owns detector and discovery backends. Public orchestration belongs to
engine and API. Preserve compatibility reexports; do not move retired
production algorithms behind `cfg(test)` merely to keep their tests running.
