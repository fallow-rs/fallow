//! Clap definition of the `fallow` command line.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

use crate::cli_agent::AgentCli;
use crate::cli_format::Format;
use crate::cli_hooks::HooksCli;
use crate::cli_impact::{ImpactCli, ImpactSortCli};
use crate::dispatch::resolve_negatable_flag;
use crate::dupes::{DupesMode, DupesOverrides};
use crate::health::SortBy;
use crate::runtime_support::{AnalysisKind, GroupBy};
use crate::{
    audit_decision_surface, doctor, flags, mutation_cli, security, setup_hooks, similar_code_cli,
    viz,
};

const TOP_LEVEL_HELP_TEMPLATE: &str =
    "{about-with-newline}\n{usage-heading} {usage}{after-help}\n\nOptions:\n{options}";

// Macros instead of consts so `concat!` can assemble the short (`-h`) and
// long (`--help`) after-help surfaces from the same source fragments.
macro_rules! top_level_task_cheat_sheet {
    () => {
        "\
When the agent is about to...
  delete an \"unused\" export or file        fallow dead-code --trace <file>:<export>
  prove exact TypeScript symbol consumers  fallow dead-code --type-aware --symbol-impact <file>:<export-or-class.method>
  find how one module reaches another      fallow trace --path <from> <to>
  delete an \"unused\" dependency            fallow dead-code --trace-dependency <name>
  migrate a dependency                     fallow trace --dependency <name> --sites
  commit or open a PR                      fallow audit --base <ref>
  read a diff before approving it          fallow review --base <ref> --brief
  prioritize refactoring                   fallow health --hotspots --targets
  ask who owns code                        fallow health --ownership
  check untested-but-reachable code        fallow health --coverage-gaps
  consolidate duplication                  fallow dupes --trace dup:<fingerprint>
  find feature flags                       fallow flags
  check architecture rules before editing  fallow guard <files>
  check cycles and boundaries after edits  fallow architecture
  surface security candidates              fallow security
  inspect a target before editing          fallow inspect --file <path>
  understand a finding                     fallow explain <issue-type>
  scope a monorepo                         --workspace <glob> / --changed-workspaces <ref>"
    };
}

macro_rules! top_level_core_command_groups {
    () => {
        "\
Analysis:
  dead-code      Analyze unused code and dependency hygiene
  guard          Show which architecture rules apply to files before editing
  architecture   Check import cycles, boundaries and policy rules after editing
  dupes          Find copy-paste and structural code duplication
  health         Analyze complexity, maintainability, hotspots, and coverage gaps
  flags          Detect feature flag usage patterns
  security       Surface local security candidates for agent verification (opt-in)
  similar-code   Find semantic implementation overlap for verification (opt-in, local)
  mutation       Measure how well the tests detect code changes (opt-in, runs tests)
  audit          Review changed files for dead code, complexity, duplication, and styling

Workflow:
  watch          Re-run analysis as files change
  fix            Auto-fix safe unused-code findings"
    };
}

macro_rules! top_level_extended_command_groups {
    () => {
        "\
Project inspection:
  list              List discovered files, entry points, plugins, boundaries, workspaces, and entry weight
  inspect           Inspect one file or exported symbol as a bundled evidence query
  trace             Trace a symbol's call chain (best-effort, syntactic)
  trace-error       Resolve a runtime stack trace's frames to project definitions
  decision-surface  Surface the structural decisions a change embeds (advisory)
  workspaces        Show monorepo workspace discovery diagnostics
  explain           Explain one issue type without running analysis
  suppressions      List active fallow-ignore suppression markers
  impact            Show what fallow has done for you (opt-in, local-only)
  viz               Generate an interactive HTML map of the codebase

Setup and configuration:
  doctor            Diagnose project readiness without changing anything
  init              Create a fallow config, optionally with a Git hook
  agent             Wire fallow into Claude Code, Codex, or Cursor in one pass
  audit-cache       Maintain reusable audit base-snapshot caches
  baselines         Prune committed baselines to the findings that remain
  recommend         Recommend a project-tailored config for an agent to author
  migrate           Migrate knip, jscpd, or stylelint config to fallow
  config            Show the resolved config and loaded config file
  config-schema     Print the fallow config JSON Schema
  plugin-schema     Print the external plugin JSON Schema
  plugin-check      Dry-run external plugins and report what they seed
  rule-pack         Manage declarative rule packs (policy-as-code)
  rule-pack-schema  Print the rule pack JSON Schema
  type-aware        Inspect the optional TypeScript semantic companion

Automation and CI:
  ci             Build PR/MR feedback envelopes
  ci-template    Print or vendor CI integration templates
  report         Re-render saved JSON as GitHub or CodeClimate output
  hooks          Install or remove fallow-managed Git and agent hooks
  setup-hooks    Deprecated: use `agent install` or `hooks install --target agent`

Runtime coverage:
  coverage       Set up or analyze runtime coverage data
  license        Manage the paid-feature license
  telemetry      Manage opt-in product telemetry

Reference:
  schema         Dump the CLI interface as machine-readable JSON
  help           Print this message or the help of a command"
    };
}

const TOP_LEVEL_AFTER_HELP: &str = concat!(
    top_level_task_cheat_sheet!(),
    "\n\n",
    top_level_core_command_groups!(),
    "\n\nRun fallow --help for the complete command list."
);

pub const TOP_LEVEL_AFTER_LONG_HELP: &str = concat!(
    top_level_task_cheat_sheet!(),
    "\n\n",
    top_level_core_command_groups!(),
    "\n\n",
    top_level_extended_command_groups!(),
    "\n\n",
    "When no command is given, fallow runs dead-code + dupes + health together.\n",
    "Use --only/--skip to select specific analyses."
);

