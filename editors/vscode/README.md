# Fallow for VS Code

Codebase intelligence for TypeScript and JavaScript. Real-time diagnostics for unused code, duplication, circular dependencies, complexity hotspots, and architecture drift, with optional runtime evidence via Fallow Runtime. Powered by [fallow](https://docs.fallow.tools), Rust-native and sub-second.

## Features

- **Real-time diagnostics** via the fallow LSP server: unused files, exports, types, dependencies, enum/class members, unresolved imports, unlisted deps, duplicate exports, circular dependencies, and code duplication
- **Quick-fix code actions**: remove unused exports, delete unused files
- **Refactor code actions**: extract duplicate code into a shared function
- **Code Lens**: reference counts above each export declaration with click-to-navigate (opens Peek References panel)
- **Hover information**: export usage status, unused status, and duplicate block locations
- **Tree views**: browse unused code by issue type and duplicates by clone family in the sidebar
- **Health view**: project health score and grade, complexity findings (click to open `file:line`), plus churn-and-complexity hotspot candidates and refactoring candidates (framed as heuristics to verify, not facts). Runs a separate, lazy `fallow health` analysis only when the view is first opened, so it never slows the editor or the other views.
- **Security Candidates view** (opt-in): surfaces local `client-server-leak` and tainted-sink CWE findings from `fallow security` as UNVERIFIED candidates for you or an AI agent to verify, never confirmed vulnerabilities. Off by default; enabling it runs a separate `fallow security` scan only when the view is opened, so it never slows the editor or the other views.
- **Status bar**: see total issue count and duplication percentage at a glance, with an optional health score/grade segment (e.g. `health: B (82)`)
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
| `Fallow: Reload Health` | Re-run the Health view analysis (score, complexity, hotspot and refactoring candidates) |
| `Fallow: Scan for Security Candidates` | Scan for local security candidates (`client-server-leak`, tainted-sink CWE findings) and populate the Security Candidates view. Requires `fallow.security.enabled`. Results are UNVERIFIED candidates to verify, never confirmed vulnerabilities. |
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
| `fallow.duplication.minTokens` | `50` | Minimum token count for a clone before it can be reported as duplicated code. |
| `fallow.duplication.minLines` | `5` | Minimum line count for a clone before it can be reported as duplicated code. |
| `fallow.duplication.minOccurrences` | `2` | Minimum number of occurrences before a clone group is reported. Defaults to `2` (every duplicated pair). Raise to `3`+ to focus on widespread copy-paste and skip context-sensitive pairs. |
| `fallow.duplication.mode` | `"mild"` | Detection mode: `strict`, `mild`, `weak`, or `semantic`. |
| `fallow.duplication.skipLocal` | `false` | Only report duplicate code that appears across different directories. |
| `fallow.duplication.crossLanguage` | `false` | Compare TypeScript and JavaScript files after stripping TypeScript type annotations. |
| `fallow.duplication.ignoreImports` | `false` | Exclude import declarations from duplicate-code detection. |
| `fallow.health.enabled` | `true` | Show the Fallow Health view (score and grade, complexity findings, hotspot candidates, refactoring candidates). When off, the Health view stays empty and no extra analysis runs. |
| `fallow.health.hotspots` | `true` | Include git churn hotspots in the Health view. Hotspot analysis walks git history; disable on very large repositories to keep the Health refresh fast. Has no effect outside a git repository. |
| `fallow.health.topFindings` | `20` | Maximum number of complexity findings shown in the Health view (passed to `fallow health --top`). |
| `fallow.health.statusBar` | `true` | Show the project health score and grade in the Fallow status bar item. |
| `fallow.security.enabled` | `false` | Show the Security Candidates view and surface local `client-server-leak` and tainted-sink CWE findings from `fallow security`. Off by default. Findings are UNVERIFIED candidates to verify, never confirmed vulnerabilities. When enabled, the scan runs only when the view is opened, so it never slows the main sidebar. |
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
