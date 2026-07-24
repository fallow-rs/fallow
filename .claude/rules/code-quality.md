---
paths:
  - "**/*.rs"
  - "Cargo.toml"
  - ".clippy.toml"
---

# Rust code quality

- Read `docs/development/quality-gates.md` for current commands and repository
  conventions.
- Read the relevant nested `AGENTS.md` and implementation reference before
  changing a subsystem.
- Treat the Rust source assertions, workspace lint configuration, and build
  profiles as the source of truth. Do not duplicate their current values here.
- Keep lint suppressions narrow and justified.
- Normalize paths in cross-platform assertions.
- Update durable shared references when an invariant changes.
