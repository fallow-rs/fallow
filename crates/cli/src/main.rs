#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "CLI binary produces intentional terminal output"
)]

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod api;
mod audit;
mod baseline;
mod check;
mod ci;
mod ci_template;
mod codeowners;
mod combined;
mod config;
mod coverage;
mod dupes;
mod error;
mod explain;
mod fix;
mod flags;
mod health;
mod health_types;
mod init;
mod license;
mod list;
mod migrate;
mod rayon_pool;
mod regression;
mod report;
mod runtime_support;
mod schema;
mod setup_hooks;
mod validate;
mod vital_signs;
mod watch;

use check::{CheckOptions, IssueFilters, TraceOptions};
use dupes::{DupesMode, DupesOptions};
use error::emit_error;
use health::{HealthOptions, SortBy};
use list::ListOptions;
pub use runtime_support::{AnalysisKind, GroupBy};
pub(crate) use runtime_support::{build_ownership_resolver, load_config, load_config_for_analysis};

// ── CLI definition ───────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "fallow",
    about = "Codebase analyzer for TypeScript/JavaScript: unused code, circular dependencies, code duplication, complexity hotspots, and architecture boundary violations",
    version,
    after_help = "When no command is given, runs dead-code + dupes + health together.\nUse --only/--skip to select specific analyses."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Project root directory
    #[arg(short, long, global = true)]
    root: Option<PathBuf>,

    /// Path to config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml)
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    /// Output format (alias: --output)
    #[arg(
        short,
        long,
        visible_alias = "output",
        global = true,
        default_value = "human"
    )]
    format: Format,

    /// Suppress progress output
    #[arg(short, long, global = true)]
    quiet: bool,

    /// Disable incremental caching
    #[arg(long, global = true)]
    no_cache: bool,

    /// Number of parser threads
    #[arg(long, global = true)]
    threads: Option<usize>,

    /// Only report issues in files changed since this git ref (e.g., main, HEAD~5)
    #[arg(long, visible_alias = "base", global = true)]
    changed_since: Option<String>,

    /// Path to a unified diff (e.g. `git diff --unified=0 main...HEAD`) used
    /// for line-level scoping of the `hot-path-touched` runtime-coverage
    /// verdict. When supplied, a hot function is only flagged if a line in
    /// `[start_line, end_line]` was modified, instead of the file-level
    /// match `--changed-since` produces. Falls back to `FALLOW_DIFF_FILE`
    /// when the flag is omitted, so CI scripts that already export the env
    /// var keep working unchanged.
    #[arg(long = "diff-file", value_name = "PATH", global = true)]
    diff_file: Option<PathBuf>,

    /// Compare against a previously saved baseline file
    #[arg(long, global = true)]
    baseline: Option<PathBuf>,

    /// Save the current results as a baseline file
    #[arg(long, global = true)]
    save_baseline: Option<PathBuf>,

    /// Production mode: exclude test/story/dev files, only start/build scripts,
    /// report type-only dependencies
    #[arg(long, global = true)]
    production: bool,

    /// Run dead-code analysis in production mode when using bare combined mode.
    #[arg(long = "production-dead-code")]
    production_dead_code: bool,

    /// Run health analysis in production mode when using bare combined mode.
    #[arg(long = "production-health")]
    production_health: bool,

    /// Run duplication analysis in production mode when using bare combined mode.
    #[arg(long = "production-dupes")]
    production_dupes: bool,

    /// Scope output to one or more workspaces. Accepts exact package names, globs
    /// (matched against the package name AND the workspace path relative to the repo
    /// root), and `!`-prefixed negations. Values can be comma-separated or repeated.
    /// The full cross-workspace graph is still built; only issues are filtered.
    ///
    /// Examples:
    ///   -w web,admin
    ///   -w 'apps/*'
    ///   -w 'apps/*,!apps/legacy'
    ///
    /// Use single quotes around patterns with `!` or glob chars. In bash,
    /// unquoted `!` triggers history expansion and double quotes are not enough.
    #[arg(short, long, global = true, value_delimiter = ',')]
    workspace: Option<Vec<String>>,

    /// Scope output to workspaces containing any file changed since the given git ref.
    /// Auto-derives the set of touched packages in a monorepo so CI jobs don't have
    /// to maintain a hand-written workspace list. Git is required; a missing ref or
    /// non-git directory is a hard error, so failure is visible instead of quietly
    /// widening back to the full monorepo. Mutually exclusive with --workspace.
    ///
    /// Example:
    ///   fallow --changed-workspaces origin/main
    #[arg(long, global = true, value_name = "REF")]
    changed_workspaces: Option<String>,

    /// Group output by owner (.github/CODEOWNERS) or by directory (no CODEOWNERS needed).
    /// Partitions all issues into labeled sections for team-level triage and dashboards.
    #[arg(long, global = true)]
    group_by: Option<GroupBy>,

    /// Show pipeline performance timing breakdown
    #[arg(long, global = true)]
    performance: bool,

    /// Include metric definitions and rule descriptions in output.
    /// JSON: adds a `_meta` object with docs URLs, metric ranges, and interpretations.
    /// Always enabled for MCP server responses.
    #[arg(long, global = true)]
    explain: bool,

    /// Show a per-pattern breakdown for default duplicates ignores.
    /// Human and markdown output only; machine formats suppress the note.
    #[arg(long, global = true)]
    explain_skipped: bool,

    /// Show only category counts without individual items
    #[arg(long, global = true)]
    summary: bool,

    /// CI mode: equivalent to --format sarif --fail-on-issues --quiet
    #[arg(long, global = true)]
    ci: bool,

    /// Exit with code 1 if issues are found
    #[arg(long, global = true)]
    fail_on_issues: bool,

    /// Write SARIF output to a file (in addition to the primary --format output)
    #[arg(long, global = true, value_name = "PATH")]
    sarif_file: Option<PathBuf>,

    /// Fail if issue count increased beyond tolerance compared to a regression baseline.
    /// Use --save-regression-baseline to create a baseline first, then
    /// --fail-on-regression on subsequent runs to detect regressions.
    #[arg(long, global = true)]
    fail_on_regression: bool,

    /// Allowed issue count increase before a regression is flagged.
    /// Use "N%" for percentage (e.g., "2%") or "N" for absolute count (e.g., "5").
    /// Default: "0" (any increase fails). Only used with --fail-on-regression.
    #[arg(long, global = true, value_name = "TOLERANCE", default_value = "0")]
    tolerance: String,

    /// Path to the regression baseline file for --fail-on-regression.
    /// Default: .fallow/regression-baseline.json
    #[arg(long, global = true, value_name = "PATH")]
    regression_baseline: Option<PathBuf>,

    /// Save the current issue counts as a regression baseline.
    /// Without a path: writes into the config file (.fallowrc.json / .fallowrc.jsonc / fallow.toml).
    /// With a path: writes a standalone JSON file.
    #[expect(
        clippy::option_option,
        reason = "clap pattern: None=not passed, Some(None)=flag only (write to config), Some(Some(path))=write to file"
    )]
    #[arg(long, global = true, value_name = "PATH", num_args = 0..=1, default_missing_value = "")]
    save_regression_baseline: Option<Option<String>>,

    /// Run only specific analyses when no subcommand is given (comma-separated: dead-code,dupes,health)
    #[arg(long, value_delimiter = ',')]
    only: Vec<AnalysisKind>,

    /// Skip specific analyses when no subcommand is given (comma-separated: dead-code,dupes,health)
    #[arg(long, value_delimiter = ',')]
    skip: Vec<AnalysisKind>,

    /// Override duplication detection mode in combined mode.
    #[arg(long = "dupes-mode", global = true)]
    dupes_mode: Option<DupesMode>,

    /// Override duplication threshold in combined mode.
    #[arg(long = "dupes-threshold", global = true)]
    dupes_threshold: Option<f64>,

    /// Compute health score (0-100 with letter grade) in combined mode.
    /// Use with `--trend` to show score deltas in PR comments.
    #[arg(long)]
    score: bool,

    /// Compare current health metrics against the most recent saved snapshot
    /// and show per-metric deltas. Implies --score.
    #[arg(long)]
    trend: bool,

    /// Save a vital signs snapshot for trend tracking in combined mode.
    /// Provide a path or omit for the default `.fallow/snapshots/` location.
    #[expect(
        clippy::option_option,
        reason = "clap pattern: None=not passed, Some(None)=default path, Some(Some(path))=custom path"
    )]
    #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = "")]
    save_snapshot: Option<Option<String>>,

    /// Report unused exports in entry files instead of auto-marking them as used.
    /// Catches typos in framework exports (e.g., `meatdata` instead of `metadata`).
    /// Also configurable via `includeEntryExports: true` in the config file; the
    /// CLI flag wins when set.
    #[arg(long, global = true)]
    include_entry_exports: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Analyze project for unused code and circular dependencies
    #[command(name = "dead-code", alias = "check")]
    Check {
        /// Only report unused files
        #[arg(long)]
        unused_files: bool,

        /// Only report unused exports
        #[arg(long)]
        unused_exports: bool,

        /// Only report unused dependencies
        #[arg(long)]
        unused_deps: bool,

        /// Only report unused type exports
        #[arg(long)]
        unused_types: bool,

        /// Opt in to private type leak API hygiene findings and only report that issue type
        #[arg(long)]
        private_type_leaks: bool,

        /// Only report unused enum members
        #[arg(long)]
        unused_enum_members: bool,

        /// Only report unused class members
        #[arg(long)]
        unused_class_members: bool,

        /// Only report unresolved imports
        #[arg(long)]
        unresolved_imports: bool,

        /// Only report unlisted dependencies
        #[arg(long)]
        unlisted_deps: bool,

        /// Only report duplicate exports
        #[arg(long)]
        duplicate_exports: bool,

        /// Only report circular dependencies
        #[arg(long)]
        circular_deps: bool,

        /// Only report boundary violations
        #[arg(long)]
        boundary_violations: bool,

        /// Only report stale suppressions
        #[arg(long)]
        stale_suppressions: bool,

        /// Only report unused pnpm catalog entries
        #[arg(long)]
        unused_catalog_entries: bool,

        /// Also run duplication analysis and cross-reference with dead code
        #[arg(long)]
        include_dupes: bool,

        /// Trace why an export is used/unused (format: `FILE:EXPORT_NAME`)
        #[arg(long, value_name = "FILE:EXPORT")]
        trace: Option<String>,

        /// Trace all edges for a file (imports, exports, importers)
        #[arg(long, value_name = "PATH")]
        trace_file: Option<String>,

        /// Trace where a dependency is used
        #[arg(long, value_name = "PACKAGE")]
        trace_dependency: Option<String>,

        /// Show only the top N items per category
        #[arg(long)]
        top: Option<usize>,

        /// Only report issues in the specified file(s). Accepts multiple values.
        /// The full project graph is still built, but only issues in matching files
        /// are reported. Useful for lint-staged pre-commit hooks.
        #[arg(long, value_name = "PATH")]
        file: Vec<std::path::PathBuf>,
    },

    /// Watch for changes and re-run analysis
    Watch {
        /// Don't clear the screen between re-analyses
        #[arg(long)]
        no_clear: bool,
    },

    /// Auto-fix issues (remove unused exports, dependencies, enum members)
    Fix {
        /// Dry run — show what would be changed without modifying files
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation prompt (required in non-TTY environments like CI or AI agents)
        #[arg(long, alias = "force")]
        yes: bool,
    },

    /// Initialize a .fallowrc.json configuration file (optionally a git
    /// pre-commit hook). Use `.fallowrc.jsonc` for editor-native JSON-with-comments
    /// support; both extensions are auto-discovered.
    ///
    /// `--hooks` scaffolds a shell-level Git pre-commit hook under
    /// `.git/hooks/` that runs fallow on changed files. The clearer hook
    /// namespace is `fallow hooks install --target git`; `init --hooks`
    /// remains as a convenience during project initialization.
    Init {
        /// Generate TOML instead of JSONC
        #[arg(long)]
        toml: bool,

        /// Scaffold a shell-level pre-commit git hook in `.git/hooks/` that
        /// runs fallow on changed files. Alias for
        /// `fallow hooks install --target git`.
        #[arg(long)]
        hooks: bool,

        /// Fallback base branch/ref for the pre-commit hook when no upstream is set
        #[arg(long, requires = "hooks")]
        branch: Option<String>,
    },

    /// Install or remove fallow-managed Git and agent hooks.
    ///
    /// Use `fallow hooks install --target git` for a shell-level Git
    /// pre-commit hook. Use `fallow hooks install --target agent` for a
    /// Claude Code / Codex gate that blocks agent `git commit` / `git push`
    /// commands until `fallow audit` passes.
    Hooks {
        #[command(subcommand)]
        subcommand: HooksCli,
    },

    /// CI helpers for PR/MR feedback envelopes.
    Ci {
        #[command(subcommand)]
        subcommand: CiCli,
    },

    /// Print the JSON Schema for fallow configuration files
    ConfigSchema,

    /// Print the JSON Schema for external plugin files
    PluginSchema,

    /// Show the resolved config and which config file was loaded
    ///
    /// Walks up from the project root looking for `.fallowrc.json`,
    /// `.fallowrc.jsonc`, `fallow.toml`, or `.fallow.toml`, resolves `extends`, and prints
    /// the final config as JSON. Use `--path` to print only the config
    /// file path (useful in shell scripts). Exit code 0 if a config was
    /// found, 3 if only defaults are in effect.
    Config {
        /// Print only the config file path (one line, no JSON)
        #[arg(long)]
        path: bool,
    },

    /// List discovered entry points and files
    List {
        /// Show entry points
        #[arg(long)]
        entry_points: bool,

        /// Show all discovered files
        #[arg(long)]
        files: bool,

        /// Show active plugins
        #[arg(long)]
        plugins: bool,

        /// Show architecture boundary zones, rules, and per-zone file counts
        #[arg(long)]
        boundaries: bool,
    },

    /// Find code duplication / clones across the project
    Dupes {
        /// Detection mode: strict, mild, weak, or semantic
        /// (defaults to the value in `.fallowrc.jsonc`, or `mild` if unset).
        #[arg(long)]
        mode: Option<DupesMode>,

        /// Minimum token count for a clone
        /// (defaults to the value in `.fallowrc.jsonc`, or `50` if unset).
        #[arg(long)]
        min_tokens: Option<usize>,

        /// Minimum line count for a clone
        /// (defaults to the value in `.fallowrc.jsonc`, or `5` if unset).
        #[arg(long)]
        min_lines: Option<usize>,

        /// Fail if duplication exceeds this percentage (0 = no limit)
        /// (defaults to the value in `.fallowrc.jsonc`, or `0` if unset).
        #[arg(long)]
        threshold: Option<f64>,

        /// Only report cross-directory duplicates
        #[arg(long)]
        skip_local: bool,

        /// Enable cross-language detection (strip TS type annotations for TS↔JS matching)
        #[arg(long)]
        cross_language: bool,

        /// Exclude import declarations from clone detection (reduces noise from sorted import blocks)
        #[arg(long)]
        ignore_imports: bool,

        /// Show only the N largest clone groups
        #[arg(long)]
        top: Option<usize>,

        /// Trace all clones at a specific location (format: `FILE:LINE`)
        #[arg(long, value_name = "FILE:LINE")]
        trace: Option<String>,
    },

    /// Analyze function complexity (cyclomatic + cognitive)
    ///
    /// By default, shows all existing sections: health score, complexity findings,
    /// file scores, hotspots, and refactoring targets. When any section flag is
    /// specified, only those sections are shown.
    Health {
        /// Maximum cyclomatic complexity threshold (overrides config)
        #[arg(long)]
        max_cyclomatic: Option<u16>,

        /// Maximum cognitive complexity threshold (overrides config)
        #[arg(long)]
        max_cognitive: Option<u16>,

        /// Maximum CRAP score threshold (overrides config, default 30.0).
        /// Functions meeting or exceeding this score are reported alongside
        /// complexity findings. Pair with `--coverage` for accurate scoring.
        #[arg(long)]
        max_crap: Option<f64>,

        /// Show only the N most complex functions
        #[arg(long)]
        top: Option<usize>,

        /// Sort by: cyclomatic (default), cognitive, lines, or severity
        #[arg(long, default_value = "cyclomatic")]
        sort: SortBy,

        /// Show only complexity findings (functions exceeding thresholds).
        /// By default all sections are shown; use this to select only complexity.
        #[arg(long)]
        complexity: bool,

        /// Show only per-file health scores (fan-in, fan-out, dead code ratio, maintainability index).
        /// Requires full analysis pipeline (graph + dead code detection).
        /// Sorted by maintainability index ascending (worst first). --sort and --baseline
        /// apply to complexity findings only, not file scores.
        #[arg(long)]
        file_scores: bool,

        /// Show only static test coverage gaps: runtime files and exports with no
        /// dependency path from any discovered test root. Requires full analysis pipeline.
        #[arg(long)]
        coverage_gaps: bool,

        /// Show only hotspots: files that are both complex and frequently changing.
        /// Combines git churn history with complexity data. Requires a git repository.
        #[arg(long)]
        hotspots: bool,

        /// Attach ownership signals to hotspot entries: bus factor, contributor
        /// count, declared CODEOWNERS owner, and ownership drift. Implies
        /// `--hotspots`. Requires a git repository.
        #[arg(long)]
        ownership: bool,

        /// Privacy mode for author emails emitted with `--ownership`.
        /// Defaults to `handle` (local-part only). Use `raw` for OSS repos
        /// where authors are public, or `hash` to emit non-reversible
        /// pseudonyms in regulated environments. Implies `--ownership`.
        #[arg(long, value_name = "MODE", value_enum)]
        ownership_emails: Option<EmailModeArg>,

        /// Show only refactoring targets: ranked recommendations based on complexity,
        /// coupling, churn, and dead code signals. Requires full analysis pipeline.
        #[arg(long)]
        targets: bool,

        /// Filter refactoring targets by effort level (low, medium, high).
        /// Implies --targets.
        #[arg(long, value_enum)]
        effort: Option<EffortFilter>,

        /// Show only the project health score (0–100) with letter grade (A/B/C/D/F).
        /// The score is included by default when no section flags are set.
        #[arg(long)]
        score: bool,

        /// Fail if the health score is below this threshold (0–100).
        /// Implies --score. Useful as a CI quality gate.
        #[arg(long, value_name = "N")]
        min_score: Option<f64>,

        /// Only exit with error for findings at or above this severity.
        /// Use --min-severity critical to ignore moderate/high findings in CI.
        #[arg(long, value_name = "LEVEL", value_enum)]
        min_severity: Option<crate::health_types::FindingSeverity>,

        /// Git history window for hotspot analysis (default: 6m).
        /// Accepts durations (6m, 90d, 1y, 2w) or ISO dates (2025-06-01).
        #[arg(long, value_name = "DURATION")]
        since: Option<String>,

        /// Minimum number of commits for a file to be included in hotspot ranking (default: 3)
        #[arg(long, value_name = "N")]
        min_commits: Option<u32>,

        /// Save a vital signs snapshot for trend tracking.
        /// Defaults to `.fallow/snapshots/{timestamp}.json` if no path is given.
        /// Forces file-scores, hotspot, and score computation for complete metrics.
        #[expect(
            clippy::option_option,
            reason = "clap pattern: None=not passed, Some(None)=flag only, Some(Some(path))=with value"
        )]
        #[arg(long, value_name = "PATH", num_args = 0..=1, default_missing_value = "")]
        save_snapshot: Option<Option<String>>,

        /// Compare current metrics against the most recent saved snapshot.
        /// Reads from `.fallow/snapshots/` and shows per-metric deltas with
        /// directional indicators. Implies --score.
        #[arg(long)]
        trend: bool,

        /// Path to coverage data (coverage-final.json) for exact per-function
        /// CRAP scores. Generate with `jest --coverage`, `vitest run --coverage
        /// --provider istanbul`, or any Istanbul-compatible tool. Requires
        /// Istanbul format (not v8/c8 native format). Accepts a single
        /// Istanbul coverage map JSON file or a directory containing
        /// coverage-final.json. Use --coverage-root when the file was generated
        /// in a different environment (CI runner, Docker). Affects CRAP scores
        /// only, not --coverage-gaps. Also configurable via FALLOW_COVERAGE env var.
        #[arg(long, value_name = "PATH")]
        coverage: Option<PathBuf>,

        /// Absolute prefix to strip from file paths in coverage data before
        /// prepending the project root. Use when coverage was generated in a
        /// different environment (CI runner, Docker). Example: if coverage paths
        /// start with /home/runner/work/myapp and the project root is ./,
        /// pass --coverage-root /home/runner/work/myapp.
        #[arg(long, value_name = "PATH")]
        coverage_root: Option<PathBuf>,

        /// File or directory containing runtime coverage input. Accepts a
        /// V8 coverage directory, a single V8 JSON file, or a single
        /// Istanbul coverage map JSON file (commonly coverage-final.json).
        #[arg(long, value_name = "PATH")]
        runtime_coverage: Option<PathBuf>,

        /// Threshold for hot-path classification
        #[arg(long, default_value_t = 100)]
        min_invocations_hot: u64,

        /// Minimum total trace volume before the sidecar allows high-confidence
        /// `safe_to_delete` / `review_required` verdicts. Below this the
        /// sidecar caps confidence at `medium` to protect against overconfident
        /// verdicts on new or low-traffic services. Omit to use the sidecar's
        /// spec default (5000).
        #[arg(long, value_name = "N")]
        min_observation_volume: Option<u32>,

        /// Fraction of total trace count below which an invoked function is
        /// classified as `low_traffic` rather than `active`. Expressed as a
        /// decimal (e.g. `0.001` for 0.1%). Omit to use the sidecar's spec
        /// default (0.001).
        #[arg(long, value_name = "RATIO")]
        low_traffic_threshold: Option<f64>,
    },

    /// Detect feature flag patterns in the codebase
    ///
    /// Identifies environment variable flags (process.env.FEATURE_*),
    /// SDK calls (LaunchDarkly, Statsig, Unleash, GrowthBook), and
    /// config object patterns (opt-in). Reports flag locations, detection
    /// confidence, and cross-reference with dead code findings.
    Flags {
        /// Show only the top N flags
        #[arg(long)]
        top: Option<usize>,
    },

    /// Explain one fallow issue type without running an analysis.
    ///
    /// Prints the rule rationale, a worked example, fix guidance, and the
    /// relevant docs URL. Accepts values like `unused-export`,
    /// `fallow/unused-export`, `unused-exports`, and `code-duplication`.
    Explain {
        /// Issue type or rule id to explain
        issue_type: String,
    },

    /// Audit changed files for dead code, complexity, and duplication.
    ///
    /// Purpose-built for reviewing AI-generated code and PR quality gates.
    /// Combines dead-code + complexity + duplication scoped to changed files
    /// and returns a verdict (pass/warn/fail).
    /// Auto-detects the base branch if --changed-since/--base is not set.
    /// By default, only findings introduced by the changeset affect the verdict;
    /// inherited findings are reported with new-vs-inherited attribution and
    /// individual JSON findings include `introduced: true/false`. Use
    /// `--gate all` or `[audit] gate = "all"` to fail on every finding in
    /// changed files without running the extra base-snapshot attribution pass.
    ///
    /// The global --baseline / --save-baseline flags are rejected on audit.
    /// Use --dead-code-baseline, --health-baseline, and --dupes-baseline
    /// (or their config equivalents) because each sub-analysis uses a
    /// different baseline format.
    Audit {
        /// Run dead-code analysis in production mode for this audit.
        #[arg(long = "production-dead-code")]
        production_dead_code: bool,

        /// Run health analysis in production mode for this audit.
        #[arg(long = "production-health")]
        production_health: bool,

        /// Run duplication analysis in production mode for this audit.
        #[arg(long = "production-dupes")]
        production_dupes: bool,

        /// Compare dead-code issues against a saved baseline
        /// (produced by `fallow dead-code --save-baseline`).
        #[arg(long)]
        dead_code_baseline: Option<PathBuf>,

        /// Compare health findings against a saved baseline
        /// (produced by `fallow health --save-baseline`).
        #[arg(long)]
        health_baseline: Option<PathBuf>,

        /// Compare duplication clone groups against a saved baseline
        /// (produced by `fallow dupes --save-baseline`).
        #[arg(long)]
        dupes_baseline: Option<PathBuf>,

        /// Maximum CRAP score threshold (overrides config, default 30.0).
        /// Functions meeting or exceeding this score cause audit to fail.
        /// Pair with `--coverage` for accurate scoring.
        #[arg(long)]
        max_crap: Option<f64>,

        /// Path to Istanbul-format coverage data (coverage-final.json) for
        /// accurate per-function CRAP scores in the health sub-analysis. Also
        /// configurable via FALLOW_COVERAGE.
        #[arg(long, value_name = "PATH")]
        coverage: Option<PathBuf>,

        /// Absolute prefix to strip from coverage data paths before CRAP matching.
        /// Use when coverage was generated under a different checkout root in CI or Docker.
        #[arg(long, value_name = "PATH")]
        coverage_root: Option<PathBuf>,

        /// Which findings affect the audit verdict.
        ///
        /// new-only (default): fail only on findings introduced by the current
        /// changeset. all: fail on every finding in changed files and skip
        /// base-snapshot attribution.
        #[arg(long, value_enum)]
        gate: Option<AuditGateArg>,

        /// Paid runtime-coverage sidecar input. Accepts a V8 directory, a
        /// single V8 JSON file, or an Istanbul coverage map JSON. Spawns
        /// the `fallow-cov` sidecar as part of the audit pipeline so the
        /// `hot-path-touched` verdict surfaces alongside dead-code and
        /// complexity findings without requiring a second `fallow health`
        /// invocation in CI. License-gated; the verdict is informational
        /// (no exit code change) until a future `--gate hot-path-touched`
        /// knob lands.
        #[arg(long, value_name = "PATH")]
        runtime_coverage: Option<PathBuf>,

        /// Threshold for hot-path classification, forwarded to the sidecar
        /// when `--runtime-coverage` is set.
        #[arg(long, default_value_t = 100)]
        min_invocations_hot: u64,
    },

    /// Dump the CLI interface as machine-readable JSON for agent introspection
    Schema,

    /// Print or vendor CI integration templates.
    ///
    /// Use `fallow ci-template gitlab` to print the GitLab CI template, or
    /// `fallow ci-template gitlab --vendor` to write the template plus the
    /// bash helper files that enable MR comments without downloading from
    /// raw.githubusercontent.com at pipeline runtime.
    CiTemplate {
        #[command(subcommand)]
        subcommand: CiTemplateCli,
    },

    /// Migrate configuration from knip or jscpd to fallow
    Migrate {
        /// Generate `fallow.toml` instead of JSONC
        #[arg(long, conflicts_with = "jsonc")]
        toml: bool,

        /// Write JSONC content to `.fallowrc.jsonc` instead of `.fallowrc.json`. The
        /// generated content is the same JSONC (with `//` comments) either way; the
        /// `.jsonc` extension lets editors auto-detect JSON-with-comments syntax
        /// highlighting and silences linters that flag comments in `.json`. Without
        /// `--jsonc` or `--toml`, fallow auto-mirrors the source extension: a
        /// `knip.jsonc` migration writes `.fallowrc.jsonc`, a `knip.json` migration
        /// writes `.fallowrc.json`.
        #[arg(long)]
        jsonc: bool,

        /// Only preview the generated config without writing
        #[arg(long)]
        dry_run: bool,

        /// Path to source config file (auto-detect if not specified)
        #[arg(long, value_name = "PATH")]
        from: Option<PathBuf>,
    },

    /// Manage the license for continuous/cloud runtime monitoring.
    ///
    /// Verification is offline against an Ed25519 public key compiled into
    /// the binary. The license file lives at `~/.fallow/license.jwt` (or
    /// `$FALLOW_LICENSE_PATH`); `$FALLOW_LICENSE` env var takes precedence
    /// and is the recommended path for shared CI runners.
    License {
        #[command(subcommand)]
        subcommand: LicenseCli,
    },

    /// Runtime coverage workflow.
    ///
    /// `setup` is the resumable single-entry-point first-run flow: license
    /// check → sidecar install → coverage recipe → analysis. Spec:
    /// `.internal/spec-runtime-coverage-phase-2.md` (private repo).
    Coverage {
        #[command(subcommand)]
        subcommand: CoverageCli,
    },

    /// Install or remove a Claude Code PreToolUse hook that gates
    /// `git commit` / `git push` on `fallow audit`, so the agent cleans
    /// findings before the command runs.
    ///
    /// This is the legacy AGENT-level enforcement command. Prefer
    /// `fallow hooks install --target agent` for new setup. It writes into
    /// `.claude/settings.json` + `.claude/hooks/fallow-gate.sh` (and
    /// optionally an `AGENTS.md` managed block for Codex). For a
    /// shell-level Git pre-commit hook in `.git/hooks/`, see
    /// `fallow hooks install --target git` instead. Both targets can be used
    /// together: git hooks catch human commits, agent hooks catch agent
    /// commits.
    ///
    /// See `/integrations/claude-hooks` in the docs for the full recipe.
    SetupHooks {
        /// Target a specific agent surface (default: auto-detect).
        #[arg(long, value_enum)]
        agent: Option<setup_hooks::HookAgentArg>,

        /// Print what would be written or removed without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Overwrite a user-edited hook script, invalid settings.json, or
        /// remove a user-edited script during uninstall.
        #[arg(long)]
        force: bool,

        /// Write to the user's home directory instead of the project root.
        #[arg(long)]
        user: bool,

        /// Append `.claude/` to the project's `.gitignore`.
        #[arg(long)]
        gitignore_claude: bool,

        /// Remove the fallow-gate handler, hook script, and AGENTS.md
        /// managed block instead of installing them. Idempotent: reports
        /// "unchanged" when nothing to remove.
        #[arg(long)]
        uninstall: bool,
    },
}

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum HooksTargetArg {
    /// Shell-level Git pre-commit hook under .git/hooks/ or .husky/.
    Git,
    /// Agent-level Claude Code / Codex gate.
    Agent,
}

