# Fallow domain language

Shared glossary for everyone working on fallow. Keeps the language consistent across code, docs, issues, commits, and agent prompts. Update this file whenever a new term enters the vocabulary or an existing term gets reused for a second concept.

Pattern follows [mattpocock/skills/CONTEXT.md](https://github.com/mattpocock/skills/blob/main/CONTEXT.md): term, definition, what to avoid, then relationships and flagged ambiguities.

## Analysis vocabulary

**Issue**
A detected code-quality finding produced by an analysis pass. Has an issue type, severity, location, and optional suppress action. Examples: an unused export, a circular dependency, a boundary violation, a clone family.
_Avoid_: "finding" (use only inside the schema where it is the literal field name), "problem", "warning" (severity is a separate concept). Never use "issue" to mean a GitHub issue without qualifying as "GitHub issue".

**Issue type**
The canonical category an Issue belongs to. Examples: `unused-files`, `unused-exports`, `unused-deps`, `unused-types`, `unused-enum-members`, `unused-class-members`, `unresolved-imports`, `unlisted-deps`, `duplicate-export`, `circular-dependency`, `boundary-violation`, `complexity-hotspot`. Used for severity rules, suppression directives, and filtering.
_Avoid_: "category", "kind", "rule" (rule is a config concept).

**Rule**
A config-level setting that maps an Issue type to a severity. Severities are `error` (fail CI, default), `warn` (exit 0), or `off` (skip). Lives under the `rules` block in config files.
_Avoid_: "check" as a synonym (check is also a command name).

**Severity**
The CI-impact level of an Issue type, set per rule. One of `error`, `warn`, `off`. Determines whether the issue contributes to a non-zero exit code, only renders in output, or is suppressed entirely.
_Avoid_: "level" (overlaps with logging level).

**Rule pack**
A standalone declarative JSON/JSONC file of project policy rules (`banned-call`, `banned-import`), listed via the `rulePacks` config key and loaded as pure data (no project code executes). Matches report as `policy-violation` issues identified by `<pack>/<rule-id>`. Distinct from **Rule** (the config severity mapping): a rule pack CONTAINS pack rules; the `rules."policy-violation"` entry is the Rule that sets the issue type's master severity.
_Avoid_: "custom rules" (collides with the `ConfigShape::CustomRules` telemetry concept, which means "the user changed the `rules` severity map"), "policy file" (underspecified), "plugin" (plugins detect frameworks; packs encode policy).

**Pack rule**
One entry in a Rule pack's `rules` array: an id, a kind (`banned-call` / `banned-import`), matchers, optional `files`/`exclude` scoping, an optional message, and an optional per-rule severity that overrides the `policy-violation` master per finding.
_Avoid_: bare "rule" when a config-severity Rule could be meant; qualify as "pack rule".

**Suppress**
The mechanism to silence an Issue at a specific location. Two forms: `// fallow-ignore-next-line <issue-type>` (single line) and `// fallow-ignore-file <issue-type>` (whole file). Distinct from `rules` config which is project-wide.
_Avoid_: "ignore", "skip", "exclude" (those have other meanings in config schemas).

## Analysis areas

The five top-level analysis areas the bare `fallow` command produces. Fallow is a codebase analyzer, not a dead-code tool; references to fallow MUST cite multiple areas.

**Unused code**
Detection of files, exports, dependencies, types, enum members, and class members that are reachable in source but never referenced. Includes unresolved imports and unlisted dependency detection.

**Circular dependencies**
Cycles in the module import graph. Reported per cycle with the participating files.

**Duplication**
Clone detection across the codebase. Groups duplicated regions into clone Families.

**Complexity hotspots**
Functions or modules above complexity thresholds. Contributes to health risk profiles.

**Boundary violations**
Imports that cross declared architectural boundaries. Configured per-project via the boundaries config.

## Commands

Each subcommand owns a piece of the analysis pipeline. Bare `fallow` runs the full set; named subcommands narrow the scope.

**`fallow`** (bare)
Runs the full pipeline: unused code + duplication + health (which includes complexity hotspots and circular dependencies). The marketing-default invocation.
_Avoid_: calling this "fallow check" or "fallow dead-code"; both are scoped subcommands.

**`fallow dead-code`** (alias: `fallow check`)
Dead-code-only analysis. Faster than bare `fallow`. Does not run dupes or health. Canonical name is `dead-code`; `check` is retained as a backwards-compatible alias from before the 2026-03-26 rename (commit 75c9884e0e3).
_Avoid_: introducing `check` in new code, docs, or skills; prefer `dead-code` everywhere. Update legacy `check` mentions opportunistically. Never use either name to mean the full pipeline.

**`fallow dupes`**
Duplication detection only.

**`fallow health`**
Health-metric pass: complexity hotspots, ownership risk, risk profiles, coupling concentration. Runs only health, not dead-code.

**`fallow audit`**
CI hot-path command. Optimized for fast pre-merge gating. Supports `--gate`, `--diff-file`, `--hot-path-touched`, and produces a `signals[]` array consumable by GitHub Action and GitLab CI integrations. Highest perf-priority command.

**`fallow fix`**
Auto-fix pass with `--dry-run` preview. Applies catalog-driven fix actions to a subset of issue types.

**`fallow watch`**
Watch mode. Re-runs analysis on file changes.

**`fallow coverage`**
Runtime coverage subcommand family (Fallow Runtime; continuous and cloud use is licensed, a single local capture is free). Subcommands: `coverage setup`, `coverage analyze`, `coverage upload-inventory`, `coverage upload-source-maps`, `coverage upload-static-findings`. Combines static analysis with V8 runtime coverage.

**`fallow explain`**
Per-issue explanation. Tells the user why an Issue was raised and what the fix options are.

**Regression gating** (`--fail-on-regression`, `--tolerance`, `--baseline` / `--save-baseline`)
Regression-state evaluation against a baseline. Flags on the analysis commands, not a subcommand.

**`fallow license`**
License lifecycle: `license activate`, `license status`, `license refresh`, `license deactivate`. Offline Ed25519 JWT verification.

**`fallow init`**, **`fallow schema`**, **`fallow migrate`**
Config bootstrap, capability-manifest emission, and migration from knip / jscpd / stylelint configs.

## Plugin and workspace concepts

**Plugin**
A fallow framework-detection plugin. Knows about a specific framework or build tool (Next.js, Vite, Astro, Storybook, Vitest, etc.) and contributes entry points, file patterns, suppress rules, and dependency knowledge. Lives in `crates/core/src/plugins/` and `npm/fallow/skills/` per plugin discovery rules.
_Avoid_: "fallow plugin" when referring to a Claude Code plugin or an npm/cargo plugin; qualify the context every time.

**Workspace**
A pnpm/npm/yarn monorepo workspace. Fallow discovers workspaces via package.json globs and analyzes each workspace package's entry points and boundaries.
_Avoid_: using "workspace" for the broader analysis project; the project as a whole is the project root.

**Entry point**
A file fallow treats as a graph root: not unused even if nothing else imports it. Determined by config (`entry`), plugins (framework conventions), and package.json (`bin`, `main`, `module`, `exports`).

**Re-export chain**
A barrel-file propagation path. When `b.ts` re-exports `a.ts` and `c.ts` re-exports `b.ts`, fallow resolves the chain iteratively (ADR-005) so that usage of the symbol in `c.ts` counts against the original definition in `a.ts`.
_Avoid_: "barrel" alone (barrel is the file form; chain is the resolution behavior).

**Boundary**
A declared architectural rule: source pattern X may not import target pattern Y. Configured under `boundaries` in config.

## Health and metrics

**Hotspot**
A complexity hotspot: function or module above a complexity threshold. Surfaced via `fallow health --hotspots`.

**CRAP**
Change Risk Anti-Patterns score. Combines cyclomatic complexity with coverage to flag risky-untested code. Fallow ships three CRAP tiers depending on what coverage signal is available.

**Tier (CRAP)**
The data quality of the CRAP score: `estimated` (no coverage data, complexity only), `Istanbul` (Istanbul/c8 coverage JSON), or `binary` (fallow runtime coverage from the sidecar). Higher tier = stronger signal.
_Avoid_: using "tier" for license plans without qualifying as "license tier".

**Risk profile**
A composed health view per-module: complexity, ownership risk (bus factor, drift, declared owner), coupling concentration, duplication penalty, large-function drill-down. Surfaced via `fallow health` with risk-profile flags.

**Ownership risk**
Bus factor + author drift + declared owner mismatch + unowned hotspots. Surfaced via `fallow health --hotspots --ownership`. Based on git blame, not LSP semantics.

**Signals**
The `signals[]` array in `fallow audit` JSON output. Each signal is one piece of evidence (verdict, line overlap, hot-path-touched, etc.) used by the audit consumer (action, CI, MR comment).

**Hot path / Hot-path-touched**
A hot path is an entry-point-reachable file or function. `hot-path-touched` is the audit verdict that fires when a PR's diff overlaps a hot path.

## Paid runtime layer

The closed-source feature set built on top of the open-source static analysis.

**Sidecar**
The closed-source binary that captures V8 runtime coverage and emits protocol-compliant payloads. Distributed via the `fallow-cov` npm package. First clean release was `sidecar-v0.1.5`; v0.1.0 to v0.1.4 are tombstones.

**Protocol**
The `fallow-cov-protocol` crate. Open-source schema that the sidecar and fallow agree on for runtime coverage payloads. Lives in a separate repo, published to crates.io.
_Avoid_: "protocol" without qualifying when the conversation drifts to other RPC concepts.

**Trial**
A time-limited license that unlocks paid features. Lives behind the `fallow license` command. Trials feed the grace ladder.

**Grace ladder**
The license verification fallback chain: 7 days soft warning, 30 days hard warning, hard-fail after. Applies to expired or unreachable license states.

**License tier**
The plan attached to a license: free, paid, trial. Distinct from CRAP tier.

## Output formats

Fallow emits the same Issue set in multiple formats, selected via `--format`. Format is CLI-only, not in config.

- **human**: default colored terminal output
- **json**: machine-readable, schema-versioned
- **sarif**: GitHub code-scanning compatible
- **compact**: one line per issue, grep-friendly
- **markdown**: PR-comment friendly
- **badge**: status badge generation
- **codeclimate**: GitLab Code Quality compatible

## Suppress and config

**Config file**
First-match-wins discovery: `.fallowrc.json` > `.fallowrc.jsonc` > `fallow.toml` > `.fallow.toml`. No merging. The first one found wins.
_Avoid_: ESLint terminology like `root: true`; fallow's model is different.

**Inline suppress**
See "Suppress" under Analysis vocabulary above.

## Repo and dev

**Companion repo**
A peer repository to `fallow-rs/fallow` that ships a related artifact: `fallow-docs` (Mintlify docs), `fallow-skills` (the public `/fallow` Claude Code skill), `fallow-cov-protocol` (open-source protocol crate), `fallow-tools` (marketing site), `fallow-cloud` (private cloud monorepo).

**Worktree**
A parallel git worktree of fallow main. The bare-config'd `~/Sites/fallow/` is the primary checkout; `~/Sites/fallow-2/` is a sister worktree. Multiple agents and background sessions often run across these in parallel.

**Bare repo**
The bare-config'd `~/Sites/fallow/` checkout where commits originate. Worktrees and parallel sessions must not push from their own working dirs without an ephemeral worktree (see `CLAUDE.md` worktree rules).

**ADR**
Architecture Decision Record. Kept in a private repo (symlinked locally at `decisions/<NNN>-<slug>.md`). Documents load-bearing structural choices. ADRs are forward-only; superseded ADRs link to their replacement.

**.plans/**
Gitignored directory for in-flight feature plans. `/fallow-implement` writes its Phase 1 plan to `.plans/<feature-slug>.md`; downstream skills (`/panel-review`, the implementation phase, `/fallow-review`) re-read the file rather than relying on context overflow. Plans are transient; promoted concepts go to ADRs.

## Relationships

- A **Project** has many **Workspaces** and many **Plugins**.
- A **Workspace** has many **Entry points**.
- A **Plugin** contributes **Entry points** and **Suppress** patterns to a Workspace.
- An **Issue** has one **Issue type**, one **Severity**, one or more locations, and zero or one **Suppress actions**.
- A **Rule** maps an **Issue type** to a **Severity** for one Project.
- A **Rule pack** contains many **Pack rules**; each match produces one `policy-violation` **Issue** carrying its effective per-rule **Severity**.
- A **Family** groups duplicated regions across one or more **Workspaces**.
- A **Hotspot** belongs to one **Workspace** and contributes to the **Risk profile** of that workspace's module.
- A **CRAP score** depends on **Complexity** (always available) plus an optional **Coverage tier** (estimated / Istanbul / binary).
- **Signals** belong to one `fallow audit` run and are emitted to one audit consumer (action, CI, MR comment).
- **Sidecar** ships **Coverage payloads** that conform to the **Protocol** schema, gated by a **License** with a **Tier** and a **Grace ladder** state.

## Flagged ambiguities

Terms that have collided historically and need explicit qualification on every use.

- **Issue**: detected code finding (this glossary) vs GitHub issue. Always qualify as "GitHub issue" for the second meaning.
- **Plugin**: fallow framework-detection plugin vs Claude Code plugin vs generic npm/cargo plugin. Default in this codebase is fallow plugin; qualify otherwise.
- **Coverage**: runtime coverage (paid sidecar feature) vs `fallow coverage` command surface vs test coverage. Use the qualified form when discussing across two of these.
- **Workspace**: pnpm/npm/yarn monorepo workspace vs Claude Code workspace vs "the project". Qualify when the context could be read either way.
- **Tier**: CRAP coverage tier (estimated / Istanbul / binary) vs license tier (free / paid / trial). Always qualify.
- **Check**: legacy alias for `fallow dead-code` (kept for backwards compatibility) vs general code check. New code, docs, and skills should use `dead-code`.
- **Health**: `fallow health` command vs general code health. Same as check.
- **Family**: duplicate clone family (this glossary) vs plugin family (does not exist as a fallow concept; treat as a typo if encountered).
- **Audit**: `fallow audit` command vs general code audit (e.g. SIG audit, security audit). Qualify by command name when ambiguous.
- **Protocol**: `fallow-cov-protocol` crate vs general RPC protocol terminology. Default to the crate in fallow contexts.
- **Bare**: `bare fallow` (no subcommand, full pipeline) vs bare git repo (the `~/Sites/fallow/` checkout). Distinct concepts that both appear in dev conversations.
- **Rule**: config severity mapping (`rules` block) vs Pack rule (an entry in a Rule pack's `rules` array). Three `rules` surfaces exist: the config map, the pack file array, and the rule-id in `<pack>/<rule-id>`. Qualify as "pack rule" when not referring to the severity map.

## Resolved ambiguities

Terms that used to be ambiguous and have been resolved. Listed so the resolution survives.

- "Dead code tool" as a category for fallow has been retired. Fallow is a **codebase analyzer**; never reduce to dead-code-only language in public communication.
- "Backlog" is not a fallow term. Issues live in GitHub Issues at `fallow-rs/fallow`; do not call that "the backlog".
- "Finding" survives only as the literal field name in JSON output; in prose, use "Issue".
