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
  <a href="https://socket.dev/npm/package/fallow"><img src="https://img.shields.io/badge/socket-report-6e56cf" alt="Socket"></a>
  <a href="https://github.com/fallow-rs/fallow/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License"></a>
</p>

<p align="center">
  <a href="https://fallow.tools/docs/">Docs</a> ·
  <a href="https://fallow.tools/docs/quickstart/">Quickstart</a> ·
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

● Circular dependencies (24)
  packages/vitest/src/runtime/runner/artifact.ts (6 cycles)
    → run.ts → context.ts → artifact.ts

── Duplication ────────────────────────────────────
⚠ 185 lines (0.2%) duplicated across 6 files

● High complexity functions (28)
  packages/vitest/src/runtime/runner/run.ts
    :566 runTest CRITICAL
          32 ! cyclomatic   47 ! cognitive  203 lines

── Styling ────────────────────────────────────────
  font sizes mix 3 units (3 px, 2 rem, 1 em; candidate, standardize unless intentional)

✗ dead code: 68 issues · complexity: 28 findings · duplication: 7 clone groups · 73 changed files
  audit gate excluded 98 inherited findings (run with --gate all to enforce)
```

<sub>Excerpt from fallow 3.30.0 on vitest, over its last 15 commits, with timings removed. The gate failed (exit code 1) on findings in the changed files. It did not count the 98 findings that existed before the change.</sub>

fallow reads your whole repository as one graph: modules, exports, dependencies, functions, and styling tokens. Every analysis uses that graph. It shows where the code is hard to change, where the architecture drifts, what is copied, what nothing uses, and what a pull request puts at risk.

fallow runs in four places. All four read the same config file and use the same analysis engine.

| Where | Start with |
|---|---|
| [Your terminal](#in-your-terminal) | `npx fallow` |
| [Pull requests](#in-pull-requests) | `uses: fallow-rs/fallow@v3` |
| [Your editor](#editors-and-integrations) | The VS Code extension or `fallow-lsp` |
| [Coding agents](#with-coding-agents) | `npx fallow agent install` |

## What fallow finds

| Question | Analysis | Command |
|---|---|---|
| Is this change safe to merge? | [Changed-file gate](https://fallow.tools/docs/cli/audit/) over complexity, duplication, unused code, and styling drift, with a pass, warn, or fail result | `fallow audit` |
| Where is the code hard to change? | [Complexity hotspots, a 0 to 100 health score, and refactoring targets](https://fallow.tools/docs/explanations/health/), with git churn and ownership | `fallow health` |
| Does the architecture hold? | [Boundary violations](https://fallow.tools/docs/analysis/boundaries/) with `bulletproof`, `layered`, `hexagonal`, and `feature-sliced` presets, and circular dependencies | `fallow dead-code --boundary-violations`, `fallow guard` |
| What is copied? | [Code duplication](https://fallow.tools/docs/analysis/duplication/) in JS, TS, CSS, and Vue, Svelte, and Astro components | `fallow dupes` |
| Does the UI follow the design system? | [Styling drift](https://fallow.tools/docs/analysis/css-analysis/) in CSS and CSS-in-JS | `fallow health --css` |
| What does nothing use? | [Unused files, exports, types, class and enum members, and dependencies](https://fallow.tools/docs/analysis/dead-code/), with an [auto-fix](https://fallow.tools/docs/analysis/auto-fix/) and a dry-run preview | `fallow dead-code`, `fallow fix` |
| Which code paths are risky? | Security candidates, ranked by reachability from entry points (opt-in) | `fallow security` |
| Which functions do the same job? | Functions with the same intent and different syntax (opt-in, uses a local model that you download once) | `fallow similar-code` |
| Where are the feature flags? | Feature-flag patterns across the codebase | `fallow flags` |

`npx fallow viz` opens an interactive HTML map of the project with lenses for health, duplication, architecture, and unused code.

Add `--type-aware` for exact TypeScript symbol identity across aliases, re-exports, and packages. This optional pass removes false positives from interfaces and base classes ([how type-aware analysis works](docs/type-aware-analysis.md)). [Runtime coverage](https://fallow.tools/docs/analysis/runtime-coverage/) from production is an optional paid add-on that fallow merges into health and audit reports. Everything else in this README is free.

The [CLI reference](https://fallow.tools/docs/cli/global-flags/) lists every command. `fallow schema` prints all commands, flags, output formats, and exit codes as JSON.

## Why teams can depend on fallow

A tool that can fail your builds must be predictable. fallow keeps these properties from release to release:

- The same input gives the same output, with a stable fingerprint for each finding. There is no AI inside the analyzer. Only the opt-in `similar-code` command uses a pinned local model.
- `fallow audit` fails only on findings that a change introduces. Existing findings do not fail the check.
- It is fast on large codebases. fallow finds the unused code in the next.js monorepo (20,558 files) in 2.95s. Measured on fallow 2.100.0. [BENCHMARKS.md](BENCHMARKS.md) has the method and all results, and [CodSpeed](https://app.codspeed.io/fallow-rs/fallow) tracks performance on each change.
- Monorepos are first-class. fallow reads npm, yarn, and pnpm workspaces, and `--workspace <name>` scopes a run to one package.
- Over 100 built-in [framework plugins](https://fallow.tools/docs/frameworks/built-in/) find entry points and framework conventions, so the first run needs no config.
- Each command has a typed JSON output, documented exit codes, and a published [output schema](docs/output-schema.json).
- Analysis runs on your machine or CI runner. Telemetry is opt-in ([what fallow collects](docs/telemetry.md)).
- The project has a public issue tracker, a public [roadmap](ROADMAP.md), and a [security policy](SECURITY.md).

The analyzer is written in Rust and uses [Oxc](https://oxc.rs) for syntactic analysis. Static analysis needs no TypeScript compiler and no Node.js runtime.

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

The npm package includes the `fallow`, `fallow-lsp`, and `fallow-mcp` launchers. For pnpm, yarn, `cargo install fallow-cli`, and Docker, see the [installation guide](https://fallow.tools/docs/installation/).

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

`// fallow-ignore-file <issue-type>` suppresses a whole file. JSDoc tags (`@public`, `@internal`) keep intentional library API quiet. `npx fallow suppressions` lists every suppression in the project.

