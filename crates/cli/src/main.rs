#![expect(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "CLI binary produces intentional terminal output"
)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

use std::ffi::OsStr;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{CommandFactory, Parser, Subcommand};

mod api;
mod audit;
mod baseline;
mod cache_notice;
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
mod impact;
mod init;
mod license;
mod list;
mod migrate;
mod output_dupes;
mod output_envelope;
mod path_util;
mod rayon_pool;
mod regression;
mod report;
mod runtime_support;
mod schema;
mod security;
mod setup_hooks;
mod signal;
mod telemetry;
mod update_check;
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

const TOP_LEVEL_HELP_TEMPLATE: &str =
    "{about-with-newline}\n{usage-heading} {usage}{after-help}\n\nOptions:\n{options}";

const TOP_LEVEL_AFTER_HELP: &str = "\
Analysis:
  dead-code      Analyze unused code, dependency hygiene, and architecture cycles
  dupes          Find copy-paste and structural code duplication
  health         Analyze complexity, maintainability, hotspots, and coverage gaps
  flags          Detect feature flag usage patterns
  security       Surface local security candidates for agent verification (opt-in)
  audit          Review changed files for dead code, complexity, and duplication

Workflow:
  watch          Re-run analysis as files change
  fix            Auto-fix safe unused-code findings

Project inspection:
  list           List discovered files, entry points, plugins, boundaries, and workspaces
  workspaces     Show monorepo workspace discovery diagnostics
  explain        Explain one issue type without running analysis
  impact         Show what fallow has done for you (opt-in, local-only)

Setup and configuration:
  init           Create a fallow config, optionally with a Git hook
  migrate        Migrate knip or jscpd config to fallow
  config         Show the resolved config and loaded config file
  config-schema  Print the fallow config JSON Schema
  plugin-schema  Print the external plugin JSON Schema

Automation and CI:
  ci             Build PR/MR feedback envelopes
  ci-template    Print or vendor CI integration templates
  hooks          Install or remove fallow-managed Git and agent hooks
  setup-hooks    Legacy agent-hook installer

Runtime coverage:
  coverage       Set up or analyze runtime coverage data
  license        Manage the paid-feature license
  telemetry      Manage opt-in product telemetry

Reference:
  schema         Dump the CLI interface as machine-readable JSON
  help           Print this message or the help of a command

When no command is given, fallow runs dead-code + dupes + health together.
Use --only/--skip to select specific analyses.";

