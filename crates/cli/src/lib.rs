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

use std::io::IsTerminal as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod api;
#[cfg(test)]
mod architecture_boundaries;
mod audit;
mod audit_brief;
mod audit_decision_surface;
mod audit_focus;
mod audit_walkthrough;
mod base_worktree;
mod walkthrough_state;
use fallow_engine::baseline;
mod cache_notice;
mod check;
mod ci;
mod ci_template;
mod cli_format;
mod cli_hooks;
mod cli_impact;
mod cli_production;
mod cli_report;
mod cli_startup;
pub use fallow_engine::codeowners;
mod combined;
mod config;
mod coverage;
mod dupes;
pub mod explain;
mod fix;
mod flags;
mod guard;
mod health;
mod impact;
mod init;
mod inspect;
mod json_style;
mod license;
mod list;
mod migrate;
mod onboarding;
#[cfg(test)]
mod output_envelope;
mod output_runtime;
mod path_util;
mod plugin_check;
mod rayon_pool;
mod regression;
pub mod report;
mod rule_pack;
mod runtime_support;
mod schema;
mod security;
mod security_help;
mod setup_hooks;
mod signal;
mod suppressions;
mod task_matrix;
mod telemetry;
mod trace_chain;
mod update_check;
use fallow_engine::validate;
use fallow_engine::vital_signs;
mod cli_telemetry;
mod watch;

use check::{CheckOptions, IssueFilters, TraceOptions};
/// Structured error output for CLI and JSON formats.
pub mod error;
#[cfg(test)]
use cli_format::parse_format_arg;
use cli_format::{Format, FormatConfig};
use cli_hooks::{HooksCli, run_hooks_command};
use cli_impact::{ImpactCli, ImpactCrossRepoOpts, ImpactSortCli, dispatch_impact};
use cli_production::{ProductionModes, resolve_production_modes};
#[cfg(test)]
use cli_startup::build_tracing_filter;
use cli_startup::{
    bare_coverage_subcommand_error_message, cli_has_bare_coverage_input, parse_cli_args,
    run_pre_dispatch_checks, setup_tracing, validate_inputs,
};
#[cfg(test)]
use cli_telemetry::TelemetryRun;
#[cfg(test)]
use cli_telemetry::{fallback_failure_reason_for, telemetry_workflow_for_command};
use cli_telemetry::{record_run_epilogue, start_telemetry_run};
use dupes::{DupesMode, DupesOptions};
use error::emit_error;
use health::{HealthOptions, SortBy};
use list::ListOptions;
pub use runtime_support::{AnalysisKind, GroupBy};
pub(crate) use runtime_support::{
    ConfigLoadOptions, LoadConfigArgs, build_ownership_resolver, load_config,
    load_config_for_analysis,
};
#[cfg(test)]
use security_help::{SECURITY_UNSUPPORTED_GLOBAL_LONGS, SecurityHelpTarget};
use security_help::{render_security_help, security_help_target};

const DEFAULT_MIN_INVOCATIONS_HOT: u64 = 100;

const TOP_LEVEL_HELP_TEMPLATE: &str =
    "{about-with-newline}\n{usage-heading} {usage}{after-help}\n\nOptions:\n{options}";

const TOP_LEVEL_AFTER_HELP: &str = "\
Analysis:
  dead-code      Analyze unused code, dependency hygiene, and architecture cycles
  dupes          Find copy-paste and structural code duplication
  health         Analyze complexity, maintainability, hotspots, and coverage gaps
  flags          Detect feature flag usage patterns
  security       Surface local security candidates for agent verification (opt-in)
  audit          Review changed files for dead code, complexity, duplication, and styling

Workflow:
  watch          Re-run analysis as files change
  fix            Auto-fix safe unused-code findings

Project inspection:
  list           List discovered files, entry points, plugins, boundaries, and workspaces
  inspect        Inspect one file or exported symbol as a bundled evidence query
  workspaces     Show monorepo workspace discovery diagnostics
  explain        Explain one issue type without running analysis
  suppressions   List active fallow-ignore suppression markers
  impact         Show what fallow has done for you (opt-in, local-only)

Setup and configuration:
  init              Create a fallow config, optionally with a Git hook
  audit-cache       Maintain reusable audit base-snapshot caches
  recommend         Recommend a project-tailored config for an agent to author
  migrate           Migrate knip, jscpd, or stylelint config to fallow
  config            Show the resolved config and loaded config file
  config-schema     Print the fallow config JSON Schema
  plugin-schema     Print the external plugin JSON Schema
  plugin-check      Dry-run external plugins and report what they seed
  rule-pack-schema  Print the rule pack JSON Schema

Automation and CI:
  ci             Build PR/MR feedback envelopes
  ci-template    Print or vendor CI integration templates
  report         Re-render a saved --format json results file (GitHub formats)
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
Use --only/--skip to select specific analyses.