#[derive(clap::Subcommand)]
enum HooksCli {
    /// Install a fallow-managed hook.
    Install {
        /// Hook surface to install.
        #[arg(long, value_enum)]
        target: HooksTargetArg,

        /// Fallback base branch/ref for Git pre-commit hooks when no upstream is set.
        #[arg(long)]
        branch: Option<String>,

        /// Target a specific agent surface when --target agent is used.
        #[arg(long, value_enum)]
        agent: Option<setup_hooks::HookAgentArg>,

        /// Print what would be written without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Overwrite an existing managed or user-edited hook.
        #[arg(long)]
        force: bool,

        /// Write agent hooks to the user's home directory instead of the project root.
        #[arg(long)]
        user: bool,

        /// Append `.claude/` to the project's `.gitignore` for Claude agent hooks.
        #[arg(long)]
        gitignore_claude: bool,
    },

    /// Remove a fallow-managed hook.
    Uninstall {
        /// Hook surface to remove.
        #[arg(long, value_enum)]
        target: HooksTargetArg,

        /// Target a specific agent surface when --target agent is used.
        #[arg(long, value_enum)]
        agent: Option<setup_hooks::HookAgentArg>,

        /// Print what would be removed without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Remove a user-edited hook script or Git hook instead of preserving it.
        #[arg(long)]
        force: bool,

        /// Remove agent hooks from the user's home directory instead of the project root.
        #[arg(long)]
        user: bool,
    },
}

