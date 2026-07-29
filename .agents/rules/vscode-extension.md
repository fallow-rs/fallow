---
paths:
  - "editors/vscode/**"
---

# VS Code extension

Wraps the `fallow-lsp` binary with additional UI features. TypeScript codebase bundled with rolldown.

## Architecture
- `src/extension.ts` — Activation, command registration, lifecycle
- `src/client.ts` — LSP client setup (stdio transport, language selector for JS/TS/Vue/Svelte/Astro/MDX/JSON)
- `src/download.ts` — Binary auto-download from GitHub releases (5 platform targets)
- `src/commands.ts` — Analysis and fix commands (spawns `fallow` CLI via execFile)
- Tree view providers for dead code (by issue type) and duplicates (by clone family)

## Binary resolution order
1. `fallow.lspPath` setting (explicit path, always wins)
2. Local `node_modules/.bin/` in workspace root (devDependency install)
3. `fallow-lsp` in system `PATH`
4. Previously downloaded binary in extension global storage
5. Auto-download from GitHub releases (if `fallow.autoDownload` enabled)

## Key behaviors
- **Lazy CLI analysis** — deferred until sidebar is first made visible (avoids double analysis with LSP)
- **LSP notification** — custom `fallow/analysisComplete` for real-time status bar updates
- **Config watch** — restarts LSP when `fallow.lspPath` or `fallow.trace.server` changes
- **Large buffer** — 50MB maxBuffer for CLI output on large monorepos

## Settings
`fallow.lspPath`, `fallow.autoDownload`, `fallow.issueTypes`, `fallow.duplication.threshold`, `fallow.duplication.mode`, `fallow.production`, `fallow.trace.server`

## Development
```bash
cd editors/vscode
pnpm install
pnpm run build     # rolldown production bundle
pnpm run watch     # development watch mode
pnpm run lint      # tsc --noEmit
pnpm run test      # unit + integration tests (vitest)
pnpm run package   # vsce package --no-dependencies
```

## Version management
Extension version is set from the git tag by CI — do not manually update `editors/vscode/package.json` version. The release workflow handles everything.
