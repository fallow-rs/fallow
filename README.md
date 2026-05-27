<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo.svg">
    <img src="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo.svg" alt="fallow" width="290">
  </picture>
</p>

<p align="center">
  <strong>Deterministic codebase intelligence for TypeScript and JavaScript.</strong><br>
  Quality, risk, architecture, dependencies, duplication, and safe cleanup evidence for humans, CI, and agents.<br>
  Static analysis is free and open source. Optional runtime intelligence (Fallow Runtime) adds production execution evidence.<br>
  <strong>Rust-native. Zero config. Sub-second. No AI inside the analyzer.</strong>
</p>

<p align="center">
  <a href="https://github.com/fallow-rs/fallow/actions/workflows/ci.yml"><img src="https://github.com/fallow-rs/fallow/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/fallow-rs/fallow/actions/workflows/coverage.yml"><img src="https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/fallow-rs/fallow/badges/coverage.json" alt="Coverage"></a>
  <a href="https://github.com/fallow-rs/fallow/actions/workflows/ci.yml"><img src="https://img.shields.io/badge/self--analyzed-fallow-brightgreen" alt="Self analyzed"></a>
  <a href="https://crates.io/crates/fallow-cli"><img src="https://img.shields.io/crates/v/fallow-cli.svg" alt="crates.io"></a>
  <a href="https://www.npmjs.com/package/fallow"><img src="https://img.shields.io/npm/v/fallow.svg" alt="npm"></a>
  <a href="https://github.com/fallow-rs/fallow/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License"></a>
  <a href="https://docs.fallow.tools"><img src="https://img.shields.io/badge/docs-docs.fallow.tools-blue.svg" alt="Documentation"></a>
</p>

---

Fallow turns a JS/TS repository into a trusted quality report: health score, changed-code risk, hotspots, duplication, architecture issues, dependency hygiene, and cleanup opportunities. It helps you answer:

- What changed?
- What got riskier?
- What should I review?
- What should I refactor?
- What can be safely removed?

Fallow is built for maintainers, CI pipelines, editors, and AI agents that need structured evidence instead of guesses. No AI inside the analyzer. Fallow produces deterministic findings, typed output contracts, and traceable explanations that downstream tools can trust.

Fallow dogfoods its shipped JavaScript and TypeScript surfaces in CI: the VS Code extension and npm wrapper package are analyzed with fallow on every relevant change.

## Quick start

Run a changed-code audit:

```bash
npx fallow audit
```

Example output:

```
Audit scope: 7 changed files vs main

-- Dead Code ---------------------------------------

x 7 unused dependencies · 14 dev/optional dependencies
  21 issues · 1 suppressed · 0 stale suppressions

-- Duplication -------------------------------------

x 3 clone families touching changed files

-- Complexity --------------------------------------

! 2 changed functions above threshold
```

Cleanup opportunities include unused files, unused exports, unused dependencies, stale suppressions, and other code that no longer appears to carry product value.

For machine-readable output:

```bash
npx fallow audit --format json
```

For quality scoring and refactor targets:

```bash
npx fallow health --score --hotspots --targets
```

For cleanup-specific findings:

```bash
npx fallow dead-code
```

113 framework plugins. No Node.js runtime required for static analysis. No config needed for the first run.

## What is Fallow?

Fallow is a codebase intelligence engine for TypeScript and JavaScript projects.

It analyzes your repository as a system, not just as a list of files. It connects static structure, dependency relationships, duplication, complexity, architecture boundaries, package hygiene, and optional runtime evidence into one quality report.

Fallow helps teams:

- review risky pull requests before they merge
- track quality trends over time
- find architectural hotspots
- understand dependency hygiene
- detect duplicated logic
- explain why code is used, unused, risky, or safe to remove
- provide structured repo context to AI agents and editor tools

Linters check files. TypeScript checks types. Fallow checks the codebase. Fallow does not use AI to invent findings. It produces deterministic evidence that humans and agents can inspect.

## Install

```bash
npm install --save-dev fallow   # or: pnpm add -D fallow / yarn add -D fallow / bun add -d fallow
```

Installs the CLI, LSP server, MCP server, and version-matched Agent Skill into `node_modules`. For one-off CLI use, run `npx fallow`; Rust users can also run `cargo install fallow-cli`.

Parsing `fallow --format json` in TypeScript? `import type { CheckOutput } from "fallow/types"` gives you the full output contract, version-pinned to your installed CLI.

Programmatic Node API:

```bash
npm install @fallow-cli/fallow-node   # or: pnpm/yarn/bun add @fallow-cli/fallow-node
```

```ts
import { detectDeadCode, detectDuplication, computeHealth } from '@fallow-cli/fallow-node';

const findings = await detectDeadCode({ root: process.cwd() });
const dupes = await detectDuplication({ root: process.cwd(), mode: 'mild', minTokens: 30 });
const health = await computeHealth({ root: process.cwd(), score: true, ownershipEmails: 'handle' });
```

## What Fallow reports

### Quality score

A compact health score for the current state of the repository, with targets for maintainability, complexity, duplication, dependency hygiene, and architecture.

### PR risk

Changed-code analysis (`fallow audit`) that highlights files and symbols most likely to need review before merge. Returns a verdict (pass / warn / fail) and an attribution split between findings the PR introduced and pre-existing ones.

### Hotspots

Functions, files, and packages that combine complexity, churn, size, coupling, and (with the runtime layer) runtime importance.

### Duplication

Clone families and duplicated implementation patterns that increase maintenance cost. Four detection modes from exact token match to semantic clones with renamed variables.

### Architecture

Circular dependencies, boundary violations across layers and modules, re-export chains, and other dependency-graph issues. Zero-config presets for bulletproof, layered, hexagonal, and feature-sliced architectures.