When the agent is about to...
  delete an \"unused\" export or file        fallow dead-code --trace <file>:<export>
  delete an \"unused\" dependency            fallow dead-code --trace-dependency <name>
  commit or open a PR                      fallow audit --base <ref>
  prioritize refactoring                   fallow health --hotspots --targets
  ask who owns code                        fallow health --ownership
  check untested-but-reachable code        fallow health --coverage-gaps
  consolidate duplication                  fallow dupes --trace dup:<fingerprint>
  find feature flags                       fallow flags
  check architecture rules before editing  fallow guard <files>
  surface security candidates              fallow security
  inspect a target before editing          fallow inspect --file <path>
  understand a finding                     fallow explain <issue-type>
  scope a monorepo                         --workspace <glob> / --changed-workspaces <ref>";

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

    /// Allow trusted config files to extend HTTPS URLs
    #[arg(long, global = true)]
    allow_remote_extends: bool,

    /// Output format (alias: --output)
    #[arg(
        short,
        long,
        visible_alias = "output",
        global = true,
        default_value = "human"
    )]
    format: Format,

    /// Indent JSON output for manual inspection. Requires the final output format to be JSON.
    #[arg(long, global = true)]
    pretty: bool,

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

    /// Skip source files larger than this many megabytes (default 5) instead of
    /// parsing them, guarding against the out-of-memory blowup a single
    /// multi-MB generated/vendored/bundled file causes on large repos. Use `0`
    /// for no limit. Declaration files (`.d.ts`) are always analyzed. Skipped
    /// files are reported and excluded from every analysis. Also settable via
    /// `FALLOW_MAX_FILE_SIZE`.
    #[arg(long = "max-file-size", value_name = "MB", global = true)]
    max_file_size: Option<u32>,

    /// Compare against a previously saved baseline file
    #[arg(long, global = true)]
    baseline: Option<PathBuf>,

    /// Correlate this run with a previous telemetry analysis run.
    ///
    /// Used only for opt-in telemetry follow-up measurement. The value is not
    /// interpreted as a path, repository, package, or user identifier. Hidden
    /// from `--help`; agents receive the correlation token from JSON output.
    #[arg(long, global = true, value_name = "RUN_ID", hide = true)]
    parent_run: Option<String>,

    /// Save the current results as a baseline file
    #[arg(long, global = true)]
    save_baseline: Option<PathBuf>,

    /// Production mode: exclude test/story/dev files, only start/build scripts,
    /// report type-only dependencies
    #[arg(long, global = true)]
    production: bool,

    /// Force production mode OFF for every analysis, overriding a project
    /// config's `production: true` (and `FALLOW_PRODUCTION`). Conflicts with
    /// `--production`.
    #[arg(long = "no-production", global = true, conflicts_with = "production")]
    no_production: bool,

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

    /// Prefix prepended to every path in the CI-facing formats
    /// (`github-annotations`, `github-summary`, `codeclimate`,
    /// `review-github`, `review-gitlab`). CI platforms address files by
    /// repository-root-relative path, so when the analyzed project lives in a
    /// subdirectory (e.g. `packages/app/`), paths need that offset. fallow
    /// detects the offset via the git toplevel automatically; this flag
    /// overrides the detection. Pass an empty string to disable rebasing and
    /// emit paths relative to `--root`.
    #[arg(
        long = "report-path-prefix",
        visible_alias = "annotations-path-prefix",
        global = true,
        value_name = "PREFIX"
    )]
    report_path_prefix: Option<String>,

    /// Fail if issue count increased beyond tolerance compared to a regression baseline.
    #[arg(long, global = true)]
    fail_on_regression: bool,

    /// Allowed issue count increase before a regression is flagged.
    #[arg(long, global = true, value_name = "TOLERANCE", default_value = "0")]
    tolerance: String,

    /// Path to the regression baseline file.
    #[arg(long, global = true, value_name = "PATH")]
    regression_baseline: Option<PathBuf>,

    /// Save the current issue counts as a regression baseline. Omit PATH to
    /// update regression.baseline in the discovered fallow config, or create
    /// .fallowrc.json when none exists. Provide PATH to write a standalone file.
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

    /// Exclude module wiring from duplicate detection in combined mode
    /// (default). Pass `--dupes-no-ignore-imports` to count it again.
    #[arg(long = "dupes-ignore-imports", global = true)]
    dupes_ignore_imports: bool,

    /// Count module wiring as clone candidates in combined mode (opt out of the
    /// default exclusion).
    #[arg(
        long = "dupes-no-ignore-imports",
        global = true,
        conflicts_with = "dupes_ignore_imports"
    )]
    dupes_no_ignore_imports: bool,

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

    /// Path to Istanbul coverage data for exact CRAP scores in combined mode.
    /// Also settable via `FALLOW_COVERAGE` or `health.coverage`.
    #[arg(long, value_name = "PATH")]
    coverage: Option<PathBuf>,

    /// Absolute prefix to strip from Istanbul file paths in combined mode.
    /// Also settable via `FALLOW_COVERAGE_ROOT` or `health.coverageRoot`.
    #[arg(long = "coverage-root", value_name = "PATH")]
    coverage_root: Option<PathBuf>,

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

        /// Only report unused store members
        #[arg(long)]
        unused_store_members: bool,

        /// Only report unprovided injects
        #[arg(long)]
        unprovided_injects: bool,

        /// Only report unrendered components
        #[arg(long)]
        unrendered_components: bool,

        /// Only report unused component props
        #[arg(long)]
        unused_component_props: bool,

        /// Only report unused component emits
        #[arg(long)]
        unused_component_emits: bool,

        /// Only report unused component inputs
        #[arg(long)]
        unused_component_inputs: bool,

        /// Only report unused component outputs
        #[arg(long)]
        unused_component_outputs: bool,

        /// Only report unused Svelte dispatched events
        #[arg(long)]
        unused_svelte_events: bool,

        /// Only report unused server actions
        #[arg(long)]
        unused_server_actions: bool,

        /// Only report unused SvelteKit load() data keys
        #[arg(long)]
        unused_load_data_keys: bool,

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

        /// Only report rule-pack policy violations
        #[arg(long)]
        policy_violations: bool,

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

        /// Compute the impact closure for a file (the transitive
        /// affected-but-not-in-diff set + coordination gap). Walks reverse-deps
        /// and re-export chains; powers the `inspect_target` MCP tool.
        #[arg(long, value_name = "PATH")]
        impact_closure: Option<String>,

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

    /// Inspect one file or exported symbol as a bundled evidence query
    Inspect {
        /// File to inspect.
        #[arg(
            long,
            value_name = "PATH",
            conflicts_with = "symbol",
            required_unless_present = "symbol"
        )]
        file: Option<String>,

        /// Exported symbol to inspect, formatted as FILE:EXPORT.
        #[arg(long, value_name = "FILE:EXPORT", conflicts_with = "file")]
        symbol: Option<String>,

        /// OPT-IN: also attach the best-effort symbol-level call chain
        /// (`fallow trace`) as the `symbol_chain` evidence section. Only
        /// meaningful for a `--symbol` target. Default off (best-effort,
        /// syntactic, OFF the ranked path).
        #[arg(long)]
        symbol_chain: bool,

        /// OPT-IN: attach target-level git churn evidence from the health
        /// hotspot subsystem. Default off to avoid git-history latency.
        #[arg(long)]
        churn: bool,
    },

    /// Trace a symbol's call chain (best-effort, syntactic; OFF the ranked path)
    ///
    /// Walks callers UP (modules that import the symbol) and callees DOWN
    /// (import-symbol edges + intra-module call sites) via the module graph,
    /// bounded by `--depth`. Symbol-level chains are labeled best-effort per
    /// ADR-001: resolved-vs-unresolved callees are reported honestly, never
    /// silently dropped. The result is its OWN surface, NOT folded into the
    /// ranked brief and NEVER an input to the focus map / ranking.
    Trace {
        /// Target symbol, formatted as FILE:SYMBOL (e.g. src/utils.ts:formatDate).
        #[arg(value_name = "FILE:SYMBOL")]
        symbol: String,

        /// Walk UP to callers (modules that import the symbol). When neither
        /// `--callers` nor `--callees` is set, both directions are walked.
        #[arg(long)]
        callers: bool,

        /// Walk DOWN to callees (the symbol's module's import-symbol edges plus
        /// unresolved call sites). When neither flag is set, both are walked.
        #[arg(long)]
        callees: bool,

        /// Chain depth bound for both directions (default 2). Symbol-level is
        /// best-effort, so a shallow bound keeps the trace legible.
        #[arg(long, value_name = "N")]
        depth: Option<u32>,
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

    /// Initialize a .fallowrc.json configuration file, AGENTS.md guide, or git
    /// pre-commit hook. Use `.fallowrc.jsonc` for editor-native JSON-with-comments
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

        /// Scaffold a starter AGENTS.md guidance file for coding agents
        #[arg(long, conflicts_with_all = ["toml", "hooks", "branch"])]
        agents: bool,

        /// Scaffold a shell-level pre-commit git hook in `.git/hooks/` that
        /// runs fallow on changed files. Alias for
        /// `fallow hooks install --target git`.
        #[arg(long)]
        hooks: bool,

        /// Fallback base branch/ref for the pre-commit hook when no upstream is set
        #[arg(long, requires = "hooks")]
        branch: Option<String>,

        /// Record that this project deliberately stays unconfigured: persists a
        /// decline so the first-contact setup hint and the `setup` next-step
        /// stop appearing here. Writes no config file; idempotent
        #[arg(long, conflicts_with_all = ["toml", "agents", "hooks", "branch"])]
        decline: bool,
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

    /// Dry-run external plugins: report what each activated and seeded
    PluginCheck,

    /// Print the JSON Schema for rule pack files
    RulePackSchema,

    /// Manage declarative rule packs (policy-as-code)
    RulePack {
        #[command(subcommand)]
        subcommand: RulePackCli,
    },

    /// Show which architecture rules apply to files before changing them.
    Guard {
        /// Files to report on (root-relative or absolute; may not exist yet)
        #[arg(required = true, num_args = 1..)]
        files: Vec<String>,
    },

    /// Show the resolved config and which config file was loaded
    ///
    /// Walks up from the project root looking for `.fallowrc.json`,
    /// `.fallowrc.jsonc`, `fallow.toml`, or `.fallow.toml`, resolves `extends`, and prints
    /// the final config as JSON. Use `--path` to print only the config
    /// file path (useful in shell scripts). The default view always exits 0:
    /// it prints the loaded config, or, on a zero-config project, the effective
    /// defaults (fully supported). `--path` exits 3 when no config file exists,
    /// since there is no path to report.
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

    /// Recommend a project-tailored config for an agent to author.
    ///
    /// Read-only. Inspects the project (frameworks, workspace layout, tooling)
    /// and emits what fallow detected, a safe proposed config, and a list of
    /// decisions split into auto (decided from detection), default (a disclosed
    /// overridable default), and taste (a genuinely subjective choice surfaced
    /// to the user as an open question). Honors `--root` and `--format`.
    Recommend,

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

        /// Exclude module wiring from clone detection (default; covers imports,
        /// re-exports, and top-level static require bindings). Pass
        /// `--no-ignore-imports` to count it again.
        #[arg(long)]
        ignore_imports: bool,

        /// Count module wiring as clone candidates (opt out of the default
        /// exclusion).
        #[arg(long, conflicts_with = "ignore_imports")]
        no_ignore_imports: bool,

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

        /// Add structural CSS analytics: specificity hotspots, !important density,
        /// over-complex selectors, deep nesting, and conservative cleanup
        /// candidates. Standard CSS is parsed structurally; preprocessor sources
        /// are scanned only where fallow can avoid expanding Sass/Less semantics.
        #[arg(long)]
        css: bool,

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
        min_severity: Option<HealthSeverityCli>,

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

    /// List active fallow-ignore suppression markers (read-only inventory)
    ///
    /// Shows every `fallow-ignore-next-line` and `fallow-ignore-file` marker
    /// present in analyzed files, grouped per file with line, kind, level,
    /// and reason, plus project totals and a stale cross-reference against
    /// this run's stale-suppression findings. A governance surface, not a
    /// detector: always exits 0. Honors `--root`, `--format {human,json}`,
    /// `--workspace`, `--changed-workspaces`, `--changed-since`, and
    /// `--quiet`.
    Suppressions {
        /// Only list suppressions in the specified files. Accepts multiple values.
        #[arg(long, value_name = "PATH")]
        file: Vec<std::path::PathBuf>,
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

    /// Audit changed files for dead code, complexity, duplication, and styling.
    ///
    /// Purpose-built for reviewing AI-generated code and PR quality gates.
    /// Combines dead-code + complexity + duplication + styling scoped to
    /// changed files and returns a verdict (pass/warn/fail).
    ///
    /// `fallow audit` answers "will CI block this?": it gates (exit 1 on a
    /// fail verdict). The `review` alias plus `--brief` answer "where do I
    /// look?": the same analysis rendered as a deterministic orientation brief
    /// that ALWAYS exits 0, so a reviewer or agent can read it regardless of
    /// the verdict. `--format` is orthogonal to `--brief`.
    /// When `--changed-since`/`--base` is unset, the base is the git merge-base
    /// against the branch's upstream or the remote default (`origin/HEAD`,
    /// `origin/main`, `origin/master`); set `FALLOW_AUDIT_BASE` to pin it.
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
    #[command(visible_alias = "review")]
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

        /// Disable styling analytics in audit.
        #[arg(long = "no-css")]
        no_css: bool,

        /// Enable deep CSS analysis for audit explicitly: project-wide styling
        /// reachability, narrowed back to changed anchors. Deep CSS is on by
        /// default; use this to override `audit.cssDeep = false`.
        #[arg(long)]
        css_deep: bool,

        /// Disable deep CSS analysis while keeping local styling analytics on.
        #[arg(long = "no-css-deep")]
        no_css_deep: bool,

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

        /// Render the deterministic review brief instead of the gating audit
        /// report. The brief answers "where do I look?" rather than "will CI
        /// block this?", runs the same analysis, and ALWAYS exits 0 (the
        /// verdict is carried informationally). Implied by `fallow review`.
        /// Orthogonal to `--format`.
        #[arg(long)]
        brief: bool,

        /// Cap on the number of consequential structural decisions surfaced in
        /// the review brief's decision surface (the working-memory limit).
        /// Default 4; clamped to the 3-5 band (4 plus or minus 1). Only
        /// consulted on the brief path.
        #[arg(
            long,
            value_name = "N",
            default_value_t = audit_decision_surface::DEFAULT_DECISION_CAP
        )]
        max_decisions: usize,

        /// Emit the agent-contract WALKTHROUGH GUIDE: the current digest
        /// (brief + decision surface), the review direction, the JSON schema the
        /// agent must return, and a deterministic graph-snapshot hash pinned into
        /// the digest. The digest is built from the graph only (PR prose is never
        /// folded in, so it is injection-resistant). Implies the brief; always
        /// exits 0. A thin agent skill calls this to fetch the current guide,
        /// produces judgment JSON, then reopens with `--walkthrough-file`.
        #[arg(long, conflicts_with_all = ["walkthrough_file", "walkthrough"])]
        walkthrough_guide: bool,

        /// Ingest an agent's judgment JSON and POST-VALIDATE it against the
        /// LIVE graph. Rejects any judgment whose `signal_id` fallow did not emit
        /// (anti-hallucination); refuses the whole payload as stale when the
        /// echoed graph-snapshot hash no longer matches (the tree moved). The
        /// verifier is the graph, not a second model. Implies the brief; always
        /// exits 0. The agent's free-text framing is fenced as non-deterministic
        /// and never gates or auto-posts.
        #[arg(long, value_name = "PATH")]
        walkthrough_file: Option<PathBuf>,

        /// Render the existing walkthrough guide as a staged HUMAN terminal tour
        /// (Stage 1 load-bearing / Stage 2 mechanical), or markdown with
        /// `--format markdown`. Implies the brief; always exits 0.
        /// `--format json --walkthrough` emits the same agent-contract JSON as
        /// `--walkthrough-guide`.
        #[arg(long, conflicts_with_all = ["walkthrough_guide", "walkthrough_file"])]
        walkthrough: bool,

        /// Record one or more changed files as VIEWED in the local walkthrough
        /// viewed-state ledger (`.fallow/walkthrough-state.json`), then render the
        /// tour. Files already viewed (and still current) collapse into the
        /// Cleared panel. Repeatable. Stale marks (the tree moved) are ignored on
        /// render but never deleted. Only consulted on the `--walkthrough` path.
        #[arg(long, value_name = "PATH")]
        mark_viewed: Vec<PathBuf>,

        /// Expand the Cleared panel in the human/markdown walkthrough tour: list
        /// each de-prioritized and already-viewed file instead of the collapsed
        /// one-line summary. Only consulted on the `--walkthrough` path.
        #[arg(long)]
        show_cleared: bool,

        /// Expand the de-prioritized units in the review brief's weighted
        /// focus map ("show me what you de-prioritized"). The `deprioritized`
        /// escape-hatch list is ALWAYS present in `--format json` regardless; this
        /// flag only re-expands the collapse-by-default human focus render. Only
        /// consulted on the brief path.
        #[arg(long)]
        show_deprioritized: bool,
    },

    /// Maintain reusable audit base-snapshot caches.
    AuditCache {
        #[command(subcommand)]
        subcommand: AuditCacheCli,
    },

    /// Surface the consequential structural DECISIONS a change embeds (the apex
    /// of the review brief), each framed as a judgment question with the routed
    /// expert to ask.
    ///
    /// The product's decision surface: a ranked, capped (4 plus or minus 1),
    /// signal_id-anchored set of the SOLID-3 decisions (coupling/boundary,
    /// exports-aware public-API/contract, dependency). Runs the same changed-code
    /// analysis as `fallow review` but emits ONLY the decisions, separable and
    /// cheap. Every decision is suppressible with `// fallow-ignore`. Always
    /// exits 0 (advisory, never a gate). Use `--base` / `--changed-since` to pick
    /// the comparison point, exactly like `fallow audit`.
    DecisionSurface {
        /// Cap on the number of surfaced decisions (the working-memory limit).
        /// Default 4; clamped to the 3-5 band (4 plus or minus 1).
        #[arg(
            long,
            value_name = "N",
            default_value_t = audit_decision_surface::DEFAULT_DECISION_CAP
        )]
        max_decisions: usize,
    },

    /// Show what fallow has done for you: how many issues it is surfacing, the
    /// trend since the last recorded run, and how many commits it contained at
    /// the pre-commit gate.
    ///
    /// Local-only and opt-in: enable per project with `fallow impact enable`, or
    /// turn it on everywhere with `fallow impact default on`, then let your
    /// `fallow audit` / pre-commit gate runs build history. History is stored in
    /// your user config dir (never written into the repo) and forced off in CI.
    /// Impact never uploads anything and never affects exit codes.
    Impact {
        #[command(subcommand)]
        subcommand: Option<ImpactCli>,
        /// Aggregate every tracked project into one cross-repo roll-up
        /// ("what has fallow done for me across all my repos"). Reads the
        /// user config dir; ignores `--root`. Cannot combine with a subcommand.
        #[arg(long)]
        all: bool,
        /// Row ordering for `--all` (default: most recently recorded first).
        #[arg(long, value_enum, default_value_t = ImpactSortCli::Recent)]
        sort: ImpactSortCli,
        /// Cap the number of `--all` rows printed (grand totals still reflect
        /// every tracked project).
        #[arg(long)]
        limit: Option<usize>,
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
    /// `--root`, `--format {human,json,sarif}`, `--changed-since`, `--file`, `--gate`, `--diff-file`,
    /// `--diff-stdin`, `--workspace`, `--changed-workspaces`, `--ci`,
    /// `--fail-on-issues`, `--sarif-file`, `--summary`, `--explain`, and `--surface`.
    Security {
        #[command(subcommand)]
        subcommand: Option<SecuritySubcommand>,
        /// Paid runtime-coverage sidecar input. Accepts a V8 directory, a
        /// single V8 JSON file, or an Istanbul coverage map JSON. When set,
        /// `fallow security` annotates tainted-sink candidates with production
        /// runtime state and uses that state as an additive ranking signal.
        #[arg(long, value_name = "PATH")]
        runtime_coverage: Option<PathBuf>,
        /// Threshold for hot-path classification, forwarded to the sidecar
        /// when `--runtime-coverage` is set.
        #[arg(long, default_value_t = 100)]
        min_invocations_hot: u64,
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
        gate: Option<security::SecurityGateArg>,
        /// Include the agent-facing attack-surface inventory in JSON output.
        #[arg(long)]
        surface: bool,
    },

    /// Render a saved `--format json` results file in another format without
    /// re-running analysis (analyze once, render annotations and the job
    /// summary from the same file). v1 renders the GitHub-native formats only:
    /// `--format github-annotations` or `--format github-summary`.
    Report {
        /// Path to a fallow JSON results file produced by `--format json`
        /// (dead-code, dupes, health, audit, security, or bare combined).
        #[arg(long, value_name = "PATH")]
        from: PathBuf,
    },
    /// Dump fallow's capability manifest (CLI commands and flags, issue types, MCP tools, framework plugins, env vars) as machine-readable JSON for agent introspection. Always JSON, regardless of --format
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

    /// Migrate configuration from knip, jscpd, or stylelint to fallow
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

#[derive(Subcommand)]
enum SecuritySubcommand {
    /// Render verifier-retained survivor candidates from fallow output plus verifier verdicts.
    Survivors {
        /// Raw `fallow security --format json` candidate output.
        #[arg(long, value_name = "PATH")]
        candidates: PathBuf,
        /// Verifier verdict JSON file.
        #[arg(long, value_name = "PATH")]
        verdicts: PathBuf,
        /// Fail when any candidate has no matching verdict.
        #[arg(long)]
        require_verdict_for_each_candidate: bool,
    },
    /// Group unresolved security callees into actionable blind-spot output.
    #[command(name = "blind-spots")]
    BlindSpots {
        /// Scope diagnostics to selected files.
        #[arg(long, value_name = "PATH")]
        file: Vec<PathBuf>,
    },
}

#[derive(clap::Subcommand)]
enum AuditCacheCli {
    /// Remove reusable audit caches owned by an explicit project root.
    Remove {
        /// Print what would be removed without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Confirm removal in non-interactive environments.
        #[arg(long, alias = "force")]
        yes: bool,
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

        /// Also upload importer edges (which files import each function) so the
        /// cloud can show change-time blast radius. Opt-in: this builds the
        /// import graph by running the full static analysis, whereas the default
        /// upload is a fast per-file walk. The graph is cached, so a CI step that
        /// already ran analysis pays little extra.
        #[arg(long)]
        with_callers: bool,

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

#[derive(Subcommand)]
enum CiCli {
    /// Compute the provider action for a rendered sticky PR summary comment.
    PlanPrComment {
        /// Path to the rendered PR comment Markdown body.
        #[arg(long)]
        body: PathBuf,

        /// Sticky marker id used in the rendered body.
        #[arg(long)]
        marker_id: String,

        /// Treat the rendered body as a clean no-findings result.
        #[arg(long)]
        clean: bool,

        /// Existing provider comment id, when a matching sticky comment exists.
        #[arg(long)]
        existing_comment_id: Option<String>,

        /// Path to the existing provider comment body. Enables unchanged-skip planning.
        #[arg(long)]
        existing_body: Option<PathBuf>,
    },

    /// Post, update, or skip a rendered sticky PR summary comment.
    PostPrComment {
        /// Provider whose PR comment is being posted.
        #[arg(long, value_enum)]
        provider: CiProviderArg,

        /// Pull request number (GitHub).
        #[arg(long)]
        pr: Option<String>,

        /// Merge request IID (GitLab).
        #[arg(long)]
        mr: Option<String>,

        /// Path to the rendered PR comment Markdown body.
        #[arg(long)]
        body: PathBuf,

        /// Path to the typed PR comment envelope JSON, when available.
        #[arg(long)]
        envelope: Option<PathBuf>,

        /// Sticky marker id used in the rendered body.
        #[arg(long)]
        marker_id: String,

        /// Treat the rendered body as a clean no-findings result.
        #[arg(long)]
        clean: bool,

        /// GitHub repository in owner/name form. Defaults to GH_REPO or GITHUB_REPOSITORY.
        #[arg(long)]
        repo: Option<String>,

        /// GitLab project id or path. Defaults to CI_PROJECT_ID.
        #[arg(long = "project-id")]
        project_id: Option<String>,

        /// Provider API base URL. Defaults to github.com.
        #[arg(long = "api-url")]
        api_url: Option<String>,

        /// Compute the post plan without creating or updating the provider comment.
        #[arg(long)]
        dry_run: bool,
    },

    /// Post a rendered review envelope as a provider review or summary comment.
    PostReview {
        /// Provider whose review envelope is being posted.
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

        /// Compute the post plan without creating provider comments.
        #[arg(long)]
        dry_run: bool,
    },

    /// Post a GitHub Check Run from a typed PR decision surface.
    PostCheckRun {
        /// Provider whose check run is being posted. Only GitHub is supported.
        #[arg(long, value_enum)]
        provider: CiProviderArg,

        /// Path to a fallow-pr-decision JSON sidecar.
        #[arg(long)]
        decision: PathBuf,

        /// GitHub repository in owner/name form.
        #[arg(long)]
        repo: String,

        /// Head SHA the check run should attach to.
        #[arg(long = "head-sha")]
        head_sha: String,

        /// Provider API base URL. Defaults to github.com.
        #[arg(long = "api-url")]
        api_url: Option<String>,

        /// Post one check run per decision gate instead of one aggregate check.
        #[arg(long = "split-gates")]
        split_gates: bool,

        /// Print the check run payload without posting it.
        #[arg(long)]
        dry_run: bool,
    },

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

#[derive(Subcommand)]
enum RulePackCli {
    /// Scaffold a new rule pack file and wire it into the config
    Init {
        /// Pack name (default: the template name, or "team-policy")
        name: Option<String>,

        /// Template: starter, ai-safe-repo, side-effect-free-domain, clean-architecture, next-app-router
        #[arg(long, default_value = "starter")]
        template: String,

        /// Directory for the pack file, relative to the project root
        #[arg(long, default_value = "rule-packs")]
        dir: String,

        /// Only write the pack file; do not modify the config
        #[arg(long)]
        no_config: bool,
    },

    /// List configured rule packs and their rules
    List,

    /// Evaluate a pack (or all configured packs) against this project and print matches
    Test {
        /// Path to a pack file to test in isolation (default: all configured packs)
        pack: Option<PathBuf>,
    },

    /// Print the JSON Schema for rule pack files
    Schema,
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
enum CiProviderArg {
    Github,
    Gitlab,
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
    const fn to_estimate(self) -> fallow_output::EffortEstimate {
        match self {
            Self::Low => fallow_output::EffortEstimate::Low,
            Self::Medium => fallow_output::EffortEstimate::Medium,
            Self::High => fallow_output::EffortEstimate::High,
        }
    }
}

/// CLI parser for the health severity gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum HealthSeverityCli {
    Moderate,
    High,
    Critical,
}

impl HealthSeverityCli {
    /// Convert to the typed health output severity.
    const fn to_health_severity(self) -> fallow_output::FindingSeverity {
        match self {
            Self::Moderate => fallow_output::FindingSeverity::Moderate,
            Self::High => fallow_output::FindingSeverity::High,
            Self::Critical => fallow_output::FindingSeverity::Critical,
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

fn emit_known_failure(
    message: &str,
    exit_code: u8,
    output: fallow_config::OutputFormat,
    reason: telemetry::FailureReason,
) -> ExitCode {
    telemetry::note_failure_reason(reason);
    emit_error(message, exit_code, output)
}

fn emit_known_failure_with_style(
    message: &str,
    exit_code: u8,
    output: fallow_config::OutputFormat,
    json_style: json_style::JsonStyle,
    reason: telemetry::FailureReason,
) -> ExitCode {
    telemetry::note_failure_reason(reason);
    error::emit_error_with_style(message, exit_code, output, json_style)
}

fn unsupported_security_global(cli: &Cli) -> Option<&'static str> {
    if cli.baseline.is_some() {
        Some("--baseline")
    } else if cli.save_baseline.is_some() {
        Some("--save-baseline")
    } else if cli.production {
        Some("--production")
    } else if cli.no_production {
        Some("--no-production")
    } else if cli.group_by.is_some() {
        Some("--group-by")
    } else if cli.performance {
        Some("--performance")
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
    } else if cli.dupes_no_ignore_imports {
        Some("--dupes-no-ignore-imports")
    } else if cli.include_entry_exports {
        Some("--include-entry-exports")
    } else {
        None
    }
}

struct DispatchContext<'a> {
    cli: &'a Cli,
    root: &'a std::path::Path,
    output: fallow_config::OutputFormat,
    quiet: bool,
    fail_on_issues: bool,
    json_style: json_style::JsonStyle,
    threads: usize,
    tolerance: regression::Tolerance,
    save_regression_file: Option<&'a std::path::PathBuf>,
    save_to_config: bool,
}

impl DispatchContext<'_> {
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
    fallow_engine::churn::set_spawn_hook(signal::scoped_child::output);
    fallow_engine::changed_files::set_spawn_hook(signal::scoped_child::output);
}

fn install_signal_handlers() {
    if let Err(err) = signal::install_handlers() {
        use std::io::Write as _;
        let stderr = std::io::stderr();
        let mut lock = stderr.lock();
        let _ = writeln!(lock, "fallow: failed to install signal handlers: {err}");
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

/// Run the full fallow CLI: parse argv, dispatch the selected command, and
/// return the process exit code. This is the crate's single entry point; the
/// `fallow` binary and the multicall `fallow-multicall` binary both delegate
/// here so there is exactly one clap tree and one dispatch path.
pub fn run() -> ExitCode {
    install_signal_handlers();
    install_spawn_hooks();

    if std::env::var_os("FALLOW_TEST_SIGNAL_HELPER").is_some() {
        return signal_test_helper();
    }

    let (mut cli, fmt) = match parse_cli_args() {
        Ok(parsed) => parsed,
        Err(code) => return code,
    };
    if cli.pretty && !fmt.payload_is_json {
        eprintln!(
            "Error: --pretty requires JSON output. Use --format json --pretty, or remove --pretty."
        );
        return ExitCode::from(2);
    }

    if let Some(code) = run_schema_command_if_requested(&cli, fmt.json_style) {
        return code;
    }

    if let Some(code) = run_telemetry_command_if_requested(&mut cli, fmt.output, fmt.json_style) {
        return code;
    }
    let telemetry_run = start_telemetry_run(&cli, &fmt);

    let (root, threads) = match validate_inputs(&cli, fmt.output, fmt.json_style) {
        Ok(v) => v,
        Err(code) => {
            return record_run_epilogue(telemetry_run, code, None, cli.parent_run.as_deref());
        }
    };

    let FormatConfig {
        output,
        payload_is_json: _,
        quiet,
        fail_on_issues,
        json_style,
    } = fmt;

    let tolerance =
        match run_pre_dispatch_checks(&cli, &root, output, json_style, quiet, telemetry_run) {
            Ok(tolerance) => tolerance,
            Err(code) => return code,
        };

    let (save_regression_file, save_to_config) = regression_save_targets(&cli);

    let command = cli.command.take();
    let dispatch = DispatchContext {
        cli: &cli,
        root: &root,
        output,
        quiet,
        fail_on_issues,
        json_style,
        threads,
        tolerance,
        save_regression_file: save_regression_file.as_ref(),
        save_to_config,
    };
    let exit_code = match dispatch_and_finalize(&dispatch, command) {
        Ok(code) => code,
        Err(code) => return code,
    };
    record_run_epilogue(telemetry_run, exit_code, None, cli.parent_run.as_deref())
}

/// Redirect the rendered report to `--output-file` (ambient sink), dispatch the
/// command, then flush+close the report file. Returns the dispatch exit code, or
/// `Err` carrying a redirect/finalize failure code for `main` to return directly.
fn dispatch_and_finalize(
    dispatch: &DispatchContext<'_>,
    command: Option<Command>,
) -> Result<ExitCode, ExitCode> {
    let cli = dispatch.cli;
    let output = dispatch.output;
    let quiet = dispatch.quiet;

    // Set up the report-file sink before dispatch so rendering lands in the file;
    // progress and the confirmation stay on stderr.
    if let Some(path) = cli.output_file.as_deref()
        && let Err(code) = redirect_report_to_file(path, output)
    {
        return Err(code);
    }

    let exit_code = if command.is_some() && cli_has_bare_coverage_input(cli) {
        emit_error(bare_coverage_subcommand_error_message(), 2, output)
    } else {
        match command {
            None => dispatch_bare_command(dispatch),
            Some(cmd) => dispatch_subcommand(cmd, dispatch),
        }
    };

    if let Some(path) = cli.output_file.as_deref()
        && let Err(code) = finalize_report_file(path, quiet, output)
    {
        return Err(code);
    }
    Ok(exit_code)
}

fn run_telemetry_command_if_requested(
    cli: &mut Cli,
    output: fallow_config::OutputFormat,
    json_style: json_style::JsonStyle,
) -> Option<ExitCode> {
    if matches!(cli.command, Some(Command::Telemetry { .. }))
        && let Some(Command::Telemetry { subcommand }) = cli.command.take()
    {
        return Some(telemetry::run(
            map_telemetry_subcommand(subcommand),
            output,
            json_style,
        ));
    }
    None
}

fn run_schema_command_if_requested(
    cli: &Cli,
    json_style: json_style::JsonStyle,
) -> Option<ExitCode> {
    match cli.command {
        Some(Command::Schema) => Some(schema::run_schema(json_style)),
        Some(Command::ConfigSchema) => Some(init::run_config_schema(json_style)),
        Some(Command::PluginSchema) => Some(init::run_plugin_schema(json_style)),
        Some(Command::RulePackSchema) => Some(init::run_rule_pack_schema(json_style)),
        _ => None,
    }
}

fn regression_save_targets(cli: &Cli) -> (Option<std::path::PathBuf>, bool) {
    let save_file = cli.save_regression_baseline.as_ref().and_then(|opt| {
        opt.as_ref()
            .filter(|path| !path.is_empty())
            .map(std::path::PathBuf::from)
    });
    let save_to_config = cli.save_regression_baseline.is_some() && save_file.is_none();
    (save_file, save_to_config)
}

fn dispatch_bare_command(dispatch: &DispatchContext<'_>) -> ExitCode {
    let cli = dispatch.cli;
    let (run_check, run_dupes, run_health) = combined::resolve_analyses(&cli.only, &cli.skip);
    let production = match dispatch.production_modes(
        cli.production_dead_code,
        cli.production_health,
        cli.production_dupes,
    ) {
        Ok(production) => production,
        Err(code) => return code,
    };
    let coverage_inputs = match resolve_health_coverage_inputs(
        dispatch,
        cli.coverage.as_deref(),
        cli.coverage_root.as_deref(),
    ) {
        Ok(inputs) => inputs,
        Err(code) => return code,
    };
    run_bare_combined(
        dispatch,
        production,
        &coverage_inputs,
        BareAnalyses {
            run_check,
            run_dupes,
            run_health,
        },
    )
}

/// Which analyses the bare `fallow` run executes (resolved from `--only`/`--skip`).
#[derive(Clone, Copy)]
struct BareAnalyses {
    run_check: bool,
    run_dupes: bool,
    run_health: bool,
}

/// Build `CombinedOptions` for a bare `fallow` invocation and run the combined
/// pipeline.
fn run_bare_combined(
    dispatch: &DispatchContext<'_>,
    production: ProductionModes,
    coverage_inputs: &ResolvedHealthCoverageInputs,
    analyses: BareAnalyses,
) -> ExitCode {
    let cli = dispatch.cli;
    let (output, quiet, fail_on_issues) =
        (dispatch.output, dispatch.quiet, dispatch.fail_on_issues);
    combined::run_combined(&combined::CombinedOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet,
        allow_remote_extends: cli.allow_remote_extends,
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
        run_check: analyses.run_check,
        run_dupes: analyses.run_dupes,
        run_health: analyses.run_health,
        dupes_mode: cli.dupes_mode,
        dupes_threshold: cli.dupes_threshold,
        dupes_min_tokens: cli.dupes_min_tokens,
        dupes_min_lines: cli.dupes_min_lines,
        dupes_min_occurrences: cli.dupes_min_occurrences,
        dupes_skip_local: cli.dupes_skip_local,
        dupes_cross_language: cli.dupes_cross_language,
        dupes_ignore_imports: resolve_ignore_imports(
            cli.dupes_ignore_imports,
            cli.dupes_no_ignore_imports,
        ),
        score: cli.score || cli.trend,
        trend: cli.trend,
        save_snapshot: cli.save_snapshot.as_ref(),
        coverage: coverage_inputs.coverage.as_deref(),
        coverage_root: coverage_inputs.coverage_root.as_deref(),
        include_entry_exports: cli.include_entry_exports,
        regression_opts: dispatch.regression_opts(
            cli.changed_since.is_some()
                || cli.workspace.is_some()
                || cli.changed_workspaces.is_some(),
        ),
    })
}

fn dispatch_subcommand(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let cli = dispatch.cli;
    let root = dispatch.root;
    let output = dispatch.output;
    let quiet = dispatch.quiet;
    match command {
        check @ Command::Check { .. } => dispatch_check_command(check, dispatch),
        Command::Watch { no_clear } => dispatch_watch(dispatch, no_clear),
        Command::Inspect {
            file,
            symbol,
            symbol_chain,
            churn,
        } => dispatch_inspect_command(dispatch, file, symbol, symbol_chain, churn),
        Command::Trace {
            symbol,
            callers,
            callees,
            depth,
        } => dispatch_trace_command(dispatch, symbol, callers, callees, depth),
        fix @ Command::Fix { .. } => dispatch_fix_command(&fix, dispatch),
        init @ Command::Init { .. } => dispatch_init_command(init, root, quiet),
        Command::Hooks { subcommand } => {
            run_hooks_command(root, subcommand, output, dispatch.json_style)
        }
        Command::Ci { subcommand } => {
            ci::run(map_ci_subcommand(subcommand), output, dispatch.json_style)
        }
        Command::ConfigSchema => init::run_config_schema(dispatch.json_style),
        Command::PluginSchema => init::run_plugin_schema(dispatch.json_style),
        Command::PluginCheck => plugin_check::run_plugin_check(root, output, dispatch.json_style),
        Command::RulePackSchema => init::run_rule_pack_schema(dispatch.json_style),
        Command::RulePack { subcommand } => dispatch_rule_pack_command(dispatch, subcommand),
        Command::Guard { files } => dispatch_guard_command(dispatch, &files),
        Command::CiTemplate { subcommand } => dispatch_ci_template_command(subcommand),
        Command::Config { path } => config::run_config_with_options(config::RunConfigInput {
            root,
            explicit_config: cli.config.as_deref(),
            path_only: path,
            output,
            quiet,
            json_style: dispatch.json_style,
            load_options: fallow_config::ConfigLoadOptions {
                allow_remote_extends: cli.allow_remote_extends,
            },
        }),
        Command::Recommend => onboarding::run_recommend(root, output, dispatch.json_style),
        list @ (Command::Workspaces | Command::List { .. }) => {
            dispatch_list_command(&list, dispatch)
        }
        dupes @ Command::Dupes { .. } => dispatch_dupes_command(dupes, dispatch),
        health @ Command::Health { .. } => dispatch_health_command(health, dispatch),
        Command::Flags { top } => dispatch_flags_command(dispatch, top),
        Command::Suppressions { file } => dispatch_suppressions_command(dispatch, &file),
        Command::Explain { issue_type } => {
            explain::run_explain(&issue_type.join(" "), output, dispatch.json_style)
        }
        audit @ Command::Audit { .. } => dispatch_audit_command(audit, dispatch),
        Command::AuditCache { subcommand } => dispatch_audit_cache_command(dispatch, &subcommand),
        Command::DecisionSurface { max_decisions } => {
            dispatch_decision_surface(dispatch, max_decisions)
        }
        Command::Impact {
            subcommand,
            all,
            sort,
            limit,
        } => dispatch_impact(
            root,
            quiet,
            output,
            dispatch.json_style,
            subcommand,
            ImpactCrossRepoOpts { all, sort, limit },
        ),
        security @ Command::Security { .. } => dispatch_security_command(security, dispatch),
        Command::Report { from } => cli_report::run_report(&from, output, root),
        Command::Schema => unreachable!("handled above"),
        migrate @ Command::Migrate { .. } => dispatch_migrate_command(migrate, root),
        Command::License { subcommand } => {
            dispatch_license_command(subcommand, output, dispatch.json_style)
        }
        Command::Telemetry { .. } => unreachable!("handled before root validation"),
        Command::Coverage { subcommand } => dispatch_coverage_command(dispatch, &subcommand),
        setup_hooks @ Command::SetupHooks { .. } => {
            dispatch_setup_hooks_command(&setup_hooks, dispatch)
        }
    }
}

/// Destructure the `Command::Check` arm and forward to `dispatch_check`.
fn dispatch_check_command(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let filters = check_issue_filters(&command);
    let Command::Check {
        include_dupes,
        trace,
        trace_file,
        trace_dependency,
        impact_closure,
        top,
        file,
        ..
    } = command
    else {
        unreachable!("check dispatcher only handles check commands");
    };

    dispatch_check(
        dispatch,
        &CheckDispatchArgs {
            filters,
            trace_opts: TraceOptions {
                trace_export: trace,
                trace_file,
                trace_dependency,
                impact_closure,
                performance: dispatch.cli.performance,
            },
            include_dupes,
            top,
            file,
        },
    )
}

/// Map the `Command::Check` filter flags onto `IssueFilters`. Reads the flags by
/// reference (all `Copy` bools) so the caller can still move the non-filter
/// fields out of the same `Command` value afterwards. Split into two halves to
/// keep each builder within the unit-size limit.
fn check_issue_filters(command: &Command) -> IssueFilters {
    check_issue_filters_framework(command, &check_issue_filters_core(command))
}

/// First half of the `IssueFilters` mapping: core/general filter flags over a
/// `Default` base. The framework/catalog half layers on top via struct update.
fn check_issue_filters_core(command: &Command) -> IssueFilters {
    let Command::Check {
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
        policy_violations,
        stale_suppressions,
        ..
    } = command
    else {
        unreachable!("check filter builder only handles check commands");
    };

    let mut filters = IssueFilters::default();
    for (flag, active) in [
        ("--unused-files", *unused_files),
        ("--unused-exports", *unused_exports),
        ("--unused-deps", *unused_deps),
        ("--unused-types", *unused_types),
        ("--private-type-leaks", *private_type_leaks),
        ("--unused-enum-members", *unused_enum_members),
        ("--unused-class-members", *unused_class_members),
        ("--unresolved-imports", *unresolved_imports),
        ("--unlisted-deps", *unlisted_deps),
        ("--duplicate-exports", *duplicate_exports),
        ("--circular-deps", *circular_deps),
        ("--re-export-cycles", *re_export_cycles),
        ("--boundary-violations", *boundary_violations),
        ("--policy-violations", *policy_violations),
        ("--stale-suppressions", *stale_suppressions),
    ] {
        enable_check_filter(&mut filters, flag, active);
    }
    filters
}

/// Second half of the `IssueFilters` mapping: framework/component, store, svelte,
/// catalog, and dependency-override flags, layered onto the core `base`.
fn check_issue_filters_framework(command: &Command, base: &IssueFilters) -> IssueFilters {
    let Command::Check {
        unused_store_members,
        unprovided_injects,
        unrendered_components,
        unused_component_props,
        unused_component_emits,
        unused_component_inputs,
        unused_component_outputs,
        unused_svelte_events,
        unused_server_actions,
        unused_load_data_keys,
        unused_catalog_entries,
        empty_catalog_groups,
        unresolved_catalog_references,
        unused_dependency_overrides,
        misconfigured_dependency_overrides,
        ..
    } = command
    else {
        unreachable!("check filter builder only handles check commands");
    };

    let mut filters = base.clone();
    for (flag, active) in [
        ("--unused-store-members", *unused_store_members),
        ("--unprovided-injects", *unprovided_injects),
        ("--unrendered-components", *unrendered_components),
        ("--unused-component-props", *unused_component_props),
        ("--unused-component-emits", *unused_component_emits),
        ("--unused-component-inputs", *unused_component_inputs),
        ("--unused-component-outputs", *unused_component_outputs),
        ("--unused-svelte-events", *unused_svelte_events),
        ("--unused-server-actions", *unused_server_actions),
        ("--unused-load-data-keys", *unused_load_data_keys),
        ("--unused-catalog-entries", *unused_catalog_entries),
        ("--empty-catalog-groups", *empty_catalog_groups),
        (
            "--unresolved-catalog-references",
            *unresolved_catalog_references,
        ),
        (
            "--unused-dependency-overrides",
            *unused_dependency_overrides,
        ),
        (
            "--misconfigured-dependency-overrides",
            *misconfigured_dependency_overrides,
        ),
    ] {
        enable_check_filter(&mut filters, flag, active);
    }
    filters
}

fn enable_check_filter(filters: &mut IssueFilters, flag: &str, active: bool) {
    if active {
        assert!(
            filters.enable_cli_filter_flag(flag),
            "check command uses unregistered dead-code filter flag {flag}"
        );
    }
}

fn dispatch_inspect_command(
    dispatch: &DispatchContext<'_>,
    file: Option<String>,
    symbol: Option<String>,
    symbol_chain: bool,
    churn: bool,
) -> ExitCode {
    let target = match (file, symbol) {
        (Some(file), None) => inspect::InspectTarget::File { file },
        (None, Some(symbol)) => match symbol.rsplit_once(':') {
            Some((file, export_name))
                if !file.trim().is_empty() && !export_name.trim().is_empty() =>
            {
                inspect::InspectTarget::Symbol {
                    file: file.to_string(),
                    export_name: export_name.to_string(),
                }
            }
            _ => {
                return emit_error(
                    "--symbol must be formatted as FILE:EXPORT",
                    2,
                    dispatch.output,
                );
            }
        },
        _ => {
            return emit_error(
                "inspect requires exactly one of --file or --symbol",
                2,
                dispatch.output,
            );
        }
    };

    let churn_config = if churn {
        match load_config_for_analysis(
            dispatch.root,
            &dispatch.cli.config,
            ConfigLoadOptions {
                output: dispatch.output,
                no_cache: dispatch.cli.no_cache,
                threads: dispatch.threads,
                production_override: None,
                quiet: dispatch.quiet,
                allow_remote_extends: dispatch.cli.allow_remote_extends,
            },
            fallow_config::ProductionAnalysis::Health,
        ) {
            Ok(config) => Some(config),
            Err(code) => return code,
        }
    } else {
        None
    };

    inspect::run_inspect(&inspect::InspectOptions {
        root: dispatch.root,
        config_path: dispatch.cli.config.as_ref(),
        output: dispatch.output,
        json_style: dispatch.json_style,
        no_cache: dispatch.cli.no_cache,
        no_production: dispatch.cli.no_production,
        max_file_size: dispatch.cli.max_file_size,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        production: dispatch.cli.production,
        workspace: dispatch.cli.workspace.as_ref(),
        target,
        churn_cache_dir: churn_config
            .as_ref()
            .map(|config| config.cache_dir.as_path()),
        symbol_chain,
    })
}

fn dispatch_trace_command(
    dispatch: &DispatchContext<'_>,
    symbol: String,
    callers: bool,
    callees: bool,
    depth: Option<u32>,
) -> ExitCode {
    trace_chain::run_trace(&trace_chain::TraceChainOptions {
        root: dispatch.root,
        config_path: &dispatch.cli.config,
        output: dispatch.output,
        json_style: dispatch.json_style,
        no_cache: dispatch.cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        allow_remote_extends: dispatch.cli.allow_remote_extends,
        target: symbol,
        callers,
        callees,
        depth: depth.unwrap_or(fallow_types::trace_chain::DEFAULT_TRACE_DEPTH),
    })
}

fn dispatch_security_command(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::Security {
        subcommand,
        runtime_coverage,
        min_invocations_hot,
        file,
        gate,
        surface,
    } = command
    else {
        unreachable!("security dispatcher only handles security commands");
    };

    let gate = gate.map(security::SecurityGateArg::into_mode);
    let cli = dispatch.cli;
    let (output, _quiet, fail_on_issues) =
        (dispatch.output, dispatch.quiet, dispatch.fail_on_issues);
    let derived_flags = SecurityDerivedFlagState {
        output,
        json_style: dispatch.json_style,
        ci: cli.ci,
        fail_on_issues,
        sarif_file: cli.sarif_file.as_deref(),
        summary: cli.summary,
        explain: cli.explain,
        runtime_coverage: runtime_coverage.as_deref(),
        min_invocations_hot,
        file: file.as_slice(),
        gate,
        surface,
    };
    if let Some(code) = try_run_security_survivors(subcommand.as_ref(), &derived_flags) {
        return code;
    }

    let scoped_files = scoped_security_files(&file, subcommand.as_ref());
    run_security_blind_spots_or_default(
        dispatch,
        &SecurityRunInputs {
            scoped_files: &scoped_files,
            subcommand: &subcommand,
            runtime_coverage: runtime_coverage.as_deref(),
            min_invocations_hot,
            gate,
            surface,
        },
        &derived_flags,
    )
}

/// Inputs threaded from the security dispatcher into the run step. Borrows the
/// scoped file list and subcommand so they outlive the `SecurityOptions`.
struct SecurityRunInputs<'a> {
    scoped_files: &'a [PathBuf],
    subcommand: &'a Option<SecuritySubcommand>,
    runtime_coverage: Option<&'a Path>,
    min_invocations_hot: u64,
    gate: Option<security::SecurityGateMode>,
    surface: bool,
}

/// Build `SecurityOptions` and run either the blind-spots or default analysis.
fn run_security_blind_spots_or_default(
    dispatch: &DispatchContext<'_>,
    inputs: &SecurityRunInputs<'_>,
    derived_flags: &SecurityDerivedFlagState<'_>,
) -> ExitCode {
    let cli = dispatch.cli;
    let (output, quiet, fail_on_issues) =
        (dispatch.output, dispatch.quiet, dispatch.fail_on_issues);
    let opts = security::SecurityOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet,
        allow_remote_extends: cli.allow_remote_extends,
        fail_on_issues,
        sarif_file: cli.sarif_file.as_deref(),
        summary: cli.summary,
        changed_since: cli.changed_since.as_deref(),
        use_shared_diff_index: true,
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        file: inputs.scoped_files,
        surface: inputs.surface,
        gate: inputs.gate,
        runtime_coverage: inputs.runtime_coverage,
        min_invocations_hot: inputs.min_invocations_hot,
        explain: cli.explain,
    };
    if matches!(
        inputs.subcommand,
        Some(SecuritySubcommand::BlindSpots { .. })
    ) {
        if let Some(code) = validate_security_blind_spots_flags(derived_flags) {
            return code;
        }
        security::run_blind_spots(&opts)
    } else {
        security::run(&opts)
    }
}

/// Handle `fallow security survivors` as an early return. Returns `Some(code)`
/// when the subcommand is `survivors` (validated then run); `None` otherwise.
fn try_run_security_survivors(
    subcommand: Option<&SecuritySubcommand>,
    flags: &SecurityDerivedFlagState<'_>,
) -> Option<ExitCode> {
    let Some(SecuritySubcommand::Survivors {
        candidates,
        verdicts,
        require_verdict_for_each_candidate,
    }) = subcommand
    else {
        return None;
    };
    if let Some(code) = validate_security_survivors_flags(flags) {
        return Some(code);
    }
    Some(security::run_survivors(
        &security::SecuritySurvivorsOptions {
            output: flags.output,
            json_style: flags.json_style,
            candidates,
            verdicts,
            require_verdict_for_each_candidate: *require_verdict_for_each_candidate,
        },
    ))
}

/// Build the scoped file list, folding in `blind-spots` extra `--file` values.
fn scoped_security_files(
    file: &[PathBuf],
    subcommand: Option<&SecuritySubcommand>,
) -> Vec<PathBuf> {
    let mut scoped_files = file.to_vec();
    if let Some(SecuritySubcommand::BlindSpots {
        file: blind_spot_files,
    }) = subcommand
    {
        scoped_files.extend(blind_spot_files.iter().cloned());
    }
    scoped_files
}

struct SecurityDerivedFlagState<'a> {
    output: fallow_config::OutputFormat,
    json_style: json_style::JsonStyle,
    ci: bool,
    fail_on_issues: bool,
    sarif_file: Option<&'a Path>,
    summary: bool,
    explain: bool,
    runtime_coverage: Option<&'a Path>,
    min_invocations_hot: u64,
    file: &'a [PathBuf],
    gate: Option<security::SecurityGateMode>,
    surface: bool,
}

fn validate_security_survivors_flags(flags: &SecurityDerivedFlagState<'_>) -> Option<ExitCode> {
    let flag = if flags.ci {
        Some("--ci")
    } else if flags.fail_on_issues {
        Some("--fail-on-issues")
    } else if flags.sarif_file.is_some() {
        Some("--sarif-file")
    } else if flags.summary {
        Some("--summary")
    } else if flags.explain {
        Some("--explain")
    } else if flags.runtime_coverage.is_some() {
        Some("--runtime-coverage")
    } else if flags.min_invocations_hot != DEFAULT_MIN_INVOCATIONS_HOT {
        Some("--min-invocations-hot")
    } else if !flags.file.is_empty() {
        Some("--file")
    } else if flags.gate.is_some() {
        Some("--gate")
    } else if flags.surface {
        Some("--surface")
    } else {
        None
    }?;
    Some(emit_error(
        &format!("{flag} is not valid with `fallow security survivors`."),
        2,
        flags.output,
    ))
}

fn validate_security_blind_spots_flags(flags: &SecurityDerivedFlagState<'_>) -> Option<ExitCode> {
    let flag = if flags.ci {
        Some("--ci")
    } else if flags.fail_on_issues {
        Some("--fail-on-issues")
    } else if flags.sarif_file.is_some() {
        Some("--sarif-file")
    } else if flags.summary {
        Some("--summary")
    } else if flags.explain {
        Some("--explain")
    } else if flags.runtime_coverage.is_some() {
        Some("--runtime-coverage")
    } else if flags.min_invocations_hot != DEFAULT_MIN_INVOCATIONS_HOT {
        Some("--min-invocations-hot")
    } else if flags.gate.is_some() {
        Some("--gate")
    } else if flags.surface {
        Some("--surface")
    } else {
        None
    }?;
    Some(emit_error(
        &format!("{flag} is not valid with `fallow security blind-spots`."),
        2,
        flags.output,
    ))
}

fn dispatch_dupes_command(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::Dupes {
        mode,
        min_tokens,
        min_lines,
        min_occurrences,
        threshold,
        skip_local,
        cross_language,
        ignore_imports,
        no_ignore_imports,
        top,
        trace,
    } = command
    else {
        unreachable!("dupes dispatcher only handles dupes commands");
    };

    dispatch_dupes(
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
            no_ignore_imports,
            top,
            trace,
        },
    )
}

fn dispatch_init_command(command: Command, root: &Path, quiet: bool) -> ExitCode {
    let Command::Init {
        toml,
        agents,
        hooks,
        branch,
        decline,
    } = command
    else {
        unreachable!("init dispatcher only handles init commands");
    };

    init::run_init(&init::InitOptions {
        root,
        use_toml: toml,
        agents,
        hooks,
        branch: branch.as_deref(),
        decline,
        quiet,
    })
}

fn dispatch_fix_command(command: &Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::Fix {
        dry_run,
        yes,
        no_create_config,
    } = command
    else {
        unreachable!("fix dispatcher only handles fix commands");
    };

    dispatch_fix(
        dispatch,
        FixDispatchArgs {
            dry_run: *dry_run,
            yes: *yes,
            no_create_config: *no_create_config,
        },
    )
}

fn dispatch_list_command(command: &Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    match command {
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
                entry_points: *entry_points,
                files: *files,
                plugins: *plugins,
                boundaries: *boundaries,
                workspaces: *workspaces,
            },
        ),
        _ => unreachable!("list dispatcher only handles list commands"),
    }
}

