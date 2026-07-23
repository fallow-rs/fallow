---
paths:
  - "crates/core/src/plugins/**"
  - "crates/config/src/external_plugin.rs"
---

# Plugin constraints

- Read `docs/reference/plugin-internals.md` for the affected framework or tool.
- Verify every documented default config location.
- Keep activation, entry points, dependency credit, and path aliases bounded by
  the project root.
- Add fixtures for each supported config shape.
- Preserve external plugin schema and built-in plugin parity.
