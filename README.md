<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo.svg">
    <img src="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo.svg" alt="fallow" width="290">
  </picture>
</p>

<p align="center">
  <strong>Codebase intelligence for TypeScript and JavaScript.</strong><br>
  Health, complexity, duplication, architecture, styling, and unused code, from one graph of your repository.<br>
  One Rust binary. It needs no TypeScript compiler and no configuration to start.
</p>

<p align="center">
  <a href="https://www.npmjs.com/package/fallow"><img src="https://img.shields.io/npm/v/fallow.svg" alt="npm"></a>
  <a href="https://www.npmjs.com/package/fallow"><img src="https://img.shields.io/npm/dm/fallow.svg" alt="npm downloads"></a>
  <a href="https://github.com/fallow-rs/fallow/actions/workflows/ci.yml"><img src="https://github.com/fallow-rs/fallow/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/fallow-rs/fallow/actions/workflows/coverage.yml"><img src="https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/fallow-rs/fallow/badges/coverage.json" alt="Coverage"></a>
  <a href="https://app.codspeed.io/fallow-rs/fallow?utm_source=badge"><img src="https://img.shields.io/endpoint?url=https://codspeed.io/badge.json" alt="CodSpeed"></a>
  <a href="https://github.com/fallow-rs/fallow/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License"></a>
</p>

<p align="center">
  <a href="https://docs.fallow.tools">Docs</a> ·
  <a href="https://docs.fallow.tools/quickstart">Quickstart</a> ·
  <a href="#in-your-terminal">Terminal</a> ·
  <a href="#in-pull-requests">Pull requests</a> ·
  <a href="#editors-and-integrations">Editors</a> ·
  <a href="#with-coding-agents">Agents</a>
</p>

---

Run it in the root of any JS or TS project:

```bash
npx fallow
```

A pull request gate on the vitest monorepo looks like this:

```
$ npx fallow audit --base HEAD~15
Audit scope: 73 changed files vs HEAD~15

── Dead Code ──────────────────────────────────────
✗ 1 file · 28 class members · 1 unused dependency · 24 circular dependencies · …

── Duplication ────────────────────────────────────
⚠ 185 lines (0.2%) duplicated across 6 files

── Complexity ─────────────────────────────────────
✗ 28 above threshold · 1453 analyzed

── Styling ────────────────────────────────────────
  custom properties: 47 defined, 33 unreferenced in CSS, 21 undefined
  font sizes mix 3 units (3 px, 2 rem, 1 em; candidate, standardize unless intentional)

✗ dead code: 68 issues · complexity: 28 findings · duplication: 7 clone groups · 73 changed files
  audit gate excluded 98 inherited findings (run with --gate all to enforce)
```

<sub>Excerpt from fallow 3.30.0 on vitest, over its last 15 commits, with timings removed. The gate checks only what the 15 commits changed. The 98 findings that existed before the change do not block it.</sub>

fallow reads your whole repository as one graph: modules, exports, dependencies, functions, and styling tokens. Every analysis uses that graph. It shows where the code is hard to change, where the architecture drifts, what is copied, what nothing uses, and what a pull request puts at risk.

fallow runs in four places. All four read the same config file and use the same analysis engine.