fn dispatch_migrate_command(command: Command, root: &Path) -> ExitCode {
    let Command::Migrate {
        toml,
        jsonc,
        dry_run,
        from,
    } = command
    else {
        unreachable!("migrate dispatcher only handles migrate commands");
    };

    migrate::run_migrate(root, toml, jsonc, dry_run, from.as_deref())
}

fn dispatch_license_command(
    subcommand: LicenseCli,
    output: fallow_config::OutputFormat,
    json_style: json_style::JsonStyle,
) -> ExitCode {
    license::run(&map_license_subcommand(subcommand), output, json_style)
}

fn dispatch_ci_template_command(subcommand: CiTemplateCli) -> ExitCode {
    match subcommand {
        CiTemplateCli::Gitlab { vendor, force } => {
            ci_template::run_gitlab_template(&ci_template::GitlabTemplateOptions {
                vendor_dir: vendor,
                force,
            })
        }
    }
}

fn dispatch_coverage_command(dispatch: &DispatchContext<'_>, subcommand: &CoverageCli) -> ExitCode {
    let cli = dispatch.cli;
    coverage::run(
        map_coverage_subcommand(subcommand, cli.explain),
        &coverage::RunContext {
            root: dispatch.root,
            config_path: &cli.config,
            output: dispatch.output,
            json_style: dispatch.json_style,
            quiet: dispatch.quiet,
            no_cache: cli.no_cache,
            threads: dispatch.threads,
            explain: cli.explain,
            allow_remote_extends: cli.allow_remote_extends,
        },
    )
}