#[derive(Parser)]
#[command(
    name = "fallow",
    about = "Codebase analyzer for TypeScript/JavaScript: unused code, circular dependencies, code duplication, complexity hotspots, and architecture boundary violations",
    version,
    disable_version_flag = true,
    help_template = TOP_LEVEL_HELP_TEMPLATE,
    after_help = TOP_LEVEL_AFTER_HELP
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Print version.
    /// Accepts `-v`, `-V`, and `--version`; TS/JS tooling (node, npm, pnpm,
    /// yarn, bun, tsc) uses `-v`, while `-V` matches knip/oxlint/biome.
    #[arg(
        short = 'v',
        visible_short_alias = 'V',
        long = "version",
        action = clap::ArgAction::Version
    )]
    version: Option<bool>,

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

    /// Unified diff for line-level scoping.
    /// Use `-` to read from stdin. Project-level findings still bypass this
    /// filter. When both this and `--changed-since` are set, the diff filter
    /// wins for finding scope while `--changed-since` still drives file discovery.
    #[arg(long = "diff-file", value_name = "PATH", global = true)]
    diff_file: Option<PathBuf>,

    /// Read the unified diff from stdin.
    /// Equivalent to `--diff-file -`.
    #[arg(long = "diff-stdin", global = true)]
    diff_stdin: bool,

    /// Import change history from a `fallow-churn/v1` JSON file instead of `git
    /// log`, powering hotspots, ownership, and bus-factor on projects with no
    /// git repository (Yandex Arc, Mercurial, Perforce). A small wrapper
    /// translates your VCS log into the contract. Resolved relative to `--root`.
    /// Affects `health --hotspots` / `--ownership` / `--targets` only; `audit`,
    /// `impact`, and `--changed-since` still require git.
    #[arg(long = "churn-file", value_name = "PATH", global = true)]
    churn_file: Option<PathBuf>,

    /// Compare against a previously saved baseline file
    #[arg(long, global = true)]
    baseline: Option<PathBuf>,

    /// Correlate this run with a previous telemetry analysis run.
    ///
    /// Used only for opt-in telemetry follow-up measurement. The value is not
    /// interpreted as a path, repository, package, or user identifier. Hidden
    /// from `--help` until the correlation producer (`analysis_run_id`) ships.
    #[arg(long, global = true, value_name = "RUN_ID", hide = true)]
    parent_run: Option<String>,

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

    /// Scope output to selected workspaces.
    /// Accepts exact names, glob patterns, and `!`-prefixed negations.
    /// Values can be comma-separated or repeated.
    #[arg(short, long, global = true, value_delimiter = ',')]
    workspace: Option<Vec<String>>,

    /// Scope output to workspaces touched since the given git ref.
    /// Git is required. Mutually exclusive with `--workspace`.
    #[arg(long, global = true, value_name = "REF")]
    changed_workspaces: Option<String>,

    /// Group output by owner or by directory.
    #[arg(long, global = true)]
    group_by: Option<GroupBy>,

    /// Show pipeline performance timing breakdown
    #[arg(long, global = true)]
    performance: bool,

    /// Include metric definitions and rule descriptions in output.
    #[arg(long, global = true)]
    explain: bool,

    /// Emit legacy JSON root envelopes without the top-level `kind` discriminator.
    #[arg(long, global = true)]
    legacy_envelope: bool,

    /// Show a per-pattern breakdown for default duplicate ignores.
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

    /// Write the report to a file instead of stdout, for any --format (no ANSI
    /// codes). Useful on large projects where the terminal scrollback truncates
    /// the top. Progress and the confirmation stay on stderr.
    #[arg(short = 'o', long, global = true, value_name = "PATH")]
    output_file: Option<PathBuf>,

    /// Fail if issue count increased beyond tolerance compared to a regression baseline.
    #[arg(long, global = true)]
    fail_on_regression: bool,

    /// Allowed issue count increase before a regression is flagged.
    #[arg(long, global = true, value_name = "TOLERANCE", default_value = "0")]
    tolerance: String,

    /// Path to the regression baseline file.
    #[arg(long, global = true, value_name = "PATH")]
    regression_baseline: Option<PathBuf>,

    /// Save the current issue counts as a regression baseline.
    #[expect(
        clippy::option_option,
        reason = "clap pattern: None=not passed, Some(None)=flag only (write to config), Some(Some(path))=write to file"
    )]
    #[arg(long, global = true, value_name = "PATH", num_args = 0..=1, default_missing_value = "")]
    save_regression_baseline: Option<Option<String>>,

    /// Run only specific analyses when no subcommand is given.
    #[arg(long, value_delimiter = ',')]
    only: Vec<AnalysisKind>,

    /// Skip specific analyses when no subcommand is given.
    #[arg(long, value_delimiter = ',')]
    skip: Vec<AnalysisKind>,

    /// Override duplication detection mode in combined mode.
    #[arg(long = "dupes-mode", global = true)]
    dupes_mode: Option<DupesMode>,

    /// Override duplication threshold in combined mode.
    #[arg(long = "dupes-threshold", global = true)]
    dupes_threshold: Option<f64>,

    /// Override the minimum token count for clones in combined mode.
    #[arg(long = "dupes-min-tokens", global = true)]
    dupes_min_tokens: Option<usize>,

    /// Override the minimum line count for clones in combined mode.
    #[arg(long = "dupes-min-lines", global = true)]
    dupes_min_lines: Option<usize>,

    /// Override the minimum clone occurrences in combined mode (must be >= 2).
    #[arg(long = "dupes-min-occurrences", global = true, value_parser = parse_min_occurrences)]
    dupes_min_occurrences: Option<usize>,

    /// Only report cross-directory duplicates in combined mode.
    #[arg(long = "dupes-skip-local", global = true)]
    dupes_skip_local: bool,

    /// Enable cross-language duplicate detection in combined mode.
    #[arg(long = "dupes-cross-language", global = true)]
    dupes_cross_language: bool,

    /// Exclude import declarations from duplicate detection in combined mode.
    #[arg(long = "dupes-ignore-imports", global = true)]
    dupes_ignore_imports: bool,

    /// Compute health score in combined mode.
    #[arg(long)]
    score: bool,

    /// Compare current health metrics against the most recent saved snapshot.
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

        /// Only report re-export cycles
        #[arg(long)]
        re_export_cycles: bool,

        /// Only report boundary violations
        #[arg(long)]
        boundary_violations: bool,

        /// Only report stale suppressions
        #[arg(long)]
        stale_suppressions: bool,

        /// Only report unused pnpm catalog entries
        #[arg(long)]
        unused_catalog_entries: bool,

        /// Only report empty pnpm catalog groups
        #[arg(long)]
        empty_catalog_groups: bool,

        /// Only report unresolved pnpm catalog references
        #[arg(long)]
        unresolved_catalog_references: bool,

        /// Only report unused pnpm dependency overrides
        #[arg(long)]
        unused_dependency_overrides: bool,

        /// Only report misconfigured pnpm dependency overrides
        #[arg(long)]
        misconfigured_dependency_overrides: bool,

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

    /// Auto-fix issues: remove unused exports, dependencies, and enum
    /// members; add duplicate-export rules to a fallow config file.
    ///
    /// When no fallow config exists outside a monorepo subpackage, a
    /// fresh `.fallowrc.json` is created from the same scaffolding
    /// `fallow init` would emit (framework detection, `$schema`,
    /// `entry`, etc.) and the duplicate-export rules are layered on
    /// top. Inside a monorepo subpackage the create-fallback refuses
    /// and points at the workspace root. Pass `--no-create-config` to
    /// opt out of the create-fallback (recommended for pre-commit
    /// hooks, CI bots, and `fallow watch`).
    ///
    /// Use `--dry-run` to preview source-file edits and config-file
    /// diffs without writing.
    Fix {
        /// Dry run, show what would be changed without modifying files
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation prompt (required in non-TTY environments like CI or AI agents)
        #[arg(long, alias = "force")]
        yes: bool,

        /// Refuse to create a new fallow config file when none exists.
        /// Use this from pre-commit hooks, CI bots, and `fallow watch`
        /// where silently materialising a new top-level config file would
        /// surprise the user. The duplicate-export config-add path is
        /// skipped with an explanatory message; source-file edits proceed
        /// normally.
        #[arg(long)]
        no_create_config: bool,
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
    ///
    /// Precedence is first-match-wins per directory, in the order
    /// `.fallowrc.json` > `.fallowrc.jsonc` > `fallow.toml` > `.fallow.toml`,
    /// walking up to the workspace root. `.fallowrc.json` accepts JSONC
    /// (comments and trailing commas); `.fallowrc.jsonc` is identical in
    /// behavior, the extension only signals to editors that comments are
    /// expected. If two config files coexist in one directory, fallow loads the
    /// higher-precedence one and warns on stderr naming the file it ignored.
    Config {
        /// Print only the config file path (one line, no JSON)
        #[arg(long)]
        path: bool,
    },

    /// List discovered entry points, files, plugins, boundaries, and workspaces.
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

        /// Show monorepo workspaces and any workspace-discovery diagnostics
        /// (malformed package.json, unreachable glob matches, missing
        /// tsconfig references).
        #[arg(long)]
        workspaces: bool,
    },

    /// Show monorepo workspaces and any workspace-discovery diagnostics.
    ///
    /// Equivalent to `fallow list --workspaces`. Use this dedicated form
    /// when introspecting only the workspace topology (other `list`
    /// sections stay hidden).
    Workspaces,

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

        /// Minimum number of occurrences before a clone group is reported.
        /// Raise to focus on widespread copy-paste worth refactoring and skip
        /// pair-only clones.
        /// (defaults to the value in `.fallowrc.jsonc`, or `2` if unset).
        #[arg(long, value_parser = parse_min_occurrences)]
        min_occurrences: Option<usize>,

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

        /// Show only the N most-duplicated clone groups (sorted by instance
        /// count descending, then line count descending)
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

        /// Include the per-decision-point complexity breakdown (`contributions[]`)
        /// on each complexity finding in `--format json` output. Each entry names
        /// the construct (if, else-if, loop, boolean operator, ...) and its
        /// cyclomatic/cognitive weight, so a consumer can explain WHY a function
        /// scored high. Used by the VS Code inline editor breakdown. Off by
        /// default to keep CI/default output lean.
        #[arg(long)]
        complexity_breakdown: bool,

        /// Show only per-file health scores (fan-in, fan-out, dead code ratio, maintainability index).
        /// Requires full analysis pipeline (graph + dead code detection).
        /// Sorted by risk-aware triage concern: lower MI and higher CRAP risk first.
        /// --sort and --baseline apply to complexity findings only, not file scores.
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
        /// where authors are public, or `anonymized` to emit non-reversible
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

        /// Fail if the health score is below this threshold (0-100).
        /// Implies --score. The authoritative CI quality gate: when set,
        /// complexity findings become informational and the exit code is
        /// driven solely by the score (so --min-score 0 always exits 0).
        /// Composes with --min-severity (fails if either gate trips). Plain
        /// `fallow health` (no gate flag) stays advisory and exits 1 on any
        /// finding; for a gate on newly-introduced complexity use
        /// `fallow audit --gate new-only`.
        #[arg(long, value_name = "N")]
        min_score: Option<f64>,

        /// Only exit with error for findings at or above this severity.
        /// Use --min-severity critical to ignore moderate/high findings in CI.
        /// Composes with --min-score (the run fails if either gate trips).
        #[arg(long, value_name = "LEVEL", value_enum)]
        min_severity: Option<crate::health_types::FindingSeverity>,

        /// Print the score and findings but never fail CI (always exit 0).
        /// Advisory mode for surfacing health in logs without blocking.
        /// Mutually exclusive with --min-score and --min-severity.
        #[arg(long)]
        report_only: bool,

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
    /// SDK calls from common providers, and config object patterns (opt-in).
    /// Reports flag locations, detection confidence, and cross-reference
    /// with dead code findings.
    Flags {
        /// Show only the top N flags
        #[arg(long)]
        top: Option<usize>,
    },

    /// Explain one fallow issue type without running an analysis.
    ///
    /// Prints the rule rationale, a worked example, fix guidance, and the
    /// relevant docs URL. Accepts values like `unused-export`,
    /// `fallow/unused-export`, `unused exports`, and `code duplication`.
    Explain {
        /// Issue type, issue label, or rule id to explain
        #[arg(required = true, num_args = 1.., value_name = "ISSUE_TYPE")]
        issue_type: Vec<String>,
    },

    /// Audit changed files for dead code, complexity, and duplication.
    ///
    /// Purpose-built for reviewing AI-generated code and PR quality gates.
    /// Combines dead-code + complexity + duplication scoped to changed files
    /// and returns a verdict (pass/warn/fail).
    /// Auto-detects the base branch when `--changed-since`/`--base` is unset.
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

        /// Internal marker identifying a gate run (e.g. `pre-commit`), set by
        /// the generated git hook so Fallow Impact can record a containment
        /// event when the gate blocks then clears. Hidden; never changes the
        /// verdict, exit code, or output.
        #[arg(long, value_name = "MARKER", hide = true)]
        gate_marker: Option<String>,
    },

    /// Show what fallow has done for you: how many issues it is surfacing, the
    /// trend since the last recorded run, and how many commits it contained at
    /// the pre-commit gate.
    ///
    /// Local-only and opt-in: enable with `fallow impact enable`, then let your
    /// `fallow audit` / pre-commit gate runs build history. Impact never uploads
    /// anything and never affects exit codes.
    Impact {
        #[command(subcommand)]
        subcommand: Option<ImpactCli>,
    },

    /// Surface local security candidates for downstream agent verification (opt-in).
    ///
    /// Ships three complementary surfaces. (1) The graph-structural
    /// `client-server-leak` rule: a `"use client"` file that transitively imports
    /// a module reading a non-public env secret through `process.env` or
    /// `import.meta.env`. (2) The data-driven
    /// `tainted-sink` catalogue: syntactic sink sites matched against a CWE
    /// catalogue (`security_matchers.toml`) spanning categories such as
    /// dangerous-html, template-escape-bypass, command-injection, code-injection,
    /// dynamic-regex, redos-regex, resource-amplification, dynamic-module-load,
    /// sql-injection, ssrf, path-traversal, header-injection, open-redirect,
    /// cleartext-transport, electron-unsafe-webpreferences,
    /// world-writable-permission, insecure-temp-file,
    /// mysql-multiple-statements, mass-assignment, weak-crypto,
    /// deprecated-cipher, insecure-randomness,
    /// unsafe-buffer-alloc, unsafe-deserialization, prototype-pollution,
    /// zip-slip, nosql-injection, ssti, xxe, xpath-injection, and
    /// webview-injection. (3) `hardcoded-secret`,
    /// an include-required
    /// category for provider-prefix literals and high-entropy literals assigned
    /// to secret-shaped identifiers. It never runs from raw entropy alone. All
    /// findings are CANDIDATES for verification, NOT verified vulnerabilities.
    /// This command is the only
    /// surface for security findings; they never appear under bare `fallow` or
    /// the `audit` gate. Build-config and test files are excluded, and public
    /// env prefixes such as `NEXT_PUBLIC_` and `VITE_` are treated as public.
    /// Honors
    /// `--root`, `--format {human,json,sarif}`, `--changed-since`, `--file`, `--diff-file`,
    /// `--diff-stdin`, `--workspace`, `--changed-workspaces`, `--ci`,
    /// `--fail-on-issues`, `--sarif-file`, and `--summary`.
    Security {
        /// Only report security candidates in or reachable from the specified files.
        /// The full project graph is still built, but output is scoped to matching
        /// finding anchors or trace hops. Accepts multiple values.
        #[arg(long, value_name = "PATH")]
        file: Vec<std::path::PathBuf>,
        /// Opt-in regression gate: fail (exit 8) only when the change introduces a
        /// NEW security-sink candidate in the changed lines, not on the whole
        /// candidate backlog. Requires a diff source: `--changed-since <ref>`,
        /// `--diff-file <path>`, or `--diff-stdin`. There is deliberately no `all`
        /// mode (gating on the full backlog is the anti-feature this gate avoids).
        #[arg(long, value_name = "MODE")]
        gate: Option<security::SecurityGateMode>,
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

    /// Manage opt-in product telemetry.
    ///
    /// Telemetry is off by default. It never collects repository names, paths,
    /// package names, source code, config values, raw errors, or raw agent
    /// detection evidence. Use `fallow telemetry inspect --example` to see the
    /// documented payload shape, or prefix a real command with
    /// `FALLOW_TELEMETRY=inspect` to print the exact payload without sending.
    Telemetry {
        #[command(subcommand)]
        subcommand: TelemetryCli,
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

#[derive(Clone, Copy, clap::Subcommand)]
enum TelemetryCli {
    /// Show effective telemetry state, precedence, and controls.
    Status,
    /// Enable opt-in telemetry in the user-level fallow config.
    Enable,
    /// Disable telemetry in the user-level fallow config.
    Disable,
    /// Explain inspect mode or print example payloads.
    Inspect {
        /// Print documented example payloads and field purposes.
        #[arg(long)]
        example: bool,
    },
}

#[derive(Clone, Copy, clap::Subcommand)]
enum ImpactCli {
    /// Enable local Impact tracking for this project.
    Enable,
    /// Disable Impact tracking (existing history is retained).
    Disable,
    /// Show whether Impact tracking is enabled and how much history exists.
    Status,
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
    ///
    /// Each upload also carries the map's path relative to the repo root, so the
    /// source-evidence viewer can resolve a monorepo sub-package map's relative
    /// `sources[]` (e.g. `../../src/X`) to the package-prefixed source path
    /// (e.g. `dashboard/src/X`). Run from the repo root so this prefix is
    /// correct.
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
    /// Upload static dead-code findings to fallow cloud for the source-evidence viewer.
    ///
    /// Runs fallow's static analysis and uploads the `unused_export` and
    /// `dead_file` verdicts under the selected repo + git SHA. The cloud
    /// overlays them on the source view alongside the runtime coverage overlay.
    /// Findings are replace-by-SHA: each run sends the complete set for the SHA.
    UploadStaticFindings {
        /// Fallow cloud API key (bearer token).
        ///
        /// Precedence: this flag > $FALLOW_API_KEY. Generate at
        /// <https://fallow.cloud/settings#api-keys>. This must be a live API
        /// key, not a publishable ingest key.
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

        /// Explicit git SHA for these findings.
        ///
        /// Default: `git rev-parse HEAD`. Findings are keyed on this value and
        /// fully replace any prior set uploaded for the same SHA.
        #[arg(long, value_name = "SHA")]
        git_sha: Option<String>,

        /// Proceed even when the working tree has uncommitted changes.
        ///
        /// Warning: findings are generated from the working copy, so they may
        /// not match the uploaded git SHA. Commit or stash first if you want a
        /// SHA-exact upload.
        #[arg(long)]
        allow_dirty: bool,

        /// Print what would be uploaded and exit. No network call.
        #[arg(long)]
        dry_run: bool,

        /// Treat transient upload failures as warnings instead of errors
        /// (exit 0). Validation and auth errors still fail hard; this only
        /// downgrades transport and server errors.
        #[arg(long)]
        ignore_upload_errors: bool,
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
    Anonymized,
    /// Legacy spelling for anonymized output.
    #[value(hide = true)]
    Hash,
}

impl EmailModeArg {
    /// Convert to the equivalent config-level mode.
    const fn to_config(self) -> fallow_config::EmailMode {
        match self {
            Self::Raw => fallow_config::EmailMode::Raw,
            Self::Handle => fallow_config::EmailMode::Handle,
            Self::Anonymized => fallow_config::EmailMode::Anonymized,
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

/// Parse `--min-occurrences` and reject values below 2. A single occurrence
/// is not a duplicate; silently clamping would diverge from the config-file
/// validator, which also rejects `< 2`.
fn parse_min_occurrences(s: &str) -> Result<usize, String> {
    let value: usize = s
        .parse()
        .map_err(|_| format!("`{s}` is not a non-negative integer"))?;
    if value < 2 {
        return Err(format!(
            "must be at least 2 (got {value}); a single occurrence isn't a duplicate"
        ));
    }
    Ok(value)
}

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
            if path_util::is_absolute_path_any_platform(&path) {
                path
            } else {
                root.join(path)
            }
        })
    })?;
    if path_util::is_absolute_path_any_platform(&path) {
        Some(path)
    } else {
        Some(root.join(path))
    }
}

struct FormatConfig {
    output: fallow_config::OutputFormat,
    quiet: bool,
    cli_format_was_explicit: bool,
}

fn resolve_format(cli: &Cli) -> FormatConfig {
    let cli_format_was_explicit = std::env::args()
        .any(|a| a == "--format" || a == "--output" || a.starts_with("--format=") || a == "-f");
    let format: Format = if cli_format_was_explicit {
        cli.format
    } else {
        format_from_env().unwrap_or(cli.format)
    };

    let quiet = cli.quiet || quiet_from_env();

    FormatConfig {
        output: format.into(),
        quiet,
        cli_format_was_explicit,
    }
}

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

fn validate_inputs(
    cli: &Cli,
    output: fallow_config::OutputFormat,
) -> Result<(PathBuf, usize), ExitCode> {
    if matches!(&cli.command, Some(Command::Security { .. }))
        && let Some(flag) = unsupported_security_global(cli)
    {
        return Err(emit_error(
            &format!("{flag} is not valid with `fallow security`."),
            2,
            output,
        ));
    }

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

    if cli.workspace.is_some() && cli.changed_workspaces.is_some() {
        return Err(emit_error(
            "--workspace and --changed-workspaces are mutually exclusive. \
             Pick one: --workspace for explicit package names/globs, \
             --changed-workspaces for git-derived monorepo CI scoping.",
            2,
            output,
        ));
    }

    let raw_root = if let Some(root) = cli.root.clone() {
        root
    } else {
        std::env::current_dir().map_err(|err| {
            emit_error(
                &format!("Failed to get current directory: {err}"),
                2,
                output,
            )
        })?
    };
    let root = match validate::validate_root(&raw_root) {
        Ok(r) => r,
        Err(e) => {
            return Err(emit_error(&e, 2, output));
        }
    };

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

fn unsupported_security_global(cli: &Cli) -> Option<&'static str> {
    if cli.baseline.is_some() {
        Some("--baseline")
    } else if cli.save_baseline.is_some() {
        Some("--save-baseline")
    } else if cli.production {
        Some("--production")
    } else if cli.group_by.is_some() {
        Some("--group-by")
    } else if cli.performance {
        Some("--performance")
    } else if cli.explain {
        Some("--explain")
    } else if cli.explain_skipped {
        Some("--explain-skipped")
    } else if cli.fail_on_regression {
        Some("--fail-on-regression")
    } else if cli.regression_baseline.is_some() {
        Some("--regression-baseline")
    } else if cli.save_regression_baseline.is_some() {
        Some("--save-regression-baseline")
    } else if cli.dupes_mode.is_some() {
        Some("--dupes-mode")
    } else if cli.dupes_threshold.is_some() {
        Some("--dupes-threshold")
    } else if cli.dupes_min_tokens.is_some() {
        Some("--dupes-min-tokens")
    } else if cli.dupes_min_lines.is_some() {
        Some("--dupes-min-lines")
    } else if cli.dupes_min_occurrences.is_some() {
        Some("--dupes-min-occurrences")
    } else if cli.dupes_skip_local {
        Some("--dupes-skip-local")
    } else if cli.dupes_cross_language {
        Some("--dupes-cross-language")
    } else if cli.dupes_ignore_imports {
        Some("--dupes-ignore-imports")
    } else if cli.include_entry_exports {
        Some("--include-entry-exports")
    } else {
        None
    }
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
        regression::RegressionOpts {
            fail_on_regression: self.cli.fail_on_regression,
            tolerance: self.tolerance,
            regression_baseline_file: self.cli.regression_baseline.as_deref(),
            save_target: if let Some(path) = self.save_regression_file {
                regression::SaveRegressionTarget::File(path)
            } else if self.save_to_config {
                regression::SaveRegressionTarget::Config
            } else {
                regression::SaveRegressionTarget::None
            },
            scoped,
            quiet: self.quiet,
            output: self.output,
        }
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

/// Test-only helper invoked when `FALLOW_TEST_SIGNAL_HELPER=1` is set.
/// Spawns `sleep 30` via the `ScopedChild` registry so the child is
/// tracked by the signal handler, prints the child PID to stdout, then
/// busy-waits so a SIGINT/SIGTERM delivered to the parent fires the
/// signal handler (which kills the child and exits 128+signum).
///
/// When `FALLOW_TEST_SIGNAL_HELPER_GRACEFUL=1` is also set, graceful
/// mode is activated BEFORE spawning the child. In graceful mode the
/// signal handler kills the child (proving drain runs unconditionally)
/// but does NOT call `std::process::exit`, so the helper itself sees
/// `wait_with_output` return and exits 0. This is the path the
/// integration test asserts: graceful drain + clean exit. Lives in
/// `main.rs` (not tests/) because clap is already parsed below and we
/// need to intercept before that.
#[cfg(unix)]
fn signal_test_helper() -> ExitCode {
    use std::io::Write as _;
    use std::process::Command;

    if std::env::var_os("FALLOW_TEST_SIGNAL_HELPER_GRACEFUL").is_some() {
        signal::set_graceful_mode();
    }

    let mut command = Command::new("sleep");
    command.arg("30");
    let child = match signal::ScopedChild::spawn(&mut command) {
        Ok(c) => c,
        Err(err) => {
            let _ = writeln!(std::io::stderr(), "spawn sleep failed: {err}");
            return ExitCode::from(2);
        }
    };
    let pid = child.id();
    let stdout = std::io::stdout();
    let mut lock = stdout.lock();
    let _ = writeln!(lock, "{pid}");
    let _ = lock.flush();
    drop(lock);
    let _ = child.wait_with_output();
    if std::env::var_os("FALLOW_TEST_SIGNAL_HELPER_GRACEFUL").is_some() {
        return ExitCode::SUCCESS;
    }
    std::thread::sleep(std::time::Duration::from_secs(5));
    ExitCode::SUCCESS
}

#[cfg(not(unix))]
fn signal_test_helper() -> ExitCode {
    ExitCode::from(2)
}

fn install_spawn_hooks() {
    fallow_core::churn::set_spawn_hook(signal::scoped_child::output);
    fallow_core::changed_files::set_spawn_hook(signal::scoped_child::output);
}

fn install_signal_handlers() {
    if let Err(err) = signal::install_handlers() {
        use std::io::Write as _;
        let stderr = std::io::stderr();
        let mut lock = stderr.lock();
        let _ = writeln!(lock, "fallow: failed to install signal handlers: {err}");
    }
}

fn args_use_legacy_check_alias<I>(args: I) -> bool
where
    I: IntoIterator<Item = String>,
{
    let value_options = [
        "-r",
        "--root",
        "-c",
        "--config",
        "-f",
        "--format",
        "--output",
        "--threads",
        "--changed-since",
        "--base",
        "--diff-file",
        "--baseline",
        "--parent-run",
        "--save-baseline",
        "-w",
        "--workspace",
        "--changed-workspaces",
        "--group-by",
        "--file",
        "--sarif-file",
        "--only",
        "--skip",
        "--dupes-mode",
        "--dupes-threshold",
        "--dupes-min-tokens",
        "--dupes-min-lines",
        "--dupes-min-occurrences",
        "--dupes-skip-local",
        "--dupes-cross-language",
        "--dupes-ignore-imports",
        "--save-snapshot",
        "--regression-baseline",
        "--tolerance",
        "--save-regression-baseline",
    ];
    let mut skip_next = false;
    for arg in args.into_iter().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "--" {
            break;
        }
        if arg.starts_with('-') {
            let option_name = arg.split_once('=').map_or(arg.as_str(), |(name, _)| name);
            if !arg.contains('=') && value_options.contains(&option_name) {
                skip_next = true;
            }
            continue;
        }
        return arg == "check";
    }
    false
}

fn raw_args_use_legacy_check_alias() -> bool {
    args_use_legacy_check_alias(std::env::args())
}

fn warn_legacy_check_alias_if_needed(used_legacy_check_alias: bool, quiet: bool) {
    if used_legacy_check_alias && !quiet {
        eprintln!("fallow: `check` is deprecated; use `dead-code` instead.");
    }
}

/// Open `path` (creating parent dirs, truncating) and redirect report output
/// there via the ambient sink, forcing color off so the file carries no ANSI
/// codes even when attached to a TTY. Returns the error exit code if the file
/// cannot be created. Backs `--output-file`.
fn redirect_report_to_file(
    path: &std::path::Path,
    output: fallow_config::OutputFormat,
) -> Result<(), ExitCode> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return Err(emit_error(
            &format!(
                "failed to create {} for --output-file: {e}",
                parent.display()
            ),
            2,
            output,
        ));
    }
    match std::fs::File::create(path) {
        Ok(file) => {
            report::sink::set_file_sink(file);
            colored::control::set_override(false);
            Ok(())
        }
        Err(e) => Err(emit_error(
            &format!("failed to open {} for --output-file: {e}", path.display()),
            2,
            output,
        )),
    }
}