#[derive(clap::Subcommand)]
enum LicenseCli {
    /// Activate a license JWT.
    ///
    /// JWT input precedence: positional arg > `--from-file` > stdin (`-`).
    /// All paths normalize whitespace before crypto verification.
    Activate {
        /// JWT as a positional argument.
        #[arg(value_name = "JWT")]
        jwt: Option<String>,

        /// Path to a file containing the JWT.
        #[arg(long, value_name = "PATH")]
        from_file: Option<PathBuf>,

        /// Read JWT from stdin.
        #[arg(long, conflicts_with_all = ["jwt", "from_file"])]
        stdin: bool,

        /// Start a 30-day email-gated trial in one step.
        ///
        /// The trial endpoint is rate-limited to 5 requests per hour per IP.
        /// In CI or behind a shared NAT, start the trial from a developer
        /// machine and set FALLOW_LICENSE (or FALLOW_LICENSE_PATH) on the
        /// runner instead of re-running `activate --trial` per job.
        #[arg(long, requires = "email")]
        trial: bool,

        /// Email address for the trial flow.
        #[arg(long, value_name = "ADDR")]
        email: Option<String>,
    },
    /// Show the active license tier, seats, features, and days remaining.
    Status,
    /// Fetch a fresh JWT from `api.fallow.cloud` (network-only).
    Refresh,
    /// Remove the local license file.
    Deactivate,
}

#[derive(clap::Subcommand)]
enum CiTemplateCli {
    /// Print or vendor the GitLab CI template and MR integration helpers.
    Gitlab {
        /// Write ci/ and action/ helper files under DIR instead of printing the template.
        ///
        /// Passing --vendor without a DIR writes into the current directory.
        #[arg(long, value_name = "DIR", num_args = 0..=1, default_missing_value = ".")]
        vendor: Option<PathBuf>,

        /// Overwrite existing files that differ from the bundled template.
        #[arg(long)]
        force: bool,
    },
}