fn dispatch_health_command(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::Health {
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
        css,
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
    } = command
    else {
        unreachable!("health dispatcher only handles health commands");
    };

    let ownership = ownership || ownership_emails.is_some();
    let hotspots = hotspots || ownership;
    let args = HealthDispatchArgs {
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
        css,
        effort,
        score,
        min_score,
        min_severity: min_severity.map(HealthSeverityCli::to_health_severity),
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
    };
    dispatch_health(dispatch, &args)
}

fn dispatch_setup_hooks_command(command: &Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::SetupHooks {
        agent,
        dry_run,
        force,
        user,
        gitignore_claude,
        uninstall,
    } = command
    else {
        unreachable!("setup-hooks dispatcher only handles setup-hooks commands");
    };

    setup_hooks::run_setup_hooks(&setup_hooks::SetupHooksOptions {
        root: dispatch.root,
        agent: *agent,
        dry_run: *dry_run,
        force: *force,
        user: *user,
        gitignore_claude: *gitignore_claude,
        uninstall: *uninstall,
    })
}

fn dispatch_audit_command(command: Command, dispatch: &DispatchContext<'_>) -> ExitCode {
    let Command::Audit {
        production_dead_code,
        production_health,
        production_dupes,
        dead_code_baseline,
        health_baseline,
        dupes_baseline,
        max_crap,
        coverage,
        coverage_root,
        no_css,
        css_deep,
        no_css_deep,
        gate,
        runtime_coverage,
        min_invocations_hot,
        gate_marker,
        brief,
        max_decisions,
        walkthrough_guide,
        walkthrough_file,
        walkthrough,
        mark_viewed,
        show_cleared,
        show_deprioritized,
    } = command
    else {
        unreachable!("audit dispatcher only handles audit commands");
    };

    // The walkthrough flags imply the brief path (the guide digest + the
    // graph-snapshot pin are brief-path data).
    let brief = brief || walkthrough_guide || walkthrough || walkthrough_file.is_some();

    dispatch_audit(
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
            no_css,
            css_deep,
            no_css_deep,
            gate,
            runtime_coverage,
            min_invocations_hot,
            gate_marker,
            brief,
            max_decisions,
            walkthrough_guide,
            walkthrough_file,
            walkthrough,
            mark_viewed,
            show_cleared,
            show_deprioritized,
        },
    )
}