| Where | Start with |
|---|---|
| [Your terminal](#in-your-terminal) | `npx fallow` |
| [Pull requests](#in-pull-requests) | `uses: fallow-rs/fallow@v3` |
| [Your editor](#editors-and-integrations) | The VS Code extension or `fallow-lsp` |
| [Coding agents](#with-coding-agents) | `npx fallow agent install` |

## Why teams can depend on fallow

A tool that can fail your builds must be predictable. fallow keeps these properties from release to release:

- The same input gives the same output, with a stable fingerprint for each finding. There is no AI inside the analyzer.
- `fallow audit` fails only on findings that a change introduces. Existing findings do not fail the check.
- Monorepos are first-class. fallow reads npm, yarn, and pnpm workspaces, and `--workspace <name>` scopes a run to one package.
- It is fast on large monorepos. [BENCHMARKS.md](BENCHMARKS.md) has the method and the results.
- Over 100 built-in [framework plugins](https://docs.fallow.tools/frameworks/built-in) find entry points and framework conventions, so the first run needs no config.
- Each command has a typed JSON output, documented exit codes, and a published [output schema](docs/output-schema.json).
- Analysis runs on your machine or CI runner. Telemetry is opt-in ([what fallow collects](docs/telemetry.md)).
- The project has a public issue tracker, a public [roadmap](ROADMAP.md), and a [security policy](SECURITY.md).

The analyzer is written in Rust and uses [Oxc](https://oxc.rs) for syntactic analysis. Static analysis needs no TypeScript compiler and no Node.js runtime.

## What fallow finds

| Question | Analysis | Command |
|---|---|---|
| Is this change safe to merge? | [Changed-file gate](https://docs.fallow.tools/cli/audit) over complexity, duplication, unused code, and styling drift, with a pass, warn, or fail result | `fallow audit` |
| Where is the code hard to change? | [Complexity hotspots, a 0 to 100 health score, and refactoring targets](https://docs.fallow.tools/explanations/health), with git churn and ownership | `fallow health` |
| Does the architecture hold? | [Boundary violations](https://docs.fallow.tools/analysis/boundaries) with `bulletproof`, `layered`, `hexagonal`, and `feature-sliced` presets, and circular dependencies | `fallow dead-code --boundary-violations`, `fallow guard` |
| What is copied? | [Code duplication](https://docs.fallow.tools/analysis/duplication) in JS, TS, CSS, and Vue, Svelte, and Astro components | `fallow dupes` |
| Does the UI follow the design system? | [Styling drift](https://docs.fallow.tools/analysis/css-analysis) in CSS and CSS-in-JS | `fallow health --css` |
| What does nothing use? | [Unused files, exports, types, class and enum members, and dependencies](https://docs.fallow.tools/analysis/dead-code), with an [auto-fix](https://docs.fallow.tools/analysis/auto-fix) and a dry-run preview | `fallow dead-code`, `fallow fix` |
| Which code paths are risky? | Security candidates, ranked by reachability from entry points (opt-in) | `fallow security` |
| Which functions do the same job? | Functions with the same intent and different syntax (opt-in, uses a local model that you download once) | `fallow similar-code` |
| Where are the feature flags? | Feature-flag patterns across the codebase | `fallow flags` |

`npx fallow viz` opens an interactive HTML map of the project with lenses for health, duplication, architecture, and unused code.

Add `--type-aware` for exact TypeScript symbol identity across aliases, re-exports, and packages. This optional pass removes false positives from interfaces and base classes ([how type-aware analysis works](docs/type-aware-analysis.md)). Production [runtime coverage](https://docs.fallow.tools/analysis/runtime-coverage) comes from a separate hosted service, and fallow can merge it into health and audit reports.

The [CLI reference](https://docs.fallow.tools/cli/global-flags) lists every command. `fallow schema` prints all commands, flags, output formats, and exit codes as JSON.

## In your terminal

```bash
npx fallow                  # health, duplication, and unused code in one run
npx fallow audit            # only the findings that your change introduces
npx fallow fix --dry-run    # preview the auto-fixes, then run `npx fallow fix`
npx fallow health --score   # health score with the largest deductions
npx fallow dead-code --trace src/api.ts:client   # prove an export is unused before you delete it
```

To keep fallow in the project, install it as a dev dependency:

```bash
npm install --save-dev fallow
```

The npm package includes the `fallow`, `fallow-lsp`, and `fallow-mcp` launchers. For pnpm, yarn, `cargo install fallow-cli`, and Docker, see the [installation guide](https://docs.fallow.tools/installation).

To check each commit before it leaves your machine, install the managed pre-commit hook:

```bash
npx fallow hooks install --target git
```

## Adopt fallow on an existing codebase

You do not have to fix every finding before you add fallow to CI.

1. Run `npx fallow recommend`. It detects the stack and proposes a config. It changes no files.
2. Use `fallow audit` in pull requests. It compares each pull request with its base branch and blocks only the findings that the pull request adds. It needs no baseline file.
3. For full runs of `fallow`, `dead-code`, `dupes`, or `health`, save a baseline once with `npx fallow --save-baseline fallow-baseline.json`. Pass `--baseline fallow-baseline.json` on each later run, so that only new findings fail.
4. Reduce the backlog when your team has time. `fallow fix --dry-run` shows the fixes and changes no files. `fallow fix` applies them.

Most findings on a first run come from a missing entry point or from generated files. Add the missing entry points and exclude the generated files in the config:

```json
{
  "$schema": "./node_modules/fallow/schema.json",
  "entry": ["src/cli.ts"],
  "ignorePatterns": ["**/*.generated.ts"]
}
```

To keep an intentional export, add a suppression comment:

```ts
// fallow-ignore-next-line unused-export -- kept for plugin consumers
export const keepThis = 1;
```

The [adoption guide](https://docs.fallow.tools/adoption) shows the staged path. [Configuration](https://docs.fallow.tools/configuration/overview) has the full reference.

## In pull requests

GitHub Actions:

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- uses: fallow-rs/fallow@v3
  with:
    command: audit
```

GitLab CI:

```yaml
include:
  - remote: 'https://raw.githubusercontent.com/fallow-rs/fallow/v3.30.0/ci/gitlab-ci.yml'

fallow:
  extends: .fallow
```

With `command: audit`, a pull request fails only on findings that it introduces. Without `command: audit`, the Action runs the full pipeline, and any finding fails the job (`fail-on-issues` defaults to true). In a pull request, the Action scopes the analysis to the changed files, so its output can differ from a full local run.

The Action installs the fallow version that the project's `package.json` names. Pin an exact version there to use the same version in CI and on your machine. It can post a PR comment and inline review comments, and it can upload SARIF to GitHub Code Scanning. Other output formats are CodeClimate, GitHub annotations, and Markdown. The GitLab template URL names a release tag, because GitLab includes a file from a fixed ref. The [CI guide](https://docs.fallow.tools/integrations/ci) covers inputs, permissions, and a staged rollout.

For a PR comment or review comments, give the job these permissions:

```yaml
permissions:
  contents: read
  id-token: write
  pull-requests: write
  checks: write
```

Exit code 0 means no error-severity findings, or an audit result of pass or warn. Exit code 1 means error-severity findings, or an audit result of fail; the rule severity in the config (`error`, `warn`, `off`) sets which findings count. Exit code 2 means invalid input or an execution error. Treat 0 and 1 as a successful run, and do not hide exit code 2 with `|| true`.

<details>
<summary>All exit codes and the JSON error format</summary>

| Exit code | Meaning |
|---|---|
| 0 | Clean, or audit verdict pass or warn |
| 1 | Findings, or audit verdict fail (a normal outcome) |
| 2 | Validation or runtime error (JSON error envelope on stdout with `--format json`) |
| 3 | A requested resource is unavailable, for example when `config --path` finds no config |
| 4 | Runtime coverage sidecar is unavailable, unverifiable, protocol-incompatible, or terminated unexpectedly |
| 5 | Runtime coverage input could not be prepared or parsed |
| 6 | Runtime coverage sidecar reported an internal error |
| 7 | Network failure in a hosted-service operation |
| 8 | Security gate hit (`fallow security --gate`) |
| 10 | Coverage inventory or static-findings upload input or project validation failed |
| 11 | Coverage inventory or static-findings upload exceeded the server payload limit |
| 12 | Coverage inventory or static-findings upload authentication or authorization was rejected |
| 13 | Coverage inventory or static-findings upload failed after retries or returned another server error |

With `--format json`, an error arrives on stdout as `{"error": true, "message": "...", "exit_code": 2}`, not as a stack trace.

</details>

## Editors and integrations

- The [VS Code extension](https://docs.fallow.tools/integrations/vscode) shows findings in the editor.
- `fallow-lsp` gives diagnostics, hover, code actions, and code lenses in any LSP editor. [`editors/`](editors/) has setups for Zed and [Neovim](https://docs.fallow.tools/integrations/neovim).
- The Node API [`@fallow-cli/fallow-node`](https://docs.fallow.tools/integrations/node-bindings) exports `detectDeadCode`, `detectCircularDependencies`, `detectBoundaryViolations`, `detectDuplication`, `detectSimilarCode`, `detectFeatureFlags`, `computeComplexity`, and `computeHealth`. The [package API reference](crates/napi/README.md) has the options and return types.
- `fallow health --format badge > badge.svg` writes a [health badge](https://docs.fallow.tools/integrations/badges) for your README.

## With coding agents

```bash
npx fallow agent install --dry-run   # show the plan
npx fallow agent install             # apply it
```

`fallow agent install` sets up the agents that it detects (Claude Code, Codex, and Cursor) in one pass. It writes an `AGENTS.md` task map, installs the fallow skill, registers the MCP server, and adds a gate on `git commit` and `git push`. Run `npx fallow agent status` to see these changes. Run `npx fallow agent uninstall` to remove them.

To register only the [MCP server](https://docs.fallow.tools/integrations/mcp):

```json
{ "mcpServers": { "fallow": { "command": "npx", "args": ["fallow-mcp"] } } }
```

Scripts and agents that call the CLI directly add `--format json --quiet`. Each command then writes one typed JSON document to stdout. Each finding has an `actions[]` array and an `auto_fixable` flag. The types ship as `fallow/types`.

Exit codes and the JSON error format are in [In pull requests](#in-pull-requests). The [agent skills guide](https://docs.fallow.tools/integrations/agent-skills) has the details.

## Migrate from knip or jscpd

`npx fallow migrate` converts knip, jscpd, and stylelint config into fallow config. See the guides [from knip](https://docs.fallow.tools/migration/from-knip) and [from jscpd](https://docs.fallow.tools/migration/from-jscpd), and the [comparison page](https://docs.fallow.tools/migration/comparison).

## Contributing

To report a missing framework plugin or a false positive, [open an issue](https://github.com/fallow-rs/fallow/issues). [CONTRIBUTING.md](CONTRIBUTING.md) covers the development setup. [docs/README.md](docs/README.md) is the entry point for maintainer documentation.

<a href="https://github.com/fallow-rs/fallow/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=fallow-rs/fallow" alt="Contributors">
</a>

## License

MIT. See [LICENSE](LICENSE).

If fallow helps you or your team, you can support its development through [GitHub Sponsors](https://github.com/sponsors/BartWaardenburg).