#[derive(clap::Subcommand)]
enum CoverageCli {
    /// Resumable first-run setup: license + sidecar + recipe + analysis.
    Setup {
        /// Accept all prompts automatically.
        #[arg(short = 'y', long)]
        yes: bool,

        /// Print instructions instead of prompting.
        #[arg(long)]
        non_interactive: bool,

        /// Emit deterministic setup instructions as JSON. Implies --non-interactive.
        #[arg(long)]
        json: bool,
    },
    /// Analyze runtime coverage from a local artifact or explicit cloud source.
    ///
    /// Cloud mode is opt-in only. `FALLOW_API_KEY` by itself never selects
    /// cloud mode; pass `--cloud` / `--runtime-coverage-cloud`, or set
    /// `FALLOW_RUNTIME_COVERAGE_SOURCE=cloud`.
    Analyze {
        /// File or directory containing local runtime coverage input.
        #[arg(long, value_name = "PATH", conflicts_with = "cloud")]
        runtime_coverage: Option<PathBuf>,

        /// Fetch latest runtime facts from fallow cloud for the selected repo.
        #[arg(long, visible_alias = "runtime-coverage-cloud")]
        cloud: bool,

        /// Fallow cloud API key. Precedence: this flag > $FALLOW_API_KEY.
        #[arg(long, value_name = "KEY")]
        api_key: Option<String>,

        /// Override the fallow cloud base URL.
        #[arg(long, value_name = "URL")]
        api_endpoint: Option<String>,

        /// Repository identifier, for example `owner/repo`.
        ///
        /// Defaults to $FALLOW_REPO, then the parsed origin URL from
        /// `git remote get-url origin`. Slashes are percent-encoded as one
        /// URL segment when calling the cloud runtime-context endpoint.
        #[arg(long, value_name = "OWNER/REPO")]
        repo: Option<String>,

        /// Optional monorepo/project disambiguator.
        #[arg(long, value_name = "ID")]
        project_id: Option<String>,

        /// Runtime observation window to request from cloud (1..=90 days).
        #[arg(long, value_name = "DAYS", default_value_t = 30)]
        coverage_period: u16,

        /// Optional runtime environment filter.
        #[arg(long, value_name = "ENV")]
        environment: Option<String>,

        /// Optional commit SHA filter for cloud runtime facts.
        #[arg(long, value_name = "SHA")]
        commit_sha: Option<String>,

        /// Analyze production code only.
        #[arg(long)]
        production: bool,

        /// Threshold for hot-path classification.
        #[arg(long, default_value_t = 100)]
        min_invocations_hot: u64,

        /// Minimum total trace volume before high-confidence verdicts.
        #[arg(long, value_name = "N")]
        min_observation_volume: Option<u32>,

        /// Fraction of total trace count below which an invoked function is low traffic.
        #[arg(long, value_name = "RATIO")]
        low_traffic_threshold: Option<f64>,

        /// Show only the top N runtime findings and hot paths.
        #[arg(long)]
        top: Option<usize>,

        /// Show the first-class blast-radius section in human output.
        #[arg(long)]
        blast_radius: bool,

        /// Show the first-class importance section in human output.
        #[arg(long)]
        importance: bool,
    },
    /// Upload a static function inventory to fallow cloud (Production
    /// Coverage, paid). Unlocks the `untracked` filter on the dashboard by
    /// pairing runtime coverage data with the AST view of "every function
    /// that exists". See <https://docs.fallow.tools/analysis/runtime-coverage>.
    ///
    /// This command makes network calls to fallow cloud. `fallow dead-code`
    /// stays offline.
    ///
    /// Exit codes: 0 ok · 7 network · 10 validation · 11 payload too large
    /// · 12 auth rejected · 13 server error.
    UploadInventory {
        /// Fallow cloud API key (bearer token).
        ///
        /// Precedence: this flag > $FALLOW_API_KEY. Generate at
        /// <https://fallow.cloud/settings#api-keys>.
        ///
        /// Security: prefer $FALLOW_API_KEY on shared CI runners. Passing a
        /// secret on the command line may be visible to other processes via
        /// `ps` and can leak into shell history or process audit logs.
        #[arg(long, value_name = "KEY")]
        api_key: Option<String>,

        /// Override the fallow cloud base URL.
        ///
        /// Useful for staging and on-premise deployments. Also respects
        /// $FALLOW_API_URL when this flag is not set.
        #[arg(long, value_name = "URL")]
        api_endpoint: Option<String>,

        /// Project identifier, for example `fallow-cloud-api` or `owner/repo`.
        ///
        /// Defaults to $GITHUB_REPOSITORY, then $CI_PROJECT_PATH, then the
        /// parsed origin URL from `git remote get-url origin`.
        #[arg(long, value_name = "PROJECT_ID")]
        project_id: Option<String>,

        /// Explicit git SHA for this inventory.
        ///
        /// Default: `git rev-parse HEAD`. The inventory is keyed on this
        /// value; the cloud back-fills hourly buckets with a matching SHA.
        #[arg(long, value_name = "SHA")]
        git_sha: Option<String>,

        /// Proceed even when the working tree has uncommitted changes.
        ///
        /// Warning: the inventory is generated from the working copy, so it
        /// may not match the uploaded git SHA. Commit or stash first if you
        /// want a SHA-exact upload.
        #[arg(long)]
        allow_dirty: bool,

        /// Additional glob patterns to exclude from the walk.
        ///
        /// Applied after the existing fallow ignore rules. Repeatable.
        #[arg(long, value_name = "GLOB", num_args = 0..)]
        exclude_paths: Vec<String>,

        /// Prefix prepended to every emitted filePath so the static
        /// inventory joins with the runtime beacon for your deployment.
        /// Required for containerized deployments where the deployed
        /// WORKDIR rebases paths at runtime. Default: none (paths emit
        /// repo-relative, matching local runs and non-container CI).
        ///
        /// Common values: `/app` (typical Dockerfile), `/workspace`
        /// (Buildpacks / Cloud Run), `/usr/src/app` (older Node images),
        /// `/var/task` (Lambda), `/home/runner/work/<repo>/<repo>`
        /// (GitHub Actions default checkout).
        ///
        /// Must start with `/` and use POSIX separators.
        #[arg(long, value_name = "PREFIX")]
        path_prefix: Option<String>,

        /// Print what would be uploaded and exit. No network call.
        #[arg(long)]
        dry_run: bool,

        /// Treat transient upload failures as warnings instead of errors
        /// (exit 0). Validation and auth errors still fail hard; this only
        /// downgrades transport and server errors.
        #[arg(long)]
        ignore_upload_errors: bool,
    },
    /// Upload JavaScript source maps to fallow cloud for bundled runtime coverage.
    ///
    /// Scans a build output directory for `.map` files and uploads them under
    /// the selected repo + git SHA. The production beacon reports bundled
    /// paths; the cloud resolver uses these maps to remap runtime coverage back
    /// to original source files.
    UploadSourceMaps {
        /// Directory to scan recursively for source maps.
        #[arg(long, value_name = "PATH", default_value = "dist")]
        dir: PathBuf,

        /// Glob pattern, relative to --dir, selecting maps to upload.
        #[arg(long, value_name = "GLOB", default_value = "**/*.map")]
        include: String,

        /// Glob pattern, relative to --dir, selecting files to skip.
        ///
        /// Repeatable. Defaults to `**/node_modules/**`.
        #[arg(long, value_name = "GLOB", default_value = "**/node_modules/**")]
        exclude: Vec<String>,

        /// Repo name used in the API path.
        ///
        /// Defaults to package.json repository.url, then `git remote get-url origin`.
        #[arg(long, value_name = "NAME")]
        repo: Option<String>,

        /// Commit SHA to key uploads under.
        ///
        /// Defaults to $GITHUB_SHA, $CI_COMMIT_SHA, $COMMIT_SHA, then
        /// `git rev-parse HEAD`.
        #[arg(long, value_name = "SHA")]
        git_sha: Option<String>,

        /// Override the fallow cloud base URL.
        #[arg(long, value_name = "URL")]
        endpoint: Option<String>,

        /// Send only the basename as fileName by default.
        ///
        /// Use `--strip-path=false` when your runtime coverage reports bundle
        /// paths relative to the build directory, such as `assets/app.js`.
        #[arg(long, value_name = "BOOL", default_value_t = true, action = clap::ArgAction::Set)]
        strip_path: bool,

        /// Print what would be uploaded and exit. No network call.
        #[arg(long)]
        dry_run: bool,

        /// Parallel upload fanout.
        #[arg(long, value_name = "N", default_value_t = 4)]
        concurrency: usize,

        /// Stop on first upload error.
        #[arg(long)]
        fail_fast: bool,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum Format {
    Human,
    Json,
    Sarif,
    Compact,
    Markdown,
    #[value(
        name = "codeclimate",
        alias = "gitlab-codequality",
        alias = "gitlab-code-quality"
    )]
    CodeClimate,
    #[value(name = "pr-comment-github")]
    PrCommentGithub,
    #[value(name = "pr-comment-gitlab")]
    PrCommentGitlab,
    #[value(name = "review-github")]
    ReviewGithub,
    #[value(name = "review-gitlab")]
    ReviewGitlab,
    Badge,
}

#[derive(Subcommand)]
enum CiCli {
    /// Validate a rendered review envelope and compute a stable reconcile plan.
    ReconcileReview {
        /// Provider whose review envelope is being reconciled.
        #[arg(long, value_enum)]
        provider: CiProviderArg,

        /// Pull request number (GitHub).
        #[arg(long)]
        pr: Option<String>,

        /// Merge request IID (GitLab).
        #[arg(long)]
        mr: Option<String>,

        /// Path to a review-github or review-gitlab JSON envelope.
        #[arg(long)]
        envelope: PathBuf,

        /// GitHub repository in owner/name form. Defaults to GH_REPO or GITHUB_REPOSITORY.
        #[arg(long)]
        repo: Option<String>,

        /// GitLab project id or path. Defaults to CI_PROJECT_ID.
        #[arg(long = "project-id")]
        project_id: Option<String>,

        /// Provider API base URL. Defaults to github.com or CI_API_V4_URL/gitlab.com.
        #[arg(long = "api-url")]
        api_url: Option<String>,

        /// Compute the reconcile plan without posting resolution notes or resolving threads.
        #[arg(long)]
        dry_run: bool,
    },
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum CiProviderArg {
    Github,
    Gitlab,
}

impl From<Format> for fallow_config::OutputFormat {
    fn from(f: Format) -> Self {
        match f {
            Format::Human => Self::Human,
            Format::Json => Self::Json,
            Format::Sarif => Self::Sarif,
            Format::Compact => Self::Compact,
            Format::Markdown => Self::Markdown,
            Format::CodeClimate => Self::CodeClimate,
            Format::PrCommentGithub => Self::PrCommentGithub,
            Format::PrCommentGitlab => Self::PrCommentGitlab,
            Format::ReviewGithub => Self::ReviewGithub,
            Format::ReviewGitlab => Self::ReviewGitlab,
            Format::Badge => Self::Badge,
        }
    }
}

/// Filter refactoring targets by effort level.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum EffortFilter {
    Low,
    Medium,
    High,
}

impl EffortFilter {
    /// Convert to the corresponding `EffortEstimate` for comparison.
    const fn to_estimate(self) -> health_types::EffortEstimate {
        match self {
            Self::Low => health_types::EffortEstimate::Low,
            Self::Medium => health_types::EffortEstimate::Medium,
            Self::High => health_types::EffortEstimate::High,
        }
    }
}

/// Privacy mode for author emails emitted by `--ownership`.
///
/// CLI mirror of [`fallow_config::EmailMode`]. Kept as a separate enum so
/// the help text controls rendering and we don't leak config-internal
/// schema details into clap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum EmailModeArg {
    /// Show full email addresses as recorded in git history.
    Raw,
    /// Show local-part only (default). Unwraps GitHub-style noreply prefixes.
    Handle,
    /// Show stable non-cryptographic pseudonyms (`xxh3:<hex>`).
    Hash,
}

impl EmailModeArg {
    /// Convert to the equivalent config-level mode.
    const fn to_config(self) -> fallow_config::EmailMode {
        match self {
            Self::Raw => fallow_config::EmailMode::Raw,
            Self::Handle => fallow_config::EmailMode::Handle,
            Self::Hash => fallow_config::EmailMode::Hash,
        }
    }
}

/// CLI mirror of [`fallow_config::AuditGate`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum AuditGateArg {
    /// Only findings introduced by the current changeset affect the verdict.
    NewOnly,
    /// All findings in changed files affect the verdict.
    All,
}

impl From<AuditGateArg> for fallow_config::AuditGate {
    fn from(value: AuditGateArg) -> Self {
        match value {
            AuditGateArg::NewOnly => Self::NewOnly,
            AuditGateArg::All => Self::All,
        }
    }
}

// See `error.rs` — `emit_error` is re-exported via `use error::emit_error`.

// ── Environment variable helpers ─────────────────────────────────

/// Read `FALLOW_FORMAT` env var and parse it into a Format value.
fn format_from_env() -> Option<Format> {
    let val = std::env::var("FALLOW_FORMAT").ok()?;
    match val.to_lowercase().as_str() {
        "json" => Some(Format::Json),
        "human" => Some(Format::Human),
        "sarif" => Some(Format::Sarif),
        "compact" => Some(Format::Compact),
        "markdown" | "md" => Some(Format::Markdown),
        "codeclimate" | "gitlab-codequality" | "gitlab-code-quality" => Some(Format::CodeClimate),
        "pr-comment-github" => Some(Format::PrCommentGithub),
        "pr-comment-gitlab" => Some(Format::PrCommentGitlab),
        "review-github" => Some(Format::ReviewGithub),
        "review-gitlab" => Some(Format::ReviewGitlab),
        "badge" => Some(Format::Badge),
        _ => None,
    }
}

