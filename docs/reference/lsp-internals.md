# LSP internals

Use this reference for diagnostics, code actions, code lenses, hover, and LSP
lifecycle behavior.

## Ownership

- `crates/lsp/src/main.rs` is a thin binary delegator.
- `crates/lsp/src/lib.rs` owns the language server and request lifecycle.
- `crates/lsp/src/analysis.rs` calls the shared editor API and assembles an LSP
  snapshot.
- `crates/api/src/editor.rs` is the editor-facing analysis facade.
- `crates/lsp/src/diagnostics/` maps issue families to diagnostics.
- `crates/lsp/src/code_actions/`, `code_lens.rs`, and `hover.rs` own their
  protocol features.
- `crates/lsp/src/protocol.rs` owns Fallow-specific notifications and issue
  metadata projection.
- `crates/lsp/src/server_capabilities.rs` is the source of truth for advertised
  capabilities.
- `crates/types/src/issue_meta.rs` owns the shared issue catalogue.

## Invariants

- Keep analysis and fix semantics in shared APIs. The LSP adapts typed results
  to protocol objects.
- Convert paths through the LSP path helpers. Never construct document URIs by
  string concatenation.
- Publish only results that still match the current document version.
- Push and pull diagnostic clients must receive one coherent diagnostic set,
  including clears for stale findings.
- Diagnostics keep stable codes, `source: "fallow"`, actionable messages, and
  project-relative evidence where appropriate.
- Code actions must be safe, scoped, and derived from the current issue.
- Initialization options and issue metadata stay aligned with generated VS
  Code contracts.
- Shutdown must prevent late publication and clean up owned subprocess work.

## Verification

```bash
cargo test -p fallow-lsp
pnpm --dir editors/vscode run check:contracts
npm run verify:fast
```

Add protocol-level coverage for capability, URI, versioning, or push/pull
changes.
