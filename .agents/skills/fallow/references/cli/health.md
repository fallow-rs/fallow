# `health`: Function Complexity and File Health Analysis

Analyzes function complexity across the project using cyclomatic and cognitive complexity metrics. By default all sections are included (health score, complexity findings, file scores, hotspots, and refactoring targets). Use `--complexity`, `--file-scores`, `--hotspots`, `--targets`, or `--score` to show only specific sections.

Angular templates contribute synthetic `<template>` complexity findings whenever they use `@if`/`@for`/`@switch`/`@case`/`@defer (when ...)`/`@let` blocks, legacy structural directives (`*ngIf`, `*ngFor`), bound attributes (`[x]`, `(x)`, `bind-x`, `on-x`), or `{{ }}` interpolations. Both standalone external `.html` files referenced via `templateUrl` AND inline `@Component({ template: \`...\` })` literals are scanned. Inline-template findings anchor at the host `.ts` file's `@Component` decorator line and emit a `suppress-line` action with `// fallow-ignore-next-line complexity` (place the comment directly above the `@Component` decorator). External-template findings emit a `suppress-file` action with `<!-- fallow-ignore-file complexity -->` (place at the top of the `.html` file; HTML cannot express line-level comments). Tagged template literals containing `${...}` interpolations and `template:` properties bound to a variable are skipped (out of scope for the first cut). Vue, Svelte, and Astro single-file components contribute synthetic `<template>` findings too, each scored against its own control-flow vocabulary; those findings emit a `suppress-line` action with the markup comment `<!-- fallow-ignore-next-line complexity -->` placed on the line immediately above the reported line (`placement: "above-template-anchor-line"`), because the synthetic template unit anchors at its first contributing construct rather than the top of the file.

## Flags