/// Read `FALLOW_QUIET` env var: "1" or "true" (case-insensitive) means quiet.
fn quiet_from_env() -> bool {
    std::env::var("FALLOW_QUIET").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

fn bool_from_env(name: &str) -> Option<bool> {
    let value = std::env::var(name).ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Resolve an audit baseline path using CLI > config precedence.
///
/// Both sources resolve relative paths against the project root. This keeps
/// behavior consistent in CI scripts where `--root $REPO_ROOT` differs from
/// the process CWD.
fn resolve_audit_baseline_path(
    root: &std::path::Path,
    cli: Option<&std::path::Path>,
    config: Option<&str>,
) -> Option<PathBuf> {
    let path = cli.map(std::path::Path::to_path_buf).or_else(|| {
        config.map(|p| {
            let path = PathBuf::from(p);
            if path.is_absolute() {
                path
            } else {
                root.join(path)
            }
        })
    })?;
    if path.is_absolute() {
        Some(path)
    } else {
        Some(root.join(path))
    }
}

// ── Format resolution ─────────────────────────────────────────────

struct FormatConfig {
    output: fallow_config::OutputFormat,
    quiet: bool,
    cli_format_was_explicit: bool,
}

fn resolve_format(cli: &Cli) -> FormatConfig {
    // Resolve output format: CLI flag > FALLOW_FORMAT env var > default ("human").
    // clap sets the default to "human", so we only override with the env var
    // when the user did NOT explicitly pass --format on the CLI.
    let cli_format_was_explicit = std::env::args()
        .any(|a| a == "--format" || a == "--output" || a.starts_with("--format=") || a == "-f");
    let format: Format = if cli_format_was_explicit {
        cli.format
    } else {
        format_from_env().unwrap_or(cli.format)
    };

    // Resolve quiet: CLI --quiet flag > FALLOW_QUIET env var > false
    let quiet = cli.quiet || quiet_from_env();

    FormatConfig {
        output: format.into(),
        quiet,
        cli_format_was_explicit,
    }
}

// ── Tracing setup ─────────────────────────────────────────────────

/// Build the tracing filter for the CLI.
///
/// Human output should stay clean by default, even when stderr is redirected to a
/// file or captured by an agent. Internal INFO-level tracing is therefore opt-in
/// via `RUST_LOG`, while warnings remain visible. An explicitly empty `RUST_LOG`
/// disables tracing entirely, which keeps the test harness deterministic.
fn build_tracing_filter(rust_log: Option<&str>) -> tracing_subscriber::EnvFilter {
    use tracing_subscriber::filter::LevelFilter;

    let builder = tracing_subscriber::EnvFilter::builder();
    match rust_log.map(str::trim) {
        Some("") => builder
            .with_default_directive(LevelFilter::OFF.into())
            .parse_lossy("off"),
        Some(value) => builder
            .with_default_directive(LevelFilter::OFF.into())
            .parse_lossy(value),
        None => builder
            .with_default_directive(LevelFilter::WARN.into())
            .parse_lossy(""),
    }
}

/// Set up tracing for the CLI.
fn setup_tracing() {
    let rust_log = std::env::var("RUST_LOG").ok();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(build_tracing_filter(rust_log.as_deref()))
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .init();
}

// ── Input validation ──────────────────────────────────────────────

fn validate_inputs(
    cli: &Cli,
    output: fallow_config::OutputFormat,
) -> Result<(PathBuf, usize), ExitCode> {
    // Validate control characters in key string inputs
    if let Some(ref config_path) = cli.config
        && let Some(s) = config_path.to_str()
        && let Err(e) = validate::validate_no_control_chars(s, "--config")
    {
        return Err(emit_error(&e, 2, output));
    }
    if let Some(ref ws_patterns) = cli.workspace {
        for ws in ws_patterns {
            if let Err(e) = validate::validate_no_control_chars(ws, "--workspace") {
                return Err(emit_error(&e, 2, output));
            }
        }
    }
    if let Some(ref git_ref) = cli.changed_since
        && let Err(e) = validate::validate_no_control_chars(git_ref, "--changed-since")
    {
        return Err(emit_error(&e, 2, output));
    }
    if let Some(ref git_ref) = cli.changed_workspaces
        && let Err(e) = validate::validate_no_control_chars(git_ref, "--changed-workspaces")
    {
        return Err(emit_error(&e, 2, output));
    }

    // --workspace and --changed-workspaces are mutually exclusive: one is an
    // explicit list of packages, the other is git-derived. Mixing them has no
    // coherent intersection semantics, so reject early with a targeted message.
    if cli.workspace.is_some() && cli.changed_workspaces.is_some() {
        return Err(emit_error(
            "--workspace and --changed-workspaces are mutually exclusive. \
             Pick one: --workspace for explicit package names/globs, \
             --changed-workspaces for git-derived monorepo CI scoping.",
            2,
            output,
        ));
    }

    // Validate and resolve root
    let raw_root = cli
        .root
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("Failed to get current directory"));
    let root = match validate::validate_root(&raw_root) {
        Ok(r) => r,
        Err(e) => {
            return Err(emit_error(&e, 2, output));
        }
    };

    // Validate --changed-since early
    if let Some(ref git_ref) = cli.changed_since
        && let Err(e) = validate::validate_git_ref(git_ref)
    {
        return Err(emit_error(
            &format!("invalid --changed-since: {e}"),
            2,
            output,
        ));
    }

    if let Some(ref git_ref) = cli.changed_workspaces
        && let Err(e) = validate::validate_git_ref(git_ref)
    {
        return Err(emit_error(
            &format!("invalid --changed-workspaces: {e}"),
            2,
            output,
        ));
    }

    let threads = cli
        .threads
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, std::num::NonZero::get));

    rayon_pool::configure_global_pool(threads);

    Ok((root, threads))
}

/// Apply CI defaults: if `--ci` is set, override format to SARIF (unless explicit),
/// enable fail-on-issues, and set quiet. Returns (output, quiet, `fail_on_issues`).
fn apply_ci_defaults(
    ci: bool,
    mut fail_on_issues: bool,
    output: fallow_config::OutputFormat,
    quiet: bool,
    cli_format_was_explicit: bool,
) -> (fallow_config::OutputFormat, bool, bool) {
    if ci {
        let ci_output = if !cli_format_was_explicit && format_from_env().is_none() {
            fallow_config::OutputFormat::Sarif
        } else {
            output
        };
        fail_on_issues = true;
        (ci_output, true, fail_on_issues)
    } else {
        (output, quiet, fail_on_issues)
    }
}

// ── Helpers ──────────────────────────────────────────────────────

fn build_regression_opts<'a>(
    fail_on_regression: bool,
    tolerance: regression::Tolerance,
    regression_baseline: Option<&'a std::path::Path>,
    save_regression_file: Option<&'a std::path::PathBuf>,
    save_to_config: bool,
    scoped: bool,
    quiet: bool,
) -> regression::RegressionOpts<'a> {
    regression::RegressionOpts {
        fail_on_regression,
        tolerance,
        regression_baseline_file: regression_baseline,
        save_target: if let Some(path) = save_regression_file {
            regression::SaveRegressionTarget::File(path)
        } else if save_to_config {
            regression::SaveRegressionTarget::Config
        } else {
            regression::SaveRegressionTarget::None
        },
        scoped,
        quiet,
    }
}

struct DispatchContext<'a> {
    cli: &'a Cli,
    root: &'a std::path::Path,
    output: fallow_config::OutputFormat,
    quiet: bool,
    cli_format_was_explicit: bool,
    threads: usize,
    tolerance: regression::Tolerance,
    save_regression_file: Option<&'a std::path::PathBuf>,
    save_to_config: bool,
}

impl DispatchContext<'_> {
    fn ci_defaults(&self) -> (fallow_config::OutputFormat, bool, bool) {
        apply_ci_defaults(
            self.cli.ci,
            self.cli.fail_on_issues,
            self.output,
            self.quiet,
            self.cli_format_was_explicit,
        )
    }

    fn production_modes(
        &self,
        dead_code: bool,
        health: bool,
        dupes: bool,
    ) -> Result<ProductionModes, ExitCode> {
        resolve_production_modes(self.cli, self.root, self.output, dead_code, health, dupes)
    }

    fn production_for(
        &self,
        analysis: fallow_config::ProductionAnalysis,
    ) -> Result<bool, ExitCode> {
        self.production_modes(false, false, false)
            .map(|modes| modes.for_analysis(analysis))
    }

    fn regression_opts(&self, scoped: bool) -> regression::RegressionOpts<'_> {
        build_regression_opts(
            self.cli.fail_on_regression,
            self.tolerance,
            self.cli.regression_baseline.as_deref(),
            self.save_regression_file,
            self.save_to_config,
            scoped,
            self.quiet,
        )
    }
}

#[derive(Clone, Copy)]
struct ProductionModes {
    dead_code: bool,
    health: bool,
    dupes: bool,
}

impl ProductionModes {
    const fn for_analysis(self, analysis: fallow_config::ProductionAnalysis) -> bool {
        match analysis {
            fallow_config::ProductionAnalysis::DeadCode => self.dead_code,
            fallow_config::ProductionAnalysis::Health => self.health,
            fallow_config::ProductionAnalysis::Dupes => self.dupes,
        }
    }
}

fn load_config_production(
    root: &std::path::Path,
    config_path: Option<&PathBuf>,
    output: fallow_config::OutputFormat,
) -> Result<fallow_config::ProductionConfig, ExitCode> {
    let loaded = if let Some(path) = config_path {
        fallow_config::FallowConfig::load(path)
            .map(Some)
            .map_err(|e| {
                emit_error(
                    &format!("failed to load config '{}': {e}", path.display()),
                    2,
                    output,
                )
            })?
    } else {
        fallow_config::FallowConfig::find_and_load(root)
            .map(|found| found.map(|(config, _)| config))
            .map_err(|e| emit_error(&e, 2, output))?
    };

    Ok(match loaded {
        Some(config) => config.production,
        None => fallow_config::ProductionConfig::default(),
    })
}

fn resolve_production_modes(
    cli: &Cli,
    root: &std::path::Path,
    output: fallow_config::OutputFormat,
    production_dead_code: bool,
    production_health: bool,
    production_dupes: bool,
) -> Result<ProductionModes, ExitCode> {
    let config = load_config_production(root, cli.config.as_ref(), output)?;
    let env_global = bool_from_env("FALLOW_PRODUCTION");

    let resolve_one =
        |analysis: fallow_config::ProductionAnalysis, cli_specific: bool, env_name: &str| {
            if cli.production || cli_specific {
                true
            } else if let Some(value) = bool_from_env(env_name) {
                value
            } else if let Some(value) = env_global {
                value
            } else {
                config.for_analysis(analysis)
            }
        };

    Ok(ProductionModes {
        dead_code: resolve_one(
            fallow_config::ProductionAnalysis::DeadCode,
            production_dead_code,
            "FALLOW_PRODUCTION_DEAD_CODE",
        ),
        health: resolve_one(
            fallow_config::ProductionAnalysis::Health,
            production_health,
            "FALLOW_PRODUCTION_HEALTH",
        ),
        dupes: resolve_one(
            fallow_config::ProductionAnalysis::Dupes,
            production_dupes,
            "FALLOW_PRODUCTION_DUPES",
        ),
    })
}

// ── Main ─────────────────────────────────────────────────────────

