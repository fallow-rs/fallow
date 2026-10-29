//! Parameters of the `check_health` MCP tool.

use super::*;

#[derive(Default, Deserialize, JsonSchema)]
pub struct HealthParams {
    /// Project root; defaults to the working directory.
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config `extends` for this request. Defaults to false
    /// and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Maximum cyclomatic complexity threshold. Functions exceeding this are reported.
    pub max_cyclomatic: Option<u16>,

    /// Maximum cognitive complexity threshold. Functions exceeding this are reported.
    pub max_cognitive: Option<u16>,

    /// Maximum CRAP score threshold (default 30.0). Functions meeting or
    /// exceeding this score are reported alongside complexity findings. Pair
    /// with `coverage` for accurate per-function CRAP; without Istanbul data
    /// fallow estimates coverage from the module graph.
    pub max_crap: Option<f64>,

    /// Maximum physical lines per file (default 0, off). A longer file is
    /// reported as a complexity finding with `exceeded: "file_lines"`.
    /// Passed through to the CLI's `--max-file-lines` flag.
    pub max_file_lines: Option<u32>,

    /// Number of top results to return.
    pub top: Option<usize>,

    /// Sort order for results (e.g., "cyclomatic", "cognitive", "lines", "severity").
    pub sort: Option<String>,

    /// Git ref to compare against. Only files changed since this ref are analyzed.
    pub changed_since: Option<String>,

    /// Show only complexity findings. By default all sections are shown; use this to select only complexity.
    pub complexity: Option<bool>,

    /// Include the per-decision-point complexity breakdown (`contributions[]`) on
    /// each complexity finding. Each entry names the construct (if, else-if,
    /// Svelte await/then, loop, boolean operator, React hook density, wide prop
    /// count, ...) and its cyclomatic/cognitive weight, so the agent can explain
    /// WHY a function scored high and which lines to refactor. JSX depth remains
    /// descriptive `react_jsx_max_depth` context, not a contribution. Forwards
    /// `--complexity-breakdown`. Off by default to keep output lean.
    pub complexity_breakdown: Option<bool>,

    /// Add a structural CSS analytics section (`css_analytics`): specificity
    /// hotspots, `!important` density, over-complex selectors, deep nesting,
    /// design-token sprawl (distinct colors / font-sizes / z-indexes), and
    /// unreferenced custom-property / `@keyframes` cleanup candidates. Forwards
    /// `--css`. Opt-in because it reads and parses project stylesheets. Standard
    /// CSS is parsed structurally; Sass/Less sources are scanned only where
    /// fallow can stay conservative without expanding preprocessor semantics.
    pub css: Option<bool>,

    /// Show only per-file health scores, sorted by risk-aware triage concern:
    /// lower maintainability index and higher CRAP risk first.
    pub file_scores: Option<bool>,

    /// Show only hotspots: files that are both complex and frequently changing.
    pub hotspots: Option<bool>,

    /// Attach ownership signals (bus factor, contributors, declared owner,
    /// drift) to hotspot entries. Implies `hotspots`. Requires git.
    pub ownership: Option<bool>,

    /// Privacy mode for author emails when `ownership` is enabled.
    /// Implies `ownership`. Defaults to `handle` server-side when omitted.
    pub ownership_email_mode: Option<EmailModeParam>,

    /// Show only refactoring targets: ranked recommendations based on complexity, coupling, churn, and dead code.
    pub targets: Option<bool>,

    /// Explicitly request static test coverage gaps: runtime files and exports with
    /// no test dependency path. A provided config file may also enable coverage
    /// gaps via `rules.coverage-gaps` when no health sections are explicitly
    /// selected.
    pub coverage_gaps: Option<bool>,

    /// Show only the project health score (0–100) with letter grade (A/B/C/D/F).
    /// Runs duplication analysis automatically; pair with `hotspots=true` (or
    /// `targets=true`) for the churn-backed hotspot penalty.
    pub score: Option<bool>,