/// Flush the report file after rendering and print the stderr confirmation
/// (suppressed by `--quiet`). Returns the error exit code on a write failure.
fn finalize_report_file(
    path: &std::path::Path,
    quiet: bool,
    output: fallow_config::OutputFormat,
) -> Result<(), ExitCode> {
    if let Err(e) = report::sink::flush() {
        return Err(emit_error(
            &format!("failed to write {}: {e}", path.display()),
            2,
            output,
        ));
    }
    // Suppress the confirmation when nothing was rendered to the file (a command
    // that errored before producing output sends its error to stdout, not the
    // file), so we never claim "Report written" over an empty file.
    if !quiet && report::sink::wrote() {
        eprintln!("Report written to {}", path.display());
    }
    Ok(())
}

#[expect(
    clippy::too_many_lines,
    reason = "top-level CLI orchestration: parse, validate, set up output redirection, dispatch, record telemetry"
)]
fn main() -> ExitCode {
    install_signal_handlers();
    install_spawn_hooks();

    if std::env::var_os("FALLOW_TEST_SIGNAL_HELPER").is_some() {
        return signal_test_helper();
    }

    let used_legacy_check_alias = raw_args_use_legacy_check_alias();
    let mut cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => return handle_cli_parse_error(&err),
    };
    warn_legacy_check_alias_if_needed(used_legacy_check_alias, cli.quiet);
    output_envelope::set_legacy_envelope(cli.legacy_envelope);

    if let Some(workspaces) = cli.workspace.as_ref()
        && !workspaces.is_empty()
    {
        report::ci::pr_comment::set_workspace_marker_from_list(workspaces);
    }

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
    if let Some(code) = run_telemetry_command_if_requested(&mut cli, fmt.output) {
        return code;
    }
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

    let diff_source = match report::ci::diff_filter::resolve_diff_source(
        cli.diff_file.as_deref(),
        cli.diff_stdin,
        &root,
    ) {
        Ok(src) => src,
        Err(msg) => return emit_error(&msg, 2, output),
    };
    if diff_source.is_some() && cli.changed_since.is_some() && !quiet {
        eprintln!(
            "fallow: --diff-file precedes --changed-since for line-level \
             filtering; --changed-since still scopes file discovery. Drop \
             one of them to disable this combination."
        );
    }
    let suppress_warnings = quiet
        && matches!(
            diff_source,
            Some(report::ci::diff_filter::DiffSource::EnvVar(_)) | None
        );
    let _ = report::ci::diff_filter::init_shared_diff(diff_source.as_ref(), suppress_warnings);

    if (cli.ci || cli.fail_on_issues || cli.sarif_file.is_some() || cli.output_file.is_some())
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
            "--ci, --fail-on-issues, --sarif-file, and --output-file are only valid with dead-code, dupes, health, security, or bare invocation",
            2,
            output,
        );
    }

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

    let tolerance = match regression::Tolerance::parse(&cli.tolerance) {
        Ok(t) => t,
        Err(e) => return emit_error(&format!("invalid --tolerance: {e}"), 2, output),
    };

    let save_regression_file: Option<std::path::PathBuf> =
        cli.save_regression_baseline.as_ref().and_then(|opt| {
            opt.as_ref()
                .filter(|s| !s.is_empty())
                .map(std::path::PathBuf::from)
        });
    let save_to_config = cli.save_regression_baseline.is_some() && save_regression_file.is_none();

    // Redirect the rendered report to a file (ambient sink read by the report
    // layer's `outln!`). Set up before dispatch so rendering lands in the file;
    // progress and the confirmation stay on stderr.
    if let Some(path) = cli.output_file.as_deref()
        && let Err(code) = redirect_report_to_file(path, output)
    {
        return code;
    }

    let command = cli.command.take();
    let telemetry_workflow = telemetry_workflow_for_command(command.as_ref(), output);
    let telemetry_start = std::time::Instant::now();
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
    let exit_code = match command {
        None => dispatch_bare_command(&dispatch),
        Some(cmd) => dispatch_subcommand(cmd, &dispatch),
    };
    if let Some(path) = cli.output_file.as_deref()
        && let Err(code) = finalize_report_file(path, quiet, output)
    {
        return code;
    }
    record_run_epilogue(
        telemetry_workflow,
        output,
        quiet,
        telemetry_start.elapsed(),
        exit_code,
        cli.parent_run.as_deref(),
    )
}