fn main() -> ExitCode {
    let mut cli = Cli::parse();

    // Auto-suffix the sticky-comment marker with the workspace name when
    // running scoped to a single workspace package and the user did not pin
    // an explicit comment id. Parallel per-workspace jobs would otherwise
    // edit the same `<!-- fallow-id: fallow-results -->` marker on the same
    // PR/MR and race each other's bodies. Setting `FALLOW_WORKSPACE` here is
    // read by `report::ci::pr_comment::sticky_marker_id` at render time.
    // Auto-suffix the sticky-comment marker with a stable identifier derived
    // from the --workspace selection, so parallel monorepo jobs don't race
    // each other on the same PR/MR. One workspace: name as-is. N>1: hash
    // the sorted joined list into a short hex suffix so two jobs running
    // `--workspace web,admin` and `--workspace api,worker` end up with
    // distinct markers (`fallow-results-w-<hex>`).
    if let Some(workspaces) = cli.workspace.as_ref()
        && !workspaces.is_empty()
    {
        report::ci::pr_comment::set_workspace_marker_from_list(workspaces);
    }

    // Handle schema commands before tracing setup (no side effects)
    if matches!(cli.command, Some(Command::Schema)) {
        return schema::run_schema();
    }
    if matches!(cli.command, Some(Command::ConfigSchema)) {
        return init::run_config_schema();
    }
    if matches!(cli.command, Some(Command::PluginSchema)) {
        return init::run_plugin_schema();
    }

    let fmt = resolve_format(&cli);
    setup_tracing();

    let (root, threads) = match validate_inputs(&cli, fmt.output) {
        Ok(v) => v,
        Err(code) => return code,
    };

    let FormatConfig {
        output,
        quiet,
        cli_format_was_explicit,
    } = fmt;

    // Validate --ci/--fail-on-issues/--sarif-file are not used with irrelevant commands
    if (cli.ci || cli.fail_on_issues || cli.sarif_file.is_some())
        && matches!(
            cli.command,
            Some(
                Command::Init { .. }
                    | Command::ConfigSchema
                    | Command::PluginSchema
                    | Command::Schema
                    | Command::Explain { .. }
                    | Command::CiTemplate { .. }
                    | Command::Config { .. }
                    | Command::Ci { .. }
                    | Command::List { .. }
                    | Command::Flags { .. }
                    | Command::Migrate { .. }
                    | Command::License { .. }
                    | Command::Coverage { .. }
                    | Command::Hooks { .. }
                    | Command::SetupHooks { .. }
            )
        )
    {
        return emit_error(
            "--ci, --fail-on-issues, and --sarif-file are only valid with dead-code, dupes, health, or bare invocation",
            2,
            output,
        );
    }

    // Validate --only/--skip are not used with a subcommand
    if (!cli.only.is_empty() || !cli.skip.is_empty()) && cli.command.is_some() {
        return emit_error(
            "--only and --skip can only be used without a subcommand",
            2,
            output,
        );
    }
    if (cli.production_dead_code || cli.production_health || cli.production_dupes)
        && cli.command.is_some()
    {
        return emit_error(
            "--production-dead-code, --production-health, and --production-dupes can only be used without a subcommand. For audit, pass them after `audit`",
            2,
            output,
        );
    }
    if !cli.only.is_empty() && !cli.skip.is_empty() {
        return emit_error("--only and --skip are mutually exclusive", 2, output);
    }

    // Parse regression tolerance
    let tolerance = match regression::Tolerance::parse(&cli.tolerance) {
        Ok(t) => t,
        Err(e) => return emit_error(&format!("invalid --tolerance: {e}"), 2, output),
    };

    // Resolve save-regression-baseline target
    let save_regression_file: Option<std::path::PathBuf> =
        cli.save_regression_baseline.as_ref().and_then(|opt| {
            opt.as_ref()
                .filter(|s| !s.is_empty())
                .map(std::path::PathBuf::from)
        });
    let save_to_config = cli.save_regression_baseline.is_some() && save_regression_file.is_none();

    let command = cli.command.take();
    let dispatch = DispatchContext {
        cli: &cli,
        root: &root,
        output,
        quiet,
        cli_format_was_explicit,
        threads,
        tolerance,
        save_regression_file: save_regression_file.as_ref(),
        save_to_config,
    };
    match command {
        None => dispatch_bare_command(&dispatch),
        Some(cmd) => dispatch_subcommand(cmd, &dispatch),
    }
}

fn dispatch_bare_command(dispatch: &DispatchContext<'_>) -> ExitCode {
    let cli = dispatch.cli;
    let (output, quiet, fail_on_issues) = dispatch.ci_defaults();
    let (run_check, run_dupes, run_health) = combined::resolve_analyses(&cli.only, &cli.skip);
    let production = match dispatch.production_modes(
        cli.production_dead_code,
        cli.production_health,
        cli.production_dupes,
    ) {
        Ok(production) => production,
        Err(code) => return code,
    };
    combined::run_combined(&combined::CombinedOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet,
        fail_on_issues,
        sarif_file: cli.sarif_file.as_deref(),
        changed_since: cli.changed_since.as_deref(),
        diff_file: cli.diff_file.as_deref(),
        baseline: cli.baseline.as_deref(),
        save_baseline: cli.save_baseline.as_deref(),
        production: cli.production,
        production_dead_code: Some(production.dead_code),
        production_health: Some(production.health),
        production_dupes: Some(production.dupes),
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        group_by: cli.group_by,
        explain: cli.explain,
        explain_skipped: cli.explain_skipped,
        performance: cli.performance,
        summary: cli.summary,
        run_check,
        run_dupes,
        run_health,
        dupes_mode: cli.dupes_mode,
        dupes_threshold: cli.dupes_threshold,
        score: cli.score || cli.trend,
        trend: cli.trend,
        save_snapshot: cli.save_snapshot.as_ref(),
        include_entry_exports: cli.include_entry_exports,
        regression_opts: dispatch.regression_opts(
            cli.changed_since.is_some()
                || cli.workspace.is_some()
                || cli.changed_workspaces.is_some(),
        ),
    })
}

#[expect(
    clippy::too_many_lines,
    reason = "CLI dispatch handles all subcommands"
)]
fn dispatch_subcommand(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let cli = dispatch.cli;
    let root = dispatch.root;
    let output = dispatch.output;
    let quiet = dispatch.quiet;
    let threads = dispatch.threads;
    match command {
        Command::Check {
            unused_files,
            unused_exports,
            unused_deps,
            unused_types,
            private_type_leaks,
            unused_enum_members,
            unused_class_members,
            unresolved_imports,
            unlisted_deps,
            duplicate_exports,
            circular_deps,
            boundary_violations,
            stale_suppressions,
            unused_catalog_entries,
            include_dupes,
            trace,
            trace_file,
            trace_dependency,
            top,
            file,
        } => dispatch_check(
            dispatch,
            &CheckDispatchArgs {
                filters: IssueFilters {
                    unused_files,
                    unused_exports,
                    unused_deps,
                    unused_types,
                    private_type_leaks,
                    unused_enum_members,
                    unused_class_members,
                    unresolved_imports,
                    unlisted_deps,
                    duplicate_exports,
                    circular_deps,
                    boundary_violations,
                    stale_suppressions,
                    unused_catalog_entries,
                },
                trace_opts: TraceOptions {
                    trace_export: trace,
                    trace_file,
                    trace_dependency,
                    performance: cli.performance,
                },
                include_dupes,
                top,
                file,
            },
        ),
        Command::Watch { no_clear } => {
            let production = match resolve_production_modes(cli, root, output, false, false, false)
            {
                Ok(modes) => modes.for_analysis(fallow_config::ProductionAnalysis::DeadCode),
                Err(code) => return code,
            };
            watch::run_watch(&watch::WatchOptions {
                root,
                config_path: &cli.config,
                output,
                no_cache: cli.no_cache,
                threads,
                quiet,
                production,
                clear_screen: !no_clear,
                explain: cli.explain,
                include_entry_exports: cli.include_entry_exports,
            })
        }
        Command::Fix { dry_run, yes } => {
            let production = match resolve_production_modes(cli, root, output, false, false, false)
            {
                Ok(modes) => modes.for_analysis(fallow_config::ProductionAnalysis::DeadCode),
                Err(code) => return code,
            };
            fix::run_fix(&fix::FixOptions {
                root,
                config_path: &cli.config,
                output,
                no_cache: cli.no_cache,
                threads,
                quiet,
                dry_run,
                yes,
                production,
            })
        }
        Command::Init {
            toml,
            hooks,
            branch,
        } => init::run_init(&init::InitOptions {
            root,
            use_toml: toml,
            hooks,
            branch: branch.as_deref(),
        }),
        Command::Hooks { subcommand } => run_hooks_command(root, subcommand, output),
        Command::Ci { subcommand } => ci::run(map_ci_subcommand(subcommand), output),
        Command::ConfigSchema => init::run_config_schema(),
        Command::PluginSchema => init::run_plugin_schema(),
        Command::CiTemplate { subcommand } => match subcommand {
            CiTemplateCli::Gitlab { vendor, force } => {
                ci_template::run_gitlab_template(&ci_template::GitlabTemplateOptions {
                    vendor_dir: vendor,
                    force,
                })
            }
        },
        Command::Config { path } => config::run_config(root, cli.config.as_deref(), path),
        Command::List {
            entry_points,
            files,
            plugins,
            boundaries,
        } => {
            let production = match resolve_production_modes(cli, root, output, false, false, false)
            {
                Ok(modes) => modes.for_analysis(fallow_config::ProductionAnalysis::DeadCode),
                Err(code) => return code,
            };
            list::run_list(&ListOptions {
                root,
                config_path: &cli.config,
                output,
                threads,
                no_cache: cli.no_cache,
                entry_points,
                files,
                plugins,
                boundaries,
                production,
            })
        }
        Command::Dupes {
            mode,
            min_tokens,
            min_lines,
            threshold,
            skip_local,
            cross_language,
            ignore_imports,
            top,
            trace,
        } => dispatch_dupes(
            dispatch,
            &DupesDispatchArgs {
                mode,
                min_tokens,
                min_lines,
                threshold,
                skip_local,
                cross_language,
                ignore_imports,
                top,
                trace,
            },
        ),
        Command::Health {
            max_cyclomatic,
            max_cognitive,
            max_crap,
            top,
            sort,
            complexity,
            file_scores,
            coverage_gaps,
            hotspots,
            ownership,
            ownership_emails,
            targets,
            effort,
            score,
            min_score,
            min_severity,
            since,
            min_commits,
            save_snapshot,
            trend,
            coverage,
            coverage_root,
            runtime_coverage,
            min_invocations_hot,
            min_observation_volume,
            low_traffic_threshold,
        } => {
            // Resolve coverage: CLI flag > FALLOW_COVERAGE env var
            let coverage =
                coverage.or_else(|| std::env::var("FALLOW_COVERAGE").ok().map(PathBuf::from));
            // --ownership-emails implies --ownership; --ownership implies --hotspots
            let ownership = ownership || ownership_emails.is_some();
            let hotspots = hotspots || ownership;
            dispatch_health(
                dispatch,
                HealthDispatchArgs {
                    max_cyclomatic,
                    max_cognitive,
                    max_crap,
                    top,
                    sort,
                    complexity,
                    file_scores,
                    coverage_gaps,
                    hotspots,
                    ownership,
                    ownership_emails: ownership_emails.map(EmailModeArg::to_config),
                    targets,
                    effort,
                    score,
                    min_score,
                    min_severity,
                    since: since.as_deref(),
                    min_commits,
                    save_snapshot: save_snapshot.as_ref(),
                    trend,
                    coverage: coverage.as_deref(),
                    coverage_root: coverage_root.as_deref(),
                    runtime_coverage: runtime_coverage.as_deref(),
                    min_invocations_hot,
                    min_observation_volume,
                    low_traffic_threshold,
                },
            )
        }
        Command::Flags { top } => {
            let production = match resolve_production_modes(cli, root, output, false, false, false)
            {
                Ok(modes) => modes.for_analysis(fallow_config::ProductionAnalysis::DeadCode),
                Err(code) => return code,
            };
            flags::run_flags(&flags::FlagsOptions {
                root,
                config_path: &cli.config,
                output,
                no_cache: cli.no_cache,
                threads,
                quiet,
                production,
                workspace: cli.workspace.as_deref(),
                changed_workspaces: cli.changed_workspaces.as_deref(),
                changed_since: cli.changed_since.as_deref(),
                explain: cli.explain,
                top,
            })
        }
        Command::Explain { issue_type } => explain::run_explain(&issue_type, output),
        Command::Audit {
            production_dead_code,
            production_health,
            production_dupes,
            dead_code_baseline,
            health_baseline,
            dupes_baseline,
            max_crap,
            coverage,
            coverage_root,
            gate,
            runtime_coverage,
            min_invocations_hot,
        } => {
            if cli.baseline.is_some() || cli.save_baseline.is_some() {
                return emit_error(
                    "audit uses per-analysis baselines. Use --dead-code-baseline, --health-baseline, or --dupes-baseline (or save them with `fallow dead-code|health|dupes --save-baseline <file>`)",
                    2,
                    output,
                );
            }
            let audit_cfg = match load_config(
                root,
                &cli.config,
                output,
                cli.no_cache,
                threads,
                cli.production,
                quiet,
            ) {
                Ok(c) => c.audit,
                Err(code) => return code,
            };
            let production = match resolve_production_modes(
                cli,
                root,
                output,
                production_dead_code,
                production_health,
                production_dupes,
            ) {
                Ok(production) => production,
                Err(code) => return code,
            };
            let resolved_dead_code_baseline = resolve_audit_baseline_path(
                root,
                dead_code_baseline.as_deref(),
                audit_cfg.dead_code_baseline.as_deref(),
            );
            let resolved_health_baseline = resolve_audit_baseline_path(
                root,
                health_baseline.as_deref(),
                audit_cfg.health_baseline.as_deref(),
            );
            let resolved_dupes_baseline = resolve_audit_baseline_path(
                root,
                dupes_baseline.as_deref(),
                audit_cfg.dupes_baseline.as_deref(),
            );
            let coverage =
                coverage.or_else(|| std::env::var("FALLOW_COVERAGE").ok().map(PathBuf::from));
            audit::run_audit(&audit::AuditOptions {
                root,
                config_path: &cli.config,
                output,
                no_cache: cli.no_cache,
                threads,
                quiet,
                changed_since: cli.changed_since.as_deref(),
                production: cli.production,
                production_dead_code: Some(production.dead_code),
                production_health: Some(production.health),
                production_dupes: Some(production.dupes),
                workspace: cli.workspace.as_deref(),
                changed_workspaces: cli.changed_workspaces.as_deref(),
                explain: cli.explain,
                explain_skipped: cli.explain_skipped,
                performance: cli.performance,
                group_by: cli.group_by,
                dead_code_baseline: resolved_dead_code_baseline.as_deref(),
                health_baseline: resolved_health_baseline.as_deref(),
                dupes_baseline: resolved_dupes_baseline.as_deref(),
                max_crap,
                coverage: coverage.as_deref(),
                coverage_root: coverage_root.as_deref(),
                gate: gate.map_or(audit_cfg.gate, Into::into),
                include_entry_exports: cli.include_entry_exports,
                runtime_coverage: runtime_coverage.as_deref(),
                min_invocations_hot,
                diff_file: cli.diff_file.as_deref(),
            })
        }
        Command::Schema => unreachable!("handled above"),
        Command::Migrate {
            toml,
            jsonc,
            dry_run,
            from,
        } => migrate::run_migrate(root, toml, jsonc, dry_run, from.as_deref()),
        Command::License { subcommand } => license::run(&map_license_subcommand(subcommand)),
        Command::Coverage { subcommand } => coverage::run(
            map_coverage_subcommand(&subcommand, cli.explain),
            &coverage::RunContext {
                root,
                config_path: &cli.config,
                output,
                quiet,
                no_cache: cli.no_cache,
                threads,
                explain: cli.explain,
            },
        ),
        Command::SetupHooks {
            agent,
            dry_run,
            force,
            user,
            gitignore_claude,
            uninstall,
        } => setup_hooks::run_setup_hooks(&setup_hooks::SetupHooksOptions {
            root,
            agent,
            dry_run,
            force,
            user,
            gitignore_claude,
            uninstall,
        }),
    }
}

