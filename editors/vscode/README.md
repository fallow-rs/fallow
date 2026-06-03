# Fallow for VS Code

Codebase intelligence for TypeScript and JavaScript. Real-time diagnostics for unused code, duplication, circular dependencies, complexity hotspots, and architecture drift, with optional runtime evidence via Fallow Runtime. Powered by [fallow](https://docs.fallow.tools), Rust-native and sub-second.

## Features

- **Real-time diagnostics** via the fallow LSP server: unused files, exports, types, dependencies, enum/class members, unresolved imports, unlisted deps, duplicate exports, circular dependencies, and code duplication
- **Quick-fix code actions**: remove unused exports, delete unused files
- **Refactor code actions**: extract duplicate code into a shared function
- **Code Lens**: reference counts above each export declaration with click-to-navigate (opens Peek References panel)
- **Hover information**: export usage status, unused status, and duplicate block locations
- **Tree views**: browse unused code by issue type and duplicates by clone family in the sidebar
- **Status bar**: see total issue count and duplication percentage at a glance
- **Auto-fix**: remove unused exports, dependencies, and enum members with one command
- **Auto-download**: the extension downloads managed `fallow-lsp` and `fallow` CLI binaries automatically

## Installation

### From the Marketplace

Search for "Fallow" in the VS Code extensions panel, or install from the command line:

```sh
code --install-extension fallow-rs.fallow-vscode
```

### Manual

1. Install the `fallow` npm package or the standalone `fallow` / `fallow-lsp` binaries (see [fallow installation](https://docs.fallow.tools/installation))
2. Install the extension VSIX file: `code --install-extension fallow-vscode-*.vsix`

## Commands

| Command | Description |
|---------|-------------|
| `Fallow: Run Analysis` | Run full codebase analysis and update tree views |
| `Fallow: Auto-Fix Unused Exports & Dependencies` | Remove unused exports and dependencies |
| `Fallow: Preview Fixes (Dry Run)` | Show what fixes would be applied without changing files |
| `Fallow: Restart Language Server` | Restart the fallow-lsp process |
| `Fallow: Show Output Channel` | Open the Fallow output panel for debugging |
| `Fallow: Toggle Mute Code-Duplication Findings` | Hide or restore Fallow's duplicate-code squiggles in the editor |
| `Fallow: Toggle Mute All Findings` | Hide or restore every Fallow finding in the editor |
| `Fallow: Manage Diagnostic Mutes...` | Multi-select picker for individual categories |
| `Fallow: Show All Findings (Clear Mutes)` | Reset all editor mutes |

### Muting Fallow's editor squiggles

Duplicate-code findings can span many lines and drown out TypeScript / ESLint diagnostics in the editor. Fallow ships three ways to mute them locally without disabling the underlying rule:

- A right-click **Quick Fix** on any Fallow squiggle: "Mute Fallow `<category>` findings in this workspace."
- The four commands above; bind a keyboard shortcut to `fallow.toggleMuteDuplicates` for one-keystroke noise control.
- The Fallow language status item (right gutter of the status bar) appears with a yellow indicator whenever anything is muted; click it to open the manage picker.

Mute state is stored in the workspace, so it survives reload but does not bleed across projects. Precedence: rules in your `fallow.config.json` and the `fallow.issueTypes` setting take effect server-side; muting is a **local view filter only**, applied client-side. CI and `fallow check` still report every finding.

## Settings

| Setting | Default | Description |
|---------|---------|-------------|
| `fallow.lspPath` | `""` | Path to the `fallow-lsp` binary. Leave empty for auto-detection. |
| `fallow.configPath` | `""` | Path to a Fallow config file. Relative paths are resolved from the workspace root (the first folder, in multi-root workspaces). Mirrors the CLI's `--config`; empty uses config auto-discovery. |
| `fallow.autoDownload` | `true` | Automatically download managed `fallow-lsp` and `fallow` CLI binaries if not found. |
| `fallow.issueTypes` | all enabled | Toggle individual issue types on/off. |
| `fallow.duplication.threshold` | `0` | Maximum allowed duplication percentage before the analysis is marked as failing. `0` (the default) means no limit. |
| `fallow.duplication.minOccurrences` | `2` | Minimum number of occurrences before a clone group is reported. Defaults to `2` (every duplicated pair). Raise to `3`+ to focus on widespread copy-paste and skip context-sensitive pairs. |
| `fallow.duplication.mode` | `"mild"` | Detection mode: `strict`, `mild`, `weak`, or `semantic`. |
| `fallow.production` | `false` | Production mode: exclude test/dev files, only production scripts. |
| `fallow.changedSince` | `""` | Git ref (tag, branch, or SHA) to scope the Problems panel and sidebar to files changed since that ref, mirroring the CLI's `--changed-since`. Tag your current commit (e.g. `fallow-baseline`) and set this to the tag to enforce "no new issues going forward" while ignoring pre-existing findings. |
| `fallow.trace.server` | `"off"` | LSP trace level: `off`, `messages`, or `verbose`. |

## Binary resolution

The extension looks for the `fallow-lsp` binary in this order:

1. `fallow.lspPath` setting (if configured)
2. Local `node_modules/.bin/fallow-lsp`
3. `fallow-lsp` in `PATH`
4. Previously downloaded binary in extension storage
5. Auto-download from GitHub releases (if `fallow.autoDownload` is enabled)

Tree views and fix commands also need the `fallow` CLI. The extension resolves it in this order:

1. `fallow` next to the configured `fallow.lspPath` binary
2. Local `node_modules/.bin/fallow`
3. `fallow` in `PATH`
4. Previously downloaded CLI binary in extension storage
5. Auto-download from GitHub releases (if `fallow.autoDownload` is enabled)

## Development

```sh
cd editors/vscode
pnpm install
pnpm build           # Production build
pnpm watch           # Watch mode for development
pnpm lint            # Type check
pnpm test            # Unit + extension-host tests
pnpm package         # Package as .vsix
```
