---
paths:
  - "editors/vscode/**"
---

# VS Code constraints

- Read `docs/reference/vscode-internals.md` for the affected subsystem.
- Preserve both push and pull diagnostic behavior.
- Never edit generated output contracts by hand.
- Keep binary installation atomic, verified, and cross-process safe.
- Run TypeScript, codegen, extension, platform, and LSP integration checks.
