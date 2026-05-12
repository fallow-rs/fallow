use schemars::JsonSchema;
use serde::Deserialize;

/// Privacy mode for author emails emitted by the `--ownership` health flag.
///
/// Mirrors `fallow_config::EmailMode` but lives in the MCP crate so the JSON
/// Schema published to agents lists the exact set of accepted values and
/// rejects typos at the protocol layer instead of the CLI subprocess.
#[derive(Clone, Copy, Debug, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EmailModeParam {
    /// Show full email addresses as recorded in git history.
    Raw,
    /// Show local-part only (default). Unwraps GitHub-style noreply prefixes.
    Handle,
    /// Show stable non-cryptographic pseudonyms (`xxh3:<hex>`).
    Hash,
}

impl EmailModeParam {
    /// Render as the corresponding CLI flag value (`raw`, `handle`, `hash`).
    pub const fn as_cli(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Handle => "handle",
            Self::Hash => "hash",
        }
    }
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct AnalyzeParams {
    /// Root directory of the project to analyze. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml).
    pub config: Option<String>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Scope analysis to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation
    /// (e.g. `"web,admin"`, `"apps/*"`, `"apps/*,!apps/legacy"`). Patterns match
    /// against both the package name and the workspace path relative to the repo
    /// root. Passed through to the CLI's `--workspace` flag.
    pub workspace: Option<String>,

    /// Issue types to include. When set, only these types are reported.
    /// Valid values: unused-files, unused-exports, unused-types,
    /// private-type-leaks, unused-deps, unused-enum-members, unused-class-members, unresolved-imports,
    /// unlisted-deps, duplicate-exports, circular-deps, boundary-violations,
    /// stale-suppressions, unused-catalog-entries (catalog declares packages no
    /// consumer references; dead config), unresolved-catalog-references
    /// (consumer references catalogs that do not declare the package; broken
    /// config that pnpm install will reject), unused-dependency-overrides
    /// (pnpm.overrides forces a version no workspace package depends on; may be
    /// an intentional transitive CVE pin), misconfigured-dependency-overrides
    /// (pnpm.overrides key/value is unparsable; pnpm install will reject).
    pub issue_types: Option<Vec<String>>,

    #[schemars(
        description = "Set to true to check only boundary violations. Convenience alias for issue_types: [\"boundary-violations\"]"
    )]
    pub boundary_violations: Option<bool>,

    /// Compare results against a saved baseline file. Only new issues (not in the baseline) are reported.
    pub baseline: Option<String>,

    /// Save current results as a baseline file for future comparisons.
    pub save_baseline: Option<String>,

    /// Fail if issue counts regressed compared to the regression baseline.
    pub fail_on_regression: Option<bool>,

    /// Regression tolerance. Accepts a percentage ("2%") or absolute count ("5").
    pub tolerance: Option<String>,

    /// Path to a regression baseline file to compare against.
    pub regression_baseline: Option<String>,

    /// Save current results as a regression baseline file for future comparisons.
    pub save_regression_baseline: Option<String>,

    /// Group results by CODEOWNERS ownership, directory, workspace package, or
    /// GitLab CODEOWNERS section. Values: "owner", "directory", "package",
    /// "section". The `section` mode produces distinct groups per `[Section]`
    /// header even when sections share a default reviewer, and attaches an
    /// `owners: string[]` array to each group in the JSON output (populated
    /// from the section's default owners). The `owners` field is absent for
    /// Owner/Directory/Package modes.
    pub group_by: Option<String>,

    /// Only report issues in the specified file(s). Useful for lint-staged pre-commit hooks.
    /// Dependency-level issues are suppressed in file mode.
    pub file: Option<Vec<String>>,

    /// Report unused exports in entry files instead of auto-marking them as used.
    /// Catches typos in framework exports (e.g., `meatdata` instead of `metadata`).
    pub include_entry_exports: Option<bool>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct CheckChangedParams {
    /// Root directory of the project to analyze. Defaults to current working directory.
    pub root: Option<String>,

    /// Git ref to compare against (e.g., "main", "HEAD~5", a commit SHA).
    /// Only files changed since this ref are reported.
    pub since: String,

    /// Path to fallow config file.
    pub config: Option<String>,

    /// Only analyze production code.
    pub production: Option<bool>,

    /// Scope analysis to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation
    /// (e.g. `"web,admin"`, `"apps/*"`, `"apps/*,!apps/legacy"`). Patterns match
    /// against both the package name and the workspace path relative to the repo
    /// root. Passed through to the CLI's `--workspace` flag.
    pub workspace: Option<String>,

    /// Compare results against a saved baseline file. Only new issues (not in the baseline) are reported.
    pub baseline: Option<String>,

    /// Save current results as a baseline file for future comparisons.
    pub save_baseline: Option<String>,

    /// Fail if issue counts regressed compared to the regression baseline.
    pub fail_on_regression: Option<bool>,

    /// Regression tolerance. Accepts a percentage ("2%") or absolute count ("5").
    pub tolerance: Option<String>,

    /// Path to a regression baseline file to compare against.
    pub regression_baseline: Option<String>,

    /// Save current results as a regression baseline file for future comparisons.
    pub save_regression_baseline: Option<String>,

    /// Report unused exports in entry files instead of auto-marking them as used.
    pub include_entry_exports: Option<bool>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct FindDupesParams {
    /// Root directory of the project to analyze. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml).
    pub config: Option<String>,

    /// Scope analysis to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation
    /// (e.g. `"web,admin"`, `"apps/*"`, `"apps/*,!apps/legacy"`). Patterns match
    /// against both the package name and the workspace path relative to the repo
    /// root. Passed through to the CLI's `--workspace` flag.
    pub workspace: Option<String>,

    /// Detection mode: "strict" (exact tokens), "mild" (normalized identifiers),
    /// "weak" (structural only), or "semantic" (type-aware). Defaults to "mild".
    pub mode: Option<String>,

    /// Minimum token count for a clone to be reported. Default: 50.
    pub min_tokens: Option<u32>,

    /// Minimum line count for a clone to be reported. Default: 5.
    pub min_lines: Option<u32>,

    /// Minimum number of occurrences before a clone group is reported.
    /// Increase to focus on widespread copy-paste worth refactoring and skip
    /// 2-instance noise. Must be at least 2. Default: 2.
    #[schemars(range(min = 2))]
    pub min_occurrences: Option<u32>,

    /// Fail if duplication percentage exceeds this value. 0 = no limit.
    pub threshold: Option<f64>,

    /// Skip file-local duplicates, only report cross-file clones.
    pub skip_local: Option<bool>,

    /// Enable cross-language detection (strip TS type annotations for TS<->JS matching).
    pub cross_language: Option<bool>,

    /// Exclude import declarations from clone detection (reduces noise from sorted import blocks).
    pub ignore_imports: Option<bool>,

    /// Show a per-pattern breakdown for default duplicates ignores.
    /// Human-format only (human/markdown CLI output); MCP JSON responses suppress the note.
    pub explain_skipped: Option<bool>,

    /// Show only the N largest clone groups.
    pub top: Option<usize>,

    /// Compare results against a saved baseline file. Only new issues (not in the baseline) are reported.
    pub baseline: Option<String>,

    /// Save current results as a baseline file for future comparisons.
    pub save_baseline: Option<String>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,

    #[schemars(
        description = "Only report issues in files changed since this git ref (branch, tag, or commit SHA)"
    )]
    pub changed_since: Option<String>,

    /// Group clone families by CODEOWNERS ownership, directory, workspace
    /// package, or GitLab CODEOWNERS section. Values: "owner", "directory",
    /// "package", "section". `section` attaches an `owners: string[]` array
    /// to each group. Passed through to the CLI's `--group-by` flag.
    pub group_by: Option<String>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct FixParams {
    /// Root directory of the project. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file.
    pub config: Option<String>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

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
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct ProjectInfoParams {
    /// Root directory of the project. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file.
    pub config: Option<String>,

    #[schemars(description = "Show detected entry points")]
    pub entry_points: Option<bool>,
    #[schemars(description = "Show all discovered source files")]
    pub files: Option<bool>,
    #[schemars(description = "Show active framework plugins")]
    pub plugins: Option<bool>,
    #[schemars(description = "Show architecture boundary zones, rules, and per-zone file counts")]
    pub boundaries: Option<bool>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TraceExportParams {
    /// File containing the export to trace, relative to the project root.
    #[schemars(length(min = 1))]
    pub file: String,

    /// Export name to trace (use "default" for default exports).
    #[schemars(length(min = 1))]
    pub export_name: String,

    /// Root directory of the project. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file.
    pub config: Option<String>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Scope analysis to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation.
    pub workspace: Option<String>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TraceFileParams {
    /// File to trace, relative to the project root.
    #[schemars(length(min = 1))]
    pub file: String,

    /// Root directory of the project. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file.
    pub config: Option<String>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Scope analysis to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation.
    pub workspace: Option<String>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TraceDependencyParams {
    /// Package name to trace (for example "react" or "@scope/pkg").
    #[schemars(length(min = 1))]
    pub package_name: String,

    /// Root directory of the project. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file.
    pub config: Option<String>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Scope analysis to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation.
    pub workspace: Option<String>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
pub struct TraceCloneParams {
    /// File containing the clone candidate, relative to the project root.
    #[schemars(length(min = 1))]
    pub file: String,

    /// 1-based line number inside the clone candidate.
    #[schemars(range(min = 1))]
    pub line: usize,

    /// Root directory of the project. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml).
    pub config: Option<String>,

    /// Scope analysis to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation.
    pub workspace: Option<String>,

    /// Detection mode: "strict" (exact tokens), "mild" (normalized identifiers),
    /// "weak" (structural only), or "semantic" (type-aware). Defaults to "mild".
    pub mode: Option<String>,

    /// Minimum token count for a clone to be reported. Default: 50.
    pub min_tokens: Option<u32>,

    /// Minimum line count for a clone to be reported. Default: 5.
    pub min_lines: Option<u32>,

    /// Minimum number of occurrences before a clone group is reported.
    /// Increase to focus on widespread copy-paste worth refactoring and skip
    /// 2-instance noise. Must be at least 2. Default: 2.
    #[schemars(range(min = 2))]
    pub min_occurrences: Option<u32>,

    /// Fail if duplication percentage exceeds this value. 0 = no limit.
    pub threshold: Option<f64>,

    /// Skip file-local duplicates, only report cross-file clones.
    pub skip_local: Option<bool>,

    /// Enable cross-language detection (strip TS type annotations for TS<->JS matching).
    pub cross_language: Option<bool>,

    /// Exclude import declarations from clone detection (reduces noise from sorted import blocks).
    pub ignore_imports: Option<bool>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct HealthParams {
    /// Root directory of the project to analyze. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml).
    pub config: Option<String>,

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

    /// Show only per-file health scores (fan-in, fan-out, dead code ratio, maintainability index).
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

    /// Git ref to compare against (e.g., "main", "HEAD~5").
    /// Auto-detects the default branch if not specified.
    pub base: Option<String>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Run only the dead-code sub-analysis in production mode.
    pub production_dead_code: Option<bool>,

    /// Run only the health sub-analysis in production mode.
    pub production_health: Option<bool>,

    /// Run only the duplication sub-analysis in production mode.
    pub production_dupes: Option<bool>,

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
    #[schemars(description = "Project root directory (defaults to current working directory)")]
    pub root: Option<String>,
    #[schemars(description = "Path to a fallow config file")]
    pub config: Option<String>,
    #[schemars(description = "Disable the incremental parse cache")]
    pub no_cache: Option<bool>,
    #[schemars(description = "Number of threads for file parsing (defaults to CPU core count)")]
    pub threads: Option<usize>,
}

#[derive(Default, Deserialize, JsonSchema)]
pub struct FeatureFlagsParams {
    /// Root directory of the project to analyze. Defaults to current working directory.
    pub root: Option<String>,

    /// Path to fallow config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml).
    pub config: Option<String>,

    /// Only analyze production code (excludes tests, stories, dev files).
    pub production: Option<bool>,

    /// Scope analysis to one or more workspaces. Accepts a single package name
    /// for the common case, or a comma-separated list with globs and `!` negation
    /// (e.g. `"web,admin"`, `"apps/*"`, `"apps/*,!apps/legacy"`). Patterns match
    /// against both the package name and the workspace path relative to the repo
    /// root. Passed through to the CLI's `--workspace` flag.
    pub workspace: Option<String>,

    /// Filter by flag type: "environment_variable", "sdk_call", or "config_object".
    #[expect(
        dead_code,
        reason = "exposed via JSON Schema for agent discovery; CLI filter pending"
    )]
    pub flag_type: Option<String>,

    /// Filter by detection confidence: "high", "medium", or "low".
    #[expect(
        dead_code,
        reason = "exposed via JSON Schema for agent discovery; CLI filter pending"
    )]
    pub confidence: Option<String>,

    /// Disable the incremental parse cache. Forces a full re-parse of all files.
    pub no_cache: Option<bool>,

    /// Number of parser threads. Defaults to available CPU cores.
    pub threads: Option<usize>,
}