fn record_run_epilogue(
    telemetry_workflow: telemetry::Workflow,
    output: fallow_config::OutputFormat,
    quiet: bool,
    elapsed: std::time::Duration,
    exit_code: ExitCode,
    parent_run: Option<&str>,
) -> ExitCode {
    let cache_notice_printed = cache_notice::maybe_print_created_notice();
    telemetry::record_workflow(&telemetry::WorkflowRecord {
        workflow: telemetry_workflow,
        output,
        quiet,
        elapsed,
        exit_code,
        parent_run,
    });
    if exit_code == ExitCode::SUCCESS {
        let note_printed = telemetry::maybe_print_opt_in_note(output, quiet);
        update_check::maybe_nudge(output, quiet, note_printed || cache_notice_printed);
    }
    exit_code
}

fn handle_cli_parse_error(err: &clap::Error) -> ExitCode {
    if is_top_level_coverage_unknown_argument(err, std::env::args_os().skip(1)) {
        eprintln!("{}", top_level_coverage_error_message());
        return ExitCode::from(2);
    }

    let exit_code = err.exit_code();
    let _ = err.print();
    ExitCode::from(u8::try_from(exit_code).unwrap_or(2))
}

fn is_top_level_coverage_unknown_argument<I, S>(err: &clap::Error, args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    err.kind() == clap::error::ErrorKind::UnknownArgument && args_have_top_level_coverage(args)
}