fn run_hooks_command(
    root: &std::path::Path,
    subcommand: HooksCli,
    output: fallow_config::OutputFormat,
) -> ExitCode {
    match subcommand {
        HooksCli::Install {
            target: HooksTargetArg::Git,
            branch,
            agent,
            dry_run,
            force,
            user,
            gitignore_claude,
        } => {
            if agent.is_some() || user || gitignore_claude {
                return emit_error(
                    "--agent, --user, and --gitignore-claude are only valid with `fallow hooks install --target agent`",
                    2,
                    output,
                );
            }
            init::run_git_hooks_install(&init::GitHooksInstallOptions {
                root,
                branch: branch.as_deref(),
                dry_run,
                force,
            })
        }
        HooksCli::Install {
            target: HooksTargetArg::Agent,
            branch,
            agent,
            dry_run,
            force,
            user,
            gitignore_claude,
        } => {
            if branch.is_some() {
                return emit_error(
                    "--branch is only valid with `fallow hooks install --target git`",
                    2,
                    output,
                );
            }
            setup_hooks::run_setup_hooks_with_label(
                &setup_hooks::SetupHooksOptions {
                    root,
                    agent,
                    dry_run,
                    force,
                    user,
                    gitignore_claude,
                    uninstall: false,
                },
                "fallow hooks install --target agent",
            )
        }
        HooksCli::Uninstall {
            target: HooksTargetArg::Git,
            agent,
            dry_run,
            force,
            user,
        } => {
            if agent.is_some() || user {
                return emit_error(
                    "--agent and --user are only valid with `fallow hooks uninstall --target agent`",
                    2,
                    output,
                );
            }
            init::run_git_hooks_uninstall(&init::GitHooksUninstallOptions {
                root,
                dry_run,
                force,
            })
        }
        HooksCli::Uninstall {
            target: HooksTargetArg::Agent,
            agent,
            dry_run,
            force,
            user,
        } => setup_hooks::run_setup_hooks_with_label(
            &setup_hooks::SetupHooksOptions {
                root,
                agent,
                dry_run,
                force,
                user,
                gitignore_claude: false,
                uninstall: true,
            },
            "fallow hooks uninstall --target agent",
        ),
    }
}

fn map_license_subcommand(sub: LicenseCli) -> license::LicenseSubcommand {
    match sub {
        LicenseCli::Activate {
            jwt,
            from_file,
            stdin,
            trial,
            email,
        } => license::LicenseSubcommand::Activate(license::ActivateArgs {
            raw_jwt: jwt,
            from_file,
            from_stdin: stdin,
            trial,
            email,
        }),
        LicenseCli::Status => license::LicenseSubcommand::Status,
        LicenseCli::Refresh => license::LicenseSubcommand::Refresh,
        LicenseCli::Deactivate => license::LicenseSubcommand::Deactivate,
    }
}

fn map_ci_subcommand(sub: CiCli) -> ci::CiCommand {
    match sub {
        CiCli::ReconcileReview {
            provider,
            pr,
            mr,
            envelope,
            repo,
            project_id,
            api_url,
            dry_run,
        } => ci::CiCommand::ReconcileReview {
            provider: match provider {
                CiProviderArg::Github => ci::CiProvider::Github,
                CiProviderArg::Gitlab => ci::CiProvider::Gitlab,
            },
            target: pr.or(mr),
            envelope,
            repo,
            project_id,
            api_url,
            dry_run,
        },
    }
}

fn map_coverage_subcommand(sub: &CoverageCli, explain: bool) -> coverage::CoverageSubcommand {
    match sub {
        CoverageCli::Setup {
            yes,
            non_interactive,
            json,
        } => coverage::CoverageSubcommand::Setup(coverage::SetupArgs {
            yes: *yes,
            non_interactive: *non_interactive || *json,
            json: *json,
            explain,
        }),
        CoverageCli::Analyze {
            runtime_coverage,
            cloud,
            api_key,
            api_endpoint,
            repo,
            project_id,
            coverage_period,
            environment,
            commit_sha,
            production,
            min_invocations_hot,
            min_observation_volume,
            low_traffic_threshold,
            top,
            blast_radius,
            importance,
        } => coverage::CoverageSubcommand::Analyze(coverage::AnalyzeArgs {
            runtime_coverage: runtime_coverage.clone(),
            cloud: *cloud,
            api_key: api_key.clone(),
            api_endpoint: api_endpoint.clone(),
            repo: repo.clone(),
            project_id: project_id.clone(),
            coverage_period: *coverage_period,
            environment: environment.clone(),
            commit_sha: commit_sha.clone(),
            production: *production,
            min_invocations_hot: *min_invocations_hot,
            min_observation_volume: *min_observation_volume,
            low_traffic_threshold: *low_traffic_threshold,
            top: *top,
            blast_radius: *blast_radius,
            importance: *importance,
        }),
        CoverageCli::UploadInventory {
            api_key,
            api_endpoint,
            project_id,
            git_sha,
            allow_dirty,
            exclude_paths,
            path_prefix,
            dry_run,
            ignore_upload_errors,
        } => coverage::CoverageSubcommand::UploadInventory(coverage::UploadInventoryArgs {
            api_key: api_key.clone(),
            api_endpoint: api_endpoint.clone(),
            project_id: project_id.clone(),
            git_sha: git_sha.clone(),
            allow_dirty: *allow_dirty,
            exclude_paths: exclude_paths.clone(),
            path_prefix: path_prefix.clone(),
            dry_run: *dry_run,
            ignore_upload_errors: *ignore_upload_errors,
        }),
        CoverageCli::UploadSourceMaps {
            dir,
            include,
            exclude,
            repo,
            git_sha,
            endpoint,
            strip_path,
            dry_run,
            concurrency,
            fail_fast,
        } => coverage::CoverageSubcommand::UploadSourceMaps(coverage::UploadSourceMapsArgs {
            dir: dir.clone(),
            include: include.clone(),
            exclude: exclude.clone(),
            repo: repo.clone(),
            git_sha: git_sha.clone(),
            endpoint: endpoint.clone(),
            strip_path: *strip_path,
            dry_run: *dry_run,
            concurrency: *concurrency,
            fail_fast: *fail_fast,
        }),
    }
}

struct CheckDispatchArgs {
    filters: IssueFilters,
    trace_opts: TraceOptions,
    include_dupes: bool,
    top: Option<usize>,
    file: Vec<std::path::PathBuf>,
}

fn dispatch_check(dispatch: &DispatchContext<'_>, args: &CheckDispatchArgs) -> ExitCode {
    let cli = dispatch.cli;
    let (output, quiet, fail_on_issues) = dispatch.ci_defaults();
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::DeadCode) {
        Ok(production) => production,
        Err(code) => return code,
    };
    check::run_check(&CheckOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet,
        fail_on_issues,
        filters: &args.filters,
        changed_since: cli.changed_since.as_deref(),
        baseline: cli.baseline.as_deref(),
        save_baseline: cli.save_baseline.as_deref(),
        sarif_file: cli.sarif_file.as_deref(),
        production,
        production_override: Some(production),
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        group_by: cli.group_by,
        include_dupes: args.include_dupes,
        trace_opts: &args.trace_opts,
        explain: cli.explain,
        top: args.top,
        file: &args.file,
        include_entry_exports: cli.include_entry_exports,
        summary: cli.summary,
        regression_opts: dispatch.regression_opts(
            cli.changed_since.is_some()
                || cli.workspace.is_some()
                || cli.changed_workspaces.is_some()
                || !args.file.is_empty(),
        ),
        retain_modules_for_health: false,
        defer_performance: false,
    })
}

struct DupesDispatchArgs {
    mode: Option<DupesMode>,
    min_tokens: Option<usize>,
    min_lines: Option<usize>,
    threshold: Option<f64>,
    skip_local: bool,
    cross_language: bool,
    ignore_imports: bool,
    top: Option<usize>,
    trace: Option<String>,
}

fn dispatch_dupes(dispatch: &DispatchContext<'_>, args: &DupesDispatchArgs) -> ExitCode {
    let cli = dispatch.cli;
    let (output, quiet, _fail_on_issues) = dispatch.ci_defaults();
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::Dupes) {
        Ok(production) => production,
        Err(code) => return code,
    };
    dupes::run_dupes(&DupesOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet,
        mode: args.mode,
        min_tokens: args.min_tokens,
        min_lines: args.min_lines,
        threshold: args.threshold,
        skip_local: args.skip_local,
        cross_language: args.cross_language,
        ignore_imports: args.ignore_imports,
        top: args.top,
        baseline_path: cli.baseline.as_deref(),
        save_baseline_path: cli.save_baseline.as_deref(),
        production,
        production_override: Some(production),
        trace: args.trace.as_deref(),
        changed_since: cli.changed_since.as_deref(),
        changed_files: None,
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        explain: cli.explain,
        explain_skipped: cli.explain_skipped,
        summary: cli.summary,
        group_by: cli.group_by,
        performance: cli.performance,
    })
}