#[derive(Parser)]
#[command(
    name = "fallow",
    about = "Codebase analyzer for TypeScript/JavaScript: unused code, circular dependencies, code duplication, complexity hotspots, and architecture boundary violations",
    version,
    disable_version_flag = true,
    help_template = TOP_LEVEL_HELP_TEMPLATE,
    after_help = TOP_LEVEL_AFTER_HELP,
    after_long_help = TOP_LEVEL_AFTER_LONG_HELP
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Scope reported findings to this file or directory (default: whole project).
    /// The full project graph is still built; only reported items are narrowed.
    #[arg(value_name = "PATH")]
    pub path: Option<PathBuf>,

    /// Print version.
    /// Accepts `-v`, `-V`, and `--version`; TS/JS tooling (node, npm, pnpm,
    /// yarn, bun, tsc) uses `-v`, while `-V` matches knip/oxlint/biome.
    #[arg(
        short = 'v',
        visible_short_alias = 'V',
        long = "version",
        action = clap::ArgAction::Version
    )]
    pub version: Option<bool>,

    /// Project root directory
    #[arg(short, long, global = true)]
    pub root: Option<PathBuf>,

    /// Path to config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml)
    #[arg(short, long, global = true)]
    pub config: Option<PathBuf>,

    /// Allow trusted config files to extend HTTPS URLs
    #[arg(hide_short_help = true, long, global = true)]
    pub allow_remote_extends: bool,

    /// Output format (alias: --output)
    #[arg(
        short,
        long,
        visible_alias = "output",
        global = true,
        default_value = "human"
    )]
    pub format: Format,

    /// Indent JSON output for manual inspection. Requires the final output format to be JSON.
    #[arg(hide_short_help = true, long, global = true)]
    pub pretty: bool,

    /// Suppress progress output
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Disable incremental caching
    #[arg(hide_short_help = true, long, global = true)]
    pub no_cache: bool,

    /// Number of parser threads
    #[arg(hide_short_help = true, long, global = true)]
    pub threads: Option<usize>,

    /// Only report issues in files changed since this git ref (e.g., main, HEAD~5)
    ///
    /// For this run, the ref replaces the per-package refs of
    /// `workspaces.changedSince` in the config.
    #[arg(long, visible_alias = "base", global = true)]
    pub changed_since: Option<String>,

    /// Ignore the per-package refs of `workspaces.changedSince` for this run
    ///
    /// Every workspace package is then analyzed in full scope, for example to
    /// save or gate a whole-project baseline. A global `--changed-since`,
    /// `--workspace` or `--diff-file` scope still applies.
    #[arg(hide_short_help = true, long, global = true)]
    pub no_package_baselines: bool,

    /// Unified diff for line-level scoping.
    /// Use `-` to read from stdin. Project-level findings still bypass this
    /// filter. When both this and `--changed-since` are set, the diff filter
    /// wins for finding scope while `--changed-since` still drives file discovery.
    #[arg(
        hide_short_help = true,
        long = "diff-file",
        value_name = "PATH",
        global = true
    )]
    pub diff_file: Option<PathBuf>,

    /// Read the unified diff from stdin.
    /// Equivalent to `--diff-file -`.
    #[arg(hide_short_help = true, long = "diff-stdin", global = true)]
    pub diff_stdin: bool,

    /// Import change history from a `fallow-churn/v1` JSON file instead of `git
    /// log`, powering hotspots, ownership, and bus-factor on projects with no
    /// git repository (Yandex Arc, Mercurial, Perforce). A small wrapper
    /// translates your VCS log into the contract. Resolved relative to `--root`.
    /// Affects `health --hotspots` / `--ownership` / `--targets` only; `audit`,
    /// `impact`, and `--changed-since` still require git.
    #[arg(
        hide_short_help = true,
        long = "churn-file",
        value_name = "PATH",
        global = true
    )]
    pub churn_file: Option<PathBuf>,

    /// Skip source files larger than this many megabytes (default 5) instead of
    /// parsing them, guarding against the out-of-memory blowup a single
    /// multi-MB generated/vendored/bundled file causes on large repos. Use `0`
    /// for no limit. Declaration files (`.d.ts`) are always analyzed. Skipped
    /// files are reported and excluded from every analysis. Also settable via
    /// `FALLOW_MAX_FILE_SIZE`.
    #[arg(
        hide_short_help = true,
        long = "max-file-size",
        value_name = "MB",
        global = true
    )]
    pub max_file_size: Option<u32>,

    /// Compare against a previously saved baseline file. Used by bare
    /// `fallow`, `dead-code`, `architecture`, `dupes` and `health`; other
    /// subcommands reject it. On bare `fallow` it holds the dead-code baseline
    /// only: pass health and duplication baselines with `--health-baseline`
    /// and `--dupes-baseline`
    #[arg(hide_short_help = true, long, global = true)]
    pub baseline: Option<PathBuf>,

    /// How `--baseline` matches health findings: per file and category
    /// (`count`, the default) or per function identity (`identity`, strict,
    /// and only against a baseline that was saved with `--baseline-mode
    /// identity`; such a baseline still reads in count mode).
    ///
    /// A finding's identity is its file path plus its function name, so
    /// renaming or moving a function that is still in the baseline reports it
    /// as new. Re-save the baseline after that kind of refactor. Functions
    /// that share a name in one file, and unnamed functions, share one
    /// identity and can mask each other.
    ///
    /// Defaults to `count` when omitted. Saving without the flag refuses to
    /// overwrite a baseline that carries identities; pass `--baseline-mode
    /// count` explicitly to downgrade such a baseline on purpose.
    #[arg(
        hide_short_help = true,
        long = "baseline-mode",
        value_enum,
        global = true
    )]
    pub baseline_mode: Option<BaselineModeArg>,

    /// Correlate this run with a previous telemetry analysis run.
    ///
    /// Used only for opt-in telemetry follow-up measurement. The value is not
    /// interpreted as a path, repository, package, or user identifier. Hidden
    /// from `--help`; agents receive the correlation token from JSON output.
    #[arg(long, global = true, value_name = "RUN_ID", hide = true)]
    pub parent_run: Option<String>,

    /// Save the current results as a baseline file. Used by bare `fallow`,
    /// `dead-code`, `architecture`, `dupes` and `health`; other subcommands
    /// reject it. On bare `fallow` it saves the dead-code baseline only: save
    /// the others with `fallow health --save-baseline` and `fallow dupes
    /// --save-baseline`. The path must resolve inside the project root, its Git
    /// work tree, the CI workspace or a temp directory
    #[arg(hide_short_help = true, long, global = true)]
    pub save_baseline: Option<PathBuf>,

    /// Production mode: exclude test/story/dev files, only start/build scripts,
    /// report type-only dependencies
    #[arg(long, global = true)]
    pub production: bool,

    /// Force production mode OFF for every analysis, overriding a project
    /// config's `production: true` (and `FALLOW_PRODUCTION`). Conflicts with
    /// `--production`.
    #[arg(
        hide_short_help = true,
        long = "no-production",
        global = true,
        conflicts_with = "production"
    )]
    pub no_production: bool,

    /// Run dead-code analysis in production mode when using bare combined mode.
    #[arg(hide_short_help = true, long = "production-dead-code")]
    pub production_dead_code: bool,

    /// Run health analysis in production mode when using bare combined mode.
    #[arg(hide_short_help = true, long = "production-health")]
    pub production_health: bool,

    /// Run duplication analysis in production mode when using bare combined mode.
    #[arg(hide_short_help = true, long = "production-dupes")]
    pub production_dupes: bool,

    /// Scope output to selected workspaces.
    /// Accepts exact names, glob patterns, and `!`-prefixed negations.
    /// Values can be comma-separated or repeated.
    #[arg(short, long, global = true, value_delimiter = ',')]
    pub workspace: Option<Vec<String>>,

    /// Scope output to workspaces touched since the given git ref.
    /// Git is required. Mutually exclusive with `--workspace`.
    #[arg(long, global = true, value_name = "REF")]
    pub changed_workspaces: Option<String>,

    /// Group output by owner or by directory.
    #[arg(hide_short_help = true, long, global = true)]
    pub group_by: Option<GroupBy>,

    /// Keep only the matching groups of a `--group-by` health run.
    /// Accepts exact group keys, glob patterns, and `!`-prefixed negations.
    /// Values can be comma-separated or repeated. Project-level sections are
    /// not filtered. Supported by `fallow health` only.
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        value_delimiter = ',',
        value_name = "KEY",
        requires = "group_by"
    )]
    pub group: Option<Vec<String>>,

    /// Show pipeline performance timing breakdown
    #[arg(hide_short_help = true, long, global = true)]
    pub performance: bool,

    /// Include metric definitions and rule descriptions in output.
    #[arg(hide_short_help = true, long, global = true)]
    pub explain: bool,

    /// Show per-pattern counts for skipped files: default duplicate ignores on dupes
    /// and audit, built-in discovery ignores on check, dead-code, audit and the default run.
    #[arg(hide_short_help = true, long, global = true)]
    pub explain_skipped: bool,

    /// Show only category counts without individual items
    #[arg(hide_short_help = true, long, global = true)]
    pub summary: bool,

    /// CI mode: equivalent to --format sarif --fail-on-issues --quiet
    #[arg(long, global = true)]
    pub ci: bool,

    /// Exit with code 1 if issues are found
    #[arg(hide_short_help = true, long, global = true)]
    pub fail_on_issues: bool,

    /// Write SARIF output to a file (in addition to the primary --format output).
    /// Used by bare `fallow`, `dead-code`, `architecture` and `security`. The
    /// path must resolve inside the project root, its Git work tree, the CI
    /// workspace or a temp directory
    #[arg(hide_short_help = true, long, global = true, value_name = "PATH")]
    pub sarif_file: Option<PathBuf>,

    /// Write the report to a file instead of stdout, for any --format (no ANSI
    /// codes). Useful on large projects where the terminal scrollback truncates
    /// the top. Progress and the confirmation stay on stderr. The path must
    /// resolve inside the project root, its Git work tree, the CI workspace or
    /// a temp directory.
    #[arg(short = 'o', long, global = true, value_name = "PATH")]
    pub output_file: Option<PathBuf>,

    /// Prefix prepended to every path in the CI-facing formats
    /// (`github-annotations`, `github-summary`, `codeclimate`,
    /// `pr-comment-github`, `pr-comment-gitlab`, `review-github`,
    /// `review-gitlab`). CI platforms address files by
    /// repository-root-relative path, so when the analyzed project lives in a
    /// subdirectory (e.g. `packages/app/`), paths need that offset. fallow
    /// detects the offset via the git toplevel automatically; this flag
    /// overrides the detection. Pass an empty string to disable rebasing and
    /// emit paths relative to `--root`.
    #[arg(
        hide_short_help = true,
        long = "report-path-prefix",
        visible_alias = "annotations-path-prefix",
        global = true,
        value_name = "PREFIX"
    )]
    pub report_path_prefix: Option<String>,

    /// Fail if issue count increased beyond tolerance compared to a regression baseline.
    ///
    /// With `list --entry-weight`, fail when the eager source bytes of an
    /// entry grew beyond the tolerance. Without this flag that comparison is
    /// report-only.
    #[arg(hide_short_help = true, long, global = true)]
    pub fail_on_regression: bool,

    /// Exit with code 1 if a loaded --baseline has entries that match nothing in this run.
    ///
    /// Stricter than the default advisory warning: any stale entry fails, not
    /// just a quarter of the baseline. Applies in every output format. The
    /// verdict reaches a machine consumer as `gate_outcomes["stale-baseline"]`,
    /// which is published with or without this flag; the flag decides only
    /// whether that entry is `enforced`, and the exit code plus one stderr
    /// line. Inert when no baseline is loaded, on runs narrowed to part of
    /// the project (including production mode), which cannot judge a
    /// whole-project baseline, and under `health --report-only`, which never
    /// fails a run; such a run prints why it stood down instead of passing
    /// quietly. `fallow audit` always analyzes only changed files, so the gate
    /// never fires there.
    #[arg(hide_short_help = true, long, global = true)]
    pub fail_on_stale_baseline: bool,

    /// Exit with code 1 if a loaded baseline has a key that the same file at the base ref does not have.
    ///
    /// Makes a committed baseline shrink-only. A change that adds a finding and
    /// re-saves the baseline in the same commit passes `--baseline` and
    /// `--fail-on-stale-baseline`; this gate fails it and lists each new key
    /// per category on stderr. A renamed file or a moved line gives a new key,
    /// so it counts as growth. The base ref is `--baseline-base`, else the
    /// `fallow audit` base: `--changed-since` / `--base`, then
    /// `FALLOW_AUDIT_BASE`, then the merge-base with the upstream or the remote
    /// default branch. A baseline that the base ref does not have is a new
    /// baseline and passes with a note. A base ref that git cannot resolve
    /// (for example in a shallow clone) exits 2. Applies to `dead-code`,
    /// `architecture`, `dupes`, `health`, `audit` and the bare run, and reaches a machine
    /// consumer as `gate_outcomes["baseline-growth"]`.
    #[arg(hide_short_help = true, long, global = true)]
    pub fail_on_baseline_growth: bool,

    /// The git ref that --fail-on-baseline-growth compares the baseline with.
    ///
    /// Defaults to the `fallow audit` base resolution. Needs
    /// --fail-on-baseline-growth.
    #[arg(hide_short_help = true, long, global = true, value_name = "REF")]
    pub baseline_base: Option<String>,

    /// Exit with code 1 if fallow could not parse a source file cleanly.
    ///
    /// Arms the `parse-error` gate: a file with a `source-parse-degraded`
    /// entry in `workspace_diagnostics[]` fails the run, and the gate entry in
    /// `gate_outcomes["parse-error"]` names each such file. Off by default,
    /// because the parser also rejects valid syntax that is newer than the
    /// parser. Applies to `dead-code`, `architecture`, `health`, `audit` and
    /// the bare run, in every output format. The `failOnParseError` config key
    /// arms the same gate. `health --report-only` never fails a run.
    #[arg(hide_short_help = true, long, global = true)]
    pub fail_on_parse_error: bool,

    /// Allowed issue count increase before a regression is flagged.
    ///
    /// With `list --entry-weight`, the value is the allowed growth of eager
    /// source bytes per entry: a byte count such as `1024`, or a percentage
    /// such as `5%`.
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        value_name = "TOLERANCE",
        default_value = "0"
    )]
    pub tolerance: String,

    /// Path to the regression baseline file.
    #[arg(hide_short_help = true, long, global = true, value_name = "PATH")]
    pub regression_baseline: Option<PathBuf>,

    /// Save the current issue counts as a regression baseline. Omit PATH to
    /// update regression.baseline in the discovered fallow config, or create
    /// .fallowrc.json when none exists. Provide PATH to write a standalone file;
    /// PATH must resolve inside the project root, its Git work tree, the CI workspace or a temp directory.
    #[expect(
        clippy::option_option,
        reason = "clap pattern: None=not passed, Some(None)=flag only (write to config), Some(Some(path))=write to file"
    )]
    #[arg(hide_short_help = true, long, global = true, value_name = "PATH", num_args = 0..=1, default_missing_value = "")]
    pub save_regression_baseline: Option<Option<String>>,

    /// Run only specific analyses when no subcommand is given.
    #[arg(long, value_delimiter = ',')]
    pub only: Vec<AnalysisKind>,

    /// Skip specific analyses when no subcommand is given.
    #[arg(long, value_delimiter = ',')]
    pub skip: Vec<AnalysisKind>,

    /// Override duplication detection mode in combined mode.
    #[arg(hide_short_help = true, long = "dupes-mode", global = true)]
    pub dupes_mode: Option<DupesMode>,

    /// Enable function-scoped near-miss clone detection in combined mode.
    #[arg(hide_short_help = true, long = "dupes-near", global = true)]
    pub dupes_near: bool,

    /// Override duplication threshold in combined mode.
    #[arg(hide_short_help = true, long = "dupes-threshold", global = true)]
    pub dupes_threshold: Option<f64>,

    /// Override the minimum token count for clones in combined mode.
    #[arg(hide_short_help = true, long = "dupes-min-tokens", global = true)]
    pub dupes_min_tokens: Option<usize>,

    /// Override the minimum line count for clones in combined mode.
    #[arg(hide_short_help = true, long = "dupes-min-lines", global = true)]
    pub dupes_min_lines: Option<usize>,

    /// Override the minimum clone occurrences in combined mode (must be >= 2).
    #[arg(hide_short_help = true, long = "dupes-min-occurrences", global = true, value_parser = parse_min_occurrences)]
    pub dupes_min_occurrences: Option<usize>,

    /// Only report cross-directory duplicates in combined mode.
    #[arg(hide_short_help = true, long = "dupes-skip-local", global = true)]
    pub dupes_skip_local: bool,

    /// Enable cross-language duplicate detection in combined mode.
    #[arg(hide_short_help = true, long = "dupes-cross-language", global = true)]
    pub dupes_cross_language: bool,

    /// Exclude module wiring from duplicate detection in combined mode
    /// (default). Pass `--dupes-no-ignore-imports` to count it again.
    #[arg(hide_short_help = true, long = "dupes-ignore-imports", global = true)]
    pub dupes_ignore_imports: bool,

    /// Count module wiring as clone candidates in combined mode (opt out of the
    /// default exclusion).
    #[arg(
        hide_short_help = true,
        long = "dupes-no-ignore-imports",
        global = true,
        conflicts_with = "dupes_ignore_imports"
    )]
    pub dupes_no_ignore_imports: bool,

    /// Omit clone instances whose path is a symlink, or lies under a
    /// symlinked directory, in combined mode.
    #[arg(hide_short_help = true, long = "dupes-ignore-symlinks", global = true)]
    pub dupes_ignore_symlinks: bool,

    /// Report symlinked clone instances in combined mode (opt out of a config
    /// `duplicates.ignoreSymlinks: true`).
    #[arg(
        hide_short_help = true,
        long = "dupes-no-ignore-symlinks",
        global = true,
        conflicts_with = "dupes_ignore_symlinks"
    )]
    pub dupes_no_ignore_symlinks: bool,

    /// Compute health score in combined mode.
    #[arg(hide_short_help = true, long)]
    pub score: bool,

    /// Compare current health metrics against the most recent saved snapshot.
    #[arg(hide_short_help = true, long)]
    pub trend: bool,

    /// Compare current health metrics against this snapshot file in combined
    /// mode. Implies --trend and --score.
    #[arg(hide_short_help = true, long, value_name = "PATH")]
    pub trend_from: Option<PathBuf>,

    /// Save a vital signs snapshot for trend tracking in combined mode.
    /// Provide a path or omit for the default `.fallow/snapshots/` location.
    /// A given path must resolve inside the project root, its Git work tree, the CI workspace or a temp directory.
    #[expect(
        clippy::option_option,
        reason = "clap pattern: None=not passed, Some(None)=default path, Some(Some(path))=custom path"
    )]
    #[arg(hide_short_help = true, long, value_name = "PATH", num_args = 0..=1, default_missing_value = "")]
    pub save_snapshot: Option<Option<String>>,

    /// Path to Istanbul or raw V8 coverage data for exact CRAP scores in combined mode.
    /// Also settable via `FALLOW_COVERAGE` or `health.coverage`.
    #[arg(hide_short_help = true, long, value_name = "PATH")]
    pub coverage: Option<PathBuf>,

    /// Absolute prefix to strip from Istanbul file paths in combined mode.
    /// Also settable via `FALLOW_COVERAGE_ROOT` or `health.coverageRoot`.
    #[arg(hide_short_help = true, long = "coverage-root", value_name = "PATH")]
    pub coverage_root: Option<PathBuf>,

    /// Compare duplication clone groups against a saved baseline in combined
    /// mode (produced by `fallow dupes --save-baseline`).
    #[arg(hide_short_help = true, long = "dupes-baseline", value_name = "PATH")]
    pub dupes_baseline: Option<PathBuf>,

    /// Compare health findings against a saved baseline in combined mode
    /// (produced by `fallow health --save-baseline`).
    #[arg(hide_short_help = true, long = "health-baseline", value_name = "PATH")]
    pub health_baseline: Option<PathBuf>,

    /// Report unused exports in entry files instead of auto-marking them as used.
    #[arg(hide_short_help = true, long, global = true)]
    pub include_entry_exports: bool,

    /// Also report the unused exports, types, class members and enum members
    /// of unused files. By default the report hides them, because deleting the
    /// file removes them, and counts them in `cascade_hidden`.
    #[arg(hide_short_help = true, long, global = true)]
    pub show_cascade: bool,

    /// Opt in to TypeScript semantic analysis for project-wide symbol evidence.
    /// This does not emit compiler diagnostics or typed lint findings.
    #[arg(hide_short_help = true, long, global = true)]
    pub type_aware: bool,

    /// Disable TypeScript semantic analysis even when `typeAware.enabled` or
    /// `FALLOW_TYPE_AWARE` opts in, keeping this run fully syntactic.
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        conflicts_with = "type_aware"
    )]
    pub no_type_aware: bool,

    /// TypeScript project config to use for type-aware analysis (repeatable).
    #[arg(hide_short_help = true, long, global = true, value_name = "PATH", action = clap::ArgAction::Append)]
    pub type_aware_project: Vec<PathBuf>,

    /// Decide whether incomplete type-aware analysis is advisory or gating.
    #[arg(hide_short_help = true, long, global = true, value_enum)]
    pub type_aware_require: Option<TypeAwareRequireArg>,
}