    /// Fail if the health score is below this threshold (0–100). Implies --score.
    pub min_score: Option<f64>,

    /// Only exit with error for findings at or above this severity (moderate, high, critical).
    pub min_severity: Option<String>,

    /// Git history window for hotspot analysis. Accepts durations (6m, 90d, 1y) or ISO dates.
    /// It also sets the window for change coupling and change dispersion. When absent,
    /// hotspots read 6 months and change coupling and dispersion read 12 months.
    pub since: Option<String>,

    /// Minimum commits for a file to appear in hotspot ranking.
    pub min_commits: Option<u32>,

    /// Import change history from a `fallow-churn/v1` JSON file instead of `git
    /// log`, so `hotspots` / `ownership` / `targets` work on projects with no git
    /// repository (Yandex Arc, Mercurial, Perforce). A small wrapper translates
    /// the VCS log into the contract. Resolved relative to `root`. Powers the
    /// churn-backed signals (hotspots, ownership, refactoring targets) only; the
    /// `since` window labels the header but does not filter imported events (the
    /// file is authoritative).
    pub churn_file: Option<String>,

    /// Workspace packages to analyze: name or repo-relative path, comma-separated list, globs, `!` negation.
    pub workspace: Option<String>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Save a vital signs snapshot. Provide a file path, or omit value for default (`.fallow/snapshots/{timestamp}.json`).
    /// The path must resolve inside the project root, its Git work tree, the CI workspace or a temp directory.
    /// A relative path resolves against the working directory of the MCP
    /// server, not against `root`, so pass an absolute path.
    pub save_snapshot: Option<String>,

    /// Compare results against a saved baseline file. Only new issues (not in the baseline) are reported.
    /// `summary.baseline_staleness` reports how much of the baseline still matches: read `gate_trips`
    /// to see whether entries have gone stale (re-save with `save_baseline`), and read `change_scoped`
    /// first, because a narrowed run can report `matched_entries: 0` on a healthy baseline.
    /// `scope_reasons` names the channels that narrowed it.
    /// `unrecognised_format: true` means the file is not this command's baseline.
    /// It suppressed nothing, so use another path and do not re-save.
    /// `saved_by` then names the command that wrote it, when fallow knows it.
    pub baseline: Option<String>,

    /// Save current results as a baseline file for future comparisons. The path
    /// must resolve inside the project root, its Git work tree, the CI workspace or a temp directory.
    /// A relative path resolves against the working directory of the MCP
    /// server, not against `root`, so pass an absolute path.
    pub save_baseline: Option<String>,

    /// How `baseline` matches health findings and which buckets `save_baseline`
    /// writes: "count" (per file and category, the CLI default) or "identity"
    /// (per function identity, strict; requires a baseline saved in identity
    /// mode). When refreshing a baseline that was saved in identity mode, pass
    /// "identity" to preserve its identity buckets: a count-mode save drops
    /// them and is refused unless "count" is passed explicitly. Passed through
    /// to the CLI's `--baseline-mode` flag.
    pub baseline_mode: Option<BaselineModeParam>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Parser thread count; defaults to CPU cores.
    pub threads: Option<usize>,

    /// Enable the TypeScript semantic backend.
    /// `false` keeps the run syntactic, also when the config turns type-aware analysis on. `type_aware_projects` or `type_aware_require` turns it on and wins over `false`.
    pub type_aware: Option<bool>,

    /// Explicit tsconfig paths for type-aware analysis.
    pub type_aware_projects: Option<Vec<String>>,

    /// Whether incomplete semantic evidence is advisory or gating.
    pub type_aware_require: Option<TypeAwareRequireParam>,

    /// Include advisory project-local public-signature type coupling.
    pub type_coupling: Option<bool>,

    /// Compare current metrics against the most recent saved snapshot and show per-metric deltas.
    /// Implies --score. Reads from `.fallow/snapshots/`.
    pub trend: Option<bool>,