<!-- generated:flags:health:start -->
| Flag | Type | Default | Description |
|---|---|---|---|
| `--max-cyclomatic` | `string` | - | Fail if any function exceeds this cyclomatic complexity |
| `--max-cognitive` | `string` | - | Fail if any function exceeds this cognitive complexity |
| `--max-crap` | `string` | - | Fail if any function has CRAP score >= threshold. CRAP combines complexity with coverage (`CC^2 * (1 - cov/100)^3 + CC`). Pair with `--coverage` for accurate per-function CRAP; without coverage data fallow estimates coverage from the module graph. |
| `--max-file-lines` | `string` | - | Maximum physical lines per file (overrides config `health.maxFileLines`, default 0 = off). A longer file is reported as a complexity finding with `exceeded: "file_lines"` |
| `--complexity` | `bool` | `false` | Show only function complexity findings. When no section flags are set, all sections are shown by default. |
| `--complexity-breakdown` | `bool` | `false` | Add a per-decision-point `contributions[]` array to each complexity finding in `--format json`. Each entry names the construct (`if`, `else-if`, `ternary`, boolean operator, loop, `case`, `catch`, `optional-chain`, ...) and carries its source line, the metric it adds to (`cyclomatic` or `cognitive`), its weight, and the nesting depth, so a consumer can explain WHY a function scored high. Off by default (no change to existing JSON/SARIF/markdown). Used by the VS Code inline editor breakdown and the MCP `check_health` `complexity_breakdown` param. |
| `--top` | `string` | - | Only show the top N most complex functions (and file scores/hotspots/targets) |
| `--sort` | `severity\|cyclomatic\|cognitive\|lines` | `cyclomatic` | Sort order for complexity findings |
| `--file-scores` | `bool` | `false` | Show only per-file health scores (maintainability index, LOC, fan-in, fan-out, dead code ratio, complexity density, CRAP risk). Runs the full analysis pipeline. Sorted by risk-aware triage concern: lower maintainability index and higher CRAP risk first. When no section flags are set, all sections are shown by default. |
| `--coverage-gaps` | `bool` | `false` | Show runtime files and exports that no test dependency path reaches. Opt-in (default off). Configure severity via the `coverage-gaps` rule (`error`/`warn`/`off`). |
| `--shallow-directories` | `bool` | `false` | Show only shallow directories: directories whose interface is as large as what they hide. Advisory, no health score weight. Requires full analysis pipeline |
| `--hotspots` | `bool` | `false` | Show only hotspots: files that are both complex and frequently changing. Combines git churn history with complexity data. Requires a git repository. When no section flags are set, all sections are shown by default. |
| `--ownership` | `bool` | `false` | Attach ownership signals to hotspot entries: bus factor (Avelino truck factor), contributor count, top contributor with stale-days, recent contributors (top-3), `suggested_reviewers`, declared CODEOWNERS owner, `ownership_state`, ownership drift, unowned-hotspot detection. Human output gains a project-level summary line. JSON adds `low-bus-factor`, `unowned-hotspot`, `ownership-drift` action types. Test files get a `[test]` tag. Implies `--hotspots`. Requires git. |
| `--ownership-emails` | `raw\|handle\|anonymized\|hash` | - | Privacy mode for author emails. `handle` shows the local-part only (default, with GitHub noreply unwrap and deterministic same-handle disambiguation). `anonymized` emits stable `xxh3:` pseudonyms; `hash` remains accepted as the legacy spelling. `raw` shows full addresses. Use `anonymized` in regulated environments. Implies `--ownership`. Configure default via `health.ownership.emailMode`. |
| `--targets` | `bool` | `false` | Show only refactoring targets: ranked recommendations based on complexity, coupling, churn, and dead code signals. Categories: churn+complexity, circular dep, high impact, dead code, complexity, coupling, test coverage, hidden coupling. When no section flags are set, all sections are shown by default. Each target's JSON can include `direct_callers[]` (direct importers with the symbols they import) and `clone_siblings[]` (duplicate-code siblings with stable `dup:<8hex>` fingerprints for `fallow dupes --trace`); both omitted when empty. Human output adds `importers:` / `clones:` lines only when that evidence is present. |
| `--type-coupling` | `bool` | `false` | Show advisory project-local public-signature type coupling. Requires type-aware analysis and does not change the health score |
| `--css` | `bool` | `false` | Add structural CSS analytics: specificity hotspots, !important density, over-complex selectors, deep nesting, and conservative cleanup candidates. Standard CSS is parsed structurally; preprocessor sources are scanned only where fallow can avoid expanding Sass/Less semantics. Also derives `styling_health`, a descriptive A-F grade for CSS quality scored separately from the code `health_score` (never gates); it weights design-token drift (hardcoded value sprawl) over byte-identical repetition. |
| `--effort` | `low\|medium\|high` | - | Filter refactoring targets by effort level. Implies `--targets`. |
| `--score` | `bool` | `false` | Show only the project health score (0-100) with letter grade (A/B/C/D/F). The score is included by default when no section flags are set. JSON includes `health_score` object with `score`, `grade`, and `penalties` breakdown. As of v2.55.0, plain `--score` skips the churn-backed hotspot penalty so it does not run a `git log` shell-out per invocation; pass `--hotspots` (or `--targets` with `--score`) to include the hotspot penalty. Snapshot (`--save-snapshot`) and trend (`--trend`) flows still trigger hotspot vital signs so saved data stays complete. |
| `--min-score` | `string` | - | Fail (exit 1) only when the health score is below this threshold. Implies `--score`. Authoritative CI quality gate: when set, complexity findings are demoted to informational and the exit code is driven solely by the score, so `--min-score 0` always exits 0. Composes with `--min-severity`. |
| `--min-severity` | `moderate\|high\|critical` | - | Only exit with an error for findings at or above this severity. Composes with `--min-score` (the run fails if either gate trips). |
| `--report-only` | `bool` | `false` | Print the score and findings but never fail CI (always exit 0). Advisory mode. Mutually exclusive with `--min-score` and `--min-severity`. |
| `--since` | `string` | - | Git history window for hotspot analysis. Accepts durations (`6m`, `90d`, `1y`, `2w`) or ISO dates (`2025-06-01`). Ignored when `--churn-file` is set. |
| `--min-commits` | `string` | - | Minimum number of commits for a file to be included in hotspot ranking. |
| `--save-snapshot` | `string` | - | Save vital signs snapshot for trend tracking. Forces file-scores + hotspot computation. |
| `--trend` | `bool` | `false` | Compare current metrics against the most recent saved snapshot. Reads from `.fallow/snapshots/` and shows per-metric deltas with directional indicators (improving/declining/stable). Implies `--score`. |
| `--trend-from` | `string` | - | Compare current metrics against this snapshot file instead of the newest file in `.fallow/snapshots/`. Use it to restore a baseline from external storage in CI. With --group-by, groups are compared by key when the snapshot holds the same grouping. Implies --trend |
| `--coverage` | `string` | - | Path to coverage data for accurate per-function CRAP scores: an Istanbul map (`coverage-final.json`), a directory containing one, a raw V8 coverage directory (`NODE_V8_COVERAGE=<dir> node --test`), or a single V8 coverage JSON file. Transpiled V8 scripts (tsx, bundles) map back to their source files through the source map that Node records in the dump; a script that differs from the file on disk and has no source map keeps the estimate. Uses `CC^2 * (1-cov/100)^3 + CC` instead of the default estimated coverage. Relative paths resolve against `--root`. Falls back to `FALLOW_COVERAGE`, then `health.coverage`, then auto-detection. |
| `--coverage-root` | `string` | - | Absolute prefix to strip from file paths in coverage data before prepending the project root. For CI/Docker environments where coverage was generated with different absolute paths. Falls back to `FALLOW_COVERAGE_ROOT`, then `health.coverageRoot`. |
| `--runtime-coverage` | `string` | - | Merge runtime-coverage input into the health report. Accepts a V8 coverage directory (`NODE_V8_COVERAGE=...`), a single V8 coverage JSON file, or an Istanbul `coverage-final.json`. One local capture is free and does not require a license; continuous/cloud or multi-capture runtime monitoring requires an active license or trial (`fallow license activate --trial --email <addr>`). JSON output gains a `runtime_coverage` object with a top-level report verdict, per-finding `verdict` (`safe_to_delete` / `review_required` / `low_traffic` / `coverage_unavailable` / `active`), a per-finding suppression `id` (`fallow:prod:<hash>`, hashes the current line), an optional cross-surface `stable_id` join key (`fallow:fn:<hash>`, hashes file + name + start line; one value per function across findings / hot-paths / blast-radius / importance and across V8/Istanbul/oxc producers), an optional content-digest `source_hash` (line-move-immune, so baselines survive a pure line shift), an evidence block, and percentile-ranked hot paths. On protocol-0.3+ sidecars the `summary` also carries an optional `capture_quality` block (`window_seconds`, `instances_observed`, `lazy_parse_warning`, `untracked_ratio_percent`) that flags short-window captures where lazy-parsed scripts may not appear. |
| `--mutation-report` | `string` | - | Saved `fallow mutation --format json` report (for example `.fallow/mutation/last-report.json` or the `-o` file of a run). Adds the mutation score of each complexity finding next to its CRAP score. The score never changes CRAP or the health score. A function whose file changed since the run gets no score. Reads the file only; it runs no test |
| `--min-invocations-hot` | `string` | `100` | Invocation threshold for hot-path classification. Takes effect only when `--runtime-coverage` is set. |
| `--min-observation-volume` | `string` | - | Minimum total trace volume before the sidecar may emit high-confidence `safe_to_delete` / `review_required` verdicts. Below this, confidence is capped at `medium`. |
| `--low-traffic-threshold` | `string` | - | Fraction of total trace count below which an invoked function is classified `low_traffic` rather than `active`. Expressed as a decimal (0.001 = 0.1%). |