### Dependency hygiene

Unused dependencies, unresolved imports, duplicate exports, unlisted imports, type-only production deps, test-only production deps, and pnpm catalog and overrides hygiene.

### Cleanup opportunities

Unused files, unused exports, unused types, unused enum members, unused class members, stale suppression comments, and code paths that appear safe to review for removal. Opt-in API hygiene checks such as private type leaks live here too.

### Runtime intelligence (optional)

Static analysis answers what is connected. Runtime intelligence answers what actually ran in production. Hot paths, cold code, runtime-weighted health, stale flags, runtime-backed PR review. See the [Runtime intelligence](#runtime-intelligence-optional) section below.

### Agent-ready context

Structured JSON, an MCP server, and an LSP for answering "what depends on this?", "why is this used?", "what changed?", and "what action is safest?".

## Built for agents

Fallow gives AI agents structured repo truth instead of forcing them to infer everything from grep.

Agents can ask:

- Who imports this symbol?
- Why is this export considered used?
- Why is this export considered unused?
- What changed in this PR?
- Which files are risky to touch?
- Which files are architectural hotspots?
- What duplicate siblings exist?
- What cleanup action is safest?
- What evidence supports this finding?

Fallow exposes this through JSON output, typed output contracts (`import type { CheckOutput } from "fallow/types"`), the MCP server, and the LSP. Every issue in `--format json` carries a machine-actionable `actions` array with an `auto_fixable` flag so agents can self-correct.

Common agent workflow:

1. generate or edit code
2. run `fallow audit --format json`
3. inspect findings and per-issue `actions`
4. apply safe fixes or adjust the patch before opening a PR
5. hand the result to a human reviewer with better evidence

```bash
npx fallow audit --format json
npx fallow --format json
npx fallow fix --dry-run --format json
```

For full adoption instead of one-off review, see the [Fallow compliance happy path](https://github.com/fallow-rs/fallow/blob/main/docs/fallow-compliance.md). It defines the end state and includes a copy-paste agent onboarding prompt.

See [Agent integration](https://docs.fallow.tools/integrations/mcp) for MCP setup and the full list of structured tools.

## Why teams using AI need Fallow

AI accelerates code creation. It does not eliminate review, cleanup, or architecture drift.

When Claude Code, Codex, Cursor, or other tools generate changes, teams still need to know:

- did this introduce risky complexity?
- did it duplicate logic that already existed?
- did the change cross an architectural boundary it should not cross?
- did it leave behind unused code or stale dependencies?
- is this code on a hot path or a cold one?
- what should the reviewer read closely first?

Fallow answers those questions with deterministic, graph-based analysis and structured output, so both humans and agents can act on facts instead of guesses.

## More static commands

```bash
fallow                       # Full codebase analysis: cleanup + duplication + health
fallow audit                 # Audit changed files (verdict: pass/warn/fail)
fallow health                # Complexity + refactor targets
fallow dupes                 # Repeated logic
fallow dead-code             # Cleanup candidates
fallow explain unused-export # Explain a rule without analyzing
fallow watch                 # Re-analyze on file changes
fallow fix --dry-run         # Preview automatic cleanup
```

Combined mode (`fallow`) and `fallow audit` support per-analysis production mode. Precedence is CLI flags, then environment variables, then config:

```jsonc
{
  "production": {
    "health": true,
    "deadCode": false,
    "dupes": false
  }
}
```

Use `--production-health`, `--production-dead-code`, or `--production-dupes` for one invocation, or `FALLOW_PRODUCTION_HEALTH=true` and related env vars in CI. The global `--production` flag still enables production mode for every analysis.

Precedence (highest to lowest): CLI flags, per-analysis env var, global `FALLOW_PRODUCTION`, config. CLI flags only enable; env vars and config can also disable. Worked examples:

```bash
# Run health in production mode, dead-code and dupes on the full tree
fallow --production-health

# Same, via env var (useful in CI templates that pass env-only)
FALLOW_PRODUCTION_HEALTH=true fallow

# Per-analysis env wins over the global env, so this runs health in production mode
# even though the global env says off (the typical CI-template defaults case)
FALLOW_PRODUCTION=false FALLOW_PRODUCTION_HEALTH=true fallow

# CLI flags beat env vars; this turns ALL three on regardless of any FALLOW_PRODUCTION_* env
fallow --production
```

## Cleanup opportunities

Cleanup opportunities are code that no longer appears to carry product value: unused files, exports, dependencies, types, enum members, class members, unresolved imports, unlisted dependencies, duplicate exports, circular dependencies (including cross-package cycles in monorepos), boundary violations, type-only dependencies, test-only production dependencies, and stale suppression comments. Workspace package dependencies are checked like external packages, so unused or undeclared internal package edges are visible in monorepos. Entry points are auto-detected from package.json fields, package scripts, framework conventions, and plugin patterns. Public class members on classes exposed from non-private package entry points or exportless source subpath indexes are treated as library API surface, while reachable internal classes still get member-level checks. Arrow-wrapped dynamic imports (`React.lazy`, `loadable`, `defineAsyncComponent`) and proven local `child_process.fork()` runner targets are tracked as references. Script multiplexers (`concurrently`, `npm-run-all`) are analyzed to discover transitive script dependencies. JSDoc tags (`@public`, `@internal`, `@beta`, `@alpha`, `@expected-unused`) control export visibility. Private type leaks are currently opt-in API hygiene findings via `--private-type-leaks` or the `private-type-leaks` rule.

```bash
fallow dead-code                          # All dead code issues
fallow dead-code --unused-exports         # Only unused exports
fallow dead-code --private-type-leaks     # Opt-in private type leak API hygiene
fallow dead-code --circular-deps          # Only circular dependencies
fallow dead-code --boundary-violations    # Only boundary violations
fallow dead-code --stale-suppressions     # Only stale suppression comments
fallow dead-code --production             # Exclude test/dev files
fallow dead-code --changed-since main     # Only changed files (for PRs)
fallow dead-code --file src/utils.ts       # Single file (lint-staged integration)
fallow dead-code --include-entry-exports  # Also check exports from entry files
fallow dead-code --group-by owner         # Group by CODEOWNERS for team triage
fallow dead-code --group-by directory     # Group by first directory component
fallow dead-code --group-by package       # Group by workspace package (monorepo)
fallow dead-code --group-by section       # Group by GitLab CODEOWNERS section
```

## Duplication

Finds copy-pasted code blocks across your codebase. Suffix-array algorithm -- no quadratic pairwise comparison. Repeated atomic function calls are filtered by default, so long calls to an existing shared abstraction do not show up as refactoring work.

```bash
fallow dupes                              # Default (mild mode)
fallow dupes --mode semantic              # Catch clones with renamed variables
fallow dupes --skip-local                 # Only cross-directory duplicates
fallow dupes --group-by owner             # Partition clone groups by CODEOWNERS team
fallow dupes --group-by directory         # Partition clone groups by directory
fallow dupes --trace src/utils.ts:42      # Show all clones of code at this location
```

Four detection modes: **strict** (exact tokens), **mild** (default, AST-based), **weak** (different string literals), **semantic** (renamed variables and literals).

## Complexity

Surfaces the most complex functions in your codebase and identifies where to spend refactoring effort. Angular templates are included as synthetic `<template>` entries when they use control flow or complex bindings, both for external `templateUrl` files and inline `@Component({ template: \`...\` })` decorators.

```bash
fallow health                             # Functions/templates exceeding thresholds
fallow health --score                     # Project health score (0-100) with letter grade
fallow health --min-score 70              # CI gate: fail if score drops below 70
fallow health --top 20                    # 20 most complex functions
fallow health --file-scores               # Per-file maintainability index (0-100)
fallow health --hotspots                  # Riskiest files (git churn x complexity)
fallow health --hotspots --ownership      # Add bus factor, owner, drift signals
fallow health --workspace @scope/app      # Scope vital signs + score to one package
fallow health --group-by package --score  # Per-package vital signs + score (monorepos)
fallow health --targets                   # Ranked refactoring recommendations
fallow health --targets --effort low      # Only quick-win refactoring targets
fallow health --coverage-gaps             # Static test coverage gaps
fallow health --coverage coverage/coverage-final.json
fallow health --coverage artifacts/coverage.json --coverage-root /home/runner/work/myapp
fallow health --runtime-coverage ./coverage
fallow health --runtime-coverage ./coverage --min-invocations-hot 250
fallow health --trend                     # Compare against saved snapshot
fallow health --changed-since main        # Only changed files
```

## Runtime intelligence (optional)

Static analysis answers: **what is connected to what?**

Runtime intelligence answers: **what actually ran?**

Fallow Runtime is the optional paid team layer. It uses runtime coverage as the collection engine (V8 dumps via `NODE_V8_COVERAGE=...` and Istanbul `coverage-final.json` files), then merges that evidence into `fallow health` so teams and coding agents can:

- review changes on hot production paths more carefully
- delete cold code with stronger evidence
- prioritize refactors by runtime importance
- spot stale feature-flag branches and stale runtime code
- give agents factual usage data instead of assumptions

```bash
fallow license activate --trial --email you@company.com
fallow coverage setup
fallow health --runtime-coverage ./coverage
fallow coverage analyze --cloud --repo owner/repo --format json
```

Static `coverage_gaps` and runtime `runtime_coverage` are separate layers in the same `health` surface:

| Surface | Flag | Input | Answers | License |
|:--|:--|:--|:--|:--|
| Static test reachability | `--coverage-gaps` | none | which runtime files/exports have no test dependency path | no |
| Exact CRAP scoring | `--coverage` | Istanbul JSON file or `coverage-final.json` directory | how covered each function is for CRAP computation | no |
| Runtime runtime coverage | `--runtime-coverage` | V8 directory, V8 JSON file, or Istanbul JSON file | which functions actually executed, which stayed cold, which are hot | yes |

Setup details:

- `fallow license activate --trial --email ...` starts a trial and stores the signed license locally
- `fallow license refresh` refreshes the stored license before the hard-fail window
- `fallow coverage setup` detects your framework and package manager, installs the sidecar if needed, writes a collection recipe, and resumes from the current setup state on re-run
- `fallow coverage setup --yes --json` emits deterministic agent-readable setup instructions without prompts, file writes, installs, or network calls. Add `--explain` to include a `_meta` block with field definitions, enum values, warning semantics, and the docs URL. In workspaces it emits per-runtime-package `members[]`, unions `runtime_targets`, prefixes member file paths, and skips pure workspace aggregator roots
- `fallow coverage analyze --cloud --repo owner/repo --format json` explicitly fetches the latest cloud runtime facts for a repo, merges them locally with the current AST/static analysis, and emits the same `runtime_coverage` JSON block. `FALLOW_API_KEY` alone does not enable cloud mode; pass `--cloud`, `--runtime-coverage-cloud`, or set `FALLOW_RUNTIME_COVERAGE_SOURCE=cloud`.
- `fallow coverage upload-inventory` pushes a static function inventory to fallow cloud so the dashboard's `Untracked` filter (functions that exist but never run) lights up. Runs in CI, respects `.gitignore` + `--exclude-paths`, preserves same-named functions by their line-aware cloud identity, and warns when inventory paths do not overlap recent runtime paths. For containerized deployments, pass `--path-prefix /app` (or your Dockerfile `WORKDIR`) so inventory paths match what the runtime beacon reports
- `fallow coverage upload-source-maps` uploads build `.map` files from CI so bundled runtime coverage resolves back to original source paths. Defaults to `dist/**/*.map`, `$GITHUB_SHA`, and basename matching; pass `--strip-path=false` when coverage reports bundle paths like `assets/app.js`
- Cloud API calls accept `FALLOW_CA_BUNDLE=/path/to/bundle.pem` for custom PEM trust bundles. The bundle replaces the default WebPKI roots, so private-CA environments should pass a complete trust bundle. `upload-source-maps` honors 429 `Retry-After` backoff, caps waits at 60 seconds, and exits 7 when setup or transport failures prevent every upload.
- The sidecar can be installed globally or as a project devDependency; fallow resolves `FALLOW_COV_BIN`, project-local shims, package-manager bin lookups, `~/.fallow/bin/fallow-cov`, and `PATH`
- `fallow health --runtime-coverage <path>` accepts a V8 directory, a single V8 JSON file, or a single Istanbul coverage map JSON file (commonly `coverage-final.json`)
- `fallow health --coverage <path>` accepts a single Istanbul coverage map JSON file or a directory containing `coverage-final.json`
- `--coverage-root <path>` must be an absolute prefix from the Istanbul file paths. Use it when coverage was generated in CI or Docker with a different checkout root, for example `fallow health --coverage artifacts/coverage-final.json --coverage-root /home/runner/work/myapp`
- V8 dumps that include Node's `source-map-cache` are remapped through supported source-map paths before analysis, including file paths, relative paths, `webpack://...`, and `vite://...`; unsupported virtual schemes safely fall back to raw V8 handling
- `fallow health --changed-since <ref> --runtime-coverage <path>` promotes touched hot paths to a `hot-path-touched` verdict during change review

Runtime coverage is merged into the same human, JSON, SARIF, compact, markdown, and CodeClimate outputs as the rest of the health report.

Read more: [Static vs runtime intelligence](https://docs.fallow.tools/explanations/static-vs-runtime) | [Runtime coverage](https://docs.fallow.tools/analysis/runtime-coverage)

## Audit

PR risk gate for human and AI-generated code. Combines changed-file cleanup findings from the dead-code pass with changed-file complexity and duplication findings, then emits a verdict.

```bash
fallow audit                              # Auto-detects base branch
fallow audit --base main                  # Explicit base ref
fallow audit --base HEAD~3               # Audit last 3 commits
fallow audit --production-health          # Production health, full dead-code/dupes
fallow audit --coverage artifacts/coverage-final.json --coverage-root /home/runner/work/myapp
fallow audit --gate all                   # Fail on inherited findings too
fallow audit --format json                # Structured output with verdict
```

Returns a verdict: **pass** (exit 0), **warn** (exit 0, warn-severity only), or **fail** (exit 1). By default, audit compares the current tree with the base ref and gates only findings introduced by the changeset; inherited findings are counted in JSON `attribution`, individual issue objects get `introduced: true|false`, and inherited findings are shown as context. Set `--gate all` or `audit.gate: "all"` to fail on every finding in changed files without running the extra base-snapshot attribution pass.

`audit` forwards `--coverage` and `--coverage-root` to its health sub-analysis for exact Istanbul-backed CRAP scoring. Relative `--coverage` paths resolve against `--root`; `--coverage-root` must be an absolute prefix from the coverage data. `FALLOW_COVERAGE` is used as the fallback when `--coverage` is omitted.

Audit caches base snapshots under `.fallow/cache/` and may keep a SHA-scoped temporary git worktree for reuse across runs against the same base ref. When the current checkout has `node_modules`, audit links it into the base worktree so tsconfig `extends` chains into installed packages and path aliases resolve like the working tree. Transient worktrees are removed on normal exit. Use `--no-cache` to disable snapshot and reusable-worktree caching; if a process is force-killed, run `git worktree prune` to clean up stale `.git/worktrees/fallow-audit-base-*` entries.

**Per-analysis baselines.** When touching legacy files with pre-existing issues, reuse the baselines saved by the individual subcommands so audit only fails on genuinely new findings:

```bash
# Save once from a clean ref
fallow dead-code --save-baseline fallow-baselines/dead-code.json
fallow health    --save-baseline fallow-baselines/health.json
fallow dupes     --save-baseline fallow-baselines/dupes.json

# Feed into audit on every PR
fallow audit \
  --dead-code-baseline fallow-baselines/dead-code.json \
  --health-baseline    fallow-baselines/health.json \
  --dupes-baseline     fallow-baselines/dupes.json
```

Keep committed baselines outside `.fallow/`; that directory is for cache and local data and is typically gitignored. `fallow-baselines/` is the recommended default. Configure defaults in `.fallowrc.json` under `audit.deadCodeBaseline` / `audit.healthBaseline` / `audit.dupesBaseline` so CI stays one command (`fallow audit`). CLI flags override config.

## CI integration

Use the GitHub Action when you want fallow to handle installation, caching, PR scoping, annotations, review comments, SARIF, and job-summary formatting.

```yaml
name: Fallow

on:
  pull_request:

permissions:
  contents: read
  pull-requests: write # needed for comment/review-comments

jobs:
  fallow:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
        with:
          fetch-depth: 0 # best diff precision for --changed-since and hotspots
      - uses: fallow-rs/fallow@v2
        with:
          command: audit
          comment: true
          review-comments: true
```

`command: audit` is the PR gate. In pull requests, the Action auto-scopes to the PR base SHA when `changed-since` is not set, derives a unified diff for line-level filtering, and emits a verdict: **pass**, **warn**, or **fail**. With the default `fail-on-issues: true`, audit fails the job only on verdict `fail`; warn-tier findings stay visible without blocking merge.

Useful GitHub Action modes:

```yaml
# Rich PR feedback without GitHub Advanced Security
- uses: fallow-rs/fallow@v2
  with:
    command: audit
    annotations: true        # default: inline workflow annotations
    comment: true            # sticky PR summary
    review-comments: true    # inline review comments with suggestions
    review-guidance: false   # set true for collapsed "What to do" blocks
    diff-filter: added       # added | diff_context | file | nofilter
    max-comments: 50

# GitHub Code Scanning upload
permissions:
  contents: read
  security-events: write
steps:
  - uses: actions/checkout@v4
    with:
      fetch-depth: 0
  - uses: fallow-rs/fallow@v2
    with:
      command: audit
      sarif: true

# Health score, trend, hotspots, and refactor targets
- uses: fallow-rs/fallow@v2
  with:
    command: health
    score: true
    trend: true
    save-snapshot: true
    hotspots: true
    targets: true

# Monorepo scoping
- uses: fallow-rs/fallow@v2
  with:
    command: audit
    changed-workspaces: origin/main

# Keep generated action artifacts out of the workspace root
- uses: fallow-rs/fallow@v2
  with:
    command: audit
    artifacts-dir: .var/fallow

# Coverage-backed CRAP scoring in audit
- uses: fallow-rs/fallow@v2
  with:
    command: audit
    max-crap: 30
    coverage: artifacts/coverage-final.json
    coverage-root: /home/runner/work/myapp

# Runtime evidence, licensed Fallow Runtime layer
- uses: fallow-rs/fallow@v2
  with:
    command: health
    score: true
    runtime-coverage: artifacts/v8-coverage
    min-invocations-hot: 100
```

Action outputs include:

- `issues` -- command-specific issue count; for audit, this is gate-aware
- `verdict` -- audit verdict (`pass`, `warn`, `fail`)
- `gate` -- audit gate (`new-only` or `all`)
- `results` / `sarif` -- generated artifact paths
- `changed-files-unavailable` -- `true` if PR file enumeration degraded and analysis ran less scoped than expected
- `dedup-lookup-failed` / `post-skipped-reason` -- comment/review posting degradation signals

Set `artifacts-dir` to write generated files such as `fallow-results.json`, `fallow-results.sarif`, `fallow-stderr.log`, and `fallow-analysis-args.sh` under a project-local generated directory. The default is `.` for backward compatibility, and the `results` / `sarif` outputs report the resolved paths for downstream steps.

SARIF upload requires GitHub Code Scanning, which is available on public repositories and on private repositories with GitHub Advanced Security enabled. If it is unavailable, the Action skips upload with a warning and leaves the job summary, annotations, comments, and JSON output intact.

GitHub inline review comments target the current PR file state (`side: RIGHT`). Findings on deleted lines are not modeled yet; fallow's diagnostics are current-state oriented in normal use.

For GitLab, use the bundled template. It installs fallow, sets `GIT_DEPTH: "0"`, caches `.fallow/`, produces Code Quality reports by default, and can post summary comments and inline MR discussions.

```yaml
# GitLab CI -- remote include
include:
  - remote: 'https://raw.githubusercontent.com/fallow-rs/fallow/vX.Y.Z/ci/gitlab-ci.yml'

fallow:
  extends: .fallow
  variables:
    FALLOW_COMMAND: "audit"
    FALLOW_COMMENT: "true"
    FALLOW_SUMMARY_SCOPE: "diff"
    FALLOW_REVIEW: "true"
    FALLOW_REVIEW_GUIDANCE: "true"
    FALLOW_MAX_CRAP: "30"
    FALLOW_COVERAGE: "artifacts/coverage-final.json"
    FALLOW_COVERAGE_ROOT: "/home/runner/work/myapp"
```

`FALLOW_COMMENT` and `FALLOW_REVIEW` require `GITLAB_TOKEN` with API scope. In MR pipelines, the template auto-sets `FALLOW_CHANGED_SINCE` from the MR diff base SHA when possible and derives `FALLOW_DIFF_FILE` for line-level filtering. For monorepos, set `FALLOW_CHANGED_WORKSPACES: "origin/main"` to scope analysis to touched workspaces. Set `FALLOW_SUMMARY_SCOPE=diff` when the sticky summary should hide pre-existing project-level findings outside the diff.

`FALLOW_REVIEW` uses the typed `review-gitlab` envelope v2, not scraped human output. That gives the template stable v2 fingerprints, same-line comment merging, UTF-8-safe body truncation, stale-thread reconciliation via `fallow ci reconcile-review`, and GitLab diff positions for inline discussions. The review script fetches MR `diff_refs` automatically; set `FALLOW_GITLAB_BASE_SHA`, `FALLOW_GITLAB_START_SHA`, or `FALLOW_GITLAB_HEAD_SHA` only when your runner needs explicit overrides.

```yaml
# GitLab CI -- vendored include when runners cannot reach GitHub raw
# Run once locally: npx fallow ci-template gitlab --vendor
# Commit the generated ci/ + action/ files.
include:
  - local: 'ci/gitlab-ci.yml'

fallow:
  extends: .fallow
```

For any other CI system, call the CLI directly:

```bash
# PR gate with changed-file attribution
npx fallow audit --changed-since origin/main --format json --quiet

# SARIF for code scanning systems
npx fallow --ci

# Line-level PR filtering from a unified diff
git diff --unified=0 origin/main...HEAD > fallow-pr.diff
npx fallow audit --changed-since origin/main --diff-file fallow-pr.diff

# Health score gate
npx fallow health --score --min-score 80 --quiet
```

Common CI flags:

- `--group-by owner|directory|package|section` -- group output by CODEOWNERS ownership, directory, workspace package, or GitLab CODEOWNERS `[Section]` headers for team-level triage
- `--summary` -- show only category counts (no individual issues)
- `--changed-since main` -- analyze only files touched in a PR
- `--diff-file <path>` / `--diff-stdin` -- filter source-anchored findings to added diff hunks, while project-level package findings bypass analysis line filtering. Sticky summary comments can use `FALLOW_SUMMARY_SCOPE=diff` to filter project-level findings too
- `--changed-workspaces origin/main` -- scope monorepo analysis to workspaces containing any changed file (CI primitive; fails hard on git errors so CI never silently widens back to the full repo)
- `--baseline` / `--save-baseline` -- fail only on **new** issues for individual analyses; audit uses the per-analysis baselines shown above
- `--fail-on-regression` / `--tolerance 2%` -- fail only if issues **grew** beyond tolerance
- `--format sarif` -- upload to GitHub Code Scanning
- `--format codeclimate` -- GitLab Code Quality inline MR annotations
- `--format pr-comment-github` / `--format pr-comment-gitlab` -- typed sticky PR/MR comment markdown
- `--format review-github` / `--format review-gitlab` -- typed inline review envelopes for CI scripts
- `--format annotations` -- GitHub Actions inline PR annotations (no Action required)
- `--format json` / `--format markdown` -- for custom workflows (JSON includes machine-actionable `actions` per issue)
- `--format badge` -- shields.io-compatible SVG health badge (`fallow health --format badge > badge.svg`)

Both the GitHub Action and GitLab CI template auto-detect your package manager (npm/pnpm/yarn) from lock files, so install/uninstall commands in review comments match your project.

Adopt incrementally -- surface issues without blocking CI, then promote when ready:

```jsonc
{ "rules": { "unused-files": "error", "unused-exports": "warn", "circular-dependencies": "off" } }
```

### GitLab CI rich MR comments

The GitLab CI template can post rich comments directly on merge requests -- summary comments with collapsible sections and inline review discussions with suggestion blocks.

| Variable | Default | Description |
|---|---|---|
| `FALLOW_COMMENT` | `"false"` | Post a summary comment on the MR with collapsible sections per analysis |
| `FALLOW_REVIEW` | `"false"` | Post inline MR discussions from the typed `review-gitlab` envelope v2, with stable fingerprints, suggestions, dedupe, and stale-thread reconciliation |
| `FALLOW_REVIEW_GUIDANCE` | `"false"` | Add collapsed "What to do" guidance blocks to inline review discussions |
| `FALLOW_MAX_COMMENTS` | `"50"` | Maximum number of inline review comments |
| `FALLOW_SUMMARY_SCOPE` | `"all"` | Sticky MR summary scope. Use `all` to include project-level dependency/catalog/override findings even when their anchor line is outside the diff; use `diff` to apply the diff filter to those findings too. Inline review comments are unaffected |
| `FALLOW_DIFF_FILTER` | `"added"` | Filter line-level findings to added diff hunks by default; use `diff_context`, `file`, or `nofilter` to widen review scope |
| `FALLOW_GITLAB_BASE_SHA` / `FALLOW_GITLAB_START_SHA` / `FALLOW_GITLAB_HEAD_SHA` | `""` | Optional overrides for the GitLab MR `diff_refs` used to build inline discussion positions |
| `FALLOW_SCRIPTS_REF` | `""` | Pinned tag or commit for remote MR-integration scripts; leave empty to prefer vendored local `ci/` + `action/` scripts |
| `FALLOW_VERSION` | `""` | Fallow version to install. Empty reads the project's `package.json` `fallow` dependency, then falls back to `latest`; set explicitly to override the local pin |

In MR pipelines, `--changed-since` is set automatically to scope analysis to changed files, and the comment / review scripts derive a unified diff so inline discussions stay on touched lines by default. Fallow edits sticky comments in place and fingerprints inline review comments so repeated runs can skip duplicates. `FALLOW_SUMMARY_SCOPE=diff` keeps the sticky summary focused too: a pre-existing unused dependency in an unrelated package is hidden, while a newly added unused dependency in a changed `package.json` remains visible. If the diff cannot be fetched or read, fallow keeps the existing fail-open behavior and reports all findings.

The v2 review envelope keeps MR threads readable by grouping findings that land on the same path and line into one comment, preserving a machine-readable `marker_regex`, and carrying GitLab `position` data (`old_path`, `new_path`, `new_line`, and diff refs) for reliable inline discussions, including renamed files.

For remote includes, pin the template to a release tag and keep `FALLOW_SCRIPTS_REF` on the same tag or commit. If your GitLab runners cannot reach `raw.githubusercontent.com`, run `npx fallow ci-template gitlab --vendor` locally, commit the generated `ci/` and `action/` files, and use GitLab's local include syntax. The vendored template prefers local scripts and skips the remote fetch path entirely.

A `GITLAB_TOKEN` (PAT or project access token with `api` scope) is required for summary comments and inline MR discussions. GitLab's documented `CI_JOB_TOKEN` permissions allow reading MR notes, but not creating, updating, or deleting them. `CI_JOB_TOKEN` is still useful for GitLab package registry authentication.

GitLab setup gotchas:

- The template sets `GIT_STRATEGY: "fetch"` so shared templates that set `GIT_STRATEGY=none` do not leave fallow without a working tree.
- The template sets `GIT_DEPTH: "0"` so `--changed-since` can diff against the MR base SHA without shallow-clone ambiguity.
- For private GitLab npm registries, create `.npmrc` during the job with `${CI_PROJECT_ID}` and `${CI_JOB_TOKEN}` rather than committing tokens.
- For pnpm projects with `minimumReleaseAge`, add `fallow` and `@fallow-cli/*` to `minimumReleaseAgeExclude` when you need to consume a just-published fallow release immediately.

```yaml
# .gitlab-ci.yml -- full example with rich MR comments
include:
  - remote: 'https://raw.githubusercontent.com/fallow-rs/fallow/vX.Y.Z/ci/gitlab-ci.yml'

fallow:
  extends: .fallow
  variables:
    FALLOW_COMMENT: "true"       # Summary comment with collapsible sections
    FALLOW_SUMMARY_SCOPE: "diff" # Filter project-level findings in the sticky summary too
    FALLOW_REVIEW: "true"        # Inline discussions with suggestion blocks
    FALLOW_REVIEW_GUIDANCE: "true" # Collapsed "What to do" blocks in inline discussions
    FALLOW_MAX_COMMENTS: "30"    # Cap inline comments (default: 50)
    FALLOW_SCRIPTS_REF: "vX.Y.Z" # Match the pinned template ref when using remote scripts
    FALLOW_FAIL_ON_ISSUES: "true"
```

## Configuration

Works out of the box. When you need to customize, create `.fallowrc.json` or run `fallow init`:

```jsonc
// .fallowrc.json
{
  "$schema": "https://raw.githubusercontent.com/fallow-rs/fallow/main/schema.json",
  "entry": ["src/workers/*.ts", "scripts/*.ts"],
  "ignorePatterns": ["**/*.generated.ts"],
  "ignoreDependencies": ["autoprefixer"],
  "ignoreExportsUsedInFile": true,
  "rules": {
    "unused-files": "error",
    "unused-exports": "warn",
    "unused-types": "off"
  },
  "health": {
    "maxCyclomatic": 20,
    "maxCognitive": 15,
    "maxCrap": 30
  },
  "fix": {
    "catalog": {
      "deletePrecedingComments": "auto"
    }
  }
}
```

`fix.catalog.deletePrecedingComments` controls how `fallow fix` handles YAML
comment blocks immediately above removed pnpm catalog entries: `"auto"` deletes
blocks that clearly belong to the entry, `"always"` deletes every contiguous
leading block, and `"never"` preserves them. To protect a specific comment
regardless of policy, mark any line in the block with `# fallow-keep`:

```yaml
catalog:
  # fallow-keep: audit trail, CVE-2024-XXXX
  react: ^18.2.0
```

Section-banner comments (3+ repeated `=`, `-`, `*`, `_`, `~`, `+`, or `#`
characters, e.g. `# === React 18 production pins ===`) are also preserved by
the `"auto"` policy so curated dividers survive cleanup.

Architecture boundary presets enforce import rules between layers with zero manual config:

```jsonc
{ "boundaries": { "preset": "bulletproof" } } // or: layered, hexagonal, feature-sliced
```

For custom feature-module boundaries, `autoDiscover` turns each immediate child
directory into its own zone while rules still reference the logical parent:

```jsonc
{
  "boundaries": {
    "zones": [
      { "name": "app", "patterns": ["src/app/**"] },
      { "name": "features", "patterns": ["src/features/**"], "autoDiscover": ["src/features"] },
      { "name": "shared", "patterns": ["src/shared/**"] }
    ],
    "rules": [
      { "from": "app", "allow": ["features", "shared"] },
      { "from": "features", "allow": ["shared"] }
    ]
  }
}
```

When an `autoDiscover` zone also has `patterns`, discovered child zones are matched first and top-level files fall back to the parent zone. The parent rule automatically allows its discovered children, so `src/features/index.ts` barrels can re-export feature modules while non-barrel top-level files such as `src/features/types.ts` still follow the parent `features` rule. Omit `patterns` when you want only discovered child directories classified.

Run `fallow list --boundaries` to inspect the expanded rules. TOML also supported (`fallow init --toml`). The init command auto-detects your project structure (monorepo layout, frameworks, existing config) and generates a tailored config. It also adds `.fallow/` to your `.gitignore` (cache and local data). Scaffold a pre-commit `fallow audit` hook with `fallow hooks install --target git`; the hook uses the current branch upstream as its base and falls back to `--branch` (or the detected default branch) when no upstream is set. For agent gates, use `fallow hooks install --target agent`. Migrating from knip or jscpd? Run `fallow migrate`.

See the [full configuration reference](https://docs.fallow.tools/configuration/overview) for all options.

## Framework plugins

113 built-in plugins detect entry points, convention exports, config-defined aliases, and template-visible usage for your framework automatically.

| Category | Plugins |
|---|---|
| **Frameworks** | Next.js, Nuxt, Remix, Qwik, SvelteKit, Gatsby, Astro, Angular, NestJS, AdonisJS, Contentlayer, Fumadocs, Lit, Obsidian, Ember, Expo, Expo Router, Electron, and more |
| **Bundlers** | Vite, Webpack, Rspack, Rsbuild, Rollup, Rolldown, Tsup, Tsdown, Parcel |
| **Testing** | Vitest, Jest, Playwright, Cypress, Storybook, Stryker, Mocha, Ava, tap, tsd |
| **CI/CD & Release** | Danger, Commitlint, Commitizen, Semantic Release |
| **CSS** | Tailwind, PostCSS, UnoCSS, PandaCSS |
| **Databases & Backend** | Prisma, Drizzle, Knex, TypeORM, Kysely, Convex |
| **Blockchain** | Hardhat |
| **Monorepos** | Turborepo, Nx, Changesets, Syncpack, pnpm |
| **i18n** | Wuchale, next-intl, i18next |

[Full plugin list](https://docs.fallow.tools/frameworks/built-in) -- missing one? Add a [custom plugin](https://docs.fallow.tools/frameworks/custom-plugins) or [open an issue](https://github.com/fallow-rs/fallow/issues).

## Editor and agent integrations

Fallow is not an AI assistant. It is the deterministic codebase intelligence layer that your assistant, your editor, and your CI pipeline can call.

- **VS Code extension** -- tree views, status bar, one-click fixes, auto-download LSP binary ([Marketplace](https://github.com/fallow-rs/fallow/tree/main/editors/vscode))
- **LSP server** -- real-time diagnostics, hover info, code actions, Code Lens with reference counts
- **Agent Skill + MCP server** -- version-matched AI agent guidance ships in the npm package, with MCP integration for Claude Code, Codex, Cursor, Windsurf, and other agents ([fallow-skills](https://github.com/fallow-rs/fallow-skills))
- **JSON `actions` array** -- every issue in `--format json` output includes fix suggestions with `auto_fixable` flag, so agents can self-correct
- **Typed output contract** -- `import type { CheckOutput } from "fallow/types"` version-pinned to your installed CLI

## Performance

Benchmarked on real open-source projects (median of 5 runs with 2 warmups, Apple M5).

### Dead code: fallow vs knip

| Project | Files | fallow | knip v5 | knip v6 | vs v5 | vs v6 |
|:--------|------:|-------:|--------:|--------:|------:|------:|
| [zod](https://github.com/colinhacks/zod) | 174 | **25ms** | 650ms | 330ms | 26x | 13x |
| [fastify](https://github.com/fastify/fastify) | 286 | **27ms** | 933ms | 222ms | 34x | 8x |
| [preact](https://github.com/preactjs/preact) | 244 | **200ms** | 911ms | 2.15s | 5x | 11x |
| [vue/core](https://github.com/vuejs/core) | 522 | **68ms** | ---* | ---* | --- | --- |
| [TanStack/query](https://github.com/TanStack/query) | 901 | **330ms** | 2.66s | 1.08s | 8x | 3.3x |
| [vite](https://github.com/vitejs/vite) | 1,420 | **378ms** | ---* | ---* | --- | --- |
| [svelte](https://github.com/sveltejs/svelte) | 3,337 | **363ms** | 1.95s | 714ms | 5x | 2x |
| [next.js](https://github.com/vercel/next.js) | 20,416 | **1.72s** | ---* | ---* | --- | --- |

On the current benchmark fixtures, knip does not produce valid JSON results for vite, vue/core, and next.js. fallow completes on all three. See the [full comparison page](https://docs.fallow.tools/migration/comparison) for the complete matrix and current caveats. * knip exits without valid results on those fixtures.

### Duplication: fallow vs jscpd

| Project | Files | fallow | jscpd | Speedup |
|:--------|------:|-------:|------:|--------:|
| [fastify](https://github.com/fastify/fastify) | 286 | **76ms** | 1.96s | 26x |
| [vue/core](https://github.com/vuejs/core) | 522 | **124ms** | 3.11s | 25x |
| [next.js](https://github.com/vercel/next.js) | 20,416 | **2.89s** | 24.37s | 8x |

No TypeScript compiler, no Node.js runtime needed to analyze your code. [Fallow vs linters](https://docs.fallow.tools/explanations/fallow-vs-linters) | [Reproduce benchmarks](https://github.com/fallow-rs/fallow/tree/main/benchmarks)

## Suppressing findings

```ts
// fallow-ignore-next-line unused-export
export const keepThis = 1;

// fallow-ignore-next-line unused-export, complexity
export const publicComplexHelper = (value: number) => value;

// fallow-ignore-file
// Suppress all issues in this file
```

Use a comma-separated issue-kind list when one line has multiple findings.

Also supports JSDoc visibility tags (`/** @public */`, `/** @internal */`, `/** @beta */`, `/** @alpha */`) to suppress unused export reports for library APIs consumed externally.

Set `ignoreExportsUsedInFile: true` when exported helpers should stay quiet while another symbol in the same file still references them, but should be reported once they become completely unreferenced. The `{ "type": true, "interface": true }` object form is accepted for knip parity; fallow groups type aliases and interfaces under one issue, so both type-kind fields behave identically. References inside the export specifier itself (`export { foo }`, `export default foo`) do not count as same-file uses.

## Limitations

fallow uses syntactic analysis -- no type information. This is what makes it fast and deterministic, but findings that require a type-checker (cross-module type narrowing, conditional types, type-level reachability) are out of scope. Use [inline suppression comments](#suppressing-findings) or [`ignoreExports`](https://docs.fallow.tools/configuration/overview#ignoring-specific-exports) for edge cases.

## Documentation

- [Getting started](https://docs.fallow.tools)
- [Configuration reference](https://docs.fallow.tools/configuration/overview)
- [CI integration guide](https://docs.fallow.tools/integrations/ci)
- [Migrating from knip](https://docs.fallow.tools/migration/from-knip)
- [Fallow compliance happy path](https://github.com/fallow-rs/fallow/blob/main/docs/fallow-compliance.md)
- [Plugin authoring guide](https://github.com/fallow-rs/fallow/blob/main/docs/plugin-authoring.md)

## Contributing

Missing a framework plugin? Found a false positive? [Open an issue](https://github.com/fallow-rs/fallow/issues).

```bash
cargo build --workspace && cargo test --workspace
```

## License

MIT
