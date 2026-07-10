//! MCP tool parameter structs.
//!
//! Doc comments feed both rustdoc and the published JSON Schema.

use schemars::JsonSchema;
use serde::Deserialize;

/// Privacy mode for author emails emitted by `--ownership`.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EmailModeParam {
    Raw,
    Handle,
    Anonymized,
    Hash,
}

impl EmailModeParam {
    pub const fn as_cli(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Handle => "handle",
            Self::Anonymized => "anonymized",
            Self::Hash => "hash",
        }
    }
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct AnalyzeParams {
    pub root: Option<String>,

    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    pub production: Option<bool>,

    pub workspace: Option<String>,

    pub issue_types: Option<Vec<String>>,

    /// Analyze only architecture boundary violations.
    pub boundary_violations: Option<bool>,

    pub baseline: Option<String>,

    pub save_baseline: Option<String>,

    pub fail_on_regression: Option<bool>,

    pub tolerance: Option<String>,

    pub regression_baseline: Option<String>,

    pub save_regression_baseline: Option<String>,

    pub group_by: Option<String>,

    pub file: Option<Vec<String>>,

    pub include_entry_exports: Option<bool>,

    pub no_cache: Option<bool>,

    pub threads: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct CombinedParams {
    pub root: Option<String>,

    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    pub production: Option<bool>,

    pub workspace: Option<String>,

    /// Git ref to compare against when limiting all combined sections to
    /// changed files.
    pub changed_since: Option<String>,

    pub include_entry_exports: Option<bool>,

    pub no_cache: Option<bool>,

    pub threads: Option<usize>,

    pub dupes_mode: Option<String>,

    pub dupes_min_tokens: Option<u32>,

    pub dupes_min_lines: Option<u32>,

    #[schemars(range(min = 2))]
    pub dupes_min_occurrences: Option<u32>,

    pub dupes_threshold: Option<f64>,

    pub dupes_skip_local: Option<bool>,

    pub dupes_cross_language: Option<bool>,

    /// Exclude import declarations from clone detection. Defaults to the
    /// project config.
    pub dupes_ignore_imports: Option<bool>,

    /// Include only complexity findings in the health section.
    pub complexity: Option<bool>,

    /// Include per-file scores in the health section.
    pub file_scores: Option<bool>,

    /// Include project health score in the health section.
    pub score: Option<bool>,

    /// Include refactoring targets in the health section.
    pub targets: Option<bool>,

    /// Include hotspots in the health section.
    pub hotspots: Option<bool>,

    /// Maximum cyclomatic complexity threshold.
    pub max_cyclomatic: Option<u16>,

    /// Maximum cognitive complexity threshold.
    pub max_cognitive: Option<u16>,

    /// Maximum CRAP score threshold.
    pub max_crap: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CheckChangedParams {
    pub root: Option<String>,

    pub since: String,

    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    pub production: Option<bool>,

    pub workspace: Option<String>,

    pub baseline: Option<String>,

    pub save_baseline: Option<String>,

    pub fail_on_regression: Option<bool>,

    pub tolerance: Option<String>,

    pub regression_baseline: Option<String>,

    pub save_regression_baseline: Option<String>,

    pub include_entry_exports: Option<bool>,

    pub no_cache: Option<bool>,

    pub threads: Option<usize>,
}

/// Parameters for the `security_candidates` tool.
///
/// Security analysis can exceed the default MCP subprocess timeout on large
/// repos. Raise `FALLOW_TIMEOUT_SECS` in the server environment when needed.
#[derive(Default, Deserialize, JsonSchema)]
pub struct SecurityCandidatesParams {
    pub root: Option<String>,

    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Scope candidates to selected workspace roots. Mutually exclusive with
    /// `changed_workspaces`.
    pub workspace: Option<String>,

    /// Git ref to compare against when limiting candidates to changed files.
    pub changed_since: Option<String>,

    /// Scope candidates to just-edited files for the agent edit loop. Each path
    /// is passed to `fallow security --file` and matches finding anchors or
    /// trace hops.
    pub paths: Option<Vec<String>>,

    /// Scope candidates to workspaces touched since this git ref. Mutually
    /// exclusive with `workspace`.
    pub changed_workspaces: Option<String>,

    /// Include the attack-surface inventory with defensive-boundary verification
    /// prompts in the JSON response.
    pub surface: Option<bool>,

    /// Optional security regression gate. Valid values: "new" or
    /// "newly-reachable". The "newly-reachable" gate requires `changed_since`.
    pub gate: Option<String>,

    pub no_cache: Option<bool>,

    pub threads: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct FindDupesParams {
    pub root: Option<String>,

    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    pub workspace: Option<String>,

    pub mode: Option<String>,

    pub min_tokens: Option<u32>,

    pub min_lines: Option<u32>,

    #[schemars(range(min = 2))]
    pub min_occurrences: Option<u32>,

    pub threshold: Option<f64>,

    pub skip_local: Option<bool>,

    pub cross_language: Option<bool>,

    /// Exclude import declarations from clone detection. Defaults to `true`
    /// (sorted import blocks are a formatting artifact, not copy-paste); set
    /// `false` to count them again.
    pub ignore_imports: Option<bool>,

    pub explain_skipped: Option<bool>,

    pub top: Option<usize>,

    pub baseline: Option<String>,

    pub save_baseline: Option<String>,

    pub no_cache: Option<bool>,

    pub threads: Option<usize>,

    /// Git ref to compare against when limiting duplication to changed files.
    pub changed_since: Option<String>,

    pub group_by: Option<String>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct FixParams {
    pub root: Option<String>,

    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    pub production: Option<bool>,

    pub workspace: Option<String>,

    pub no_create_config: Option<bool>,

    pub no_cache: Option<bool>,

    pub threads: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct ProjectInfoParams {
    pub root: Option<String>,

    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Include entry-point patterns in the response.
    pub entry_points: Option<bool>,

    /// Include discovered source files in the response.
    pub files: Option<bool>,

    /// Include active framework plugins in the response.
    pub plugins: Option<bool>,

    /// Include discovered architecture boundary zones in the response.
    pub boundaries: Option<bool>,

    pub no_cache: Option<bool>,

    pub threads: Option<usize>,
}

/// Parameters for the `inspect_target` tool.
///
/// The tool composes several existing read-only analyses into one evidence
/// bundle. Large repositories can exceed the default MCP subprocess timeout;
/// raise `FALLOW_TIMEOUT_SECS` in the server environment when needed.
#[derive(Deserialize, JsonSchema)]
pub struct InspectTargetParams {
    pub target: InspectTarget,

    pub root: Option<String>,

    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    pub production: Option<bool>,

    pub workspace: Option<String>,

    pub no_cache: Option<bool>,

    pub threads: Option<usize>,

    /// OPT-IN (default off): also attach the best-effort symbol-level call chain
    /// (`fallow trace`) as the `symbol_chain` evidence section. Only
    /// meaningful for a SYMBOL target. Best-effort, syntactic (ADR-001), and
    /// EXPLICITLY OFF the ranked path: resolved-vs-unresolved callees are
    /// reported honestly, the chain is never folded into the ranked brief and is
    /// never a focus-map / ranking input.
    pub symbol_chain: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct GuardParams {
    /// Files to report on, project-root-relative. They do not need to exist yet.
    #[schemars(length(min = 1))]
    pub files: Vec<String>,

    /// Project root. Defaults to the MCP server's working directory.
    pub root: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InspectTarget {
    File {
        #[schemars(length(min = 1))]
        file: String,
    },
    Symbol {
        #[schemars(length(min = 1))]
        file: String,
        #[schemars(length(min = 1))]
        export_name: String,
    },
}

#[derive(Deserialize, JsonSchema)]
pub struct TraceExportParams {
    #[schemars(length(min = 1))]
    pub file: String,

    #[schemars(length(min = 1))]
    pub export_name: String,

    pub root: Option<String>,

    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    pub production: Option<bool>,

    pub workspace: Option<String>,

    pub no_cache: Option<bool>,

    pub threads: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TraceFileParams {
    #[schemars(length(min = 1))]
    pub file: String,

    pub root: Option<String>,

    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    pub production: Option<bool>,

    pub workspace: Option<String>,

    pub no_cache: Option<bool>,

    pub threads: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ImpactClosureParams {
    #[schemars(length(min = 1))]
    pub path: String,

    pub root: Option<String>,

    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    pub production: Option<bool>,

    pub workspace: Option<String>,

    pub no_cache: Option<bool>,

    pub threads: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TraceDependencyParams {
    #[schemars(length(min = 1))]
    pub package_name: String,

    pub root: Option<String>,

    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    pub production: Option<bool>,

    pub workspace: Option<String>,

    pub no_cache: Option<bool>,

    pub threads: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct TraceCloneParams {
    #[serde(default)]
    pub file: Option<String>,

    #[serde(default)]
    pub line: Option<usize>,

    #[serde(default)]
    pub fingerprint: Option<String>,

    pub root: Option<String>,

    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    pub workspace: Option<String>,

    pub mode: Option<String>,

    pub min_tokens: Option<u32>,

    pub min_lines: Option<u32>,

    #[schemars(range(min = 2))]
    pub min_occurrences: Option<u32>,

    pub threshold: Option<f64>,

    pub skip_local: Option<bool>,

    pub cross_language: Option<bool>,

    /// Exclude import declarations from clone detection. Defaults to `true`
    /// (sorted import blocks are a formatting artifact, not copy-paste); set
    /// `false` to count them again.
    pub ignore_imports: Option<bool>,

    pub no_cache: Option<bool>,

    pub threads: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct HealthParams {
    /// Root directory of the project to analyze. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml).
    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
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
    /// loop, boolean operator, React hook density, wide prop count, ...) and its
    /// cyclomatic/cognitive weight, so the agent can explain WHY a function
    /// scored high and which lines to refactor. JSX depth remains descriptive
    /// `react_jsx_max_depth` context, not a contribution. Forwards
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

    /// Scope output to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation
    /// (e.g. `"web,admin"`, `"apps/*"`, `"apps/*,!apps/legacy"`). Patterns match
    /// against both the package name and the workspace path relative to the repo
    /// root. Passed through to the CLI's `--workspace` flag.
    pub workspace: Option<String>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Save a vital signs snapshot. Provide a file path, or omit value for default (`.fallow/snapshots/{timestamp}.json`).
    pub save_snapshot: Option<String>,

    /// Compare results against a saved baseline file. Only new issues (not in the baseline) are reported.
    pub baseline: Option<String>,

    /// Save current results as a baseline file for future comparisons.
    pub save_baseline: Option<String>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,

    /// Compare current metrics against the most recent saved snapshot and show per-metric deltas.
    /// Implies --score. Reads from `.fallow/snapshots/`.
    pub trend: Option<bool>,

    /// Analysis effort level. Controls the depth of analysis: "low" (fast, surface-level),
    /// "medium" (balanced, default), "high" (thorough, includes all heuristics).
    pub effort: Option<String>,

    /// Include a natural-language summary of findings alongside the structured JSON output.
    pub summary: Option<bool>,

    /// Path to Istanbul-format coverage data (coverage-final.json) for accurate per-function CRAP scores.
    /// Accepts a file path or a directory containing coverage-final.json.
    pub coverage: Option<String>,

    /// Absolute prefix to strip from coverage data paths before prepending the project root.
    /// Use when coverage was generated in a different environment (CI runner, Docker).
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
}

/// Parameters for `check_runtime_coverage`, the focused runtime-coverage
/// entry point. A thin wrapper around `fallow health --runtime-coverage
/// <path>` with a narrow surface area so agents can pick the right tool
/// by name and pass exactly the knobs that apply to runtime coverage. A
/// single local capture is free and runs without a license; continuous or
/// multi-capture runtime monitoring (a directory containing multiple JSON
/// dumps) requires an active license JWT (`fallow license activate
/// --trial --email <addr>` to start a 30-day trial). Long V8 dumps can
/// exceed the default 120s MCP subprocess timeout; raise
/// `FALLOW_TIMEOUT_SECS` for multi-megabyte inputs.
#[derive(Deserialize, JsonSchema)]
pub struct CheckRuntimeCoverageParams {
    /// Path to runtime coverage input. Accepts a V8 coverage directory
    /// (`NODE_V8_COVERAGE=<dir>`), a single V8 coverage JSON file, or an
    /// Istanbul `coverage-final.json`. Required.
    pub coverage: String,

    /// Root directory of the project to analyze. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml).
    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Scope analysis to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation
    /// (e.g. `"web,admin"`, `"apps/*"`, `"apps/*,!apps/legacy"`). Patterns match
    /// against both the package name and the workspace path relative to the repo
    /// root. Passed through to the CLI's `--workspace` flag.
    pub workspace: Option<String>,

    /// Minimum invocation count for a function to be classified as a hot
    /// path. Inherits the CLI default (100) when omitted.
    pub min_invocations_hot: Option<u64>,

    /// Minimum total trace volume before the sidecar may emit high-confidence
    /// `safe_to_delete` or `review_required` verdicts. Below this threshold,
    /// confidence is capped at `medium` to protect against overconfident
    /// verdicts on new or low-traffic services. Inherits the sidecar default
    /// (5000) when omitted.
    pub min_observation_volume: Option<u32>,

    /// Fraction of `trace_count` below which an invoked function is
    /// classified `low_traffic` rather than `active`. Expressed as a
    /// decimal (0.001 = 0.1%). Inherits the sidecar default (0.001) when
    /// omitted.
    pub low_traffic_threshold: Option<f64>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,

    /// Maximum CRAP score threshold (default 30.0). Functions meeting or
    /// exceeding this score appear as findings alongside complexity violations.
    /// Production V8 coverage yields the most accurate per-function CRAP
    /// inputs, making this flag especially useful on this tool.
    pub max_crap: Option<f64>,

    /// Show only the top N runtime findings, hot paths, file scores, and
    /// refactoring targets. Passed through to the CLI's `--top` flag.
    pub top: Option<usize>,

    /// Group health findings by CODEOWNERS ownership, directory, workspace
    /// package, or GitLab CODEOWNERS section. Values: "owner", "directory",
    /// "package", "section". `section` attaches an `owners: string[]` array
    /// to each group. Passed through to the CLI's `--group-by` flag.
    pub group_by: Option<String>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct AuditParams {
    /// Root directory of the project to analyze. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml).
    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Git ref to compare against (e.g., "main", "HEAD~5"). When unset, the
    /// base is the git merge-base against the branch's upstream or the remote
    /// default (`origin/main`); set `FALLOW_AUDIT_BASE` in the server env to pin
    /// it.
    pub base: Option<String>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Run only the dead-code sub-analysis in production mode.
    pub production_dead_code: Option<bool>,

    /// Run only the health sub-analysis in production mode.
    pub production_health: Option<bool>,

    /// Run only the duplication sub-analysis in production mode.
    pub production_dupes: Option<bool>,

    /// Enable or disable styling analytics in audit. Defaults to enabled.
    pub css: Option<bool>,

    /// Enable or disable deep CSS analysis for audit: project-wide styling
    /// reachability and near-duplicate theme-token candidates, narrowed back to
    /// changed anchors. Defaults to enabled; set `false` to pass
    /// `--no-css-deep` on the CLI fallback path.
    pub css_deep: Option<bool>,

    /// Scope analysis to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation
    /// (e.g. `"web,admin"`, `"apps/*"`, `"apps/*,!apps/legacy"`). Patterns match
    /// against both the package name and the workspace path relative to the repo
    /// root. Passed through to the CLI's `--workspace` flag.
    pub workspace: Option<String>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,

    /// Group audit findings by CODEOWNERS ownership, directory, workspace
    /// package, or GitLab CODEOWNERS section. Values: "owner", "directory",
    /// "package", "section". `section` attaches an `owners: string[]` array
    /// to each group in the JSON output. Passed through to the CLI's
    /// `--group-by` flag; propagates to all three sub-analyses (dead-code,
    /// dupes, health) that audit runs.
    pub group_by: Option<String>,

    /// Which findings affect the audit verdict. Values: "new-only" (default)
    /// or "all". Passed through to the CLI's `--gate` flag.
    pub gate: Option<String>,

    /// Path to a dead-code baseline file (produced by `fallow dead-code
    /// --save-baseline`). When set, dead-code issues present in the
    /// baseline are excluded from the audit verdict. Passed through to
    /// the CLI's `--dead-code-baseline` flag.
    pub dead_code_baseline: Option<String>,

    /// Path to a health baseline file (produced by `fallow health
    /// --save-baseline`). When set, complexity findings present in the
    /// baseline are excluded from the audit verdict. Passed through to
    /// the CLI's `--health-baseline` flag.
    pub health_baseline: Option<String>,

    /// Path to a duplication baseline file (produced by `fallow dupes
    /// --save-baseline`). When set, clone groups present in the baseline
    /// are excluded from the audit verdict. Passed through to the CLI's
    /// `--dupes-baseline` flag.
    pub dupes_baseline: Option<String>,

    /// Show a per-pattern breakdown for default duplicates ignores.
    /// Human-format only (human/markdown CLI output); MCP JSON responses suppress the note.
    pub explain_skipped: Option<bool>,

    /// Maximum CRAP score threshold (default 30.0). Functions meeting or
    /// exceeding this score cause audit to fail. Pair with `coverage` on
    /// `check_health` for accurate per-function CRAP; without Istanbul data
    /// fallow estimates coverage from the module graph. Passed through to
    /// the CLI's `--max-crap` flag.
    pub max_crap: Option<f64>,

    /// Path to Istanbul-format coverage data (coverage-final.json) for
    /// accurate per-function CRAP scores in audit's health sub-analysis.
    /// Passed through to the CLI's `--coverage` flag.
    pub coverage: Option<String>,

    /// Absolute prefix to strip from coverage data paths before CRAP matching.
    /// Use when coverage was generated in a different checkout root in CI or Docker.
    /// Passed through to the CLI's `--coverage-root` flag.
    pub coverage_root: Option<String>,

    /// Report unused exports in entry files instead of auto-marking them as
    /// used. Catches typos in framework exports (e.g. `meatdata` instead of
    /// `metadata`). Also configurable persistently via
    /// `includeEntryExports: true` in the fallow config file; this param
    /// ORs with the config value. Passed through to the CLI's
    /// `--include-entry-exports` flag.
    pub include_entry_exports: Option<bool>,

    /// Paid runtime-coverage sidecar input (V8 directory, V8 JSON, or
    /// Istanbul coverage map JSON). When set, audit folds runtime-coverage
    /// findings into the same invocation: agents calling `audit` get the
    /// `hot-path-touched` verdict alongside dead-code and complexity in
    /// one MCP call instead of orchestrating a second
    /// `check_runtime_coverage` step. License-gated; the verdict is
    /// informational. Passed through to the CLI's `--runtime-coverage`
    /// flag.
    pub runtime_coverage: Option<String>,

    /// Threshold for hot-path classification (default 100). Forwarded to
    /// the sidecar when `runtime_coverage` is set. Passed through to the
    /// CLI's `--min-invocations-hot` flag.
    pub min_invocations_hot: Option<u64>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct ExplainParams {
    /// Issue type or rule id to explain, for example "unused-export",
    /// "fallow/unused-dependency", "high-complexity", or "code-duplication".
    pub issue_type: String,
}

/// Parameters for `list_boundaries`.
#[derive(Debug, Default, serde::Deserialize, schemars::JsonSchema)]
pub struct ListBoundariesParams {
    /// Project root directory (defaults to current working directory).
    pub root: Option<String>,

    /// Path to a fallow config file.
    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Disable the incremental parse cache.
    pub no_cache: Option<bool>,

    /// Number of threads for file parsing (defaults to CPU core count).
    pub threads: Option<usize>,
}

/// Parameters for the `recommend` config-recommendation tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct RecommendParams {
    /// Project root directory to inspect for framework, workspace, and tooling
    /// detection. `recommend` runs detection only (no config load, no analysis
    /// pipeline), so this is the sole parameter. Defaults to the current
    /// working directory.
    pub root: Option<String>,
}

/// Parameters for the `impact` value-report tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ImpactParams {
    /// Project root directory whose local value report to read. History is
    /// stored per-project in the user's private config dir (never inside the
    /// repo). Defaults to the current working directory.
    pub root: Option<String>,
}

/// Parameters for the `impact_all` cross-repo value-report tool.
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ImpactAllParams {
    /// Row ordering: `recent` (default, most recently recorded project first),
    /// `resolved` (most findings resolved first), `contained` (most commits
    /// contained first), or `name` (alphabetical by project label).
    pub sort: Option<String>,

    /// Cap the number of project rows returned. Grand totals still reflect
    /// every tracked project, including any beyond the cap. Omit for all rows.
    pub limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CodeExecuteParams {
    /// JavaScript function expression or function body. The function receives
    /// `{ fallow, root }` and must return a JSON-serializable value.
    #[schemars(length(min = 1, max = 20000))]
    pub code: String,

    /// Default project root injected into fallow host calls when their params
    /// omit `root`.
    pub root: Option<String>,

    /// Overall sandbox timeout in milliseconds. Defaults to 5000 and is capped
    /// at 30000.
    #[schemars(range(min = 1, max = 30000))]
    pub timeout_ms: Option<u64>,

    /// Maximum total bytes of fallow JSON output that sandbox host calls may
    /// read. Defaults to 1000000 and is capped at 4000000.
    #[schemars(range(min = 1024, max = 4_000_000))]
    pub max_output_bytes: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct FeatureFlagsParams {
    /// Root directory of the project to analyze. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml).
    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Scope analysis to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation
    /// (e.g. `"web,admin"`, `"apps/*"`, `"apps/*,!apps/legacy"`). Patterns match
    /// against both the package name and the workspace path relative to the repo
    /// root. Passed through to the CLI's `--workspace` flag.
    pub workspace: Option<String>,

    /// Show only the top N flags.
    pub top: Option<usize>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,
}

/// Parameters for the `list_suppressions` governance inventory tool. Wraps
/// `fallow suppressions --format json`, returning the `suppression-inventory`
/// envelope verbatim. `--changed-workspaces` is deliberately not forwarded in
/// v1 (niche; `workspace` plus `changed_since` cover the agent use cases).
#[derive(Default, Deserialize, JsonSchema)]
pub struct ListSuppressionsParams {
    /// Root directory of the project to analyze. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml).
    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Scope analysis to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation
    /// (e.g. `"web,admin"`, `"apps/*"`, `"apps/*,!apps/legacy"`). Patterns match
    /// against both the package name and the workspace path relative to the repo
    /// root. Passed through to the CLI's `--workspace` flag.
    pub workspace: Option<String>,

    /// Git ref (e.g. "main", "HEAD~5"). Scopes the inventory to files changed
    /// since the ref, the natural pull-request review scope.
    pub changed_since: Option<String>,

    /// Only list suppressions in these files. Relative paths resolve against the
    /// project root; forwarded as a repeated `--file` flag.
    pub file: Option<Vec<String>>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct DecisionSurfaceParams {
    /// Root directory of the project to analyze. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml).
    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Git ref to compare against (e.g., "main", "HEAD~5"). When unset, the
    /// base is the git merge-base against the branch's upstream or the remote
    /// default (`origin/main`); set `FALLOW_AUDIT_BASE` in the server env to pin
    /// it.
    pub base: Option<String>,

    /// Cap on the number of consequential structural decisions surfaced (the
    /// working-memory limit). Default 4; clamped to the 3-5 band (4 plus or minus
    /// 1).
    pub max_decisions: Option<usize>,

    /// Scope analysis to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation
    /// (e.g. `"web,admin"`, `"apps/*"`).
    pub workspace: Option<String>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,
}

/// Parameters for `get_token_blast_radius`, a thin wrapper around
/// `fallow health --css --format json` that steers agents to the
/// `css_analytics.token_consumers` reverse index (Tailwind v4 `@theme` tokens
/// plus CSS-in-JS `defineVars` / `createTheme`-family token definitions). Narrow
/// surface: only root/config plus the global cache knobs apply. `token_consumers`
/// abstains on partial scope, so `workspace` / `changed_since` are intentionally
/// omitted (they would only ever return empty).
#[derive(Default, Deserialize, JsonSchema)]
pub struct GetTokenBlastRadiusParams {
    /// Root directory of the project to analyze. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml).
    pub config: Option<String>,

    /// Allow trusted HTTPS config inheritance for this request.
    /// Defaults to false and never grants process-global trust.
    pub allow_remote_extends: Option<bool>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,
}