fn args_have_top_level_coverage<I, S>(args: I) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    for arg in args {
        let arg = arg.as_ref();
        let arg_str = arg.to_string_lossy();
        if arg == "--coverage" || arg_str.starts_with("--coverage=") {
            return true;
        }
        if is_top_level_command(&arg_str) {
            return false;
        }
    }
    false
}

fn is_top_level_command(arg: &str) -> bool {
    Cli::command().get_subcommands().any(|command| {
        command.get_name() == arg || command.get_all_aliases().any(|alias| alias == arg)
    })
}

fn top_level_coverage_error_message() -> &'static str {
    "error: --coverage is a flag on the `health` subcommand, not a top-level flag.\n\n\
tip: run `fallow health --coverage <coverage-final.json>` for exact CRAP scores, or `fallow coverage setup` to configure runtime coverage."
}

fn run_telemetry_command_if_requested(
    cli: &mut Cli,
    output: fallow_config::OutputFormat,
) -> Option<ExitCode> {
    if matches!(cli.command, Some(Command::Telemetry { .. }))
        && let Some(Command::Telemetry { subcommand }) = cli.command.take()
    {
        return Some(telemetry::run(map_telemetry_subcommand(subcommand), output));
    }
    None
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
        churn_file: cli.churn_file.as_deref(),
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
        dupes_min_tokens: cli.dupes_min_tokens,
        dupes_min_lines: cli.dupes_min_lines,
        dupes_min_occurrences: cli.dupes_min_occurrences,
        dupes_skip_local: cli.dupes_skip_local,
        dupes_cross_language: cli.dupes_cross_language,
        dupes_ignore_imports: cli.dupes_ignore_imports,
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
            re_export_cycles,
            boundary_violations,
            stale_suppressions,
            unused_catalog_entries,
            empty_catalog_groups,
            unresolved_catalog_references,
            unused_dependency_overrides,
            misconfigured_dependency_overrides,
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
                    re_export_cycles,
                    boundary_violations,
                    stale_suppressions,
                    unused_catalog_entries,
                    empty_catalog_groups,
                    unresolved_catalog_references,
                    unused_dependency_overrides,
                    misconfigured_dependency_overrides,
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
        Command::Watch { no_clear } => dispatch_watch(dispatch, no_clear),
        Command::Fix {
            dry_run,
            yes,
            no_create_config,
        } => dispatch_fix(
            dispatch,
            FixDispatchArgs {
                dry_run,
                yes,
                no_create_config,
            },
        ),
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
        Command::Config { path } => config::run_config(root, cli.config.as_deref(), path, output),
        Command::Workspaces => dispatch_list(dispatch, ListDispatchArgs::workspaces()),
        Command::List {
            entry_points,
            files,
            plugins,
            boundaries,
            workspaces,
        } => dispatch_list(
            dispatch,
            ListDispatchArgs {
                entry_points,
                files,
                plugins,
                boundaries,
                workspaces,
            },
        ),
        Command::Dupes {
            mode,
            min_tokens,
            min_lines,
            min_occurrences,
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
                min_occurrences,
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
            complexity_breakdown,
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
            report_only,
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
            let coverage =
                coverage.or_else(|| std::env::var("FALLOW_COVERAGE").ok().map(PathBuf::from));
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
                    complexity_breakdown,
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
                    report_only,
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
        Command::Explain { issue_type } => explain::run_explain(&issue_type.join(" "), output),
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
            gate_marker,
        } => dispatch_audit(
            dispatch,
            &AuditDispatchArgs {
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
                gate_marker,
            },
        ),
        Command::Impact { subcommand } => dispatch_impact(root, quiet, output, subcommand),
        Command::Security { file, gate } => {
            let (output, quiet, fail_on_issues) = dispatch.ci_defaults();
            security::run(&security::SecurityOptions {
                root,
                config_path: &cli.config,
                output,
                no_cache: cli.no_cache,
                threads,
                quiet,
                fail_on_issues,
                sarif_file: cli.sarif_file.as_deref(),
                summary: cli.summary,
                changed_since: cli.changed_since.as_deref(),
                use_shared_diff_index: true,
                workspace: cli.workspace.as_deref(),
                changed_workspaces: cli.changed_workspaces.as_deref(),
                file: file.as_slice(),
                gate,
            })
        }
        Command::Schema => unreachable!("handled above"),
        Command::Migrate {
            toml,
            jsonc,
            dry_run,
            from,
        } => migrate::run_migrate(root, toml, jsonc, dry_run, from.as_deref()),
        Command::License { subcommand } => {
            license::run(&map_license_subcommand(subcommand), output)
        }
        Command::Telemetry { .. } => unreachable!("handled before root validation"),
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

fn dispatch_impact(
    root: &std::path::Path,
    quiet: bool,
    output: fallow_config::OutputFormat,
    subcommand: Option<ImpactCli>,
) -> ExitCode {
    match subcommand {
        Some(ImpactCli::Enable) => {
            let newly = impact::enable(root);
            if !quiet {
                if newly {
                    println!(
                        "Fallow Impact enabled. Each `fallow audit` / pre-commit gate run is \
                         recorded locally in .fallow/impact.json (gitignored, never uploaded)."
                    );
                    println!(
                        "Tip: run `fallow init --hooks` (or add `--gate-marker pre-commit` to \
                         your existing hook's `fallow audit` line) so blocked-then-fixed \
                         commits are recorded as contained."
                    );
                } else {
                    println!("Fallow Impact is already enabled.");
                }
            }
            ExitCode::SUCCESS
        }
        Some(ImpactCli::Disable) => {
            let was_enabled = impact::disable(root);
            if !quiet {
                println!(
                    "{}",
                    if was_enabled {
                        "Fallow Impact disabled. Existing history is retained."
                    } else {
                        "Fallow Impact was already disabled."
                    }
                );
            }
            ExitCode::SUCCESS
        }
        Some(ImpactCli::Status) | None => render_impact_status(root, output),
    }
}

fn render_impact_status(root: &std::path::Path, output: fallow_config::OutputFormat) -> ExitCode {
    let store = impact::load(root);
    let report = impact::build_report(&store);
    let rendered = match output {
        fallow_config::OutputFormat::Json => impact::render_json(&report),
        fallow_config::OutputFormat::Markdown => impact::render_markdown(&report),
        fallow_config::OutputFormat::Human => impact::render_human(&report),
        fallow_config::OutputFormat::Sarif
        | fallow_config::OutputFormat::Compact
        | fallow_config::OutputFormat::CodeClimate
        | fallow_config::OutputFormat::PrCommentGithub
        | fallow_config::OutputFormat::PrCommentGitlab
        | fallow_config::OutputFormat::ReviewGithub
        | fallow_config::OutputFormat::ReviewGitlab
        | fallow_config::OutputFormat::Badge => {
            return crate::error::emit_error(
                "impact supports human, json, and markdown output",
                2,
                output,
            );
        }
    };
    println!("{rendered}");
    ExitCode::SUCCESS
}

fn telemetry_workflow_for_command(
    command: Option<&Command>,
    output: fallow_config::OutputFormat,
) -> telemetry::Workflow {
    match command {
        None | Some(Command::Flags { .. }) => telemetry::Workflow::CodeQualityReview,
        Some(Command::Check { .. }) => telemetry::Workflow::DeadCode,
        Some(Command::Dupes { .. }) => telemetry::Workflow::Dupes,
        Some(Command::Health { .. }) => telemetry::Workflow::Health,
        Some(Command::Audit { .. }) => telemetry::Workflow::Audit,
        Some(Command::Ci { .. }) => match output {
            fallow_config::OutputFormat::ReviewGitlab
            | fallow_config::OutputFormat::PrCommentGitlab
            | fallow_config::OutputFormat::CodeClimate => telemetry::Workflow::GitlabCi,
            _ => telemetry::Workflow::GithubAction,
        },
        Some(Command::Coverage { .. }) => telemetry::Workflow::RuntimeCoverageSetup,
        Some(Command::Impact { .. }) => telemetry::Workflow::Impact,
        Some(Command::Security { .. }) => telemetry::Workflow::Security,
        Some(Command::Fix { .. }) => telemetry::Workflow::Fix,
        Some(Command::Explain { .. }) => telemetry::Workflow::Explain,
        Some(
            Command::List { .. }
            | Command::Workspaces
            | Command::Watch { .. }
            | Command::Init { .. }
            | Command::Hooks { .. }
            | Command::ConfigSchema
            | Command::PluginSchema
            | Command::Config { .. }
            | Command::Schema
            | Command::CiTemplate { .. }
            | Command::Migrate { .. }
            | Command::License { .. }
            | Command::Telemetry { .. }
            | Command::SetupHooks { .. },
        ) => telemetry::Workflow::Unknown,
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

fn map_telemetry_subcommand(sub: TelemetryCli) -> telemetry::TelemetryCommand {
    match sub {
        TelemetryCli::Status => telemetry::TelemetryCommand::Status,
        TelemetryCli::Enable => telemetry::TelemetryCommand::Enable,
        TelemetryCli::Disable => telemetry::TelemetryCommand::Disable,
        TelemetryCli::Inspect { example } => telemetry::TelemetryCommand::Inspect { example },
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
        CoverageCli::UploadStaticFindings {
            api_key,
            api_endpoint,
            project_id,
            git_sha,
            allow_dirty,
            dry_run,
            ignore_upload_errors,
        } => {
            coverage::CoverageSubcommand::UploadStaticFindings(coverage::UploadStaticFindingsArgs {
                api_key: api_key.clone(),
                api_endpoint: api_endpoint.clone(),
                project_id: project_id.clone(),
                git_sha: git_sha.clone(),
                allow_dirty: *allow_dirty,
                dry_run: *dry_run,
                ignore_upload_errors: *ignore_upload_errors,
            })
        }
    }
}

struct CheckDispatchArgs {
    filters: IssueFilters,
    trace_opts: TraceOptions,
    include_dupes: bool,
    top: Option<usize>,
    file: Vec<std::path::PathBuf>,
}

#[derive(Clone, Copy)]
struct ListDispatchArgs {
    entry_points: bool,
    files: bool,
    plugins: bool,
    boundaries: bool,
    workspaces: bool,
}

impl ListDispatchArgs {
    fn workspaces() -> Self {
        Self {
            entry_points: false,
            files: false,
            plugins: false,
            boundaries: false,
            workspaces: true,
        }
    }
}

fn dispatch_watch(dispatch: &DispatchContext<'_>, no_clear: bool) -> ExitCode {
    let cli = dispatch.cli;
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::DeadCode) {
        Ok(production) => production,
        Err(code) => return code,
    };
    watch::run_watch(&watch::WatchOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output: dispatch.output,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        production,
        clear_screen: !no_clear,
        explain: cli.explain,
        include_entry_exports: cli.include_entry_exports,
    })
}

#[derive(Clone, Copy)]
struct FixDispatchArgs {
    dry_run: bool,
    yes: bool,
    no_create_config: bool,
}

fn dispatch_fix(dispatch: &DispatchContext<'_>, args: FixDispatchArgs) -> ExitCode {
    let cli = dispatch.cli;
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::DeadCode) {
        Ok(production) => production,
        Err(code) => return code,
    };
    fix::run_fix(&fix::FixOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output: dispatch.output,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        dry_run: args.dry_run,
        yes: args.yes,
        production,
        no_create_config: args.no_create_config,
    })
}

