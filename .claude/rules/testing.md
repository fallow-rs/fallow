---
paths:
  - "**/tests/**"
  - "**/*_test.rs"
  - "**/tests.rs"
  - "tests/fixtures/**"
---

# Testing conventions

- Read `tests/AGENTS.md`, the affected crate's nested `AGENTS.md`, and
  `docs/development/quality-gates.md`.
- Test behavior through the narrowest stable public API, engine, or CLI
  surface that proves the contract.
- Keep fixtures minimal. Add configuration files only when the behavior needs
  them.
- Normalize volatile values and platform-specific paths in assertions and
  snapshots.
- For bug fixes, add a minimal reproduction, a real-project smoke, and the
  relevant broad suite.
