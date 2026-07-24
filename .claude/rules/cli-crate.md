---
paths:
  - "crates/cli/**"
---

# CLI constraints

- Read `docs/reference/cli-internals.md` for the subsystem being changed.
- Preserve structured JSON errors on stdout and human errors on stderr.
- Treat paths, filtering, exit codes, schemas, generated types, Action scripts,
  GitLab scripts, and MCP callers as one compatibility surface.
- Keep output deterministic and project-root-relative.
- Run the CLI, output-format, companion, and real-project gates routed from
  `docs/development/quality-gates.md`.
