---
paths:
  - "crates/lsp/**"
---

# fallow-lsp crate

Key modules:
- `main.rs` — LSP server setup, `LanguageServer` trait impl, event handling
- `diagnostics/` — Diagnostic generation: `mod.rs` (dispatch), `unused.rs`, `structural.rs`, `quality.rs`
- `code_actions.rs` — Quick-fix and refactor code actions
- `code_lens.rs` — Reference count Code Lens above export declarations
- `hover.rs` — Hover information showing export usage, unused status, and duplicate block locations

## Diagnostic.data

When the `changedSince` filter is active, every published `Diagnostic` carries `data: { "changedSince": "<git_ref>" }` (standard LSP `Diagnostic.data` slot). Set centrally via `attach_changed_since_data` after `build_diagnostics`. AI agents reading via `vscode.languages.getDiagnostics()` can use this payload to verify the filter is on and avoid acting on baseline-excluded findings. No `data` is set when the filter is unset.

Circular-dependency diagnostics also carry `data: { "circularDependency": { "cycleId": "cycle:<hex>", "fileCount": N } }`, set per-diagnostic in `push_circular_dep_diagnostics` (`diagnostics/structural.rs`). Each file in a cycle gets its own diagnostic, anchored at the import edge that points to the next file (`CircularDependency.edges[i]` carries the `path`/`line`/`col` of that hop, computed in core via `cycle_edge_line_col`; the LSP cannot recompute it because it has no module graph). The shared `cycleId` (FNV-1a over the sorted file set, rotation-independent) lets clients fold the per-file squigglies back into one cycle. The per-edge `Url::from_file_path` is a render-only filter: an unopenable path is skipped from squiggling but never dropped from the `edges` data, so `edges.len() == files.len()` always holds. Cycles whose data has empty `edges` (historical baselines) fall back to the prior single-first-file diagnostic. `attach_changed_since_data` merges `changedSince` into this existing object rather than clobbering it.
