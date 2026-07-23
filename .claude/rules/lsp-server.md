---
paths:
  - "crates/lsp/**"
---

# LSP constraints

- Read `docs/reference/lsp-internals.md` for the affected protocol surface.
- Preserve diagnostic URI, range, message, code, and related-information
  identity across publication and code actions.
- Resolve project-relative paths against the workspace root.
- Keep push, pull, refresh, and multi-root behavior equivalent.
- Verify with protocol-level and editor-facing tests.