Common global flags for this command: [`--format`](global-flags.md), [`--quiet`](global-flags.md), [`--changed-since`](global-flags.md), [`--churn-file`](global-flags.md), [`--workspace`](global-flags.md), [`--group-by`](global-flags.md), [`--baseline`](global-flags.md), [`--baseline-mode`](global-flags.md), [`--save-baseline`](global-flags.md), [`--production`](global-flags.md), [`--no-production`](global-flags.md), [`--explain`](global-flags.md).
<!-- generated:flags:health:end -->

With `--workspace`, vital signs, the health score, hotspots, file scores, findings, and `summary.files_analyzed` are all recomputed against the scoped subset.

With `--group-by`, JSON adds `grouped_by` plus a `groups` array. Each group has its own `vital_signs`, `health_score`, `findings`, `file_scores`, `hotspots`, `large_functions`, and `targets`, recomputed against the files of the group. The top-level metrics stay project-wide, so a consumer that ignores grouping still sees the project headline. Human output adds a per-group score / files / hot / p90 summary block (worst first when `--score` is set). Markdown adds a grouped health table. SARIF results carry `properties.group`, and CodeClimate issues carry a top-level `group` field. Compact and badge output fall back to ungrouped output with a stderr note.

## Exit Codes

The gate flag in play determines what drives the exit code. Plain `fallow health` (no gate flag) stays advisory but still fails on any finding (back-compat).