fn dispatch_list(dispatch: &DispatchContext<'_>, args: ListDispatchArgs) -> ExitCode {
    let cli = dispatch.cli;
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::DeadCode) {
        Ok(production) => production,
        Err(code) => return code,
    };
    list::run_list(&ListOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output: dispatch.output,
        threads: dispatch.threads,
        no_cache: cli.no_cache,
        entry_points: args.entry_points,
        files: args.files,
        plugins: args.plugins,
        boundaries: args.boundaries,
        workspaces: args.workspaces,
        production,
    })
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
        diff_index: None,
        use_shared_diff_index: true,
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
    min_occurrences: Option<usize>,
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
        min_occurrences: args.min_occurrences,
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
        diff_index: None,
        use_shared_diff_index: true,
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

struct AuditDispatchArgs {
    production_dead_code: bool,
    production_health: bool,
    production_dupes: bool,
    dead_code_baseline: Option<PathBuf>,
    health_baseline: Option<PathBuf>,
    dupes_baseline: Option<PathBuf>,
    max_crap: Option<f64>,
    coverage: Option<PathBuf>,
    coverage_root: Option<PathBuf>,
    gate: Option<AuditGateArg>,
    runtime_coverage: Option<PathBuf>,
    min_invocations_hot: u64,
    gate_marker: Option<String>,
}