fn dispatch_audit_cache_command(
    dispatch: &DispatchContext<'_>,
    subcommand: &AuditCacheCli,
) -> ExitCode {
    match subcommand {
        AuditCacheCli::Remove { dry_run, yes } => {
            if !*dry_run && !*yes && !std::io::stdin().is_terminal() {
                return emit_error(
                    "audit-cache remove requires --yes (or --force) in non-interactive environments. Use --dry-run to preview removal first, then pass --yes to confirm.",
                    2,
                    dispatch.output,
                );
            }
            match base_worktree::remove_reusable_audit_caches(dispatch.root, *dry_run) {
                Ok(report) => {
                    let action = if *dry_run { "would remove" } else { "removed" };
                    if matches!(dispatch.output, fallow_config::OutputFormat::Json) {
                        let value = serde_json::json!({
                            "kind": "audit-cache-remove",
                            "schema_version": 1,
                            "command": "audit-cache remove",
                            "root": dispatch.root,
                            "dry_run": report.dry_run,
                            "found": report.found,
                            "would_remove": report.found.saturating_sub(report.skipped),
                            "removed": report.removed,
                            "skipped": report.skipped,
                            "complete": report.skipped == 0,
                        });
                        let output_code = report::emit_json(&value, "audit cache removal");
                        if output_code != ExitCode::SUCCESS {
                            return output_code;
                        }
                    } else if !dispatch.quiet {
                        println!(
                            "audit cache: {action} {}, skipped {} for {}",
                            if *dry_run {
                                report.found.saturating_sub(report.skipped)
                            } else {
                                report.removed
                            },
                            report.skipped,
                            dispatch.root.display(),
                        );
                    }
                    if report.skipped == 0 {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::from(2)
                    }
                }
                Err(error) => emit_error(
                    &format!(
                        "failed to remove audit caches for {}: {error}",
                        dispatch.root.display()
                    ),
                    2,
                    dispatch.output,
                ),
            }
        }
    }
}

