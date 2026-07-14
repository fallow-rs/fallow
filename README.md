<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo.svg">
    <img src="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo.svg" alt="fallow" width="290">
  </picture>
</p>

<p align="center">
  <strong>Codebase intelligence for TypeScript and JavaScript.</strong><br>
  One binary finds unused code, circular dependencies, duplication, complexity hotspots, boundary violations, and design-system styling drift. An optional paid layer, Fallow Runtime, adds production execution evidence.<br>
  <sub>Deterministic findings and typed output contracts. No AI inside the analyzer, and no TypeScript compiler or Node.js runtime needed for static analysis.</sub>
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
  <a href="https://docs.fallow.tools/integrations/mcp">MCP</a> ·
  <a href="BENCHMARKS.md">Benchmarks</a>
</p>

---

Most repositories carry code nobody dares to delete, because deleting means proving a negative: fallow reads the repository as one dependency graph, from import edges to styling tokens, and reports what that graph shows.

```
Audit scope: 19 changed files vs HEAD~15 (8fbfcb054..HEAD)

● Unused files (2)
  packages/vitest/src/public/reporters.ts
  test/coverage-test/test/configuration-options.test-d.ts

● Circular dependencies (6)
  packages/vitest/src/integrations/vi.ts
    → wait.ts → vi.ts

✗ dead code: 156 issues · complexity: 6 findings · duplication: 8 clone groups · 19 changed files (1.05s)
  audit gate excluded 163 inherited findings (run with --gate all to enforce)
```

*Excerpt from `fallow audit` on the vitest monorepo, auditing its last 15 commits. fallow 3.5.0, warm base-snapshot cache. The same run with `--format json` returns one typed JSON document.*

## Quick start

```bash
# Zero-install, full pipeline (dead code + duplication + health)
npx fallow

# Gate only what a PR changed
npx fallow audit

# Install as a devDependency
npm install --save-dev fallow

# For agents and scripts: exit 1 means findings, not failure
npx fallow audit --format json --quiet 2>/dev/null || true
```