struct ResolvedAuditInputs {
    audit_cfg: fallow_config::AuditConfig,
    cache_dir: PathBuf,
    production: ProductionModes,
    dead_code_baseline: Option<PathBuf>,
    health_baseline: Option<PathBuf>,
    dupes_baseline: Option<PathBuf>,
    coverage: Option<PathBuf>,
}

fn dispatch_audit(dispatch: &DispatchContext<'_>, args: &AuditDispatchArgs) -> ExitCode {
    let cli = dispatch.cli;
    let output = dispatch.output;

    if cli.baseline.is_some() || cli.save_baseline.is_some() {
        return emit_error(
            "audit uses per-analysis baselines. Use --dead-code-baseline, --health-baseline, or --dupes-baseline (or save them with `fallow dead-code|health|dupes --save-baseline <file>`)",
            2,
            output,
        );
    }

    let inputs = match resolve_audit_inputs(dispatch, args) {
        Ok(inputs) => inputs,
        Err(code) => return code,
    };

    run_resolved_audit(dispatch, args, &inputs)
}

fn resolve_audit_inputs(
    dispatch: &DispatchContext<'_>,
    args: &AuditDispatchArgs,
) -> Result<ResolvedAuditInputs, ExitCode> {
    let cli = dispatch.cli;
    let root = dispatch.root;
    let output = dispatch.output;
    let config = load_config(
        root,
        &cli.config,
        output,
        cli.no_cache,
        dispatch.threads,
        cli.production,
        dispatch.quiet,
    )?;
    let cache_dir = config.cache_dir.clone();
    let audit_cfg = config.audit;
    let production = resolve_production_modes(
        cli,
        root,
        output,
        args.production_dead_code,
        args.production_health,
        args.production_dupes,
    )?;
    let resolved_dead_code_baseline = resolve_audit_baseline_path(
        root,
        args.dead_code_baseline.as_deref(),
        audit_cfg.dead_code_baseline.as_deref(),
    );
    let resolved_health_baseline = resolve_audit_baseline_path(
        root,
        args.health_baseline.as_deref(),
        audit_cfg.health_baseline.as_deref(),
    );
    let resolved_dupes_baseline = resolve_audit_baseline_path(
        root,
        args.dupes_baseline.as_deref(),
        audit_cfg.dupes_baseline.as_deref(),
    );
    let coverage = args
        .coverage
        .clone()
        .or_else(|| std::env::var("FALLOW_COVERAGE").ok().map(PathBuf::from));

    Ok(ResolvedAuditInputs {
        audit_cfg,
        cache_dir,
        production,
        dead_code_baseline: resolved_dead_code_baseline,
        health_baseline: resolved_health_baseline,
        dupes_baseline: resolved_dupes_baseline,
        coverage,
    })
}

fn run_resolved_audit(
    dispatch: &DispatchContext<'_>,
    args: &AuditDispatchArgs,
    inputs: &ResolvedAuditInputs,
) -> ExitCode {
    let cli = dispatch.cli;
    audit::run_audit(
        &audit::AuditOptions {
            root: dispatch.root,
            config_path: &cli.config,
            cache_dir: &inputs.cache_dir,
            output: dispatch.output,
            no_cache: cli.no_cache,
            threads: dispatch.threads,
            quiet: dispatch.quiet,
            changed_since: cli.changed_since.as_deref(),
            production: cli.production,
            production_dead_code: Some(inputs.production.dead_code),
            production_health: Some(inputs.production.health),
            production_dupes: Some(inputs.production.dupes),
            workspace: cli.workspace.as_deref(),
            changed_workspaces: cli.changed_workspaces.as_deref(),
            explain: cli.explain,
            explain_skipped: cli.explain_skipped,
            performance: cli.performance,
            group_by: cli.group_by,
            dead_code_baseline: inputs.dead_code_baseline.as_deref(),
            health_baseline: inputs.health_baseline.as_deref(),
            dupes_baseline: inputs.dupes_baseline.as_deref(),
            max_crap: args.max_crap,
            coverage: inputs.coverage.as_deref(),
            coverage_root: args.coverage_root.as_deref(),
            gate: args.gate.map_or(inputs.audit_cfg.gate, Into::into),
            include_entry_exports: cli.include_entry_exports,
            runtime_coverage: args.runtime_coverage.as_deref(),
            min_invocations_hot: args.min_invocations_hot,
        },
        args.gate_marker.as_deref(),
    )
}