impl Cli {
    /// Tri-state CLI override for type-aware analysis: `Some(true)` for
    /// `--type-aware`, `Some(false)` for `--no-type-aware`, `None` when
    /// neither flag was passed (environment and config decide).
    pub const fn type_aware_override(&self) -> Option<bool> {
        if self.no_type_aware {
            Some(false)
        } else if self.type_aware {
            Some(true)
        } else {
            None
        }
    }

    /// The global `--dupes-*` overrides of the `duplicates` config.
    pub const fn dupes_overrides(&self) -> DupesOverrides {
        DupesOverrides {
            mode: self.dupes_mode,
            near: self.dupes_near,
            threshold: self.dupes_threshold,
            min_tokens: self.dupes_min_tokens,
            min_lines: self.dupes_min_lines,
            min_occurrences: self.dupes_min_occurrences,
            skip_local: self.dupes_skip_local,
            ignore_symlinks: resolve_negatable_flag(
                self.dupes_ignore_symlinks,
                self.dupes_no_ignore_symlinks,
            ),
            cross_language: self.dupes_cross_language,
            ignore_imports: resolve_negatable_flag(
                self.dupes_ignore_imports,
                self.dupes_no_ignore_imports,
            ),
        }
    }
}

#[derive(Clone, Copy, Subcommand)]
pub enum TypeAwareCli {
    /// Report companion availability and version compatibility without analysis.
    Status,
}