The npm package ships the `fallow`, `fallow-lsp`, and `fallow-mcp` launchers plus a version-matched agent skill, so the editor and agent integrations resolve the project-local binary instead of whatever happens to be on `PATH`. Runs are deterministic: the same input produces the same output with stable fingerprints. Re-running to verify an edit is safe. Other channels (pnpm, yarn, `cargo install fallow-cli`, and a local Docker build with a Compose example at [`examples/docker/compose.yaml`](examples/docker/compose.yaml)) are covered in the [installation guide](https://docs.fallow.tools/installation).

## What fallow reports

- [Unused files, exports, types, enum and class members, and dependencies](https://docs.fallow.tools/analysis/dead-code)
- [Circular dependencies and re-export cycles](https://docs.fallow.tools/analysis/dead-code), part of `fallow dead-code`
- [Code duplication](https://docs.fallow.tools/analysis/duplication) with a suffix-array detector covering JS/TS and CSS-family stylesheets, plus Vue/Svelte/Astro component regions
- [Complexity hotspots](https://docs.fallow.tools/explanations/health) and a 0 to 100 health score with a letter grade
- [Architecture boundary violations](https://docs.fallow.tools/analysis/boundaries) with `bulletproof`, `layered`, `hexagonal`, and `feature-sliced` presets
- [Design-system styling drift](https://docs.fallow.tools/analysis/css-analysis) for CSS and CSS-in-JS
- [A changed-file PR gate](https://docs.fallow.tools/cli/audit) with a pass, warn, or fail verdict (`fallow audit`)
- [Auto-fix](https://docs.fallow.tools/analysis/auto-fix) with a dry-run preview
- Opt-in [security candidates](docs/security-agent-verification.md) ranked by reachability from entry points (`fallow security`)

Over 100 built-in [framework plugins](https://docs.fallow.tools/frameworks/built-in) detect entry points automatically, so the first run needs no configuration. Fallow Runtime, the optional paid layer, merges production execution evidence into these same reports; see [Runtime intelligence (optional)](#runtime-intelligence-optional) and [static vs runtime](https://docs.fallow.tools/explanations/static-vs-runtime).

## Your first run

Findings on a first run usually mean fallow is missing an entry point or a framework convention, or is analyzing generated files you never meant to include. The built-in plugins take care of framework detection; it's generated code that usually needs a hint:

```json
{
  "$schema": "./node_modules/fallow/schema.json",
  "ignorePatterns": ["**/*.generated.ts"]
}
```

You do not have to author that config yourself. [`npx fallow recommend`](https://docs.fallow.tools/cli/recommend) detects the stack (frameworks, workspace layout, test runner, package manager), prints a proposed config as a safe starting point, and ends with the few genuinely subjective choices it will not decide for you. It is read-only and always exits 0; nothing changes until you save the config.

Patterns are relative to the project root and add to fallow's built-in ignore defaults (node_modules, dist, coverage, minified bundles). Config precedence is first match wins per directory, with no merging: `.fallowrc.json` (JSONC accepted) > `.fallowrc.jsonc` > `fallow.toml` > `.fallow.toml`. The full reference is the [configuration overview](https://docs.fallow.tools/configuration/overview); known limits (syntactic analysis, no type resolution) are documented in [limitations](https://docs.fallow.tools/analysis/limitations); for a hung or failed run, see [debugging](https://docs.fallow.tools/analysis/debugging).

## Commands

| Command | What it does |
|---|---|
| `npx fallow` | Full pipeline: dead code, duplication, health |
| [`npx fallow audit`](https://docs.fallow.tools/cli/audit) | Changed-file gate over dead code, complexity, duplication, and styling drift: verdict pass/warn/fail against a base ref |
| [`npx fallow dead-code`](https://docs.fallow.tools/cli/dead-code) | Unused code and circular dependencies (alias: `check`) |
| `npx fallow dead-code --trace src/file.ts:symbol` | Prove a symbol is unused before deleting it |
| [`npx fallow dupes`](https://docs.fallow.tools/cli/dupes) | Duplication; modes `strict`, `mild` (default), `weak`, `semantic` |
| [`npx fallow health --score`](https://docs.fallow.tools/cli/health) | Complexity, 0 to 100 health score, hotspots; `--css` adds structural CSS analytics |
| [`npx fallow fix --dry-run`](https://docs.fallow.tools/cli/fix) | Preview auto-fixes; apply with `npx fallow fix` |
| `npx fallow guard src/file.ts` | Which boundary rules apply to a file before editing |
| `npx fallow security` | Opt-in security candidates; `--gate new --changed-since <ref>` fails only on introduced ones |
| `npx fallow explain <issue-type>` | Explain a rule without analyzing |
| [`npx fallow recommend`](https://docs.fallow.tools/cli/recommend) | Detect the stack and propose a config; subjective choices stay open questions |
| [`npx fallow init`](https://docs.fallow.tools/cli/init) | Scaffold config; `--agents` scaffolds an AGENTS.md |
| `npx fallow migrate` | Migrate from knip, jscpd, or stylelint config |
| `npx fallow schema` | Machine-readable capability manifest (always JSON) |

Every other command, one line each:

| Command | Purpose |
|---|---|
| `fallow review --brief` | Advisory orientation brief over changed files; always exits 0 |
| `fallow inspect --file src/api.ts` | Evidence bundle for one file, or one symbol via `--symbol src/api.ts:client` |
| `fallow trace src/utils.ts:formatDate` | Symbol-level call chains: callers up, callees down |
| `fallow watch` | Re-run analysis on file changes (interactive use; agents should not run it) |
| `fallow flags` | Detect feature-flag patterns |
| `fallow suppressions` | Inventory of `fallow-ignore` markers |
| `fallow list` | Entry points, files, plugins, and boundaries (`--boundaries`) |
| `fallow workspaces` | Monorepo workspace discovery diagnostics |
| `fallow config` | Resolved configuration and which file provided it |
| `fallow decision-surface` | Ranked structural decisions a change embeds |
| `fallow impact` | Opt-in, local-only report of what fallow caught; `--all` spans repos |
| `fallow report --from results.json` | Re-render a saved JSON result in another output format |
| `fallow ci ...` | PR/MR feedback helpers (comments, reviews, check runs) |
| `fallow ci-template gitlab --vendor` | Vendor the GitLab CI template for offline runners |
| `fallow hooks install --target git` | Managed pre-commit hook; `--target agent` writes agent-gate hooks |
| `fallow rule-pack init` | Declarative policy rule packs (`list`, `test`, `schema`) |
| `fallow plugin-check` | Dry-run an external framework plugin |
| `fallow config-schema` | JSON Schema for config; also `plugin-schema` and `rule-pack schema` |
| `fallow license activate --trial --email you@company.com` | Fallow Runtime licensing (`status`, `refresh`, `deactivate`) |
| `fallow telemetry status` | Opt-in telemetry, off by default (`enable`, `disable`, `inspect --example`) |
| `fallow coverage setup` | Runtime coverage workflow (`analyze`, `upload-inventory`, `upload-source-maps`, `upload-static-findings`) |

Per-command flags come from `fallow schema` (machine-readable) or the [CLI reference](https://docs.fallow.tools/cli/global-flags).

## Output and exit codes

For machine consumption, add `--format json --quiet` to any command, discard stderr, and append `|| true` in shells that treat exit code 1 as failure.

| `--format` | What you get |
|---|---|
| `human` (default) | Terminal report with a `Next:` suggestion line |
| `json` | The machine contract: one typed JSON document on stdout |
| `sarif` | GitHub Code Scanning and other SARIF consumers |
| `compact` | One grep-friendly line per finding |
| `markdown` (`md` accepted via `FALLOW_FORMAT`) | Markdown report |
| `codeclimate` (aliases `gitlab-codequality`, `gitlab-code-quality`) | GitLab Code Quality report |
| `github-annotations` | Workflow-command annotations; render on fork PRs without a write token |
| `github-summary` | Job-summary markdown for `$GITHUB_STEP_SUMMARY` |
| `pr-comment-github`, `pr-comment-gitlab`, `review-github`, `review-gitlab`, `badge` | Typed CI feedback envelopes, plus a shields.io-compatible SVG health badge |

| Exit code | Meaning |
|---|---|
| 0 | Clean, or audit verdict pass or warn |
| 1 | Findings, or audit verdict fail (a normal outcome) |
| 2 | Validation or runtime error (JSON error envelope on stdout with `--format json`) |
| 7 | Network failure (license and cloud operations) |
| 8 | Security gate hit (`fallow security --gate`) |

Rule severity maps onto exit codes: `error` fails CI (the default), `warn` exits 0, `off` skips the rule ([rules reference](https://docs.fallow.tools/configuration/rules)).

The JSON contract, in short:

- a root `kind` discriminator names the analysis that produced the document
- per-issue `actions[]` with an `auto_fixable` flag, so a script knows which findings it can hand to `fallow fix`
- root `next_steps[]` suggestions are runnable as-is
- errors arrive as `{"error": true, "message": "...", "exit_code": 2}` on stdout, not as a stack trace

Typed contracts ship with the npm package: `import type { CheckOutput, FallowJsonOutput } from "fallow/types"`. The generated schema lives at [docs/output-schema.json](docs/output-schema.json). The output format is CLI-only, via `--format` or `FALLOW_FORMAT`, and is never set in config; all environment variables are listed in [docs/environment-variables.md](docs/environment-variables.md).

## Built for agents

The JSON contract and exit codes above are the agent interface.

```json
{ "mcpServers": { "fallow": { "command": "npx", "args": ["fallow-mcp"] } } }
```

The [MCP server](https://docs.fallow.tools/integrations/mcp) covers analysis, audit, health, duplication, tracing, fix preview and apply, boundary guard checks, and target inspection; every tool documents its nearest CLI fallback, and a bounded read-only Code Mode sandbox composes analysis calls without filesystem or network access.

- [`npx fallow recommend --format json`](https://docs.fallow.tools/cli/recommend) is the onboarding entry point: it returns the detected stack, a proposed config, and every decision with its tier and rationale, and it ships the subjective choices as ready-to-ask questions with options and tradeoffs. An agent authors `.fallowrc.json` from evidence and asks the user only what fallow will not decide
- A version-matched agent skill ships in the npm package under `node_modules/fallow/skills/fallow` ([agent skills](https://docs.fallow.tools/integrations/agent-skills), companion repo [fallow-skills](https://github.com/fallow-rs/fallow-skills))
- `npx fallow init --agents` scaffolds an AGENTS.md with a task-to-command matrix
- `npx fallow hooks install --target agent` gates `git commit` and `git push` on `fallow audit` ([hooks](https://docs.fallow.tools/integrations/claude-hooks))
- A compliance loop with a copy-paste agent prompt: [docs/fallow-compliance.md](docs/fallow-compliance.md)
- To verify security candidates from an agent harness, follow [docs/security-agent-verification.md](docs/security-agent-verification.md)
- Never run `fallow watch` in an agent loop; it does not exit. Telemetry is off by default and opt-in only, with `DO_NOT_TRACK` honored ([docs/telemetry.md](docs/telemetry.md))

## Suppressing findings

```ts
// fallow-ignore-next-line unused-export -- kept for plugin consumers
export const keepThis = 1;
```

`// fallow-ignore-file <issue-type>` suppresses the whole file. Both marker forms take a comma-separated list of issue kinds and an optional `-- <reason>` suffix that suppression hygiene records. JSDoc visibility tags (`@public`, `@internal`, `@expected-unused -- <reason>`) keep intentional library API surface quiet. `fallow suppressions` prints the inventory.

For staged [adoption](https://docs.fallow.tools/adoption), save a baseline once from a clean ref with `--save-baseline`, then pass `--baseline` on every run; `fallow audit` accepts per-analysis baselines. Full syntax lives at [suppression](https://docs.fallow.tools/configuration/suppression).

## CI

GitHub Actions:

```yaml
- uses: actions/checkout@v4
  with:
    fetch-depth: 0
- uses: fallow-rs/fallow@v3
```

The Action defaults to the full pipeline with SARIF output and PR-scoped analysis via automatic base detection. Annotations, sticky comments, and review comments are inputs documented in the [CI guide](https://docs.fallow.tools/integrations/ci).

GitLab:

```yaml
include:
  - remote: 'https://raw.githubusercontent.com/fallow-rs/fallow/v3.5.0/ci/gitlab-ci.yml'

fallow:
  extends: .fallow
```

MR comments need a `GITLAB_TOKEN` with api scope; `CI_JOB_TOKEN` cannot create notes. Runners that cannot reach GitHub raw can vendor the template with `npx fallow ci-template gitlab --vendor`.

CI runs diff-scoped on pull requests by default, so CI output can legitimately differ from a full local run. Commit baseline files to keep CI and local in agreement.

## Runtime intelligence (optional)

Fallow Runtime is the optional paid layer. It merges production execution evidence (V8 coverage dumps via `NODE_V8_COVERAGE`, or Istanbul files) into `fallow health` and `fallow audit`: hot paths for careful review, cold-code deletion confidence, runtime-weighted health, and stale-flag evidence. A single local coverage capture is free; continuous and cloud runtime monitoring requires a license. Everything else in this README is free and needs no license.

```bash
npx fallow license activate --trial --email you@company.com   # 30-day trial, offline Ed25519 verification
npx fallow coverage setup                                     # resumable first-run flow
```

Details: [runtime coverage](https://docs.fallow.tools/analysis/runtime-coverage) and [static vs runtime](https://docs.fallow.tools/explanations/static-vs-runtime).

## Editors and integrations

- [VS Code extension](https://docs.fallow.tools/integrations/vscode)
- Zed and Neovim setups under the [`editors/`](https://github.com/fallow-rs/fallow/tree/main/editors) tree ([Neovim guide](https://docs.fallow.tools/integrations/neovim))
- The `fallow-lsp` server: diagnostics, hover, code actions, and code lenses. It resolves the project-local binary from a devDependency install
- The Node API [`@fallow-cli/fallow-node`](https://docs.fallow.tools/integrations/node-bindings) exports `detectDeadCode`, `detectDuplication`, and `computeHealth`
- [README badges](https://docs.fallow.tools/integrations/badges): `fallow health --format badge > badge.svg`

## Migrating from other tools

`npx fallow migrate` translates existing knip, jscpd, or stylelint configuration into a fallow config. Guides: [from knip](https://docs.fallow.tools/migration/from-knip), [from jscpd](https://docs.fallow.tools/migration/from-jscpd), and the [comparison page](https://docs.fallow.tools/migration/comparison).

## Performance

On the dead-code benchmark set, fallow analyzes fastify in 64ms where knip 6 takes 205ms, and preact in 74ms against 2.01s (27.1x). The counterweight is real too: knip measures faster on astro and TypeScript, and jscpd remains faster at raw duplication scanning. fallow also completes analysis on three projects in the set where knip's runs errored on those projects' own config files (next.js at 20,558 files, vite, and vue/core).

Measured on fallow 2.100.0, Apple M5, medians of 5 cold runs. Methodology and full tables live in [BENCHMARKS.md](BENCHMARKS.md), with reproduction scripts under [`benchmarks/`](benchmarks/). For how fallow relates to lint tooling, see [fallow vs linters](https://docs.fallow.tools/explanations/fallow-vs-linters).

## Documentation

Everything lives at [docs.fallow.tools](https://docs.fallow.tools): [quickstart](https://docs.fallow.tools/quickstart), [configuration overview](https://docs.fallow.tools/configuration/overview), [CLI reference](https://docs.fallow.tools/cli/global-flags), [MCP](https://docs.fallow.tools/integrations/mcp), [CI](https://docs.fallow.tools/integrations/ci), and [limitations](https://docs.fallow.tools/analysis/limitations). A machine-readable index is at [docs.fallow.tools/llms.txt](https://docs.fallow.tools/llms.txt).

In this repository:

- [docs/architecture-invariants.md](docs/architecture-invariants.md): the invariants the analyzer holds itself to
- [docs/plugin-authoring.md](docs/plugin-authoring.md): writing an external framework plugin
- [BENCHMARKS.md](BENCHMARKS.md): benchmark methodology and reference tables
- [ROADMAP.md](ROADMAP.md): where fallow is headed
- [CONTEXT.md](CONTEXT.md): domain vocabulary for contributors and agents

## Contributing and license

Missing a framework plugin? Found a false positive? [Open an issue](https://github.com/fallow-rs/fallow/issues). Development setup is covered in [CONTRIBUTING.md](CONTRIBUTING.md).

MIT, see [LICENSE](LICENSE).