    /// Compare current metrics against this snapshot file instead of the newest
    /// file in `.fallow/snapshots/`. Implies `trend`. With `group_by`, each group
    /// is compared by key when the snapshot holds the same grouping. A
    /// relative path resolves against the working directory of the MCP
    /// server, so pass an absolute path. Passed through to `--trend-from`.
    pub trend_from: Option<String>,

    /// Analysis effort level. Controls the depth of analysis: "low" (fast, surface-level),
    /// "medium" (balanced, default), "high" (thorough, includes all heuristics).
    pub effort: Option<String>,

    /// Include a natural-language summary of findings alongside the structured JSON output.
    pub summary: Option<bool>,

    /// Path to coverage data for accurate per-function CRAP scores: an Istanbul
    /// map (coverage-final.json), a directory containing one, a raw V8 coverage
    /// directory (`NODE_V8_COVERAGE`), or a single V8 coverage JSON file.
    /// When omitted, `FALLOW_COVERAGE` and then the `health.coverage` config
    /// field apply, on the typed route and on the CLI fallback alike, with the
    /// same precedence as `fallow health --coverage`.
    pub coverage: Option<String>,

    /// Absolute prefix to strip from coverage data paths before prepending the project root.
    /// Use when coverage was generated in a different environment (CI runner, Docker).
    /// When omitted, `FALLOW_COVERAGE_ROOT` and then the `health.coverageRoot`
    /// config field apply, on the typed route and on the CLI fallback alike,
    /// with the same precedence as `fallow health --coverage-root`.
    pub coverage_root: Option<String>,

    /// Path to runtime coverage input. Accepts a V8 coverage directory
    /// (`NODE_V8_COVERAGE=...`), a single V8 coverage JSON file, or an
    /// Istanbul `coverage-final.json`. A single local capture is free and
    /// runs without a license; continuous or multi-capture runtime
    /// monitoring (a directory containing multiple JSON dumps) requires an
    /// active license. Run `fallow license activate --trial --email <addr>`
    /// to start a 30-day trial when you need continuous monitoring.
    /// Runtime coverage can exceed the default 120s MCP subprocess timeout
    /// on large dumps; raise `FALLOW_TIMEOUT_SECS` accordingly.
    pub runtime_coverage: Option<String>,

    /// Minimum invocation count for a function to be classified as a hot
    /// path in runtime-coverage output. Inherits the CLI default (100)
    /// when omitted. Takes effect only when `runtime_coverage` is also
    /// set; silently ignored otherwise.
    pub min_invocations_hot: Option<u64>,

    /// Minimum total trace volume before the sidecar may emit high-confidence
    /// `safe_to_delete` or `review_required` verdicts. Below this threshold,
    /// confidence is capped at `medium` to protect against overconfident
    /// verdicts on new or low-traffic services. Inherits the sidecar default
    /// (5000) when omitted. Takes effect only when `runtime_coverage` is
    /// also set; silently ignored otherwise.
    pub min_observation_volume: Option<u32>,

    /// Fraction of `trace_count` below which an invoked function is
    /// classified `low_traffic` rather than `active`. Expressed as a
    /// decimal (0.001 = 0.1%). Inherits the sidecar default (0.001) when
    /// omitted. Takes effect only when `runtime_coverage` is also set;
    /// silently ignored otherwise.
    pub low_traffic_threshold: Option<f64>,

    /// Group health findings by CODEOWNERS ownership, directory, workspace
    /// package, or GitLab CODEOWNERS section. Values: "owner", "directory",
    /// "package", "section". `section` attaches an `owners: string[]` array
    /// to each group. Passed through to the CLI's `--group-by` flag.
    pub group_by: Option<String>,

    /// Keep only the matching groups of a `group_by` run: exact group keys,
    /// globs, and `!`-prefixed negations, comma-separated. Project-level
    /// sections are not filtered. Requires `group_by`. Passed through to the
    /// CLI's `--group` flag.
    pub group: Option<String>,
}