struct HealthDispatchArgs<'a> {
    max_cyclomatic: Option<u16>,
    max_cognitive: Option<u16>,
    max_crap: Option<f64>,
    top: Option<usize>,
    sort: health::SortBy,
    complexity: bool,
    file_scores: bool,
    coverage_gaps: bool,
    hotspots: bool,
    ownership: bool,
    ownership_emails: Option<fallow_config::EmailMode>,
    targets: bool,
    effort: Option<EffortFilter>,
    score: bool,
    min_score: Option<f64>,
    min_severity: Option<health_types::FindingSeverity>,
    since: Option<&'a str>,
    min_commits: Option<u32>,
    save_snapshot: Option<&'a Option<String>>,
    trend: bool,
    coverage: Option<&'a std::path::Path>,
    coverage_root: Option<&'a std::path::Path>,
    runtime_coverage: Option<&'a std::path::Path>,
    min_invocations_hot: u64,
    min_observation_volume: Option<u32>,
    low_traffic_threshold: Option<f64>,
}

fn dispatch_health(dispatch: &DispatchContext<'_>, args: HealthDispatchArgs<'_>) -> ExitCode {
    let cli = dispatch.cli;
    let root = dispatch.root;
    let threads = dispatch.threads;
    let (output, quiet, _fail_on_issues) = dispatch.ci_defaults();
    let HealthDispatchArgs {
        max_cyclomatic,
        max_cognitive,
        max_crap,
        top,
        sort,
        complexity,
        file_scores,
        coverage_gaps,
        hotspots,
        ownership,
        ownership_emails,
        targets,
        effort,
        score,
        min_score,
        min_severity,
        since,
        min_commits,
        save_snapshot,
        trend,
        coverage,
        coverage_root,
        runtime_coverage,
        min_invocations_hot,
        min_observation_volume,
        low_traffic_threshold,
    } = args;
    // --effort implies --targets
    let targets = targets || effort.is_some();
    // --min-score, --save-snapshot, --trend, and --format badge imply --score
    let badge_format = matches!(output, fallow_config::OutputFormat::Badge);
    let score = score || min_score.is_some() || trend || badge_format;
    let snapshot_requested = save_snapshot.is_some();
    // No section flags = show all (including score). Any flag set = show only those.
    // --save-snapshot and --trend are orthogonal (not section flags) but force score.
    let any_section = complexity || file_scores || coverage_gaps || hotspots || targets || score;
    let eff_score = if any_section { score } else { true } || snapshot_requested;
    // Score needs dead-code/file-score inputs and duplication for accuracy.
    // Plain --score keeps churn-backed hotspot penalties tied to --hotspots/--targets,
    // but snapshots and trend comparisons need complete vital signs.
    let force_full = snapshot_requested || eff_score;
    let needs_hotspot_vitals = snapshot_requested || trend;
    let score_only_output =
        score && !complexity && !file_scores && !coverage_gaps && !hotspots && !targets && !trend;
    let eff_file_scores = if any_section { file_scores } else { true } || force_full;
    let eff_coverage_gaps = if any_section { coverage_gaps } else { false };
    let eff_hotspots = if any_section { hotspots } else { true } || needs_hotspot_vitals;
    let eff_complexity = if any_section { complexity } else { true };
    let eff_targets = if any_section { targets } else { true };
    let runtime_coverage = if let Some(path) = runtime_coverage {
        match health::coverage::prepare_options(
            path,
            min_invocations_hot,
            min_observation_volume,
            low_traffic_threshold,
            output,
        ) {
            Ok(options) => Some(options),
            Err(code) => return code,
        }
    } else {
        None
    };
    let production = match resolve_production_modes(cli, root, output, false, false, false) {
        Ok(modes) => modes.for_analysis(fallow_config::ProductionAnalysis::Health),
        Err(code) => return code,
    };
    health::run_health(&HealthOptions {
        root,
        config_path: &cli.config,
        output,
        no_cache: cli.no_cache,
        threads,
        quiet,
        max_cyclomatic,
        max_cognitive,
        max_crap,
        top,
        sort,
        production,
        production_override: Some(production),
        changed_since: cli.changed_since.as_deref(),
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        baseline: cli.baseline.as_deref(),
        save_baseline: cli.save_baseline.as_deref(),
        complexity: eff_complexity,
        file_scores: eff_file_scores,
        coverage_gaps: eff_coverage_gaps,
        config_activates_coverage_gaps: !any_section,
        hotspots: eff_hotspots,
        ownership: ownership && eff_hotspots,
        ownership_emails,
        targets: eff_targets,
        force_full,
        score_only_output,
        enforce_coverage_gap_gate: true,
        effort: effort.map(EffortFilter::to_estimate),
        score: eff_score,
        min_score,
        min_severity,
        since,
        min_commits,
        explain: cli.explain,
        summary: cli.summary,
        save_snapshot: save_snapshot.map(|opt| PathBuf::from(opt.as_deref().unwrap_or_default())),
        trend,
        group_by: cli.group_by,
        coverage,
        coverage_root,
        performance: cli.performance,
        runtime_coverage,
        diff_file: cli.diff_file.as_deref(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── CLI definition validity ─────────────────────────────────────

    /// Validates that the CLI definition has no flag name collisions, missing
    /// fields, or other structural errors. Catches issues like a global alias
    /// `--base` colliding with a subcommand's `--base` flag.
    #[test]
    fn cli_definition_has_no_flag_collisions() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    /// Guard against deferred-work wording leaking into clap-rendered help.
    /// `stub`, `placeholder`, and `not yet` framings tell users the feature
    /// is broken or pending; they belong in tracked issues, not in `--help`.
    /// Walk every (sub)command and assert each rendered long-help is clean.
    #[test]
    fn cli_help_text_contains_no_implementation_status_wording() {
        use clap::CommandFactory;
        let mut root = Cli::command();
        let mut violations: Vec<(String, String)> = Vec::new();
        visit_help(&mut root, "fallow", &mut violations);
        assert!(
            violations.is_empty(),
            "found implementation-status wording in --help output:\n{}",
            violations
                .iter()
                .map(|(cmd, line)| format!("  {cmd}: {line}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    fn visit_help(cmd: &mut clap::Command, path: &str, violations: &mut Vec<(String, String)>) {
        let help = cmd.render_long_help().to_string();
        for line in scan_forbidden(&help) {
            violations.push((path.to_owned(), line));
        }
        let names: Vec<String> = cmd
            .get_subcommands()
            .map(|sub| sub.get_name().to_owned())
            .collect();
        for name in names {
            // Skip the synthetic `help` subcommand clap injects automatically.
            if name == "help" {
                continue;
            }
            if let Some(sub) = cmd.find_subcommand_mut(&name) {
                let sub_path = format!("{path} {name}");
                visit_help(sub, &sub_path, violations);
            }
        }
    }

    fn scan_forbidden(s: &str) -> Vec<String> {
        let lower = s.to_ascii_lowercase();
        let mut out = Vec::new();
        for word in ["stub", "placeholder"] {
            if let Some(idx) = find_whole_word(&lower, word) {
                out.push(extract_line(s, idx));
            }
        }
        if let Some(idx) = lower.find("not yet") {
            out.push(extract_line(s, idx));
        }
        out
    }

    fn find_whole_word(haystack: &str, word: &str) -> Option<usize> {
        let bytes = haystack.as_bytes();
        let mut start = 0;
        while let Some(rel) = haystack[start..].find(word) {
            let abs = start + rel;
            let before_ok = abs == 0 || !bytes[abs - 1].is_ascii_alphanumeric();
            let after_idx = abs + word.len();
            let after_ok = after_idx >= bytes.len() || !bytes[after_idx].is_ascii_alphanumeric();
            if before_ok && after_ok {
                return Some(abs);
            }
            start = abs + word.len();
        }
        None
    }

    fn extract_line(s: &str, byte_idx: usize) -> String {
        let line_start = s[..byte_idx].rfind('\n').map_or(0, |i| i + 1);
        let line_end = s[byte_idx..].find('\n').map_or(s.len(), |i| byte_idx + i);
        s[line_start..line_end].trim().to_owned()
    }

    // ── emit_error ──────────────────────────────────────────────────

    #[test]
    fn emit_error_returns_given_exit_code() {
        let code = emit_error("test error", 2, fallow_config::OutputFormat::Human);
        assert_eq!(code, ExitCode::from(2));
    }

    // ── format/quiet parsing logic ─────────────────────────────────
    // Note: format_from_env() and quiet_from_env() read process-global
    // env vars, so we test the underlying parsing logic directly to
    // avoid unsafe set_var/remove_var and parallel test interference.

    #[test]
    fn format_parsing_covers_all_variants() {
        // The format_from_env function lowercases then matches.
        // Test the same logic inline.
        let parse = |s: &str| -> Option<Format> {
            match s.to_lowercase().as_str() {
                "json" => Some(Format::Json),
                "human" => Some(Format::Human),
                "sarif" => Some(Format::Sarif),
                "compact" => Some(Format::Compact),
                "markdown" | "md" => Some(Format::Markdown),
                "codeclimate" | "gitlab-codequality" | "gitlab-code-quality" => {
                    Some(Format::CodeClimate)
                }
                "pr-comment-github" => Some(Format::PrCommentGithub),
                "pr-comment-gitlab" => Some(Format::PrCommentGitlab),
                "review-github" => Some(Format::ReviewGithub),
                "review-gitlab" => Some(Format::ReviewGitlab),
                "badge" => Some(Format::Badge),
                _ => None,
            }
        };
        assert!(matches!(parse("json"), Some(Format::Json)));
        assert!(matches!(parse("JSON"), Some(Format::Json)));
        assert!(matches!(parse("human"), Some(Format::Human)));
        assert!(matches!(parse("sarif"), Some(Format::Sarif)));
        assert!(matches!(parse("compact"), Some(Format::Compact)));
        assert!(matches!(parse("markdown"), Some(Format::Markdown)));
        assert!(matches!(parse("md"), Some(Format::Markdown)));
        assert!(matches!(parse("codeclimate"), Some(Format::CodeClimate)));
        assert!(matches!(
            parse("gitlab-codequality"),
            Some(Format::CodeClimate)
        ));
        assert!(matches!(
            parse("gitlab-code-quality"),
            Some(Format::CodeClimate)
        ));
        assert!(matches!(
            parse("pr-comment-github"),
            Some(Format::PrCommentGithub)
        ));
        assert!(matches!(
            parse("pr-comment-gitlab"),
            Some(Format::PrCommentGitlab)
        ));
        assert!(matches!(parse("review-github"), Some(Format::ReviewGithub)));
        assert!(matches!(parse("review-gitlab"), Some(Format::ReviewGitlab)));
        assert!(matches!(parse("badge"), Some(Format::Badge)));
        assert!(parse("xml").is_none());
        assert!(parse("").is_none());
    }

    #[test]
    fn quiet_parsing_logic() {
        let parse = |s: &str| -> bool { s == "1" || s.eq_ignore_ascii_case("true") };
        assert!(parse("1"));
        assert!(parse("true"));
        assert!(parse("TRUE"));
        assert!(parse("True"));
        assert!(!parse("0"));
        assert!(!parse("false"));
        assert!(!parse("yes"));
    }

    #[test]
    fn tracing_filter_defaults_to_warn_without_env() {
        assert_eq!(build_tracing_filter(None).to_string(), "warn");
    }

    #[test]
    fn tracing_filter_respects_explicit_env_directives() {
        assert_eq!(build_tracing_filter(Some("info")).to_string(), "info");
    }

    #[test]
    fn tracing_filter_treats_empty_env_as_off() {
        assert_eq!(build_tracing_filter(Some("")).to_string(), "off");
        assert_eq!(build_tracing_filter(Some("   ")).to_string(), "off");
    }
}
