# Fallow CLI Reference

The command catalogue of the fallow CLI. Each command and each shared topic has its own file under `cli/`, with the complete flag specifications. Read only the file of the command that you need.

## Reference files

| File | Covers |
|------|--------|
| [`cli/dead-code.md`](cli/dead-code.md) | `dead-code`: Dead Code Analysis |
| [`cli/architecture.md`](cli/architecture.md) | `architecture`: Cycles, Boundaries and Policy Rules |
| [`cli/dupes.md`](cli/dupes.md) | `dupes`: Duplication Detection |
| [`cli/fix.md`](cli/fix.md) | `fix`: Auto-Remove Unused Code |
| [`cli/list.md`](cli/list.md) | `list`: Project Introspection |
| [`cli/init.md`](cli/init.md) | `init`: Config Generation |
| [`cli/hooks.md`](cli/hooks.md) | `hooks`: Managed Hook Status And Installation |
| [`cli/agent.md`](cli/agent.md) | `agent`: One-Pass Agent Onboarding |
| [`cli/migrate.md`](cli/migrate.md) | `migrate`: Config Migration |
| [`cli/health.md`](cli/health.md) | `health`: Function Complexity and File Health Analysis |
| [`cli/audit.md`](cli/audit.md) | `audit`: Changed-File Quality Gate |
| [`cli/flags.md`](cli/flags.md) | `flags`: Feature Flag Detection |
| [`cli/security.md`](cli/security.md) | `security`: Security Candidate Detection |
| [`cli/inspect.md`](cli/inspect.md) | `inspect`: Target Evidence Bundle |
| [`cli/trace.md`](cli/trace.md) | `trace`: Symbol Call Chains |
| [`cli/decision-surface.md`](cli/decision-surface.md) | `decision-surface`: Structural Decisions |
| [`cli/explain.md`](cli/explain.md) | `explain`: Rule Explanation |
| [`cli/schema.md`](cli/schema.md) | `schema`: Capability Manifest |
| [`cli/config-schema.md`](cli/config-schema.md) | `config-schema`: Config JSON Schema |
| [`cli/plugin-schema.md`](cli/plugin-schema.md) | `plugin-schema`: Plugin JSON Schema |
| [`cli/plugin-check.md`](cli/plugin-check.md) | `plugin-check`: Verify external plugins |
| [`cli/rule-pack-schema.md`](cli/rule-pack-schema.md) | `rule-pack-schema`: Rule Pack JSON Schema |
| [`cli/cloud.md`](cli/cloud.md) | `cloud`: Log In to Fallow Cloud |
| [`cli/license.md`](cli/license.md) | `license`: Manage the Fallow Cloud License |
| [`cli/telemetry.md`](cli/telemetry.md) | `telemetry`: Opt-in Product Telemetry |
| [`cli/impact.md`](cli/impact.md) | `impact`: Local Impact History |
| [`cli/coverage.md`](cli/coverage.md) | `coverage`: Production-Coverage Workflow |
| [`cli/config.md`](cli/config.md) | `config`: Show Resolved Config |
| [`cli/global-flags.md`](cli/global-flags.md) | Global Flags |
| [`cli/environment-variables.md`](cli/environment-variables.md) | Environment Variables |
| [`cli/output-formats.md`](cli/output-formats.md) | Output Formats |
| [`cli/mutation.md`](cli/mutation.md) | `mutation --kill-matrix`: Redundant test evidence |
| [`cli/ci.md`](cli/ci.md) | `ci`: Provider-Aware Review Automation |
| [`cli/ci-integration.md`](cli/ci-integration.md) | CI Integration |
| [`cli/json-output.md`](cli/json-output.md) | JSON Output Structure |
| [`cli/configuration.md`](cli/configuration.md) | Configuration File Format |
| [`cli/inline-suppression.md`](cli/inline-suppression.md) | Inline Suppression Comments |

## Commands

Every fallow command with its purpose and key flags. The table is regenerated from `fallow schema` by scripts/generate-agent-docs.mjs; edit the curated Purpose cells in place, never the identity columns.