#[derive(Subcommand)]
pub enum Command {
    /// Analyze project for unused code and dependency hygiene
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

        /// Opt in to `@deprecated` exports that are still in use and only report that issue type
        #[arg(long)]
        deprecated_exports_in_use: bool,

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

        /// Review optional props omitted by known reachable callers (enables this rule)
        #[arg(long)]
        absent_component_props: bool,

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

        /// Only report Effect layers composed only into unused layers
        #[arg(long)]
        unused_effect_layers: bool,

        /// Only report Effect tagged errors that no code produces
        #[arg(long)]
        unused_tagged_errors: bool,

        /// Only report catchTag handlers for a tag that no code produces
        #[arg(long)]
        unproduced_catch_tags: bool,

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

        /// Only report circular dependencies.
        /// Deprecated alias: use `fallow architecture --cycles`.
        #[arg(long)]
        circular_deps: bool,

        /// Only report re-export cycles.
        /// Deprecated alias: use `fallow architecture --cycles`.
        #[arg(long)]
        re_export_cycles: bool,

        /// Only report dependency cycles between workspace packages.
        /// Deprecated alias: use `fallow architecture --cycles`.
        #[arg(long)]
        package_cycles: bool,

        /// Only report boundary violations.
        /// Deprecated alias: use `fallow architecture --boundaries`.
        #[arg(long)]
        boundary_violations: bool,

        /// Only report rule-pack policy violations.
        /// Deprecated alias: use `fallow architecture --policy`.
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

        /// Only report unused package-manager dependency overrides
        #[arg(long)]
        unused_dependency_overrides: bool,

        /// Only report misconfigured package-manager dependency overrides
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

        /// Compute exact-symbol consumers, affected files, and targeted tests.
        #[arg(long, value_name = "FILE:EXPORT")]
        symbol_impact: Option<String>,

        /// Show only the top N items per category. Human output only: the JSON,
        /// SARIF, and CodeClimate envelopes drive exit codes and CI baselines,
        /// so they always carry every finding and a consumer slices the arrays
        /// itself.
        #[arg(long)]
        top: Option<usize>,

        /// Only report issues in the specified file(s). Accepts multiple values.
        /// The full project graph is still built, but only issues in matching files
        /// are reported. Useful for lint-staged pre-commit hooks.
        #[arg(long, value_name = "PATH")]
        file: Vec<std::path::PathBuf>,