fn dispatch_flags_command(dispatch: &DispatchContext<'_>, top: Option<usize>) -> ExitCode {
    let cli = dispatch.cli;
    let root = dispatch.root;
    let output = dispatch.output;
    let quiet = dispatch.quiet;
    let threads = dispatch.threads;
    let production = match resolve_production_modes(cli, root, output, false, false, false) {
        Ok(modes) => modes.for_analysis(fallow_config::ProductionAnalysis::DeadCode),
        Err(code) => return code,
    };
    flags::run_flags(&flags::FlagsOptions {
        root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads,
        quiet,
        allow_remote_extends: cli.allow_remote_extends,
        production,
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        changed_since: cli.changed_since.as_deref(),
        explain: cli.explain,
        top,
    })
}

fn dispatch_suppressions_command(
    dispatch: &DispatchContext<'_>,
    file: &[std::path::PathBuf],
) -> ExitCode {
    let cli = dispatch.cli;
    let root = dispatch.root;
    let output = dispatch.output;
    let production = match resolve_production_modes(cli, root, output, false, false, false) {
        Ok(modes) => modes.for_analysis(fallow_config::ProductionAnalysis::DeadCode),
        Err(code) => return code,
    };
    suppressions::run_suppressions(&suppressions::SuppressionsOptions {
        root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        allow_remote_extends: cli.allow_remote_extends,
        production,
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        changed_since: cli.changed_since.as_deref(),
        file,
    })
}

fn dispatch_guard_command(dispatch: &DispatchContext<'_>, files: &[String]) -> ExitCode {
    guard::run_guard(&guard::GuardOptions {
        root: dispatch.root,
        config_path: &dispatch.cli.config,
        output: dispatch.output,
        json_style: dispatch.json_style,
        quiet: dispatch.quiet,
        allow_remote_extends: dispatch.cli.allow_remote_extends,
        files,
    })
}

fn dispatch_rule_pack_command(dispatch: &DispatchContext<'_>, subcommand: RulePackCli) -> ExitCode {
    let ctx = rule_pack::RulePackContext {
        root: dispatch.root,
        config_path: &dispatch.cli.config,
        output: dispatch.output,
        json_style: dispatch.json_style,
        quiet: dispatch.quiet,
        no_cache: dispatch.cli.no_cache,
        threads: Some(dispatch.threads),
        allow_remote_extends: dispatch.cli.allow_remote_extends,
    };
    rule_pack::run(&map_rule_pack_subcommand(subcommand), &ctx)
}

fn map_rule_pack_subcommand(subcommand: RulePackCli) -> rule_pack::RulePackSubcommand {
    match subcommand {
        RulePackCli::Init {
            name,
            template,
            dir,
            no_config,
        } => rule_pack::RulePackSubcommand::Init(rule_pack::InitArgs {
            name,
            template,
            dir,
            no_config,
        }),
        RulePackCli::List => rule_pack::RulePackSubcommand::List,
        RulePackCli::Test { pack } => {
            rule_pack::RulePackSubcommand::Test(rule_pack::TestArgs { pack })
        }
        RulePackCli::Schema => rule_pack::RulePackSubcommand::Schema,
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
        command @ CiCli::PlanPrComment { .. } => map_ci_plan_pr_comment(command),
        command @ CiCli::PostPrComment { .. } => map_ci_post_pr_comment(command),
        command @ CiCli::PostReview { .. } => map_ci_post_review(command),
        command @ CiCli::PostCheckRun { .. } => map_ci_post_check_run(command),
        command @ CiCli::ReconcileReview { .. } => map_ci_reconcile_review(command),
    }
}

fn map_ci_plan_pr_comment(command: CiCli) -> ci::CiCommand {
    let CiCli::PlanPrComment {
        body,
        marker_id,
        clean,
        existing_comment_id,
        existing_body,
    } = command
    else {
        unreachable!("ci plan-pr-comment mapper called with different variant");
    };

    ci::CiCommand::PlanPrComment {
        body,
        marker_id,
        clean,
        existing_comment_id,
        existing_body,
    }
}

fn map_ci_post_pr_comment(command: CiCli) -> ci::CiCommand {
    let CiCli::PostPrComment {
        provider,
        pr,
        mr,
        body,
        envelope,
        marker_id,
        clean,
        repo,
        project_id,
        api_url,
        dry_run,
    } = command
    else {
        unreachable!("ci post-pr-comment mapper called with different variant");
    };

    ci::CiCommand::PostPrComment {
        provider: map_ci_provider(provider),
        target: pr.or(mr),
        body,
        envelope,
        marker_id,
        clean,
        repo,
        project_id,
        api_url,
        dry_run,
    }
}

fn map_ci_post_review(command: CiCli) -> ci::CiCommand {
    let CiCli::PostReview {
        provider,
        pr,
        mr,
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    } = command
    else {
        unreachable!("ci post-review mapper called with different variant");
    };

    ci::CiCommand::PostReview {
        provider: map_ci_provider(provider),
        target: pr.or(mr),
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    }
}

fn map_ci_post_check_run(command: CiCli) -> ci::CiCommand {
    let CiCli::PostCheckRun {
        provider,
        decision,
        repo,
        head_sha,
        api_url,
        split_gates,
        dry_run,
    } = command
    else {
        unreachable!("ci post-check-run mapper called with different variant");
    };

    ci::CiCommand::PostCheckRun {
        provider: map_ci_provider(provider),
        decision,
        repo,
        head_sha,
        api_url,
        split_gates,
        dry_run,
    }
}

fn map_ci_reconcile_review(command: CiCli) -> ci::CiCommand {
    let CiCli::ReconcileReview {
        provider,
        pr,
        mr,
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    } = command
    else {
        unreachable!("ci reconcile-review mapper called with different variant");
    };

    ci::CiCommand::ReconcileReview {
        provider: map_ci_provider(provider),
        target: pr.or(mr),
        envelope,
        repo,
        project_id,
        api_url,
        dry_run,
    }
}

fn map_ci_provider(provider: CiProviderArg) -> ci::CiProvider {
    match provider {
        CiProviderArg::Github => ci::CiProvider::Github,
        CiProviderArg::Gitlab => ci::CiProvider::Gitlab,
    }
}

fn map_coverage_subcommand(sub: &CoverageCli, explain: bool) -> coverage::CoverageSubcommand {
    match sub {
        CoverageCli::Setup {
            yes,
            non_interactive,
            json,
        } => map_coverage_setup(*yes, *non_interactive, *json, explain),
        CoverageCli::Analyze { .. } => map_coverage_analyze(sub),
        CoverageCli::UploadInventory { .. } => map_coverage_upload_inventory(sub),
        CoverageCli::UploadSourceMaps { .. } => map_coverage_upload_source_maps(sub),
        CoverageCli::UploadStaticFindings { .. } => map_coverage_upload_static_findings(sub),
    }
}

fn map_coverage_setup(
    yes: bool,
    non_interactive: bool,
    json: bool,
    explain: bool,
) -> coverage::CoverageSubcommand {
    coverage::CoverageSubcommand::Setup(coverage::SetupArgs {
        yes,
        non_interactive: non_interactive || json,
        json,
        explain,
    })
}

fn map_coverage_analyze(sub: &CoverageCli) -> coverage::CoverageSubcommand {
    let CoverageCli::Analyze {
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
    } = sub
    else {
        unreachable!("coverage analyze mapper called with non-analyze variant");
    };
    coverage::CoverageSubcommand::Analyze(coverage::AnalyzeArgs {
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
    })
}

fn map_coverage_upload_inventory(sub: &CoverageCli) -> coverage::CoverageSubcommand {
    let CoverageCli::UploadInventory {
        api_key,
        api_endpoint,
        project_id,
        git_sha,
        allow_dirty,
        exclude_paths,
        path_prefix,
        dry_run,
        with_callers,
        ignore_upload_errors,
    } = sub
    else {
        unreachable!("coverage inventory mapper called with non-inventory variant");
    };
    coverage::CoverageSubcommand::UploadInventory(coverage::UploadInventoryArgs {
        api_key: api_key.clone(),
        api_endpoint: api_endpoint.clone(),
        project_id: project_id.clone(),
        git_sha: git_sha.clone(),
        allow_dirty: *allow_dirty,
        exclude_paths: exclude_paths.clone(),
        path_prefix: path_prefix.clone(),
        dry_run: *dry_run,
        with_callers: *with_callers,
        ignore_upload_errors: *ignore_upload_errors,
    })
}

fn map_coverage_upload_source_maps(sub: &CoverageCli) -> coverage::CoverageSubcommand {
    let CoverageCli::UploadSourceMaps {
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
    } = sub
    else {
        unreachable!("coverage source-map mapper called with non-source-map variant");
    };
    coverage::CoverageSubcommand::UploadSourceMaps(coverage::UploadSourceMapsArgs {
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
    })
}

fn map_coverage_upload_static_findings(sub: &CoverageCli) -> coverage::CoverageSubcommand {
    let CoverageCli::UploadStaticFindings {
        api_key,
        api_endpoint,
        project_id,
        git_sha,
        allow_dirty,
        dry_run,
        ignore_upload_errors,
    } = sub
    else {
        unreachable!("coverage static-findings mapper called with non-static variant");
    };
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
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        allow_remote_extends: cli.allow_remote_extends,
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
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        allow_remote_extends: cli.allow_remote_extends,
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
        json_style: dispatch.json_style,
        threads: dispatch.threads,
        no_cache: cli.no_cache,
        entry_points: args.entry_points,
        files: args.files,
        plugins: args.plugins,
        boundaries: args.boundaries,
        workspaces: args.workspaces,
        production,
        allow_remote_extends: cli.allow_remote_extends,
    })
}