| Invocation | Exit 0 when | Exit 1 when |
|------------|-------------|-------------|
| `fallow health` (no gate flag) | no function exceeds a threshold | any function exceeds a threshold |
| `--min-score N` | score >= N (findings informational) | score < N |
| `--min-severity LEVEL` | no finding at or above LEVEL | any finding at or above LEVEL |
| `--min-score N --min-severity LEVEL` | score >= N AND no finding >= LEVEL | score < N OR a finding >= LEVEL |
| `--report-only` | always | never |

`--report-only` with `--min-score` / `--min-severity` exits 2 (mutually exclusive). The `--runtime-coverage` and coverage-gap gates stay independent and are not demoted by `--min-score`. For gating only newly-introduced complexity, use `fallow audit --gate new-only`.

## Examples

```bash
# Full complexity analysis with JSON output
fallow health --format json --quiet

# Project health score with letter grade
fallow health --format json --quiet --score

# CI gate: fail if score below 70
fallow health --format json --quiet --min-score 70

# Top 10 most complex functions
fallow health --format json --quiet --top 10

# Sort by cognitive complexity
fallow health --format json --quiet --sort cognitive

# Custom thresholds
fallow health --format json --quiet --max-cyclomatic 15 --max-cognitive 10

# Per-file health scores
fallow health --format json --quiet --file-scores

# Top 20 files by triage concern
fallow health --format json --quiet --file-scores --top 20

# Only analyze files changed since main
fallow health --format json --quiet --changed-since main

# Single workspace package
fallow health --format json --quiet --workspace my-package

# Incremental adoption with baseline
fallow health --format json --quiet --save-baseline fallow-baselines/health.json
fallow health --format json --quiet --baseline fallow-baselines/health.json

# Strict adoption: a hotspot that replaces a baselined hotspot is reported
fallow health --format json --quiet --save-baseline fallow-baselines/health.json --baseline-mode identity
fallow health --format json --quiet --baseline fallow-baselines/health.json --baseline-mode identity

# CI: fail if any function is too complex
fallow health --max-cyclomatic 25 --max-cognitive 20 --quiet

# Hotspot analysis (complex + frequently changing files)
fallow health --format json --quiet --hotspots

# Hotspots from the last year
fallow health --format json --quiet --hotspots --since 1y

# Hotspots with at least 5 commits
fallow health --format json --quiet --hotspots --min-commits 5

# Top 10 hotspots from the last 90 days
fallow health --format json --quiet --hotspots --since 90d --top 10

# Ranked refactoring recommendations
fallow health --format json --quiet --targets

# Top 5 refactoring targets
fallow health --format json --quiet --targets --top 5

# Only low-effort refactoring targets (quick wins)
fallow health --format json --quiet --effort low

# Save a vital signs snapshot for trend tracking
fallow health --format json --quiet --save-snapshot

# Save snapshot to a custom path
fallow health --format json --quiet --save-snapshot .fallow/baseline-snapshot.json

# Compare current metrics against the most recent snapshot
fallow health --format json --quiet --trend
```

## JSON Output Structure

```json
{
  "kind": "health",
  "schema_version": 7,
  "version": "3.49.0",
  "elapsed_ms": 32,
  "summary": {
    "files_analyzed": 482,
    "functions_analyzed": 3200,
    "functions_above_threshold": 3,
    "max_cyclomatic_threshold": 20,
    "max_cognitive_threshold": 15
  },
  "findings": [
    {
      "path": "src/parser.ts",
      "name": "parseExpression",
      "line": 42,
      "col": 0,
      "cyclomatic": 28,
      "cognitive": 22,
      "line_count": 95,
      "exceeded": "both"
    }
  ]
}
```