        /// Only report the findings with these `finding_id` values. Repeat the flag
        /// or pass a comma-separated list. Ids stay the same under every filter.
        /// The JSON output adds `finding_id_query`: a missing id means "resolved"
        /// only when `conclusive` is true.
        #[arg(long = "finding-id", value_name = "ID", value_delimiter = ',')]
        finding_id: Vec<String>,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },

    /// Check import cycles, boundaries and policy rules after editing
    ///
    /// Run `fallow guard <files>` before an edit to see the rules that apply to
    /// those files.
    Architecture {
        /// Only report import cycles: circular dependencies, re-export cycles
        /// and package cycles
        #[arg(long)]
        cycles: bool,

        /// Only report boundary violations, boundary coverage and forbidden calls
        #[arg(long)]
        boundaries: bool,

        /// Only report rule-pack policy violations
        #[arg(long)]
        policy: bool,

        /// Only report issues in the specified file(s). Accepts multiple values.
        /// The full project graph is still built, but only issues in matching files
        /// are reported.
        #[arg(long, value_name = "PATH")]
        file: Vec<std::path::PathBuf>,

        /// Only report the findings with these `finding_id` values. Repeat the flag
        /// or pass a comma-separated list.
        #[arg(long = "finding-id", value_name = "ID", value_delimiter = ',')]
        finding_id: Vec<String>,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
    },

    /// Watch for changes and re-run analysis
    Watch {
        /// Don't clear the screen between re-analyses
        #[arg(long)]
        no_clear: bool,
    },

    /// Inspect the optional TypeScript semantic companion.
    TypeAware {
        #[command(subcommand)]
        subcommand: TypeAwareCli,
    },

    /// Diagnose project readiness without analysis or mutation.
    ///
    /// Checks the root, config resolution, workspace discovery, external
    /// plugins, and the optional type-aware companion. Uses only local reads;
    /// it performs no cache writes, telemetry, network requests, or third-party
    /// execution. Supports human and JSON output.
    #[command(override_help = doctor::HELP)]
    Doctor,

    /// Find semantically similar functions with a pinned local model (opt-in).
    ///
    /// Results are unverified candidates. The model score is not a probability,
    /// gate, vulnerability verdict, or safe-refactor decision. Model setup is
    /// explicit, and project source remains local and offline during analysis.
    SimilarCode {
        #[command(subcommand)]
        subcommand: Option<similar_code_cli::SimilarCodeSubcommand>,
        /// Minimum cosine similarity retained as an unverified candidate.
        #[arg(long, value_name = "0..1")]
        threshold: Option<f64>,
        /// Minimum source lines per extracted function.
        #[arg(long, value_name = "N")]
        min_lines: Option<usize>,
        /// Cap displayed candidates after bounded full-corpus comparison.
        #[arg(long, value_name = "N")]
        top: Option<usize>,
        /// Report pairs touching one of these project-relative files.
        #[arg(long, value_name = "PATH")]
        file: Vec<PathBuf>,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// Measure how well the tests detect changes in the code (opt-in).
    ///
    /// Changes the code in small ways (mutants) and runs the project tests
    /// against each mutant. A mutant that no test fails on survives. The
    /// default scope is the changed code against the base ref, as `audit`.
    /// `--plan` and `inspect` run no code. Fallow does not write to project
    /// files. The report lists any path that the tests changed.
    #[command(disable_help_subcommand = true)]
    Mutation {
        #[command(subcommand)]
        subcommand: Option<mutation_cli::MutationSubcommand>,
        #[command(flatten)]
        args: mutation_cli::MutationArgs,
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

    /// Trace a symbol's call chain, or the shortest import path between two
    /// modules (best-effort, syntactic; OFF the ranked path)
    ///
    /// Walks callers UP (modules that import the symbol) and callees DOWN
    /// (import-symbol edges + intra-module call sites) via the module graph,
    /// bounded by `--depth`. Symbol-level chains are labeled best-effort per
    /// ADR-001: resolved-vs-unresolved callees are reported honestly, never
    /// silently dropped. The result is its OWN surface, NOT folded into the
    /// ranked brief and NEVER an input to the focus map / ranking.
    ///
    /// `--path <FROM> <TO>` answers a different question: the shortest chain of
    /// imports by which one module reaches another. Type-only hops are reported
    /// rather than skipped, and an unreachable pair reports `reachable: false`
    /// instead of an error.
    Trace {
        /// Target symbol, formatted as FILE:SYMBOL (e.g. src/utils.ts:formatDate).
        /// Omitted when `--path` or `--dependency` is used.
        #[arg(
            value_name = "FILE:SYMBOL",
            required_unless_present_any = ["path", "dependency"]
        )]
        symbol: Option<String>,

        /// How the code uses each imported name of a package: file and call
        /// counts, one hop through project wrappers, and a count of each use
        /// that the trace cannot resolve
        ///
        /// `--sites`, `--specifier`, `--limit` and `--cursor` page the usage
        /// sites. `--callers --depth N` adds the files that import the users.
        #[arg(
            long,
            value_name = "PACKAGE",
            conflicts_with_all = ["symbol", "path", "callees"]
        )]
        dependency: Option<String>,

        /// With `--dependency`, report only these imported names
        /// (repeatable or comma-separated). Turns on the site page.
        #[arg(
            long,
            value_name = "NAME",
            value_delimiter = ',',
            requires = "dependency"
        )]
        specifier: Vec<String>,

        /// With `--dependency`, list the usage sites with file, line and column
        #[arg(long, requires = "dependency")]
        sites: bool,

        /// With `--dependency`, the largest number of sites on a page
        /// (1 to 500, default 50). Turns on the site page.
        #[arg(
            long,
            value_name = "N",
            requires = "dependency",
            value_parser = clap::value_parser!(u16).range(1..=500)
        )]
        limit: Option<u16>,

        /// With `--dependency`, the `next_cursor` of the previous page. Turns
        /// on the site page.
        #[arg(long, value_name = "TOKEN", requires = "dependency")]
        cursor: Option<String>,

        /// Shortest import path between two modules, as two file paths
        /// (e.g. `--path src/app.ts src/db.ts`). Mutually exclusive with the
        /// symbol target and the call-chain flags.
        #[arg(
            long,
            num_args = 2,
            value_names = ["FROM", "TO"],
            conflicts_with_all = ["symbol", "callers", "callees", "depth"]
        )]
        path: Vec<String>,

        /// With `--path`, follow only static value imports, so the route
        /// explains why TO loads before FROM runs
        ///
        /// `import()`, lazy globs, worker loads and `import type` do not
        /// qualify.
        #[arg(long, requires = "path")]
        eager_only: bool,

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

    /// Resolve a runtime stack trace's frames to the definitions they name
    /// (best-effort, syntactic; OFF the ranked path)
    ///
    /// Reads a V8 / Node or SpiderMonkey / JavaScriptCore stack trace from a
    /// file or from stdin, classifies every frame as in-project,
    /// node-modules, or out-of-corpus, and asks the module graph which
    /// definitions the in-project frames name.
    ///
    /// The result refuses to overclaim. A frame matching several definitions
    /// reports `ambiguous` and lists all of them rather than picking one; a
    /// frame matching nothing reports `not_found` rather than disappearing; a
    /// frame the graph was never asked about reports `not_attempted`. Every
    /// frame read stays in the array in input order and `counts` publishes the
    /// per-outcome totals, so a caller can see what went unanswered.
    ///
    /// No source maps are read. A frame inside generated build output is
    /// reported as such, because a stale map rebinds silently to the wrong
    /// line.
    #[command(name = "trace-error")]
    TraceError {
        /// Stack trace file, or `-` to read stdin. Defaults to stdin. A
        /// relative path is resolved against the project root, matching
        /// `--diff-file`.
        #[arg(value_name = "FILE")]
        trace_file: Option<String>,
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

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        /// Only fixes touching scoped files are planned and applied.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
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

    /// Wire fallow into the coding-agent harnesses used by this project in
    /// one pass (AGENTS.md task map, skill, MCP server, commit/push gate), or
    /// show and remove what was installed. `fallow init --agents` and
    /// `fallow hooks install --target agent` remain the single-piece
    /// commands underneath.
    Agent {
        #[command(subcommand)]
        subcommand: AgentCli,
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

    /// List discovered entry points, files, plugins, boundaries, workspaces, and the
    /// startup import weight.
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

        /// Show the startup import weight of each runtime entry point, in
        /// source bytes (not bundle size)
        ///
        /// For each runtime entry: the modules and source bytes that load
        /// before the entry runs, the modules behind `import()` and behind
        /// workers or forks, the packages on the startup path, and the single
        /// imports that keep the most bytes eager. Source bytes include types
        /// and comments. An import without the `type` keyword counts as eager,
        /// even when it brings in only types that TypeScript removes.
        #[arg(long)]
        entry_weight: bool,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
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

        /// Enable function-scoped near-miss clone detection.
        #[arg(long)]
        near: bool,

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

        /// Omit clone instances whose path is a symlink, or lies under a
        /// symlinked directory. A clone group with fewer than two remaining
        /// instances is not reported. Without this flag, JSON output marks
        /// these instances with `is_symlink: true`.
        #[arg(long)]
        ignore_symlinks: bool,

        /// Report symlinked clone instances (opt out of a config
        /// `duplicates.ignoreSymlinks: true`).
        #[arg(long, conflicts_with = "ignore_symlinks")]
        no_ignore_symlinks: bool,

        /// Show only the N highest-ranked clone groups. Ranking combines clone
        /// size, occurrence count, and capped directory or line spread.
        /// `clone_families[]` is rebuilt from the groups that survive, so it
        /// narrows too. `stats` keeps describing the whole corpus;
        /// `clone_groups_shown` / `clone_groups_omitted` and
        /// `clone_families_shown` / `clone_families_omitted` report the split.
        /// Refused with exit code 2 alongside `--group-by`: grouped output
        /// reports per-bucket stats over every clone group in the bucket, which
        /// a global top-N truncation would silently contradict.
        #[arg(long)]
        top: Option<usize>,

        /// Omit the verbatim source text from each clone instance in
        /// `--format json`. The file and line/column range still address the
        /// same code, and this is most of the payload on a duplicated codebase.
        #[arg(long)]
        no_fragments: bool,

        /// Trace all clones at a specific location (format: `FILE:LINE`)
        #[arg(long, value_name = "FILE:LINE")]
        trace: Option<String>,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
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

        /// Show advisory project-local public-signature type coupling. Requires
        /// type-aware analysis and does not change the health score.
        #[arg(long)]
        type_coupling: bool,

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
        /// The complexity-* rules apply first: a finding whose rule is warn
        /// never fails the run, whatever its severity.
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
        /// A given path must resolve inside the project root, its Git work tree, the CI workspace or a temp directory.
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

        /// Compare current metrics against this snapshot file instead of the
        /// newest file in `.fallow/snapshots/`. Use it to restore a baseline
        /// from external storage in CI. With --group-by, groups are compared
        /// by key when the snapshot holds the same grouping. Implies --trend.
        #[arg(long, value_name = "PATH")]
        trend_from: Option<PathBuf>,

        /// Path to coverage data for exact per-function CRAP scores. Accepts an
        /// Istanbul coverage map JSON file (coverage-final.json, from
        /// `jest --coverage`, `vitest run --coverage`, c8 or nyc), a directory
        /// containing coverage-final.json, a raw V8 coverage directory
        /// (`NODE_V8_COVERAGE=<dir> node --test`), or a single V8 coverage JSON
        /// file. Transpiled V8 scripts (tsx, bundles) map back to their source
        /// files through the source map that Node records in the dump. A
        /// script that differs from the file on disk and has no source map
        /// keeps the estimate. Use --coverage-root when
        /// the data was generated in a different environment (CI runner,
        /// Docker). Affects CRAP scores only, not --coverage-gaps. Also
        /// configurable via FALLOW_COVERAGE env var.
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
        /// A single local capture is free. Continuous or multi-capture
        /// monitoring needs a license (see `fallow license`).
        #[arg(long, value_name = "PATH")]
        runtime_coverage: Option<PathBuf>,

        /// Saved `fallow mutation --format json` report (for example
        /// `.fallow/mutation/last-report.json` or the `-o` file of a run).
        /// Adds the mutation score of each complexity finding next to its
        /// CRAP score. The score never changes CRAP or the health score. A
        /// function whose file changed since the run gets no score. Reads
        /// the file only; it runs no test.
        #[arg(long, value_name = "FILE")]
        mutation_report: Option<PathBuf>,

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

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
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

        /// Add a retirement report: one row per flag, with the reasons the
        /// flag can be retired. Advisory only; nothing is removed.
        #[arg(long)]
        retirement: bool,

        /// Keep only retirement rows with this reason (repeatable)
        #[arg(long = "reason", value_name = "CODE", requires = "retirement")]
        reasons: Vec<flags::RetirementReasonArg>,

        /// Order of the retirement rows
        #[arg(
            long,
            value_name = "KEY",
            requires = "retirement",
            default_value = "age"
        )]
        sort: flags::RetirementSortArg,

        /// How to measure flag age: blame (lower bound), pickaxe (first
        /// commit with the name, slower) or off
        #[arg(
            long,
            value_name = "MODE",
            requires = "retirement",
            default_value = "blame"
        )]
        flag_age: flags::FlagAgeArg,

        /// Keep only retirement rows at least this many days old
        #[arg(long, value_name = "DAYS", requires = "retirement")]
        min_age: Option<u64>,

        /// Vendor flag export (JSON, read offline) that adds the
        /// fully-rolled-out, archived-in-vendor, missing-in-vendor and
        /// vendor-only reasons
        #[arg(long, value_name = "FILE", requires = "retirement")]
        flag_state: Option<std::path::PathBuf>,

        /// Exit with code 1 when a flag in scope is older than this many
        /// days. Opt-in; needs a flag age
        #[arg(long, value_name = "DAYS", requires = "retirement")]
        max_flag_age: Option<u64>,
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

        /// Path to Istanbul coverage data (coverage-final.json) or raw V8
        /// coverage (a `NODE_V8_COVERAGE` directory or one V8 JSON file) for
        /// accurate per-function CRAP scores in the health sub-analysis. Also
        /// configurable via FALLOW_COVERAGE or health.coverage.
        #[arg(long, value_name = "PATH")]
        coverage: Option<PathBuf>,

        /// Absolute prefix to strip from coverage data paths before CRAP matching.
        /// Use when coverage was generated under a different checkout root in CI or Docker.
        /// Also configurable via FALLOW_COVERAGE_ROOT or health.coverageRoot.
        #[arg(long, value_name = "PATH")]
        coverage_root: Option<PathBuf>,

        /// Run only these sub-analyses (comma-separated: dead-code,
        /// architecture, dupes, health), with the same values as bare
        /// `fallow --only`. Replaces the `audit.deadCode`, `audit.health` and
        /// `audit.dupes` selection. The review brief (`--brief`, `fallow
        /// review`) keeps every sub-analysis.
        #[arg(long, value_delimiter = ',', conflicts_with = "skip")]
        only: Vec<AnalysisKind>,

        /// Skip these sub-analyses (comma-separated: dead-code, architecture,
        /// dupes, health), with the same values as bare `fallow --skip`, on
        /// top of the `audit.*` selection.
        #[arg(long, value_delimiter = ',')]
        skip: Vec<AnalysisKind>,

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

        /// How a worsened complexity finding affects the audit verdict.
        ///
        /// A worsened finding was already in the base snapshot, and a measured
        /// value (cyclomatic, cognitive, CRAP or line count) is higher now.
        /// warn (default): the verdict is at least warn, the run does not
        /// fail. error: a worsened finding counts like an introduced finding.
        /// Overrides `audit.worsened` in config. Has no effect with
        /// `--gate all`, which counts every finding.
        #[arg(long, value_enum)]
        worsened: Option<AuditWorsenedArg>,

        /// Runtime coverage input. Accepts a V8 directory, a single V8 JSON
        /// file, or an Istanbul coverage map JSON. Runs the `fallow-cov`
        /// sidecar inside the audit, so the `hot-path-touched` verdict shows
        /// next to the dead-code and complexity findings without a second
        /// `fallow health` run in CI. The verdict is informational and does
        /// not change the exit code. A single local capture is free.
        /// Continuous or multi-capture monitoring needs a license (see
        /// `fallow license`).
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
        /// echoed graph-snapshot hash no longer matches (the tree moved); rejects
        /// an `action` outside `block`, `address`, `consider`, `fyi`
        /// (`invalid-action`). The verifier is the graph, not a second model.
        /// Implies the brief; always exits 0. The agent's free-text framing and
        /// action label are fenced as non-deterministic and never gate or
        /// auto-post.
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
        /// consulted on the `--walkthrough` path.
        #[arg(long)]
        show_deprioritized: bool,

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// Maintain reusable audit base-snapshot caches.
    AuditCache {
        #[command(subcommand)]
        subcommand: AuditCacheCli,
    },

    /// Maintain the committed baseline files that the config points at.
    Baselines {
        #[command(subcommand)]
        subcommand: BaselinesCli,
    },

    /// Surface the consequential structural DECISIONS a change embeds (the apex
    /// of the review brief), each framed as a judgment question with the routed
    /// expert to ask.
    ///
    /// The product's decision surface: a ranked, capped (4 plus or minus 1),
    /// signal_id-anchored set of the SOLID-3 decisions (coupling/boundary,
    /// exports-aware public-API/contract, dependency). Runs the same changed-code
    /// analysis as `fallow review` but emits ONLY the decisions, separable and
    /// cheap. Coupling and public-API decisions are suppressible with
    /// `// fallow-ignore`; a dependency decision anchors on `package.json` and
    /// has no suppress action. Never gates: exits 0 after a successful run, and
    /// exits 2 on invalid input (for example the global `--baseline` or
    /// `--save-baseline` flag) like every other command. Use `--base` /
    /// `--changed-since` to pick the comparison point, exactly like `fallow audit`.
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
        /// Runtime coverage input. Accepts a V8 directory, a single V8 JSON
        /// file, or an Istanbul coverage map JSON. When set, `fallow security`
        /// adds production runtime state to tainted-sink candidates and uses
        /// that state as an extra ranking signal. A single local capture is
        /// free. Continuous or multi-capture monitoring needs a license (see
        /// `fallow license`).
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

        /// Scope reported findings to this file or directory (default: whole project).
        /// The full project graph is still built; only reported items are narrowed.
        #[arg(value_name = "PATH")]
        path: Option<PathBuf>,
    },

    /// Render a saved `--format json` results file in another format without
    /// re-running analysis (analyze once, then render every CI surface from
    /// the same file). Supports GitHub annotations/summary, CodeClimate,
    /// SARIF, markdown, and GitHub/GitLab PR-comment and review formats.
    Report {
        /// Path to a fallow JSON results file produced by `--format json`
        /// (dead-code, architecture, dupes, health, audit, security, or bare
        /// combined).
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
    /// `setup` is the resumable first-run flow: license check, sidecar
    /// install, coverage recipe, then analysis.
    Coverage {
        #[command(subcommand)]
        subcommand: CoverageCli,
    },

    /// Install or remove a Claude Code PreToolUse hook that gates
    /// `git commit` / `git push` on `fallow audit`, so the agent cleans
    /// findings before the command runs.
    ///
    /// Deprecated: use `fallow agent install` (one pass for every harness)
    /// or `fallow hooks install --target agent` (the gate alone). This
    /// command keeps working throughout fallow 3 and is removed in the next
    /// major. It writes into `.claude/settings.json` +
    /// `.claude/hooks/fallow-gate.sh`, or for Codex into `.codex/hooks.json` +
    /// `.codex/hooks/fallow-gate.sh` plus an `AGENTS.md` routing block. For a
    /// shell-level Git pre-commit hook in
    /// `.git/hooks/`, see `fallow hooks install --target git` instead.
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

    /// Generate an interactive HTML map of the codebase
    Viz {
        /// Output file path (default: fallow-viz.html in project root)
        #[arg(long = "out", value_name = "PATH")]
        output: Option<PathBuf>,

        /// Don't open the output file in the browser
        #[arg(long)]
        no_open: bool,

        /// Visualization output format
        #[arg(long = "viz-format", default_value = "html")]
        viz_format: viz::VizFormat,
    },
}

