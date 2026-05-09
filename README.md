<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo-dark.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo.svg">
    <img src="https://raw.githubusercontent.com/fallow-rs/fallow/main/assets/logo.svg" alt="fallow" width="290">
  </picture>
</p>

<p align="center">
  <strong>Codebase intelligence for TypeScript & JavaScript.</strong><br>
  Free static analysis for unused code, duplication, complexity, and architecture drift.<br>
  Optional runtime intelligence for hot paths, cold paths, and runtime-backed code decisions.<br>
  <strong>Built for AI-assisted development. No AI inside.</strong><br>
  <strong>Rust-native. Zero config. Sub-second.</strong>
</p>

<p align="center">
  <a href="https://github.com/fallow-rs/fallow/actions/workflows/ci.yml"><img src="https://github.com/fallow-rs/fallow/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/fallow-rs/fallow/actions/workflows/coverage.yml"><img src="https://img.shields.io/endpoint?url=https://raw.githubusercontent.com/fallow-rs/fallow/badges/coverage.json" alt="Coverage"></a>
  <a href="https://crates.io/crates/fallow-cli"><img src="https://img.shields.io/crates/v/fallow-cli.svg" alt="crates.io"></a>
  <a href="https://www.npmjs.com/package/fallow"><img src="https://img.shields.io/npm/v/fallow.svg" alt="npm"></a>
  <a href="https://github.com/fallow-rs/fallow/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="MIT License"></a>
  <a href="https://docs.fallow.tools"><img src="https://img.shields.io/badge/docs-docs.fallow.tools-blue.svg" alt="Documentation"></a>
</p>

---

```bash
npx fallow --summary
```

```
Dead Code Summary

      12  Unused files
      47  Unused exports
       8  Unused types
       3  Unused dependencies
       2  Circular dependencies

      72  Total

Duplication Summary

      18  Clone families
      53  Clone groups
   2,140  Duplicated lines
    4.2%  Duplication rate

Health Summary

     612  Functions analyzed
       9  Above threshold
    89.4  Average maintainability (good)
```

**Static analysis is free and open source. Runtime intelligence is optional.**

94 framework plugins. No Node.js runtime required for static analysis. No config needed for the first run.

Fallow builds a project-wide understanding of your TS/JS codebase instead of checking one file at a time. Use it to review AI-generated changes faster, clean up dead code, reduce duplication, find risky complexity, and enforce architecture boundaries. Add the runtime layer when you want to know what actually executed in production.

**Fallow is the codebase truth layer your coding agent can call. It is not an AI assistant.**

## Install

```bash
npm install --save-dev fallow   # or: pnpm add -D fallow / yarn add -D fallow / bun add -d fallow
```

Installs the CLI, LSP server, MCP server, and version-matched Agent Skill into `node_modules`. For one-off CLI use, run `npx fallow`; Rust users can also run `cargo install fallow-cli`.

Programmatic Node API:

```bash
npm install @fallow-cli/fallow-node   # or: pnpm/yarn/bun add @fallow-cli/fallow-node
```

```ts
import { detectDeadCode, detectDuplication, computeHealth } from '@fallow-cli/fallow-node';

const deadCode = await detectDeadCode({ root: process.cwd() });
const dupes = await detectDuplication({ root: process.cwd(), mode: 'mild', minTokens: 30 });
const health = await computeHealth({ root: process.cwd(), score: true, ownershipEmails: 'handle' });
```

## Start here

```bash
fallow                      # Dead code + duplication + health
fallow dead-code            # Cleanup candidates
fallow dupes                # Repeated logic
fallow health               # Complexity + refactor targets
fallow fix --dry-run        # Preview automatic cleanup
```

## What it finds

- **Dead code**: unused files, exports, dependencies, types, cycles, boundaries, stale suppressions, GraphQL documents linked by `#import`, plus opt-in API hygiene checks such as private type leaks
- **Duplication**: repeated blocks from exact to semantic clones
- **Complexity**: high-risk functions, file scores, hotspots, and refactor targets
- **Architecture drift**: boundary violations across layers and modules

## Why Fallow exists

Linters check files. TypeScript checks types. Fallow checks the codebase.

It builds a module graph across the whole project so it can find problems that file-local tools cannot:

| What | Linter | Fallow |
|---|---|---|
| Unused variable in a function | yes | no |
| Unused export that nothing imports | no | yes |
| File that nothing imports | no | yes |
| Circular dependency across modules | no | yes |
| Duplicate code blocks across files | no | yes |
| Dependency in package.json never imported | no | yes |

[Full comparison: fallow vs ESLint, Biome, knip, ts-prune](https://docs.fallow.tools/explanations/fallow-vs-linters)

## Why teams using AI need Fallow

AI accelerates code creation. It does not eliminate review, cleanup, or architecture drift.

When Claude Code, Codex, Cursor, or other tools generate changes, teams still need to know:

- did this introduce dead code?
- did it duplicate logic that already existed?
- did complexity get worse?
- did the change cross a boundary it should not cross?
- is this code on a hot path or a cold one?
- what should the reviewer read closely first?

Fallow answers those questions with deterministic, graph-based analysis and structured output, so both humans and agents can act on facts instead of guesses.

## How agents use Fallow

Agents do not need to guess from limited context. They can call Fallow directly via the CLI or MCP.

Common agent workflow:

1. generate or edit code
2. run `fallow --format json`
3. inspect dead code, duplication, health findings, and per-issue `actions`
4. apply safe fixes or adjust the patch before opening a PR
5. hand the result to a human reviewer with better evidence

```bash
npx fallow --format json
npx fallow audit --format json
npx fallow fix --dry-run --format json
```

For full adoption instead of one-off review, see the [Fallow compliance happy path](https://github.com/fallow-rs/fallow/blob/main/docs/fallow-compliance.md). It defines the end state clearly: repo-wide dead code and duplication findings are fixed or explicitly documented, `fallow health` reaches `Above threshold: 0` for the repo's chosen thresholds, and `fallow audit` becomes the change-set gate once adoption is wired in. It also includes a copy-paste agent onboarding prompt.

See [Agent integration](https://docs.fallow.tools/integrations/mcp) for MCP setup and the full list of structured tools.

## More static commands

```bash
fallow audit                 # Audit changed files (verdict: pass/warn/fail)
fallow explain unused-export # Explain a rule without analyzing
fallow --production-health   # Combined mode: production health, full dead-code/dupes
fallow watch                 # Re-analyze on file changes
fallow fix                   # Apply automatic cleanup after previewing
```

Combined mode and `fallow audit` support per-analysis production mode. Precedence is CLI flags, then environment variables, then config:

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

## Dead code

Finds unused files, exports, dependencies, types, enum members, class members, unresolved imports, unlisted dependencies, duplicate exports, circular dependencies (including cross-package cycles in monorepos), boundary violations, type-only dependencies, test-only production dependencies, and stale suppression comments. Workspace package dependencies are checked like external packages, so unused or undeclared internal package edges are visible in monorepos. Entry points are auto-detected from package.json fields, framework conventions, and plugin patterns. Arrow-wrapped dynamic imports (`React.lazy`, `loadable`, `defineAsyncComponent`) are tracked as references. Script multiplexers (`concurrently`, `npm-run-all`) are analyzed to discover transitive script dependencies. JSDoc tags (`@public`, `@internal`, `@beta`, `@alpha`, `@expected-unused`) control export visibility. Private type leaks are currently opt-in API hygiene findings via `--private-type-leaks` or the `private-type-leaks` rule.

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

Finds copy-pasted code blocks across your codebase. Suffix-array algorithm -- no quadratic pairwise comparison.

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
- The sidecar can be installed globally or as a project devDependency; fallow resolves `FALLOW_COV_BIN`, project-local shims, package-manager bin lookups, `~/.fallow/bin/fallow-cov`, and `PATH`
- `fallow health --runtime-coverage <path>` accepts a V8 directory, a single V8 JSON file, or a single Istanbul coverage map JSON file (commonly `coverage-final.json`)
- `fallow health --coverage <path>` accepts a single Istanbul coverage map JSON file or a directory containing `coverage-final.json`
- `--coverage-root <path>` must be an absolute prefix from the Istanbul file paths. Use it when coverage was generated in CI or Docker with a different checkout root, for example `fallow health --coverage artifacts/coverage-final.json --coverage-root /home/runner/work/myapp`
- V8 dumps that include Node's `source-map-cache` are remapped through supported source-map paths before analysis, including file paths, relative paths, `webpack://...`, and `vite://...`; unsupported virtual schemes safely fall back to raw V8 handling
- `fallow health --changed-since <ref> --runtime-coverage <path>` promotes touched hot paths to a `hot-path-touched` verdict during change review

Runtime coverage is merged into the same human, JSON, SARIF, compact, markdown, and CodeClimate outputs as the rest of the health report.

Read more: [Static vs runtime intelligence](https://docs.fallow.tools/explanations/static-vs-runtime) | [Runtime coverage](https://docs.fallow.tools/analysis/runtime-coverage)

## Audit

Quality gate for AI-generated code and PRs. Combines dead code + complexity + duplication scoped to changed files.

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

```yaml
# GitHub Action
- uses: fallow-rs/fallow@v2
  with:
    command: audit
    max-crap: 30
    coverage: artifacts/coverage-final.json
    coverage-root: /home/runner/work/myapp

# GitLab CI -- remote include
include:
  - remote: 'https://raw.githubusercontent.com/fallow-rs/fallow/vX.Y.Z/ci/gitlab-ci.yml'
fallow:
  extends: .fallow
  variables:
    FALLOW_COMMAND: "audit"
    FALLOW_MAX_CRAP: "30"
    FALLOW_COVERAGE: "artifacts/coverage-final.json"
    FALLOW_COVERAGE_ROOT: "/home/runner/work/myapp"
```

```yaml
# GitLab CI -- vendored include when runners cannot reach GitHub raw
# Run once locally: npx fallow ci-template gitlab --vendor
# Commit the generated ci/ + action/ files.
include:
  - local: 'ci/gitlab-ci.yml'

fallow:
  extends: .fallow
```

```yaml
# Any CI
script:
  - npx fallow --ci
```

`--ci` enables SARIF output, quiet mode, and non-zero exit on issues. Also supports:

- `--group-by owner|directory|package|section` -- group output by CODEOWNERS ownership, directory, workspace package, or GitLab CODEOWNERS `[Section]` headers for team-level triage
- `--summary` -- show only category counts (no individual issues)
- `--changed-since main` -- analyze only files touched in a PR
- `--changed-workspaces origin/main` -- scope monorepo analysis to workspaces containing any changed file (CI primitive; fails hard on git errors so CI never silently widens back to the full repo)
- `--baseline` / `--save-baseline` -- fail only on **new** issues
- `--fail-on-regression` / `--tolerance 2%` -- fail only if issues **grew** beyond tolerance
- `--format sarif` -- upload to GitHub Code Scanning
- `--format codeclimate` / `--format gitlab-codequality` -- GitLab Code Quality inline MR annotations
- `--format pr-comment-github` / `--format pr-comment-gitlab` -- typed sticky PR/MR comment markdown
- `--format review-github` / `--format review-gitlab` -- typed inline review envelopes for CI scripts
- `--format annotations` -- GitHub Actions inline PR annotations (no Action required)
- `--format json` / `--format markdown` -- for custom workflows (JSON includes machine-actionable `actions` per issue)
- `--format badge` -- shields.io-compatible SVG health badge (`fallow health --format badge > badge.svg`)

Both the GitHub Action and GitLab CI template auto-detect your package manager (npm/pnpm/yarn) from lock files, so install/uninstall commands in review comments match your project.

SARIF upload requires GitHub Code Scanning, which is available on public repositories and on private repositories with GitHub Advanced Security enabled. If it is unavailable, the action skips upload with a warning and leaves the job summary and primary output intact.

GitHub inline review comments target the current PR file state (`side: RIGHT`). Findings on deleted lines are not modeled yet; fallow's diagnostics are current-state oriented in normal use.

Adopt incrementally -- surface issues without blocking CI, then promote when ready:

```jsonc
{ "rules": { "unused-files": "error", "unused-exports": "warn", "circular-dependencies": "off" } }
```

### GitLab CI rich MR comments

The GitLab CI template can post rich comments directly on merge requests -- summary comments with collapsible sections and inline review discussions with suggestion blocks.

| Variable | Default | Description |
|---|---|---|
| `FALLOW_COMMENT` | `"false"` | Post a summary comment on the MR with collapsible sections per analysis |
| `FALLOW_REVIEW` | `"false"` | Post inline MR discussions at the relevant lines, with `suggestion` blocks for unused exports and unused imports |
| `FALLOW_MAX_COMMENTS` | `"50"` | Maximum number of inline review comments |
| `FALLOW_SCRIPTS_REF` | `""` | Pinned tag or commit for remote MR-integration scripts; leave empty to prefer vendored local `ci/` + `action/` scripts |
| `FALLOW_VERSION` | `""` | Fallow version to install. Empty reads the project's `package.json` `fallow` dependency, then falls back to `latest`; set explicitly to override the local pin |

In MR pipelines, `--changed-since` is set automatically to scope analysis to changed files. Fallow edits sticky comments in place and fingerprints inline review comments so repeated runs can skip duplicates.

The comment merging pipeline groups unused exports per file and deduplicates clone reports, keeping MR threads readable.

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
    FALLOW_REVIEW: "true"        # Inline discussions with suggestion blocks
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
  }
}
```

Architecture boundary presets enforce import rules between layers with zero manual config:

```jsonc
{ "boundaries": { "preset": "bulletproof" } } // or: layered, hexagonal, feature-sliced
```

Run `fallow list --boundaries` to inspect the expanded rules. TOML also supported (`fallow init --toml`). The init command auto-detects your project structure (monorepo layout, frameworks, existing config) and generates a tailored config. It also adds `.fallow/` to your `.gitignore` (cache and local data). Scaffold a pre-commit `fallow audit` hook with `fallow hooks install --target git`; the hook uses the current branch upstream as its base and falls back to `--branch` (or the detected default branch) when no upstream is set. For agent gates, use `fallow hooks install --target agent`. Migrating from knip or jscpd? Run `fallow migrate`.

See the [full configuration reference](https://docs.fallow.tools/configuration/overview) for all options.

## Framework plugins

94 built-in plugins detect entry points, convention exports, config-defined aliases, and template-visible usage for your framework automatically.

| Category | Plugins |
|---|---|
| **Frameworks** | Next.js, Nuxt, Remix, Qwik, SvelteKit, Gatsby, Astro, Angular, NestJS, Lit, Expo, Expo Router, Electron, and more |
| **Bundlers** | Vite, Webpack, Rspack, Rsbuild, Rollup, Rolldown, Tsup, Tsdown, Parcel |
| **Testing** | Vitest, Jest, Playwright, Cypress, Storybook, Mocha, Ava, tap, tsd |
| **CSS** | Tailwind, PostCSS, UnoCSS, PandaCSS |
| **Databases & Backend** | Prisma, Drizzle, Knex, TypeORM, Kysely, Convex |
| **Blockchain** | Hardhat |
| **Monorepos** | Turborepo, Nx, Changesets, Syncpack, pnpm |

[Full plugin list](https://docs.fallow.tools/frameworks/built-in) -- missing one? Add a [custom plugin](https://docs.fallow.tools/frameworks/custom-plugins) or [open an issue](https://github.com/fallow-rs/fallow/issues).

## Editor & AI support

Fallow is not an AI assistant. It is the codebase truth layer your assistant can call.

- **VS Code extension** -- tree views, status bar, one-click fixes, auto-download LSP binary ([Marketplace](https://github.com/fallow-rs/fallow/tree/main/editors/vscode))
- **LSP server** -- real-time diagnostics, hover info, code actions, Code Lens with reference counts
- **Agent Skill + MCP server** -- version-matched AI agent guidance ships in the npm package, with MCP integration for Claude Code, Codex, Cursor, Windsurf, and other agents ([fallow-skills](https://github.com/fallow-rs/fallow-skills))
- **JSON `actions` array** -- every issue in `--format json` output includes fix suggestions with `auto_fixable` flag, so agents can self-correct

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

// fallow-ignore-file
// Suppress all issues in this file
```

Also supports JSDoc visibility tags (`/** @public */`, `/** @internal */`, `/** @beta */`, `/** @alpha */`) to suppress unused export reports for library APIs consumed externally.

Set `ignoreExportsUsedInFile: true` when exported helpers should stay quiet while another symbol in the same file still references them, but should be reported once they become completely unreferenced. The `{ "type": true, "interface": true }` object form is accepted for knip parity; fallow groups type aliases and interfaces under one issue, so both type-kind fields behave identically. References inside the export specifier itself (`export { foo }`, `export default foo`) do not count as same-file uses.

## Limitations

fallow uses syntactic analysis -- no type information. This is what makes it fast, but type-level dead code is out of scope. Use [inline suppression comments](#suppressing-findings) or [`ignoreExports`](https://docs.fallow.tools/configuration/overview#ignoring-specific-exports) for edge cases.

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