struct HealthDispatchArgs<'a> {
    max_cyclomatic: Option<u16>,
    max_cognitive: Option<u16>,
    max_crap: Option<f64>,
    top: Option<usize>,
    sort: health::SortBy,
    complexity: bool,
    complexity_breakdown: bool,
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
    report_only: bool,
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
        complexity_breakdown,
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
        report_only,
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
    if report_only && (min_score.is_some() || min_severity.is_some()) {
        return emit_error(
            "--report-only cannot be combined with --min-score or --min-severity. \
             --report-only always exits 0; drop it to gate on score/severity, or \
             drop the gate flags to stay advisory.",
            2,
            output,
        );
    }
    let targets = targets || effort.is_some();
    let badge_format = matches!(output, fallow_config::OutputFormat::Badge);
    let score = score || min_score.is_some() || trend || badge_format;
    let snapshot_requested = save_snapshot.is_some();
    let any_section = complexity || file_scores || coverage_gaps || hotspots || targets || score;
    let eff_score = if any_section { score } else { true } || snapshot_requested;
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
        diff_index: None,
        use_shared_diff_index: true,
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        baseline: cli.baseline.as_deref(),
        save_baseline: cli.save_baseline.as_deref(),
        complexity: eff_complexity,
        complexity_breakdown,
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
        report_only,
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
        churn_file: cli.churn_file.as_deref(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Validates that the CLI definition has no flag name collisions, missing
    /// fields, or other structural errors. Catches issues like a global alias
    /// `--base` colliding with a subcommand's `--base` flag.
    #[test]
    fn cli_definition_has_no_flag_collisions() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    /// The high-value commands each get their own telemetry workflow instead of
    /// the `Unknown` catch-all, so "is anyone using `fallow impact`?" is
    /// answerable. Admin / low-signal commands stay `Unknown`.
    #[test]
    fn high_value_commands_route_to_distinct_workflows() {
        use clap::Parser;
        use fallow_config::OutputFormat;

        let distinct = [
            (vec!["fallow", "impact"], telemetry::Workflow::Impact),
            (vec!["fallow", "security"], telemetry::Workflow::Security),
            (vec!["fallow", "fix"], telemetry::Workflow::Fix),
            (
                vec!["fallow", "explain", "unused-exports"],
                telemetry::Workflow::Explain,
            ),
        ];
        for (argv, expected) in distinct {
            let cli = Cli::try_parse_from(&argv).expect("argv parses");
            assert_eq!(
                telemetry_workflow_for_command(cli.command.as_ref(), OutputFormat::Json),
                expected,
                "{argv:?} should map to {expected:?}"
            );
        }

        for argv in [
            vec!["fallow", "list"],
            vec!["fallow", "config"],
            vec!["fallow", "schema"],
        ] {
            let cli = Cli::try_parse_from(&argv).expect("argv parses");
            assert_eq!(
                telemetry_workflow_for_command(cli.command.as_ref(), OutputFormat::Json),
                telemetry::Workflow::Unknown,
                "{argv:?} should stay Unknown"
            );
        }
    }

    /// `-v`, `-V`, and `--version` must all trigger clap's Version action so
    /// the version prints regardless of which spelling the user reaches for
    /// (issue #916). clap surfaces a Version action from `try_get_matches_from`
    /// as the `DisplayVersion` error kind.
    #[test]
    fn version_flag_accepts_lower_v_upper_v_and_long() {
        use clap::CommandFactory;
        for argv in [["fallow", "-v"], ["fallow", "-V"], ["fallow", "--version"]] {
            let err = Cli::command()
                .try_get_matches_from(argv)
                .expect_err("version flag should short-circuit parsing");
            assert_eq!(
                err.kind(),
                clap::error::ErrorKind::DisplayVersion,
                "{argv:?} should trigger the Version action"
            );
        }
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

    #[test]
    fn top_level_help_groups_commands_by_workflow() {
        use clap::CommandFactory;
        let help = Cli::command().render_long_help().to_string();
        let expected_order = [
            "Analysis:",
            "  dead-code",
            "  dupes",
            "  health",
            "  flags",
            "  security",
            "  audit",
            "Workflow:",
            "  watch",
            "  fix",
            "Project inspection:",
            "  list",
            "  workspaces",
            "  explain",
            "  impact",
            "Setup and configuration:",
            "  init",
            "  migrate",
            "  config",
            "  config-schema",
            "  plugin-schema",
            "Automation and CI:",
            "  ci",
            "  ci-template",
            "  hooks",
            "  setup-hooks",
            "Runtime coverage:",
            "  coverage",
            "  license",
            "Reference:",
            "  schema",
            "  help",
            "Options:",
        ];
        let mut cursor = 0;
        for needle in expected_order {
            let Some(offset) = help[cursor..].find(needle) else {
                panic!("top-level help missing `{needle}` after byte {cursor}:\n{help}");
            };
            cursor += offset + needle.len();
        }
    }

    #[test]
    fn programmatic_common_options_track_analysis_affecting_cli_globals() {
        use clap::CommandFactory;

        let cli_flags: std::collections::BTreeSet<String> = Cli::command()
            .get_arguments()
            .filter(|arg| arg.is_global_set())
            .filter_map(|arg| arg.get_long().map(str::to_owned))
            .filter(|name| {
                matches!(
                    name.as_str(),
                    "root"
                        | "config"
                        | "no-cache"
                        | "threads"
                        | "changed-since"
                        | "diff-file"
                        | "production"
                        | "workspace"
                        | "changed-workspaces"
                        | "explain"
                        | "legacy-envelope"
                )
            })
            .collect();
        let programmatic_flags: std::collections::BTreeSet<String> =
            fallow_cli::programmatic::COMMON_ANALYSIS_OPTION_FLAGS
                .iter()
                .map(|flag| (*flag).to_owned())
                .collect();

        assert_eq!(programmatic_flags, cli_flags);
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

    #[test]
    fn emit_error_returns_given_exit_code() {
        let code = emit_error("test error", 2, fallow_config::OutputFormat::Human);
        assert_eq!(code, ExitCode::from(2));
    }

    #[test]
    fn top_level_coverage_flag_reports_health_hint() {
        let Err(err) = Cli::try_parse_from(["fallow", "--coverage"]) else {
            panic!("top-level --coverage should fail to parse");
        };
        assert!(is_top_level_coverage_unknown_argument(
            &err,
            [std::ffi::OsString::from("--coverage")]
        ));
        let message = top_level_coverage_error_message();
        assert!(message.contains("`health` subcommand"));
        assert!(message.contains("fallow health --coverage <coverage-final.json>"));
        assert!(message.contains("fallow coverage setup"));
    }

    #[test]
    fn subcommand_coverage_flag_keeps_regular_clap_error() {
        let Err(err) = Cli::try_parse_from(["fallow", "dead-code", "--coverage"]) else {
            panic!("dead-code --coverage should fail to parse");
        };
        assert!(!is_top_level_coverage_unknown_argument(
            &err,
            [
                std::ffi::OsString::from("dead-code"),
                std::ffi::OsString::from("--coverage"),
            ]
        ));
    }

    #[test]
    fn legacy_check_alias_detection_ignores_option_values() {
        assert!(args_use_legacy_check_alias(vec![
            "fallow".to_string(),
            "check".to_string(),
            "--summary".to_string(),
        ]));
        assert!(!args_use_legacy_check_alias(vec![
            "fallow".to_string(),
            "--root".to_string(),
            "check".to_string(),
            "dead-code".to_string(),
        ]));
        assert!(!args_use_legacy_check_alias(vec![
            "fallow".to_string(),
            "dead-code".to_string(),
            "--file".to_string(),
            "check".to_string(),
        ]));
    }

    #[test]
    fn format_parsing_covers_all_variants() {
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