#[derive(Subcommand)]
pub enum SecuritySubcommand {
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
pub enum BaselinesCli {
    /// Remove the baseline entries that match no current finding.
    ///
    /// Reads the files that `audit.deadCodeBaseline`, `audit.healthBaseline`
    /// and `audit.dupesBaseline` name, runs one whole-project analysis, and
    /// removes each entry that `--baseline` would no longer match. Prune never
    /// adds an entry: a new finding stays outside the baseline, and the normal
    /// gates report it. To record new findings on purpose, use
    /// `fallow <dead-code|health|dupes> --save-baseline`.
    ///
    /// Exit codes: 0 when the files are pruned or have nothing to prune, 1 with
    /// `--check` when an entry can be pruned, and 2 when a file cannot be read
    /// or pruned. A file with an older key form is skipped with the command
    /// that saves it again.
    Prune {
        /// Report the entries that a prune would remove, and write nothing.
        #[arg(long)]
        check: bool,

        /// The dead-code baseline to prune. Overrides `audit.deadCodeBaseline`.
        #[arg(long, value_name = "PATH")]
        dead_code_baseline: Option<PathBuf>,

        /// The health baseline to prune. Overrides `audit.healthBaseline`.
        #[arg(long, value_name = "PATH")]
        health_baseline: Option<PathBuf>,

        /// The duplication baseline to prune. Overrides `audit.dupesBaseline`.
        #[arg(long, value_name = "PATH")]
        dupes_baseline: Option<PathBuf>,

        /// The coverage input for CRAP scores, as for `fallow health
        /// --coverage`. Use the input that the health baseline was saved with.
        #[arg(long, value_name = "PATH")]
        coverage: Option<PathBuf>,

        /// Rebase the file paths of the coverage input onto this root.
        #[arg(long, value_name = "PATH")]
        coverage_root: Option<PathBuf>,
    },
}

#[derive(clap::Subcommand)]
pub enum AuditCacheCli {
    /// Remove reusable audit caches owned by an explicit project root.
    ///
    /// Deletes this project's cache entries unconditionally, warm or not. To
    /// apply the age-based GC policy across every cache entry instead, use
    /// `fallow audit-cache prune`.
    Remove {
        /// Print what would be removed without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Confirm removal in non-interactive environments.
        #[arg(long, alias = "force")]
        yes: bool,
    },

