---
paths:
  - "editors/vscode/**"
---

# VS Code constraints

Read `docs/reference/vscode-internals.md` for binary lifecycle, configuration,
views, protocol contracts, and verification. `editors/vscode/AGENTS.md` provides
the scoped workflow; package.json and configKeys.ts own the current inventory.

- Exercise command registration and rendered UI behavior through actual runtime
  entrypoints. Source spelling is not evidence that configuration is wired correctly.
- Preserve explicit binary resolution priority and verify managed downloads
  before execution.
- Keep editor display settings separate from LSP initialization and CLI analysis.
- Follow the release workflow for extension versions and platform packaging.