fn dispatch_check(dispatch: &DispatchContext<'_>, args: &CheckDispatchArgs) -> ExitCode {
    let cli = dispatch.cli;
    let (output, quiet, fail_on_issues) =
        (dispatch.output, dispatch.quiet, dispatch.fail_on_issues);
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::DeadCode) {
        Ok(production) => production,
        Err(code) => return code,
    };
    check::run_check(&CheckOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet,
        allow_remote_extends: cli.allow_remote_extends,
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

/// Resolve the three-state `ignoreImports` CLI override from the opt-in /
/// opt-out flag pair. clap's `conflicts_with` guarantees the two are never both
/// set, so this maps `--no-ignore-imports` -> `Some(false)`, `--ignore-imports`
/// -> `Some(true)`, and neither -> `None` (defer to config, which defaults to
/// `true`).
fn resolve_ignore_imports(ignore_imports: bool, no_ignore_imports: bool) -> Option<bool> {
    if no_ignore_imports {
        Some(false)
    } else if ignore_imports {
        Some(true)
    } else {
        None
    }
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
    no_ignore_imports: bool,
    top: Option<usize>,
    trace: Option<String>,
}

fn dispatch_dupes(dispatch: &DispatchContext<'_>, args: &DupesDispatchArgs) -> ExitCode {
    let cli = dispatch.cli;
    let (output, quiet, _fail_on_issues) =
        (dispatch.output, dispatch.quiet, dispatch.fail_on_issues);
    let production = match dispatch.production_for(fallow_config::ProductionAnalysis::Dupes) {
        Ok(production) => production,
        Err(code) => return code,
    };
    dupes::run_dupes(&DupesOptions {
        root: dispatch.root,
        config_path: &cli.config,
        output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet,
        allow_remote_extends: cli.allow_remote_extends,
        mode: args.mode,
        min_tokens: args.min_tokens,
        min_lines: args.min_lines,
        min_occurrences: args.min_occurrences,
        threshold: args.threshold,
        skip_local: args.skip_local,
        cross_language: args.cross_language,
        ignore_imports: resolve_ignore_imports(args.ignore_imports, args.no_ignore_imports),
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
    no_css: bool,
    css_deep: bool,
    no_css_deep: bool,
    gate: Option<AuditGateArg>,
    runtime_coverage: Option<PathBuf>,
    min_invocations_hot: u64,
    gate_marker: Option<String>,
    brief: bool,
    max_decisions: usize,
    /// Emit the agent-contract walkthrough guide instead of the brief body.
    walkthrough_guide: bool,
    /// Post-validate an agent's judgment JSON from this path against the
    /// live graph.
    walkthrough_file: Option<PathBuf>,
    /// Render the existing walkthrough guide as a staged human/markdown tour.
    walkthrough: bool,
    /// Changed files to record as VIEWED before rendering the tour.
    mark_viewed: Vec<PathBuf>,
    /// Expand the Cleared panel (de-prioritized + viewed) in the tour.
    show_cleared: bool,
    /// Expand the de-prioritized units in the human focus map.
    show_deprioritized: bool,
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
        LoadConfigArgs {
            output,
            no_cache: cli.no_cache,
            threads: dispatch.threads,
            production: cli.production,
            quiet: dispatch.quiet,
            allow_remote_extends: cli.allow_remote_extends,
        },
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

fn audit_css_enabled(config: &fallow_config::AuditConfig, args: &AuditDispatchArgs) -> bool {
    !args.no_css && config.css.unwrap_or(true)
}

fn audit_css_deep_enabled(config: &fallow_config::AuditConfig, args: &AuditDispatchArgs) -> bool {
    audit_css_enabled(config, args)
        && !args.no_css_deep
        && (args.css_deep || config.css_deep.unwrap_or(true))
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
            json_style: dispatch.json_style,
            no_cache: cli.no_cache,
            threads: dispatch.threads,
            quiet: dispatch.quiet,
            allow_remote_extends: cli.allow_remote_extends,
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
            // Styling analytics, including deep cross-file reachability, is on
            // by default in `fallow audit`; both layers remain verdict-neutral
            // unless a user escalates a styling rule to error.
            css: audit_css_enabled(&inputs.audit_cfg, args),
            css_deep: audit_css_deep_enabled(&inputs.audit_cfg, args),
            runtime_coverage: args.runtime_coverage.as_deref(),
            min_invocations_hot: args.min_invocations_hot,
            brief: args.brief,
            max_decisions: args.max_decisions,
            walkthrough_guide: args.walkthrough_guide,
            walkthrough: args.walkthrough,
            mark_viewed: &args.mark_viewed,
            show_cleared: args.show_cleared,
            walkthrough_file: args.walkthrough_file.as_deref(),
            show_deprioritized: args.show_deprioritized,
        },
        args.gate_marker.as_deref(),
    )
}

/// Dispatch `fallow decision-surface`: the separable apex. Reuses the audit
/// input resolution in brief mode (changed-code scope) with all gating /
/// coverage / baseline knobs defaulted, then renders ONLY the decision surface.
fn dispatch_decision_surface(dispatch: &DispatchContext<'_>, max_decisions: usize) -> ExitCode {
    let args = decision_surface_audit_args(max_decisions);
    let inputs = match resolve_audit_inputs(dispatch, &args) {
        Ok(inputs) => inputs,
        Err(code) => return code,
    };
    audit::run_decision_surface(&decision_surface_audit_options(
        dispatch,
        &inputs,
        max_decisions,
    ))
}

fn decision_surface_audit_args(max_decisions: usize) -> AuditDispatchArgs {
    AuditDispatchArgs {
        production_dead_code: false,
        production_health: false,
        production_dupes: false,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        no_css: true,
        css_deep: false,
        no_css_deep: false,
        gate: None,
        runtime_coverage: None,
        min_invocations_hot: 0,
        gate_marker: None,
        brief: true,
        max_decisions,
        walkthrough_guide: false,
        walkthrough_file: None,
        walkthrough: false,
        mark_viewed: Vec::new(),
        show_cleared: false,
        show_deprioritized: false,
    }
}

fn decision_surface_audit_options<'a>(
    dispatch: &'a DispatchContext<'a>,
    inputs: &'a ResolvedAuditInputs,
    max_decisions: usize,
) -> audit::AuditOptions<'a> {
    let cli = dispatch.cli;
    audit::AuditOptions {
        root: dispatch.root,
        config_path: &cli.config,
        cache_dir: &inputs.cache_dir,
        output: dispatch.output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        allow_remote_extends: cli.allow_remote_extends,
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
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: inputs.audit_cfg.gate,
        include_entry_exports: cli.include_entry_exports,
        // Decision-surface (brief apex) does not render styling; keep it lean.
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 0,
        brief: true,
        max_decisions,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    }
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
    css: bool,
    effort: Option<EffortFilter>,
    score: bool,
    min_score: Option<f64>,
    min_severity: Option<fallow_output::FindingSeverity>,
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

struct ResolvedHealthCoverageInputs {
    coverage: Option<PathBuf>,
    coverage_root: Option<PathBuf>,
}

fn resolve_health_coverage_inputs(
    dispatch: &DispatchContext<'_>,
    cli_coverage: Option<&std::path::Path>,
    cli_coverage_root: Option<&std::path::Path>,
) -> Result<ResolvedHealthCoverageInputs, ExitCode> {
    let env_coverage = path_from_env("FALLOW_COVERAGE");
    let env_coverage_root = path_from_env("FALLOW_COVERAGE_ROOT");
    let needs_config_coverage = cli_coverage.is_none() && env_coverage.is_none();
    let needs_config_coverage_root = cli_coverage_root.is_none() && env_coverage_root.is_none();
    let config_health = if needs_config_coverage || needs_config_coverage_root {
        Some(
            load_config(
                dispatch.root,
                &dispatch.cli.config,
                LoadConfigArgs {
                    output: dispatch.output,
                    no_cache: dispatch.cli.no_cache,
                    threads: dispatch.threads,
                    production: dispatch.cli.production,
                    quiet: dispatch.quiet,
                    allow_remote_extends: dispatch.cli.allow_remote_extends,
                },
            )?
            .health,
        )
    } else {
        None
    };

    Ok(ResolvedHealthCoverageInputs {
        coverage: cli_coverage
            .map(std::path::Path::to_path_buf)
            .or(env_coverage)
            .or_else(|| {
                config_health
                    .as_ref()
                    .and_then(|health| health.coverage.clone())
            }),
        coverage_root: cli_coverage_root
            .map(std::path::Path::to_path_buf)
            .or(env_coverage_root)
            .or_else(|| {
                config_health
                    .as_ref()
                    .and_then(|health| health.coverage_root.clone())
            }),
    })
}

fn path_from_env(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn validate_health_report_only_gate(
    report_only: bool,
    min_score: Option<f64>,
    min_severity: Option<fallow_output::FindingSeverity>,
    output: fallow_config::OutputFormat,
) -> Result<(), ExitCode> {
    if report_only && (min_score.is_some() || min_severity.is_some()) {
        return Err(emit_error(
            "--report-only cannot be combined with --min-score or --min-severity. \
             --report-only always exits 0; drop it to gate on score/severity, or \
             drop the gate flags to stay advisory.",
            2,
            output,
        ));
    }

    Ok(())
}

fn resolve_runtime_coverage_options(
    runtime_coverage: Option<&std::path::Path>,
    min_invocations_hot: u64,
    min_observation_volume: Option<u32>,
    low_traffic_threshold: Option<f64>,
    output: fallow_config::OutputFormat,
) -> Result<Option<fallow_engine::health::RuntimeCoverageOptions>, ExitCode> {
    let Some(path) = runtime_coverage else {
        return Ok(None);
    };

    health::coverage::prepare_options(
        path,
        min_invocations_hot,
        min_observation_volume,
        low_traffic_threshold,
        output,
    )
    .map(Some)
}

fn dispatch_health(dispatch: &DispatchContext<'_>, args: &HealthDispatchArgs<'_>) -> ExitCode {
    let cli = dispatch.cli;
    let root = dispatch.root;
    let (output, _quiet, _fail_on_issues) =
        (dispatch.output, dispatch.quiet, dispatch.fail_on_issues);
    if let Err(code) = validate_health_report_only_gate(
        args.report_only,
        args.min_score,
        args.min_severity,
        output,
    ) {
        return code;
    }
    let runtime_coverage = match resolve_runtime_coverage_options(
        args.runtime_coverage,
        args.min_invocations_hot,
        args.min_observation_volume,
        args.low_traffic_threshold,
        output,
    ) {
        Ok(options) => options,
        Err(code) => return code,
    };
    let production = match resolve_production_modes(cli, root, output, false, false, false) {
        Ok(modes) => modes.for_analysis(fallow_config::ProductionAnalysis::Health),
        Err(code) => return code,
    };
    let coverage_inputs =
        match resolve_health_coverage_inputs(dispatch, args.coverage, args.coverage_root) {
            Ok(inputs) => inputs,
            Err(code) => return code,
        };
    let run = derive_health_dispatch_run(args, output, &coverage_inputs, runtime_coverage);
    run_health_dispatch(dispatch, args, ResolvedHealthDispatch { run, production })
}

fn derive_health_dispatch_run<'a>(
    args: &'a HealthDispatchArgs<'a>,
    output: fallow_config::OutputFormat,
    coverage_inputs: &'a ResolvedHealthCoverageInputs,
    runtime_coverage: Option<fallow_engine::health::RuntimeCoverageOptions>,
) -> fallow_engine::health::HealthRunOptions<'a> {
    fallow_engine::health::derive_health_run_options(fallow_engine::health::HealthRunOptionsInput {
        output,
        thresholds: health_threshold_overrides(args),
        top: args.top,
        sort: args.sort.clone().into(),
        complexity: args.complexity,
        file_scores: args.file_scores,
        coverage_gaps: args.coverage_gaps,
        hotspots: args.hotspots,
        ownership: args.ownership,
        ownership_emails: args.ownership_emails,
        targets: args.targets,
        css: args.css,
        effort: args.effort.map(EffortFilter::to_estimate),
        score: args.score,
        gates: health_gate_options(args),
        snapshot_requested: args.save_snapshot.is_some(),
        trend: args.trend,
        since: args.since,
        min_commits: args.min_commits,
        coverage_inputs: health_coverage_inputs(coverage_inputs),
        runtime_coverage,
    })
}

fn health_threshold_overrides(
    args: &HealthDispatchArgs<'_>,
) -> fallow_engine::health::HealthThresholdOverrides {
    fallow_engine::health::HealthThresholdOverrides {
        max_cyclomatic: args.max_cyclomatic,
        max_cognitive: args.max_cognitive,
        max_crap: args.max_crap,
    }
}

fn health_gate_options(args: &HealthDispatchArgs<'_>) -> fallow_engine::health::HealthGateOptions {
    fallow_engine::health::HealthGateOptions {
        min_score: args.min_score,
        min_severity: args.min_severity,
        report_only: args.report_only,
    }
}

fn health_coverage_inputs(
    coverage_inputs: &ResolvedHealthCoverageInputs,
) -> fallow_engine::health::HealthCoverageInputs<'_> {
    fallow_engine::health::HealthCoverageInputs {
        coverage: coverage_inputs.coverage.as_deref(),
        coverage_root: coverage_inputs.coverage_root.as_deref(),
    }
}

/// Resolved inputs threaded from `dispatch_health` into the `HealthOptions`
/// builder. Owns the normalized engine run contract and resolved production
/// mode.
struct ResolvedHealthDispatch<'a> {
    run: fallow_engine::health::HealthRunOptions<'a>,
    production: bool,
}