<!-- generated:commands:start -->
| Command | Purpose | Key Flags |
|---|---|---|
| `fallow` | Run full codebase analysis: cleanup + duplication + health (default) | `--only`, `--skip`, `--production`, `--production-dead-code`, `--production-health`, `--production-dupes`, `--ci`, `--fail-on-issues`, `--group-by`, `--summary`, `--fail-on-regression`, `--tolerance`, `--regression-baseline`, `--save-regression-baseline`, `--score`, `--trend`, `--save-snapshot`, `--include-entry-exports` |
| `dead-code` | Dead code analysis (`check` is an alias) | `--unused-exports`, `--changed-since`, `--changed-workspaces`, `--production`, `--file`, `--include-entry-exports`, `--stale-suppressions`, `--ci`, `--group-by`, `--summary`, `--fail-on-regression`, `--tolerance`, `--regression-baseline`, `--save-regression-baseline` |
| `architecture` | Check import cycles, boundaries and policy rules; optionally inventory shared-state access | `--cycles`, `--boundaries`, `--policy`, `--shared-state`, `--file`, `--finding-id`, `path` |
| `watch` | Watch for changes and re-run analysis | `--no-clear` |
| `type-aware` | Inspect the optional TypeScript semantic companion |  |
| `doctor` | Diagnose project readiness without analysis or mutation |  |
| `similar-code` | Find semantically similar functions with a pinned local model (opt-in) | `--threshold`, `--min-lines`, `--top`, `--file` |
| `mutation` | Measure how well the tests detect changes in the code (opt-in) | `--all`, `--plan`, `--max-mutants`, `--mutants-per-line`, `--function-scope`, `--time-budget`, `--concurrency`, `--runtime`, `--runner-config`, `--runner-project`, `--allow-dirty`, `--allow-side-effects` |
| `inspect` | Compose one evidence bundle for a file or exported symbol, with optional Fallow Cloud production data | `--file <path>`, `--symbol <file>:<export>`, `--cloud`, `--repo <owner/repo>` |
| `cloud` | Log in to Fallow Cloud with the device flow and store a read-only API key (login, logout) | `login --machine-name <name>`, `login --api-url <url>`, `logout` |
| `trace` | Trace a symbol's call chain, or with `--path` the shortest import path between two modules (best-effort, syntactic; OFF the ranked path) | `symbol`, `--path <FROM> <TO>`, `--eager-only`, `--callers`, `--callees`, `--depth` |
| `trace-error` | Resolve a runtime stack trace's frames to the definitions they name (best-effort, syntactic; OFF the ranked path) | `trace_file` |
| `fix` | Auto-remove unused exports/deps | `--dry-run`, `--yes` (required in non-TTY) |
| `init` | Generate config file, AGENTS.md agent guide, or pre-commit hook | `--toml`, `--agents`, `--hooks`, `--branch` |
| `hooks` | Inspect, install, or remove fallow-managed Git and agent hooks | `status`, `install --target git`, `install --target agent`, `uninstall --target git`, `uninstall --target agent` |
| `agent` | Wire fallow into Claude Code, Codex, or Cursor in one pass: AGENTS.md task map, skill, MCP server, commit/push gate; `status` and `uninstall` cover the same surfaces | `install --harness auto\|claude\|codex\|cursor`, `install --dry-run`, `install --approve`, `install --without <guide\|skill\|mcp\|hooks>`, `status`, `uninstall` |
| `ci` | CI helpers for PR/MR feedback envelopes |  |
| `ci reconcile-review` | Resolve stale review threads on a PR/MR by joining a typed review envelope (`--format review-github` / `review-gitlab`) against the provider's existing comments + threads. Posts an idempotent "Resolved in `<sha>`" follow-up per stale fingerprint, marker keyed on (fingerprint, short-sha) so re-runs on the same commit don't duplicate. A failed provider mutation blocks only the rest of that fingerprint's lifecycle, so every other stale fingerprint still resolves in the same run; JSON can include `apply_hint`, `failed_fingerprints`, and `unapplied_fingerprints` when `apply_errors` is non-empty. `ci post-review` reports the same three fields for the reconcile pass it runs after posting. | `--provider`, `--pr` (GH) / `--mr` (GL), `--repo` / `--project-id`, `--api-url`, `--envelope`, `--dry-run` |
| `config-schema` | Print the JSON Schema for fallow configuration files |  |
| `plugin-schema` | Print the JSON Schema for external plugin files |  |
| `plugin-check` | Dry-run external plugins: reports activation + what each `manifestEntries` rule matched/seeded/warned. Verify a `fallow-plugin-*.jsonc` before a full run. Always exits 0. | `--format json`, `--root` |
| `rule-pack-schema` | Print the JSON Schema for rule pack files |  |
| `rule-pack` | Manage declarative rule packs (policy-as-code) |  |
| `guard` | Show which architecture rules apply to files before changing them | `files` |
| `config` | Show the loaded config path and resolved config (verifies which `.fallowrc.json` is in effect); `--resolved` shows each decision of a run and its cause | `--path`, `--resolved`, `--for`, `--only`, `--skip`, `--production-dead-code`, `--production-health`, `--production-dupes` |
| `recommend` | Recommend a project-tailored config for an agent to author |  |
| `list` | Inspect project structure | `--files`, `--entry-points`, `--plugins`, `--boundaries`, `--workspaces`, `--entry-weight` |
| `workspaces` | Inspect monorepo workspaces + discovery diagnostics (shorthand for `list --workspaces`) | (no flags) |
| `dupes` | Code duplication detection | `--mode`, `--near`, `--threshold`, `--top`, `--changed-since`, `--workspace`, `--changed-workspaces`, `--skip-local`, `--cross-language`, `--ignore-imports`, `--explain-skipped`, `--fail-on-regression`, `--tolerance`, `--regression-baseline`, `--save-regression-baseline` |
| `health` | Function complexity analysis (also covers component templates as synthetic `<template>` findings: Angular external `.html` files via `templateUrl` AND inline `@Component({ template: \`...\` })` literals, plus Vue, Svelte and Astro single-file components; suppress an Angular external template with `<!-- fallow-ignore-file complexity -->` at the top of the `.html` file, an Angular inline template with `// fallow-ignore-next-line complexity` directly above the `@Component` decorator, and a `.svelte` / `.vue` / `.astro` template with `<!-- fallow-ignore-next-line complexity -->` on the line immediately above the reported line) | `--complexity`, `--max-cyclomatic`, `--max-cognitive`, `--max-crap`, `--max-file-lines`, `--top`, `--sort`, `--file-scores`, `--hotspots`, `--ownership`, `--ownership-emails`, `--targets`, `--effort`, `--score`, `--min-score`, `--since`, `--min-commits`, `--save-snapshot`, `--trend`, `--coverage-gaps`, `--coverage`, `--coverage-root`, `--runtime-coverage`, `--min-invocations-hot`, `--min-observation-volume`, `--low-traffic-threshold`, `--css`, `--complexity-breakdown`, `--min-severity`, `--report-only`, `--workspace`, `--changed-workspaces`, `--baseline`, `--save-baseline` |
| `flags` | Detect feature flag patterns (env vars, SDK calls, config objects) | `--top`, `--retirement`, `--reason`, `--min-age`, `--flag-state`, `--max-flag-age` |
| `suppressions` | List active fallow-ignore suppression markers (read-only inventory) | `--file` |
| `explain` | Explain one issue type without running analysis | `<issue-type>`, `--format json` |
| `audit` | Combined dead-code + complexity + duplication + styling for changed files, returns a verdict; `fallow review` is an alias for `fallow audit --brief` (advisory orientation brief, always exits 0) | `--base`, `--gate`, `--brief`, `--max-decisions`, `--walkthrough-guide`, `--walkthrough-file`, `--show-deprioritized`, `--production`, `--production-dead-code`, `--production-health`, `--production-dupes`, `--workspace`, `--changed-workspaces`, `--ci`, `--fail-on-issues`, `--explain`, `--explain-skipped`, `--dead-code-baseline`, `--health-baseline`, `--dupes-baseline`, `--max-crap`, `--coverage`, `--coverage-root`, `--only`, `--skip`, `--no-css`, `--css-deep`, `--no-css-deep`, `--include-entry-exports` |
| `audit-cache` | Maintain reusable audit base-snapshot caches |  |
| `baselines` | Maintain committed baselines: `baselines prune` removes the entries of fixed findings and never adds one | `prune --check`, `prune --dead-code-baseline`, `prune --health-baseline`, `prune --dupes-baseline`, `prune --production`, `prune --include-entry-exports` |
| `decision-surface` | Surface the consequential structural DECISIONS a change embeds (the apex of the review brief), each framed as a judgment question with the routed expert to ask | `--max-decisions` |
| `impact` | Show what fallow has done for you: how many issues it is surfacing, the trend since the last recorded run, and how many commits it contained at the pre-commit gate | `--all`, `--sort`, `--limit` |
| `security` | Surface opt-in local security candidates for agent verification (not confirmed vulnerabilities). Rule families include the graph rule `client-server-leak`, a data-driven `tainted-sink` catalogue, and the include-required `hardcoded-secret` category for provider-prefix credentials and high-entropy literals assigned to secret-shaped identifiers. Most catalogue rows require non-literal input; narrowly literal-aware rows flag deterministic unsafe literals. Rules default off; suppress a file with `// fallow-ignore-file security-sink`; scope categories with `security.categories`. Add project-local request object names with `security.requestReceivers`; it extends the built-in `req` / `request` / `ctx` / `context` / `event` allowlist for HTTP `query`, `params`, and `body` reads. `hardcoded-secret` runs only when listed in `security.categories.include`. | `--format human\|json\|sarif`, `--changed-since`, `--file`, `--diff-file`, `--workspace`, `--changed-workspaces`, `--surface`, `--ci`, `--fail-on-issues`, `--sarif-file`, `--summary` |
| `report` | Render a saved `--format json` results file in another format without re-running analysis (analyze once, render annotations and the job summary from the same file). With a CI format, `--mutation-report` adds a saved `fallow mutation --format json` report, and `--mutation-not-run` states why mutation testing did not run. | `--from`, `--mutation-report`, `--mutation-not-run` |
| `schema` | Dump CLI definition as JSON | `--cli-reference` |
| `feedback` | Send feedback to the Fallow maintainers as a GitHub issue | `message`, `--type`, `--impact`, `--task`, `--command`, `--expected`, `--actual`, `--agent` |
| `ci-template` | Print or vendor CI integration templates |  |
| `migrate` | Convert knip/jscpd config | `--dry-run`, `--from PATH` |
| `license` | Manage the local license JWT for continuous/cloud runtime monitoring (activate, status, refresh, deactivate) | `activate --trial --email <addr>`, `activate --from-file`, `activate --stdin`, `status`, `refresh`, `deactivate` |
| `telemetry` | Manage opt-in, off-by-default product telemetry (never collects code, paths, or names). Agents must not enable it; only the user may | `status`, `enable`, `disable`, `inspect --example` |
| `coverage` | Runtime coverage setup, focused analysis, and cloud inventory workflow helper | `setup`, `setup --yes`, `setup --non-interactive`, `analyze --runtime-coverage <path>`, `analyze --cloud --repo owner/repo`, `upload-inventory` |
| `coverage upload-source-maps` | Upload build source maps from CI so bundled runtime coverage resolves to original source paths. Retries 429 `Retry-After` and transient gateway failures. Use `FALLOW_CA_BUNDLE` for complete custom PEM trust bundles. | `--dir dist`, `--git-sha <sha>`, `--repo <name>`, `--strip-path=false`, `--dry-run` |
| `setup-hooks` | Deprecated (removed in the next major): use `agent install` or `hooks install --target agent`; still installs the Claude Code PreToolUse gate with a stderr warning | `--agent`, `--dry-run`, `--force`, `--user`, `--gitignore-claude`, `--uninstall` |
| `viz` | Render the codebase as a self-contained interactive HTML map (treemap + import graph) with six primary lenses (Overview, Unused, Duplication, Architecture, Health, Security) and Dependencies, Frameworks, Styling, and Feature flags under an adaptive More menu, each with click-through detail panels. Every lens carries an availability state (complete, disabled, not applicable, unavailable) next to its count, so an analysis that did not run reads as missing data instead of as zero findings. Or emit the import graph as text. Read-only. | `--out <path>`, `--no-open`, `--viz-format html\|dot\|mermaid`, `--root`, `--config`, `--production`, `--no-cache` |

Run `fallow <command> --help` for the full flag list per command (see also the per-command files under references/cli/).
<!-- generated:commands:end -->