    /// Apply the audit cache GC policy now and report every entry.
    ///
    /// Runs the same reclaim policy every `fallow audit` run already applies
    /// silently: orphaned-sidecar cleanup, age-based reclaim under the
    /// resolved threshold, and cross-repo reclaim of abandoned entries whose
    /// recorded owner root no longer exists. It also bounds the base-snapshot
    /// files in the project cache directory (`pass: "snapshot"`): files older
    /// than the threshold go, and only the newest 32 stay. Entries owned by
    /// other live projects are never touched. Use `--dry-run` to preview every decision
    /// the policy would take without touching the filesystem. `--root` is
    /// optional and defaults to the current directory. Reported sizes come
    /// from a full recursive walk of each cache entry, which can take a few
    /// seconds on large caches. To delete one project's caches
    /// unconditionally, use `fallow audit-cache remove --root <path> --yes`.
    Prune {
        /// Preview decisions without touching the filesystem.
        #[arg(long)]
        dry_run: bool,

        /// Age threshold in days for this invocation. Overrides
        /// FALLOW_AUDIT_CACHE_MAX_AGE_DAYS and the `audit.cacheMaxAgeDays`
        /// config field (default 30). `0` disables age-based reclaim but
        /// still reclaims orphaned sidecars and entries whose recorded owner
        /// root is gone; unconditional deletion of one project's caches is
        /// `fallow audit-cache remove --root <path> --yes`.
        #[arg(long, value_name = "N")]
        max_age_days: Option<u32>,
    },
}

#[derive(clap::Subcommand)]
pub enum LicenseCli {
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
    ///
    /// The stored license JWT is the primary identity proof. When it is
    /// missing, or the cloud reports it as too stale to exchange, the request
    /// is retried with a full-access API key.
    Refresh {
        /// Fallow Cloud API key (bearer token) used when the stored license
        /// JWT cannot be refreshed.
        ///
        /// Precedence: this flag > $FALLOW_API_KEY. Generate at
        /// <https://fallow.cloud/settings#api-keys>.
        ///
        /// Security: prefer $FALLOW_API_KEY on shared CI runners. Passing a
        /// secret on the command line may be visible to other processes via
        /// `ps` and can leak into shell history or process audit logs.
        #[arg(long, value_name = "KEY")]
        api_key: Option<String>,
    },
    /// Remove the local license file.
    Deactivate,
}

#[derive(Clone, Copy, clap::Subcommand)]
pub enum TelemetryCli {
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
pub enum CiTemplateCli {
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
pub enum CoverageCli {
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
        /// File or directory containing local runtime coverage input. A
        /// single local capture is free. Continuous or multi-capture
        /// monitoring needs a license (see `fallow license`).
        #[arg(long, value_name = "PATH", conflicts_with = "cloud")]
        runtime_coverage: Option<PathBuf>,