/// Build `HealthOptions` from the parsed args plus the resolved dispatch inputs,
/// then run the health analysis.
fn run_health_dispatch(
    dispatch: &DispatchContext<'_>,
    args: &HealthDispatchArgs<'_>,
    resolved: ResolvedHealthDispatch<'_>,
) -> ExitCode {
    let cli = dispatch.cli;
    let (output, quiet, _fail_on_issues) =
        (dispatch.output, dispatch.quiet, dispatch.fail_on_issues);
    let run = resolved.run;
    let sections = run.sections;
    let production = resolved.production;
    health::run_health(
        &HealthOptions {
            root: dispatch.root,
            config_path: &cli.config,
            output,
            no_cache: cli.no_cache,
            threads: dispatch.threads,
            quiet,
            thresholds: run.thresholds,
            top: run.top,
            sort: run.sort,
            production,
            production_override: Some(production),
            allow_remote_extends: cli.allow_remote_extends,
            changed_since: cli.changed_since.as_deref(),
            diff_index: None,
            use_shared_diff_index: true,
            workspace: cli.workspace.as_deref(),
            changed_workspaces: cli.changed_workspaces.as_deref(),
            baseline: cli.baseline.as_deref(),
            save_baseline: cli.save_baseline.as_deref(),
            complexity: sections.complexity,
            file_scores: sections.file_scores,
            coverage_gaps: sections.coverage_gaps,
            config_activates_coverage_gaps: !sections.any_section,
            hotspots: sections.hotspots,
            ownership: run.ownership,
            ownership_emails: run.ownership_emails,
            targets: sections.targets,
            css: sections.css,
            css_deep: false,
            force_full: sections.force_full,
            score_only_output: sections.score_only_output,
            enforce_coverage_gap_gate: true,
            effort: run.effort,
            score: sections.score,
            gates: run.gates,
            since: run.since,
            min_commits: run.min_commits,
            explain: cli.explain,
            summary: cli.summary,
            save_snapshot: args
                .save_snapshot
                .map(|opt| PathBuf::from(opt.as_deref().unwrap_or_default())),
            trend: args.trend,
            coverage_inputs: run.coverage_inputs,
            performance: cli.performance,
            runtime_coverage: run.runtime_coverage,
            churn_file: cli.churn_file.as_deref(),
            complexity_breakdown: args.complexity_breakdown,
            group_by: cli.group_by.map(Into::into),
        },
        dispatch.json_style,
    )
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

    #[test]
    fn regression_baseline_help_explains_the_default_destination() {
        use clap::CommandFactory;
        let help = Cli::command().render_long_help().to_string();

        assert!(help.contains("Omit PATH to update regression.baseline"));
        assert!(help.contains("discovered fallow config"));
        assert!(help.contains("create .fallowrc.json when none exists"));
    }

    /// The root `--help` cheat sheet is a static const that cannot call the
    /// shared renderer, so this test is the only guard that it stays in sync
    /// with `TASK_MATRIX`. Every row's command string must appear verbatim.
    #[test]
    fn after_help_lists_every_task_matrix_command() {
        for row in crate::task_matrix::TASK_MATRIX {
            assert!(
                TOP_LEVEL_AFTER_HELP.contains(row.command),
                "root --help cheat sheet is missing task-matrix command '{}'; \
                 update TOP_LEVEL_AFTER_HELP to match TASK_MATRIX",
                row.command
            );
        }
    }

    /// The high-value and coarse admin commands each get a distinct telemetry
    /// workflow instead of the `Unknown` catch-all, so command families stay
    /// answerable without uploading raw command lines.
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
            (
                vec!["fallow", "watch"],
                telemetry::Workflow::CodeQualityReview,
            ),
            (
                vec!["fallow", "list"],
                telemetry::Workflow::ProjectInventory,
            ),
            (
                vec!["fallow", "workspaces"],
                telemetry::Workflow::ProjectInventory,
            ),
            (
                vec!["fallow", "schema"],
                telemetry::Workflow::ProjectInventory,
            ),
            (vec!["fallow", "init"], telemetry::Workflow::Setup),
            (
                vec!["fallow", "hooks", "install", "--target", "git"],
                telemetry::Workflow::Setup,
            ),
            (vec!["fallow", "config-schema"], telemetry::Workflow::Setup),
            (vec!["fallow", "plugin-schema"], telemetry::Workflow::Setup),
            (
                vec!["fallow", "rule-pack-schema"],
                telemetry::Workflow::Setup,
            ),
            (vec!["fallow", "config"], telemetry::Workflow::Setup),
            (
                vec!["fallow", "ci-template", "gitlab"],
                telemetry::Workflow::Setup,
            ),
            (vec!["fallow", "migrate"], telemetry::Workflow::Setup),
            (
                vec!["fallow", "telemetry", "status"],
                telemetry::Workflow::Setup,
            ),
            (vec!["fallow", "setup-hooks"], telemetry::Workflow::Setup),
            (
                vec!["fallow", "audit-cache", "remove", "--root", "."],
                telemetry::Workflow::Setup,
            ),
            (
                vec!["fallow", "license", "status"],
                telemetry::Workflow::License,
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
            "  recommend",
            "  migrate",
            "  config",
            "  config-schema",
            "  plugin-schema",
            "  plugin-check",
            "  rule-pack-schema",
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
    fn security_help_hides_globals_rejected_by_security_validator() {
        let help = render_security_help(SecurityHelpTarget::Parent);

        for long in SECURITY_UNSUPPORTED_GLOBAL_LONGS {
            assert!(
                !help_contains_long_flag(&help, long),
                "security help must hide unsupported --{long}:\n{help}"
            );
        }

        for long in [
            "root",
            "config",
            "format",
            "quiet",
            "no-cache",
            "threads",
            "changed-since",
            "diff-file",
            "diff-stdin",
            "workspace",
            "changed-workspaces",
            "ci",
            "fail-on-issues",
            "sarif-file",
            "summary",
            "output-file",
            "max-file-size",
            "explain",
            "surface",
        ] {
            assert!(
                help_contains_long_flag(&help, long),
                "security help must keep supported --{long}:\n{help}"
            );
        }
    }

    #[test]
    fn security_help_detection_covers_subcommand_and_help_alias_forms() {
        assert_eq!(
            security_help_target(["security", "--help"]),
            Some(SecurityHelpTarget::Parent)
        );
        assert_eq!(
            security_help_target(["security", "-h"]),
            Some(SecurityHelpTarget::Parent)
        );
        assert_eq!(
            security_help_target(["--format", "json", "security", "--help"]),
            Some(SecurityHelpTarget::Parent)
        );
        assert_eq!(
            security_help_target(["help", "security"]),
            Some(SecurityHelpTarget::Parent)
        );
        assert_eq!(
            security_help_target(["security", "survivors", "--help"]),
            Some(SecurityHelpTarget::Survivors)
        );
        assert_eq!(
            security_help_target(["security", "survivors", "-h"]),
            Some(SecurityHelpTarget::Survivors)
        );
        assert_eq!(
            security_help_target(["help", "security", "survivors"]),
            Some(SecurityHelpTarget::Survivors)
        );
        assert_eq!(
            security_help_target(["security", "blind-spots", "--help"]),
            Some(SecurityHelpTarget::BlindSpots)
        );
        assert_eq!(
            security_help_target(["help", "security", "blind-spots"]),
            Some(SecurityHelpTarget::BlindSpots)
        );
        assert_eq!(security_help_target(["health", "--help"]), None);
        assert_eq!(security_help_target(["help", "health"]), None);
    }

    #[test]
    fn security_unsupported_global_validator_matches_hidden_help_contract() {
        for (argv, expected) in [
            (vec!["fallow", "security", "--performance"], "--performance"),
            (
                vec!["fallow", "security", "--baseline", "base.json"],
                "--baseline",
            ),
            (
                vec!["fallow", "security", "--dupes-mode", "weak"],
                "--dupes-mode",
            ),
        ] {
            let cli = Cli::try_parse_from(argv).expect("security global parses before validation");
            assert_eq!(unsupported_security_global(&cli), Some(expected));
        }

        let explain = Cli::try_parse_from(["fallow", "security", "--explain"])
            .expect("security --explain parses");
        assert_eq!(unsupported_security_global(&explain), None);
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
                        | "allow-remote-extends"
                        | "no-cache"
                        | "threads"
                        | "changed-since"
                        | "diff-file"
                        | "production"
                        | "workspace"
                        | "changed-workspaces"
                        | "explain"
                )
            })
            .collect();
        let programmatic_flags: std::collections::BTreeSet<String> =
            fallow_api::COMMON_ANALYSIS_OPTION_FLAGS
                .iter()
                .map(|flag| (*flag).to_owned())
                .collect();

        assert_eq!(programmatic_flags, cli_flags);
    }

    #[test]
    fn dead_code_registry_filter_flags_are_exposed_by_clap() {
        use clap::CommandFactory;

        let cli = Cli::command();
        let dead_code = cli
            .get_subcommands()
            .find(|command| command.get_name() == "dead-code")
            .expect("dead-code subcommand is registered");
        let cli_flags: std::collections::BTreeSet<String> = dead_code
            .get_arguments()
            .filter_map(|arg| arg.get_long().map(|long| format!("--{long}")))
            .collect();

        for flag in fallow_types::issue_meta::DEAD_CODE_FILTER_FLAGS.iter() {
            assert!(
                cli_flags.contains(*flag),
                "registry filter flag {flag} is missing from dead-code clap args"
            );
        }
    }

    fn help_contains_long_flag(help: &str, long: &str) -> bool {
        let flag = format!("--{long}");
        help.split(|c: char| c.is_whitespace() || c == ',' || c == '[' || c == ']')
            .any(|token| token == flag)
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

    fn telemetry_run_for_mode(mode: telemetry::AnalysisMode) -> TelemetryRun {
        TelemetryRun {
            workflow: telemetry::Workflow::Health,
            output: fallow_config::OutputFormat::Json,
            quiet: true,
            start: std::time::Instant::now(),
            context: telemetry::WorkflowContext {
                run_scope: telemetry::RunScope::FullProject,
                config_shape: telemetry::ConfigShape::Default,
                output_destination: telemetry::OutputDestination::Stdout,
                analysis_mode: mode,
            },
        }
    }

    #[test]
    fn fallback_failure_reason_skips_success_and_findings() {
        let run = telemetry_run_for_mode(telemetry::AnalysisMode::Static);

        assert_eq!(fallback_failure_reason_for(&run, ExitCode::SUCCESS), None);
        assert_eq!(fallback_failure_reason_for(&run, ExitCode::from(1)), None);
    }

    #[test]
    fn fallback_failure_reason_classifies_network_auth_and_analysis() {
        let static_run = telemetry_run_for_mode(telemetry::AnalysisMode::Static);
        let cloud_run = telemetry_run_for_mode(telemetry::AnalysisMode::ProductionCoverage);

        assert_eq!(
            fallback_failure_reason_for(&static_run, ExitCode::from(api::NETWORK_EXIT_CODE)),
            Some(telemetry::FailureReason::Network),
        );
        assert_eq!(
            fallback_failure_reason_for(&static_run, ExitCode::from(12)),
            Some(telemetry::FailureReason::Auth),
        );
        assert_eq!(
            fallback_failure_reason_for(&cloud_run, ExitCode::from(3)),
            Some(telemetry::FailureReason::Auth),
        );
        assert_eq!(
            fallback_failure_reason_for(&static_run, ExitCode::from(2)),
            Some(telemetry::FailureReason::Analysis),
        );
    }

    #[test]
    fn bare_coverage_flags_parse_without_subcommand() {
        let cli = Cli::try_parse_from([
            "fallow",
            "--coverage",
            "coverage/coverage-final.json",
            "--coverage-root",
            "/ci/workspace",
        ])
        .expect("bare combined coverage flags should parse");
        assert!(cli.command.is_none());
        assert_eq!(
            cli.coverage.as_deref(),
            Some(std::path::Path::new("coverage/coverage-final.json"))
        );
        assert_eq!(
            cli.coverage_root.as_deref(),
            Some(std::path::Path::new("/ci/workspace"))
        );
    }

    #[test]
    fn bare_coverage_before_subcommand_is_detectable() {
        let cli = Cli::try_parse_from([
            "fallow",
            "--coverage",
            "coverage/coverage-final.json",
            "dead-code",
        ])
        .expect("clap should parse pre-subcommand bare coverage for custom rejection");
        assert!(cli.command.is_some());
        assert!(cli_has_bare_coverage_input(&cli));
        let message = bare_coverage_subcommand_error_message();
        assert!(message.contains("bare combined-mode flags"));
        assert!(message.contains("fallow health --coverage <coverage-final.json>"));
    }

    #[test]
    fn subcommand_coverage_flag_keeps_regular_clap_error() {
        let Err(err) = Cli::try_parse_from(["fallow", "dead-code", "--coverage"]) else {
            panic!("dead-code --coverage should fail to parse");
        };
        assert_eq!(err.kind(), clap::error::ErrorKind::UnknownArgument);
    }

    #[test]
    fn format_parsing_covers_all_variants() {
        assert!(matches!(parse_format_arg("json"), Some(Format::Json)));
        assert!(matches!(parse_format_arg("JSON"), Some(Format::Json)));
        assert!(matches!(parse_format_arg("human"), Some(Format::Human)));
        assert!(matches!(parse_format_arg("sarif"), Some(Format::Sarif)));
        assert!(matches!(parse_format_arg("compact"), Some(Format::Compact)));
        assert!(matches!(
            parse_format_arg("markdown"),
            Some(Format::Markdown)
        ));
        assert!(matches!(parse_format_arg("md"), Some(Format::Markdown)));
        assert!(matches!(
            parse_format_arg("codeclimate"),
            Some(Format::CodeClimate)
        ));
        assert!(matches!(
            parse_format_arg("gitlab-codequality"),
            Some(Format::CodeClimate)
        ));
        assert!(matches!(
            parse_format_arg("gitlab-code-quality"),
            Some(Format::CodeClimate)
        ));
        assert!(matches!(
            parse_format_arg("pr-comment-github"),
            Some(Format::PrCommentGithub)
        ));
        assert!(matches!(
            parse_format_arg("pr-comment-gitlab"),
            Some(Format::PrCommentGitlab)
        ));
        assert!(matches!(
            parse_format_arg("review-github"),
            Some(Format::ReviewGithub)
        ));
        assert!(matches!(
            parse_format_arg("review-gitlab"),
            Some(Format::ReviewGitlab)
        ));
        assert!(matches!(parse_format_arg("badge"), Some(Format::Badge)));
        assert!(parse_format_arg("xml").is_none());
        assert!(parse_format_arg("").is_none());
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