The [adoption guide](https://fallow.tools/docs/adoption/) shows the staged path. [Configuration](https://fallow.tools/docs/configuration/overview/) has the full reference.

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
  - remote: 'https://raw.githubusercontent.com/fallow-rs/fallow/v3.31.0/ci/gitlab-ci.yml'

fallow:
  extends: .fallow
```

To start in report-only mode, add `fail-on-issues: false`. Remove it when the team is ready for a gate. With `command: audit`, a pull request fails only on findings that it introduces. Without `command: audit`, the Action runs the full pipeline, and any finding fails the job (`fail-on-issues` defaults to true). In a pull request, the Action scopes the analysis to the changed files, so its output can differ from a full local run.

The Action installs the fallow version that the project's `package.json` names. Pin an exact version there to use the same version in CI and on your machine. It can post a PR comment and inline review comments, and it can upload SARIF to GitHub Code Scanning. Other output formats are CodeClimate, GitHub annotations, and Markdown. The GitLab template URL names a release tag, because GitLab includes a file from a fixed ref. The [CI guide](https://fallow.tools/docs/integrations/ci/) covers inputs, permissions, and a staged rollout.

For a PR comment or review comments, give the job these permissions. `id-token: write` is optional: it lets the Action post as the fallow bot. Without it, the comments come from `github-actions[bot]`.

```yaml
permissions:
  contents: read
  id-token: write
  pull-requests: write
  checks: write
```

Exit code 0 means no error-severity findings, or an audit result of pass or warn. Exit code 1 means error-severity findings, or an audit result of fail. In CI, exit code 1 fails the job, and that is the gate. The rule severity in the config (`error`, `warn`, `off`) sets which findings count. Exit code 2 means invalid input or an execution error. A script that reads the JSON output can treat 0 and 1 as a completed run, but must not hide exit code 2 with `|| true`.

<details>
<summary>All exit codes and the JSON error format</summary>

| Exit code | Meaning |
|---|---|
| 0 | Clean, or audit result pass or warn |
| 1 | Findings, or audit result fail (a normal outcome) |
| 2 | Validation or runtime error (JSON error on stdout with `--format json`) |
| 3 | A requested resource is unavailable, for example when `config --path` finds no config |
| 8 | Security gate hit (`fallow security --gate`) |
| 4 to 7, 10 to 13 | Runtime coverage, license, network, and upload errors |

With `--format json`, an error arrives on stdout as `{"error": true, "message": "...", "exit_code": 2}`, not as a stack trace.

</details>

## Editors and integrations

- The [VS Code extension](https://fallow.tools/docs/integrations/vscode/) shows findings in the editor.
- `fallow-lsp` gives diagnostics, hover, code actions, and code lenses in any LSP editor. [`editors/`](editors/) has setups for Zed and [Neovim](https://fallow.tools/docs/integrations/neovim/).
- The Node API [`@fallow-cli/fallow-node`](https://fallow.tools/docs/integrations/node-bindings/) exports `detectDeadCode`, `detectCircularDependencies`, `detectBoundaryViolations`, `detectDuplication`, `detectSimilarCode`, `detectFeatureFlags`, `computeComplexity`, and `computeHealth`. The [package API reference](crates/napi/README.md) has the options and return types.
- `fallow health --format badge > badge.svg` writes a [health badge](https://fallow.tools/docs/integrations/badges/) for your README.

## With coding agents

```bash
npx fallow agent install --dry-run   # show the plan
npx fallow agent install             # apply it
```

`fallow agent install` sets up the agents that it detects (Claude Code, Codex, and Cursor) in one pass. It writes an `AGENTS.md` task map, installs the `fallow` and `fallow-setup` skills, registers the MCP server, and adds a gate on `git commit` and `git push`. Run `npx fallow agent status` to see these changes. Run `npx fallow agent uninstall` to remove them.

To register only the [MCP server](https://fallow.tools/docs/integrations/mcp/):

```json
{ "mcpServers": { "fallow": { "command": "npx", "args": ["fallow-mcp"] } } }
```

Scripts and agents that call the CLI directly add `--format json --quiet`. Each command then writes one typed JSON document to stdout:

- A root `kind` field names the analysis that made the document.
- Each finding has an `actions[]` array and an `auto_fixable` flag, so a script knows what `fallow fix` can do.
- Root `next_steps[]` suggestions are commands that run as written.
- The types ship as `fallow/types`, and [docs/output-schema.json](docs/output-schema.json) is the full schema.

Do not run `fallow watch` in an agent loop, because it does not exit.

Exit codes and the JSON error format are in [In pull requests](#in-pull-requests). The [agent skills guide](https://fallow.tools/docs/integrations/agent-skills/) has the details.

## Migrate existing config

`npx fallow migrate` converts knip, jscpd, and stylelint config into fallow config. The migration guides for [knip](https://fallow.tools/docs/migration/from-knip/) and [jscpd](https://fallow.tools/docs/migration/from-jscpd/) show each step.

## Contributing

To report a missing framework plugin or a false positive, [open an issue](https://github.com/fallow-rs/fallow/issues). [CONTRIBUTING.md](CONTRIBUTING.md) covers the development setup. [docs/README.md](docs/README.md) is the entry point for maintainer documentation.

<a href="https://github.com/fallow-rs/fallow/graphs/contributors">
  <img src="https://contrib.rocks/image?repo=fallow-rs/fallow" alt="Contributors">
</a>

## License

MIT. See [LICENSE](LICENSE).

If fallow helps you or your team, you can support its development through [GitHub Sponsors](https://github.com/sponsors/fallow-rs).