        /// Fetch latest runtime facts from Fallow Cloud for the selected repo.
        #[arg(long, visible_alias = "runtime-coverage-cloud")]
        cloud: bool,

        /// Fallow Cloud API key. Precedence: this flag > $FALLOW_API_KEY.
        #[arg(long, value_name = "KEY")]
        api_key: Option<String>,

        /// Override the Fallow Cloud base URL.
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

        /// List cloud runtime functions with no local counterpart on stderr.
        #[arg(long)]
        debug_unmatched: bool,
    },
    /// Upload a static function inventory to Fallow Cloud. Needs a fallow
    /// cloud API key. Unlocks the `untracked` filter on the dashboard by
    /// pairing runtime coverage data with the AST view of "every function
    /// that exists". See <https://fallow.tools/docs/analysis/runtime-coverage/>.
    ///
    /// This command makes network calls to Fallow Cloud. `fallow dead-code`
    /// stays offline.
    ///
    /// Exit codes: 0 ok · 7 network · 10 validation · 11 payload too large
    /// · 12 auth rejected · 13 server error.
    UploadInventory {
        /// Fallow Cloud API key (bearer token).
        ///
        /// Precedence: this flag > $FALLOW_API_KEY. Generate at
        /// <https://fallow.cloud/settings#api-keys>.
        ///
        /// Security: prefer $FALLOW_API_KEY on shared CI runners. Passing a
        /// secret on the command line may be visible to other processes via
        /// `ps` and can leak into shell history or process audit logs.
        #[arg(long, value_name = "KEY")]
        api_key: Option<String>,

        /// Override the Fallow Cloud base URL.
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
    /// Upload JavaScript source maps to Fallow Cloud for bundled runtime coverage.
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

        /// Override the Fallow Cloud base URL.
        #[arg(long, value_name = "URL")]
        endpoint: Option<String>,

        /// Send only the basename as fileName by default.
        ///
        /// Use `--strip-path=false` when your runtime coverage reports bundle
        /// paths relative to the build directory, such as `assets/app.js`.
        /// When two maps share a basename, each of them is sent under its path
        /// in the build directory, so that no map replaces another.
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
    /// Upload static dead-code findings to Fallow Cloud for the source-evidence viewer.
    ///
    /// Runs fallow's static analysis and uploads the `unused_export` and
    /// `dead_file` verdicts under the selected repo + git SHA. The cloud
    /// overlays them on the source view alongside the runtime coverage overlay.
    /// Findings are replace-by-SHA: each run sends the complete set for the SHA.
    UploadStaticFindings {
        /// Fallow Cloud API key (bearer token).
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

        /// Override the Fallow Cloud base URL.
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
    /// Read the production facts of changed code from Fallow Cloud.
    ///
    /// Sends the changed files or functions to the cloud review-packet
    /// endpoint and prints the answer as JSON: per function the production
    /// calls, the tracking state and the evidence window. It runs no local
    /// analysis and pulls no full runtime context, so the answer stays small.
    /// Without --file and --function, the files changed against the base are
    /// sent. The base resolves like `fallow audit`: `--base` (alias of
    /// `--changed-since`), then $FALLOW_AUDIT_BASE, then the merge-base with
    /// the upstream or the remote default branch.
    ReviewPacket {
        /// Fallow Cloud API key. Precedence: this flag > $FALLOW_API_KEY.
        #[arg(long, value_name = "KEY")]
        api_key: Option<String>,

        /// Override the Fallow Cloud base URL.
        #[arg(long, value_name = "URL")]
        api_endpoint: Option<String>,

        /// Repository identifier, for example `owner/repo`.
        ///
        /// Defaults to $FALLOW_REPO, then the parsed origin URL from
        /// `git remote get-url origin`.
        #[arg(long, value_name = "OWNER/REPO")]
        repo: Option<String>,

        /// Optional monorepo/project disambiguator.
        #[arg(long, value_name = "ID")]
        project_id: Option<String>,

        /// Runtime observation window (1..=90 days). The cloud default is 30.
        #[arg(long, value_name = "DAYS")]
        coverage_period: Option<u16>,

        /// Optional commit SHA of the deployment to read.
        #[arg(long, value_name = "SHA")]
        commit_sha: Option<String>,

        /// Repo-relative file to include. Repeatable.
        #[arg(long = "file", value_name = "PATH")]
        files: Vec<String>,

        /// Function to include, as FILE:NAME or FILE:NAME:LINE. Repeatable.
        #[arg(long = "function", value_name = "FILE:NAME[:LINE]")]
        functions: Vec<String>,
    },
    /// Read how production behavior changed between two deployments.
    ///
    /// Prints the cloud deployment change report as JSON: per function the
    /// change kind (new_called, new_not_called, heated_up, cooled_down,
    /// stopped, unchanged) between the deployment and its base. Pass the
    /// base deployment commit with `--base <sha>`; without it, the cloud
    /// picks the previous deployment with production runtime.
    DeploymentChanges {
        /// Fallow Cloud API key. Precedence: this flag > $FALLOW_API_KEY.
        #[arg(long, value_name = "KEY")]
        api_key: Option<String>,

        /// Override the Fallow Cloud base URL.
        #[arg(long, value_name = "URL")]
        api_endpoint: Option<String>,

        /// Repository identifier, for example `owner/repo`.
        ///
        /// Defaults to $FALLOW_REPO, then the parsed origin URL from
        /// `git remote get-url origin`.
        #[arg(long, value_name = "OWNER/REPO")]
        repo: Option<String>,

        /// Commit SHA of the deployment. Default: `git rev-parse HEAD`.
        #[arg(long, value_name = "SHA")]
        sha: Option<String>,

        /// Show only one change kind.
        #[arg(long, value_name = "KIND", value_parser = [
            "stopped", "new_not_called", "heated_up", "cooled_down", "new_called", "unchanged",
        ])]
        change: Option<String>,

        /// Page size (1..=200).
        #[arg(long, value_name = "N")]
        limit: Option<u16>,

        /// Cursor from `meta.cursor` of the previous page.
        #[arg(long, value_name = "CURSOR")]
        cursor: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum CiCli {
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
pub enum RulePackCli {
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

/// CLI mirror of [`fallow_engine::baseline::HealthBaselineMode`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum BaselineModeArg {
    /// Match a saved health baseline per file and finding category.
    #[default]
    Count,
    /// Match a saved health baseline per function identity and finding
    /// category, so a hotspot that replaces another hotspot is reported.
    Identity,
}

impl From<BaselineModeArg> for fallow_engine::baseline::HealthBaselineMode {
    fn from(value: BaselineModeArg) -> Self {
        match value {
            BaselineModeArg::Count => Self::Count,
            BaselineModeArg::Identity => Self::Identity,
        }
    }
}

#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum CiProviderArg {
    Github,
    Gitlab,
}

/// CLI mirror of [`fallow_config::TypeAwareRequire`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum TypeAwareRequireArg {
    /// Keep conservative findings and report semantic gaps.
    BestEffort,
    /// Fail the quality gate when a requested semantic query is incomplete.
    Complete,
}

impl From<TypeAwareRequireArg> for fallow_config::TypeAwareRequire {
    fn from(value: TypeAwareRequireArg) -> Self {
        match value {
            TypeAwareRequireArg::BestEffort => Self::BestEffort,
            TypeAwareRequireArg::Complete => Self::Complete,
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
    pub(crate) const fn to_estimate(self) -> fallow_output::EffortEstimate {
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
    pub(crate) const fn to_health_severity(self) -> fallow_output::FindingSeverity {
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
    pub(crate) const fn to_config(self) -> fallow_config::EmailMode {
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

/// CLI value of `--worsened`, mirroring `fallow_config::AuditWorsened`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum AuditWorsenedArg {
    /// A worsened finding makes the verdict at least warn.
    Warn,
    /// A worsened finding counts like an introduced finding.
    Error,
}

impl From<AuditWorsenedArg> for fallow_config::AuditWorsened {
    fn from(value: AuditWorsenedArg) -> Self {
        match value {
            AuditWorsenedArg::Warn => Self::Warn,
            AuditWorsenedArg::Error => Self::Error,
        }
    }
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
