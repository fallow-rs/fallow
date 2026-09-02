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
- `initializationOptions.mutedCategories` accepts exact diagnostic codes from
  the shared issue catalogue. Known string codes are unioned with diagnostics
  disabled through `issueTypes: false`; unknown and non-string entries are
  ignored so older servers remain permissive with newer clients.
- Initialization options are read once during `initialize`. Clients must
  restart the language server after changing `mutedCategories` or
  `health.inlineComplexity`.
- `health.inlineComplexity` is opt-in and supplies threshold-exceeding function
  complexity to Code Lens. It is not a project Health report or an editor-owned
  Health view.
- Security candidate diagnostics remain opt-in through the project config.
  `security-sink` and `security-client-server-leak` default to `off`, retain
  those exact diagnostic codes, and publish at information severity because
  candidates are not verified vulnerabilities.
- Code actions must be safe, scoped, and derived from the current issue.
- Initialization options and issue metadata stay aligned with generated VS
  Code contracts.
- Shutdown must prevent late publication and clean up owned subprocess work.

## Editor parity boundary

The shared LSP owns diagnostics, hover, quick fixes, Code Lens, and their
initialization contract. Host-specific sidebars, status items, full Health
reports, and full Security reports are outside that protocol. Editors without
custom UI contribution points should expose the shared LSP features and direct
users to a separately installed `fallow` CLI for the complete project reports.

When documenting CLI invocation from an editor, preserve the process contract:
run from the project root, treat exit `1` as a successful analysis with
findings, and treat exit `2` as a configuration, input, or execution error.
`fallow security` is advisory unless `--fail-on-issues`, an error-severity
security rule, or an explicit security gate changes its exit behavior.

## Verification

```bash
cargo test -p fallow-lsp
pnpm --dir editors/vscode run check:contracts
npm run verify:fast
```

Add protocol-level coverage for capability, URI, versioning, or push/pull
changes.
