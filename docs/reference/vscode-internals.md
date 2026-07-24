# VS Code internals

Use this reference for extension activation, binary lifecycle, configuration,
views, and LSP client behavior.

## Ownership

- `editors/vscode/src/extension.ts`: activation, lifecycle, commands, and view
  wiring.
- `editors/vscode/src/client.ts`: LSP client setup and middleware.
- `editors/vscode/src/commands.ts`: CLI subprocess requests.
- `editors/vscode/src/configKeys.ts`: restart, reanalysis, and render-only
  configuration groups.
- `editors/vscode/src/diagnosticFilter.ts`: client-side diagnostic filtering
  and severity projection.
- `editors/vscode/src/download.ts`: managed binary download and verification.
- `editors/vscode/package.json`: commands, settings, views, menus, and engine
  compatibility.
- `editors/vscode/src/generated/`: generated shared contracts. Never hand-edit
  these files.

Use `package.json` and `configKeys.ts` for the current configuration inventory.
Do not duplicate the full setting list in durable prose.

## Invariants

- Binary resolution follows the documented priority: explicit user path,
  workspace dependency, system path, managed binary, then auto-download.
- Validate managed downloads before execution.
- Keep LSP analysis, health, audit, security, and runtime coverage as separate
  lazy workflows unless a measured UX change justifies combining them.
- Configuration changes restart only the surfaces whose initialization state
  changed. Reanalysis and render-only settings must not cause unnecessary LSP
  restarts.
- Diagnostic filtering applies only to diagnostics with
  `source: "fallow"`.
- Generated issue metadata, output contracts, and initialization options stay
  synchronized with Rust sources.
- Multi-root workspace selection remains explicit and paths stay scoped to the
  selected project.
- `dist/` is generated for packaging and remains untracked.

## Verification

```bash
pnpm --dir editors/vscode run lint
pnpm --dir editors/vscode run check:contracts
pnpm --dir editors/vscode run build
```

Run focused editor tests for command, configuration, download, or diagnostic
changes.
