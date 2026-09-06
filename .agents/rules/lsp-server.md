---
paths:
  - "crates/lsp/**"
---

# LSP constraints

Read `docs/reference/lsp-internals.md` for server lifecycle, diagnostic data,
document staleness, code actions, and protocol contracts. The entrypoint is
`crates/lsp/src/lib.rs`; `code_actions/` and `diagnostics/` own rendering.

- Exercise client events through the server, including stale-buffer and
  cancellation behavior, before changing lifecycle logic.
- Preserve `Diagnostic.data` when adding changed-since scope metadata.
- Keep byte offsets, UTF-16 positions, URI conversion, and project paths distinct.
- Keep the LSP thin over shared analysis. Test-only adapters must not duplicate
  the behavior the runtime tests are intended to exercise.