`health.thresholdOverrides[]` config entries can raise local cyclomatic, cognitive, CRAP, or unit-size (large-function line-count) ceilings for matching files and optional exact function names. When an override affects output, health JSON includes top-level `threshold_overrides[]` state entries (`active`, `stale`, `insufficient`, or `no_match`). Each entry names the `dimension` it describes (`complexity` or `crap`): one configured override produces one entry per dimension it participates in, so group on `override_index` to count configured overrides rather than counting entries. `insufficient` means the override raised that dimension's ceiling but the matched code still exceeds the raised value, so the finding survives the override. An entry's `outstanding[]` lists every dimension the matched unit still breaches after the override applied, whether or not the entry configures that ceiling, which is how an `active` override can sit next to a surviving finding. `no_match` entries are emitted on full-repo runs (they are suppressed only when the run is scoped by `--changed-since`, `--diff-index`, `--workspace`, or `--changed-workspaces`), so a glob that matches nothing is reported instead of silently doing nothing. Complexity findings evaluated with local ceilings include `effective_thresholds` and `threshold_source: "override"` so agents can see which thresholds drove the finding and avoid treating configured exceptions as hidden suppressions.

When the unit size very-high-risk percentage is >= 3%, the JSON output includes a `large_functions` array listing functions exceeding 60 lines of code:

```json
{
  "large_functions": [
    {
      "path": "src/parser.ts",
      "name": "parseExpression",
      "line": 42,
      "line_count": 95
    }
  ]
}
```

This drill-down shows which specific functions are driving the unit size penalty in the health score, making it actionable without a separate analysis pass.

With `--file-scores`, the JSON output also includes `file_scores` array and `summary.files_scored` / `summary.average_maintainability`:

```json
{
  "summary": {
    "files_scored": 482,
    "average_maintainability": 88.5,
    "coverage_model": "static_estimated",
    "coverage_source_consistency": "uniform"
  },
  "file_scores": [
    {
      "path": "src/parser.ts",
      "fan_in": 8,
      "fan_out": 4,
      "dead_code_ratio": 0.25,
      "complexity_density": 0.22,
      "maintainability_index": 75.1,
      "total_cyclomatic": 42,
      "total_cognitive": 35,
      "function_count": 12,
      "lines": 190,
      "crap_max": 42.0,
      "crap_above_threshold": 2
    }
  ]
}
```

The `file_scores` array is sorted by risk-aware triage concern: the larger of low-MI concern and CRAP risk. This keeps files with very high untested complexity near the top even when their Maintainability Index is not the lowest.

