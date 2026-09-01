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
- A managed download is complete only once its file descriptor is closed.
  Stream completion is not enough: Unix refuses to execute a file that any
  process still holds open for writing, and the extension chmods, renames,
  and spawns what it just downloaded.
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
- The extension runs as a workspace extension. Local and remote extension hosts
  therefore select binaries and platform-specific VSIX payloads for the machine
  that owns the workspace, not the VS Code UI machine.
- `editors/vscode/scripts/vsix-targets.mjs` is the closed source of truth for
  VSIX targets and their TypeScript backend packages. Target packages contain
  one matching backend; the untargeted universal fallback contains all
  supported backends.
- `dist/` is generated for packaging and remains untracked.

## Verification

```bash
pnpm --dir editors/vscode run lint
pnpm --dir editors/vscode run check:contracts
pnpm --dir editors/vscode run build
pnpm --dir editors/vscode run test:packaging
```

Run focused editor tests for command, configuration, download, or diagnostic
changes. Release packaging uses `package:variants` to build the universal
fallback and every platform target from one build into an explicit output
directory. Verify extracted artifacts with `verify:vsix` and the matching
`--target` value.