The `crap_max` field is the highest CRAP (Change Risk Anti-Patterns) score among functions in the file, using the canonical formula `CC^2 * (1 - cov/100)^3 + CC`. It is always the raw measured value. The default model (`static_estimated`) estimates per-function coverage from export references: directly test-referenced = 85%, indirectly test-reachable = 40%, untested = 0%. Provide `--coverage <path>` with an Istanbul `coverage-final.json` or raw V8 coverage for measured scores (`istanbul` model; `summary.coverage_input_format` is `istanbul` or `v8`). When CRAP findings use the static estimate, the human output prints one note above the findings that says no coverage file was found; `--explain` adds the estimate tiers to that note. The `crap_above_threshold` field counts functions whose rounded CRAP meets or exceeds their effective ceiling, resolved from `health.thresholdOverrides` over the global `maxCrap` / `--max-crap` value (default 30); it is 0 when CRAP enforcement is disabled (`maxCrap: 0`). Rows whose breaches were let through by configuration carry two additional fields: `crap_exempted` (functions at or above the canonical 30 baseline but below their effective ceiling; omitted when 0) and `crap_effective_threshold` (the lowest effective ceiling among the file's functions, present only when it differs from `summary.max_crap_threshold`). When `--file-scores` is active, `summary.coverage_model` indicates the model used (`"static_estimated"` or `"istanbul"`). When CRAP findings carry `coverage_source`, `summary.coverage_source_consistency` is `uniform` or `mixed`; grouped health JSON mirrors this as `groups[].coverage_source_consistency`.

Maintainability index formula: `100 - (complexity_density × 30) - (dead_code_ratio × 20) - min(ln(fan_out+1) × 4, 15)`, clamped to 0–100. Higher is better. Type-only exports are excluded from dead_code_ratio. Zero-function files (barrels) are excluded by default.

With `--hotspots`, the JSON output includes a `hotspots` array and `hotspot_summary`:

```json
{
  "hotspot_summary": {
    "since": "6m",
    "min_commits": 3,
    "files_analyzed": 482,
    "files_excluded": 312,
    "shallow_clone": false,
    "clock": {
      "source": "head_commit",
      "epoch_secs": 1788782400,
      "reproducible": true
    }
  },
  "hotspots": [
    {
      "path": "src/parser.ts",
      "score": 92,
      "commits": 28,
      "weighted_commits": 34.5,
      "lines_added": 410,
      "lines_deleted": 180,
      "complexity_density": 0.22,
      "fan_in": 8,
      "trend": "Accelerating",
      "median_files_per_commit": 3,
      "median_lines_per_commit": 42
    }
  ]
}
```

`median_files_per_commit` and `median_lines_per_commit` measure change dispersion. They are the medians over the commits that changed the file, and they count analyzed files only. A commit with more analyzed files than `health.changeCoupling.maxFilesPerCommit` (default 30) is not counted. Both fields are absent when the history has no commit ids, for example imported churn without the `commit` field. Without `--since`, they read 12 months of history, while the hotspot score keeps 6 months.

When the hotspots section runs on git history, the JSON output also includes a `change_coupling` object. It lists hidden pairs: two files that change together while no import or re-export chain connects them. An agent that follows imports does not find the second file of such a pair.

```json
{
  "change_coupling": {
    "since": "12 months",
    "commits_analyzed": 640,
    "commits_skipped": 12,
    "total_pairs": 4,
    "thresholds": {
      "max_files_per_commit": 30,
      "min_revisions": 5,
      "min_shared_revisions": 5,
      "min_degree": 0.3,
      "strong_degree": 0.5
    },
    "pairs": [
      {
        "path_a": "src/api/orders.ts",
        "path_b": "src/ui/order-form.ts",
        "revisions_a": 12,
        "revisions_b": 10,
        "shared_revisions": 9,
        "degree": 0.82,
        "strong": true
      }
    ]
  }
}
```

`degree` is `shared_revisions` divided by the mean of `revisions_a` and `revisions_b`, rounded to two decimals. A pair is `strong` when the rounded `degree` reaches `strong_degree` (0.5). `pairs` is cut to `--top`, and `total_pairs` keeps the full count. The `health.changeCoupling` config object changes the thresholds: `minRevisions` (default 5), `minSharedRevisions` (default 5), `minDegree` (from 0 to 1, default 0.3) and `maxFilesPerCommit` (default 30). Without `--since`, the section reads 12 months of history. The section is advisory: it has no weight in the health score and never gates. A shallow clone, or a window with fewer analyzed commits than `minRevisions`, gives an empty `pairs` list and a `workspace_diagnostics[]` entry with kind `change-coupling-skipped` and `cause` `shallow-clone` or `too-few-commits`.

Hotspot score formula: `normalized_churn × normalized_complexity × 100`, scaled 0–100. Higher means more urgent to refactor. The `trend` field indicates recent change velocity: `Accelerating` (increasing churn), `Stable` (constant), or `Cooling` (decreasing). Files below `--min-commits` are excluded. The `shallow_clone` field warns when git history is truncated (shallow clone), which may undercount commits. The `clock` object reports the single instant `weighted_commits` and ownership `stale_days` were measured against: `source` is `head_commit` (the default, identical for every run over one commit), `environment` (pinned with `FALLOW_CLOCK_EPOCH`), or `wall_clock` (no commit timestamp was readable, so the numbers drift between runs and `reproducible` is false). Pass `epoch_secs` back as `FALLOW_CLOCK_EPOCH` to reproduce a run's churn-derived numbers.

With `--targets`, the JSON output includes a `targets` array with ranked refactoring recommendations:

```json
{
  "targets": [
    {
      "path": "src/parser.ts",
      "priority": 82.5,
      "efficiency": 27.5,
      "recommendation": "Split high-impact file - 25 dependents amplify every change",
      "category": "split_high_impact",
      "effort": "high",
      "confidence": "medium",
      "factors": [
        {
          "metric": "complexity_density",
          "value": 0.75,
          "threshold": 0.3,
          "detail": "density 0.75 exceeds 0.3"
        },
        {
          "metric": "fan_in",
          "value": 25.0,
          "threshold": 10.0,
          "detail": "25 files depend on this"
        }
      ]
    }
  ],
  "target_thresholds": {
    "fan_in_p95": 12.0,
    "fan_in_p75": 5.0,
    "fan_out_p95": 15.0,
    "fan_out_p90": 8
  }
}
```

Targets are sorted by `efficiency` (priority / effort_numeric) descending, surfacing quick wins first. The `target_thresholds` object exposes the adaptive percentile-based thresholds used for scoring. Priority formula: `min(complexity_density, 1) x 30 + hotspot_boost x 25 + dead_code_ratio x 20 + fan_in_norm x 15 + fan_out_norm x 10`, clamped to 0-100. Fan-in and fan-out normalization uses the project's p95 values (with floors). Categories: `urgent_churn_complexity`, `break_circular_dependency`, `split_high_impact`, `remove_dead_code`, `extract_complex_functions`, `extract_dependencies`, `add_test_coverage`, `resolve_hidden_coupling`. Each target includes `efficiency`, `effort` (low/medium/high), `confidence` (high/medium/low, data source reliability), and contributing `factors`.

The `add_test_coverage` category fires when a file has 2+ functions whose rounded CRAP meets or exceeds their effective ceiling (`health.thresholdOverrides` over the global `maxCrap` / `--max-crap`, default 30) and complexity density > 0.3. A file whose breaching functions are all exempted by configuration produces no target. The `crap_max` metric appears in contributing factors for these targets, with `threshold` set to the file's lowest effective ceiling (the run global when no override applies).

The `resolve_hidden_coupling` category fires for a file that has a strong hidden pair (degree at least 0.5) and that no other category claims. The rule runs last, so a file keeps its existing category. The target lists the partner files in `evidence.co_change_partners`, strongest first. Every target of a file with a strong hidden pair also gets the `hidden_coupling` factor.

## Vital Signs

All `health` JSON output includes a `vital_signs` object with project-wide metrics:

```json
{
  "vital_signs": {
    "dead_file_pct": 3.2,
    "dead_export_pct": 8.1,
    "avg_cyclomatic": 4.5,
    "critical_complexity_pct": 1.2,
    "p90_cyclomatic": 12,
    "maintainability_avg": 88.5,
    "maintainability_low_pct": 4.1,
    "hotspot_count": 7,
    "hotspot_top_pct_count": 3,
    "circular_dep_count": 2,
    "circular_deps_per_k_files": 4.1,
    "unused_dep_count": 3,
    "unused_deps_per_k_files": 6.2,
    "unit_size_profile": {
      "low_risk": 82.1,
      "medium_risk": 11.4,
      "high_risk": 4.3,
      "very_high_risk": 2.2
    },
    "functions_over_60_loc_per_k": 22.0,
    "unit_interfacing_profile": {
      "low_risk": 95.6,
      "medium_risk": 3.8,
      "high_risk": 0.5,
      "very_high_risk": 0.1
    },
    "p95_fan_in": 8,
    "coupling_high_pct": 2.3
  }
}
```

Fields are `null` when the corresponding data source is not available (e.g., `hotspot_count` is null without `--hotspots` or when git is not available).

Health score formula v3 retains the scale-invariant density and tail fields: `critical_complexity_pct`, `maintainability_low_pct`, `unused_deps_per_k_files`, `circular_deps_per_k_files`, and `functions_over_60_loc_per_k`. The hotspot penalty is `hotspot_count / max(ceil(total_files × 0.01), 1) × 10`, capped at 10. Only files with a hotspot score of at least 50 enter `hotspot_count`; an empty scope has no hotspot penalty. `hotspot_top_pct_count` remains a rank diagnostic and does not determine the penalty.

The `unit_size_profile` and `unit_interfacing_profile` are risk distribution histograms (low risk / medium risk / high risk / very high risk as percentages). `p95_fan_in` is the 95th percentile of incoming dependencies. `coupling_high_pct` is the percentage of files above the effective coupling threshold.

With `--score`, the JSON output includes a `health_score` object:

```json
{
  "health_score": {
    "formula_version": 3,
    "score": 72.9,
    "grade": "B",
    "penalties": {
      "dead_files": 3.1,
      "dead_exports": 6.0,
      "complexity": 0.0,
      "p90_complexity": 0.0,
      "maintainability": 0.0,
      "unused_deps": 10.0,
      "circular_deps": 4.0,
      "unit_size": 0.0,
      "coupling": 0.0,
      "duplication": 4.0
    }
  }
}
```

Score is reproducible: `100 - sum(penalties) == score`. `formula_version` identifies the scoring formula; version 3 retains the density and tail metrics from version 2 and corrects the hotspot penalty to use the thresholded count. Penalty fields are absent when the pipeline didn't run. `--score` automatically runs duplication analysis; add `--hotspots` (or combine `--score --targets`) when the score should include the churn-backed hotspot penalty. Grades: A (>= 85), B (70-84), C (55-69), D (40-54), F (< 40).

## Health Trend

With `--trend`, the JSON output includes a `health_trend` object comparing current metrics against the most recent saved snapshot:

```json
{
  "health_trend": {
    "compared_to": {
      "timestamp": "2026-03-25T14:30:00Z",
      "git_sha": "a1b2c3d",
      "score": 70.2,
      "grade": "B",
      "score_formula_version": 3
    },
    "metrics": [
      {
        "name": "score",
        "label": "Health Score",
        "previous": 70.2,
        "current": 72.9,
        "delta": 2.7,
        "direction": "improving",
        "unit": ""
      },
      {
        "name": "dead_file_pct",
        "label": "Dead Files",
        "previous": 5.1,
        "current": 4.2,
        "delta": -0.9,
        "direction": "improving",
        "unit": "%",
        "previous_count": { "value": 13, "total": 255 },
        "current_count": { "value": 11, "total": 262 }
      }
    ],
    "snapshots_loaded": 3,
    "overall_direction": "improving"
  }
}
```

Metrics tracked: `score`, `dead_file_pct`, `dead_export_pct`, `avg_cyclomatic`, `maintainability_avg`, `unused_dep_count`, `circular_dep_count`, `hotspot_count`, `unit_size_very_high_pct`, `p95_fan_in`, `duplication_pct`. Each metric includes `direction` (`improving`, `declining`, `stable`). Percentage metrics include `previous_count`/`current_count` with raw numerator and denominator. `--trend` requires at least one saved snapshot in `.fallow/snapshots/`. Current snapshots use schema version 12 and record `score_formula_version`. A score delta is emitted only when both scores exist and their formula versions are known and equal. An older snapshot without this metadata retains its historical score and grade, but has no score delta. Raw metric trends still compare across formula changes. The JSON baseline exposes the stored formula version when known; human, Markdown, GitHub and compact output explain an omitted score comparison.

## Vital Signs Snapshots

`--save-snapshot` persists a `VitalSignsSnapshot` JSON file for trend tracking across runs. Snapshots automatically include the health score and grade. The snapshot contains more detail than the inline `vital_signs` object:

```json
{
  "snapshot_schema_version": 12,
  "timestamp": "2025-12-01T10:30:00Z",
  "score_formula_version": 3,
  "vital_signs": {
    "dead_file_pct": 3.2,
    "dead_export_pct": 8.1,
    "avg_cyclomatic": 4.5,
    "critical_complexity_pct": 1.2,
    "p90_cyclomatic": 12,
    "maintainability_avg": 88.5,
    "maintainability_low_pct": 4.1,
    "hotspot_count": 7,
    "hotspot_top_pct_count": 3,
    "circular_dep_count": 2,
    "circular_deps_per_k_files": 4.1,
    "unused_dep_count": 3,
    "unused_deps_per_k_files": 6.2,
    "functions_over_60_loc_per_k": 22.0
  },
  "counts": {
    "total_files": 482,
    "dead_files": 15,
    "total_exports": 1200,
    "dead_exports": 97,
    "total_dependencies": 42,
    "unused_dependencies": 3
  },
  "git_sha": "abc1234",
  "git_branch": "main",
  "shallow_clone": false
}
```

The snapshot `snapshot_schema_version` is independent of the report `schema_version`. Default path: `.fallow/snapshots/<timestamp>.json`. The `--save-snapshot` flag forces file-scores and hotspot computation to populate all vital signs fields.
