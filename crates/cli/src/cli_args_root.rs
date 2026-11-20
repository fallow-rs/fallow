//! The `fallow` root command: its help text and the global options.

use super::*;

const TOP_LEVEL_HELP_TEMPLATE: &str =
    "{about-with-newline}\n{usage-heading} {usage}{after-help}\n\nOptions:\n{options}";

// Macros instead of consts so `concat!` can assemble the short (`-h`) and
// long (`--help`) after-help surfaces from the same source fragments. The
// cheat sheet file ends with a newline, so one `\n` follows it below.
macro_rules! top_level_task_cheat_sheet {
    () => {
        include_str!("cli_task_cheat_sheet.txt")
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
  cloud          Log in to Fallow Cloud and store a read-only API key
  license        Manage the paid-feature license
  telemetry      Manage opt-in product telemetry

Reference:
  schema         Dump the CLI interface as machine-readable JSON
  help           Print this message or the help of a command"
    };
}

const TOP_LEVEL_AFTER_HELP: &str = concat!(
    top_level_task_cheat_sheet!(),
    "\n",
    top_level_core_command_groups!(),
    "\n\nRun fallow --help for the complete command list."
);

pub const TOP_LEVEL_AFTER_LONG_HELP: &str = concat!(
    top_level_task_cheat_sheet!(),
    "\n",
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

    /// Print version. Accepts `-v`, `-V`, and `--version`.
    #[arg(
        short = 'v',
        visible_short_alias = 'V',
        long = "version",
        action = clap::ArgAction::Version
    )]
    pub version: Option<bool>,

    /// Project root directory
    #[arg(short, long, global = true, help_heading = "Project")]
    pub root: Option<PathBuf>,

    /// Path to config file (.fallowrc.json, .fallowrc.jsonc, fallow.toml, or .fallow.toml)
    #[arg(short, long, global = true, help_heading = "Project")]
    pub config: Option<PathBuf>,

    /// Allow trusted config files to extend HTTPS URLs
    #[arg(hide_short_help = true, long, global = true, help_heading = "Project")]
    pub allow_remote_extends: bool,

    /// Output format (alias: --output)
    #[arg(
        short,
        long,
        visible_alias = "output",
        global = true,
        default_value = "human",
        help_heading = "Output"
    )]
    pub format: Format,

    #[command(flatten)]
    pub json: crate::json_style::JsonPresentationArgs,

    /// Suppress progress output; `audit` also prints only actionable findings
    #[arg(short, long, global = true, help_heading = "Output")]
    pub quiet: bool,

    /// Only report issues in files changed since this git ref (e.g., main, HEAD~5)
    ///
    /// For this run, the ref replaces the per-package refs of
    /// `workspaces.changedSince` in the config.
    #[arg(long, visible_alias = "base", global = true, help_heading = "Scope")]
    pub changed_since: Option<String>,

    /// Ignore the per-package refs of `workspaces.changedSince` for this run
    ///
    /// Every workspace package is then analyzed in full scope, for example to
    /// save or gate a whole-project baseline. A global `--changed-since`,
    /// `--workspace` or `--diff-file` scope still applies.
    #[arg(hide_short_help = true, long, global = true, help_heading = "Scope")]
    pub no_package_baselines: bool,

    /// Unified diff for line-level scoping.
    /// Use `-` to read from stdin. Project-level findings still bypass this
    /// filter. When both this and `--changed-since` are set, the diff filter
    /// wins for finding scope while `--changed-since` still drives file discovery.
    #[arg(
        hide_short_help = true,
        long = "diff-file",
        value_name = "PATH",
        global = true,
        help_heading = "Scope"
    )]
    pub diff_file: Option<PathBuf>,

    /// Read the unified diff from stdin.
    /// Equivalent to `--diff-file -`.
    #[arg(
        hide_short_help = true,
        long = "diff-stdin",
        global = true,
        help_heading = "Scope"
    )]
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
        global = true,
        help_heading = "Project"
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
        global = true,
        help_heading = "Scope"
    )]
    pub max_file_size: Option<u32>,

    /// Compare against a previously saved baseline file. Used by bare
    /// `fallow`, `dead-code`, `architecture`, `dupes` and `health`; other
    /// subcommands reject it. On bare `fallow` it holds the dead-code baseline
    /// only: pass health and duplication baselines with `--health-baseline`
    /// and `--dupes-baseline`
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        help_heading = "Baselines"
    )]
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
        global = true,
        help_heading = "Baselines"
    )]
    pub baseline_mode: Option<BaselineModeArg>,

    /// Correlate this run with a previous telemetry analysis run.
    ///
    /// Used only for opt-in telemetry follow-up measurement. The value is not
    /// interpreted as a path, repository, package, or user identifier. Hidden
    /// from `--help`; agents receive the correlation token from JSON output.
    #[arg(
        long,
        global = true,
        value_name = "RUN_ID",
        hide = true,
        help_heading = "Project"
    )]
    pub parent_run: Option<String>,

    /// Save the current results as a baseline file. Used by bare `fallow`,
    /// `dead-code`, `architecture`, `dupes` and `health`; other subcommands
    /// reject it. On bare `fallow` it saves the dead-code baseline only: save
    /// the others with `fallow health --save-baseline` and `fallow dupes
    /// --save-baseline`. The path must resolve inside the project root, its Git
    /// work tree, the CI workspace or a temp directory
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        help_heading = "Baselines"
    )]
    pub save_baseline: Option<PathBuf>,

    /// Production mode: exclude test/story/dev files, only start/build scripts,
    /// report type-only dependencies
    #[arg(long, global = true, help_heading = "Scope")]
    pub production: bool,

    /// Force production mode OFF for every analysis, overriding a project
    /// config's `production: true` (and `FALLOW_PRODUCTION`). Conflicts with
    /// `--production`.
    #[arg(
        hide_short_help = true,
        long = "no-production",
        global = true,
        conflicts_with = "production",
        help_heading = "Scope"
    )]
    pub no_production: bool,

    /// Run dead-code analysis in production mode when using bare combined mode.
    #[arg(
        hide_short_help = true,
        long = "production-dead-code",
        help_heading = "Scope"
    )]
    pub production_dead_code: bool,

    /// Run health analysis in production mode when using bare combined mode.
    #[arg(
        hide_short_help = true,
        long = "production-health",
        help_heading = "Scope"
    )]
    pub production_health: bool,

    /// Run duplication analysis in production mode when using bare combined mode.
    #[arg(
        hide_short_help = true,
        long = "production-dupes",
        help_heading = "Scope"
    )]
    pub production_dupes: bool,

    /// Scope output to selected workspaces.
    /// Accepts exact names, glob patterns, and `!`-prefixed negations.
    /// Values can be comma-separated or repeated.
    #[arg(
        short,
        long,
        global = true,
        value_delimiter = ',',
        help_heading = "Scope"
    )]
    pub workspace: Option<Vec<String>>,

    /// Scope output to workspaces touched since the given git ref.
    /// Git is required. Mutually exclusive with `--workspace`.
    #[arg(long, global = true, value_name = "REF", help_heading = "Scope")]
    pub changed_workspaces: Option<String>,

    /// Group output by owner or by directory.
    #[arg(hide_short_help = true, long, global = true, help_heading = "Output")]
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
        requires = "group_by",
        help_heading = "Output"
    )]
    pub group: Option<Vec<String>>,

    /// Include metric definitions and rule descriptions in output.
    #[arg(hide_short_help = true, long, global = true, help_heading = "Output")]
    pub explain: bool,

    /// Show per-pattern counts for skipped files: default duplicate ignores on dupes
    /// and audit, built-in discovery ignores on check, dead-code, audit and the default run.
    #[arg(hide_short_help = true, long, global = true, help_heading = "Output")]
    pub explain_skipped: bool,

    /// Show only category counts without individual items
    #[arg(hide_short_help = true, long, global = true, help_heading = "Output")]
    pub summary: bool,

    /// CI mode: equivalent to --format sarif --fail-on-issues --quiet
    #[arg(long, global = true, help_heading = "Gates")]
    pub ci: bool,

    /// Exit 1 on issues, also where the default is 0: `dupes`, and bare `fallow` in the json, sarif, codeclimate, GitHub, comment and review formats
    #[arg(hide_short_help = true, long, global = true, help_heading = "Gates")]
    pub fail_on_issues: bool,

    /// Write SARIF output to a file (in addition to the primary --format output).
    /// Used by bare `fallow`, `dead-code`, `architecture` and `security`. The
    /// path must resolve inside the project root, its Git work tree, the CI
    /// workspace or a temp directory
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        value_name = "PATH",
        help_heading = "Output"
    )]
    pub sarif_file: Option<PathBuf>,

    /// Write the report to a file instead of stdout, for any --format (no ANSI
    /// codes). Useful on large projects where the terminal scrollback truncates
    /// the top. Progress and the confirmation stay on stderr. The path must
    /// resolve inside the project root, its Git work tree, the CI workspace or
    /// a temp directory.
    #[arg(
        short = 'o',
        long,
        global = true,
        value_name = "PATH",
        help_heading = "Output"
    )]
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
        value_name = "PREFIX",
        help_heading = "Output"
    )]
    pub report_path_prefix: Option<String>,

    /// Fail if issue count increased beyond tolerance compared to a regression baseline.
    ///
    /// With `list --entry-weight`, fail when the eager source bytes of an
    /// entry grew beyond the tolerance. Without this flag that comparison is
    /// report-only.
    #[arg(hide_short_help = true, long, global = true, help_heading = "Gates")]
    pub fail_on_regression: bool,

    /// Exit with code 1 if a loaded --baseline has entries that match nothing in this run.
    ///
    /// Stricter than the default advisory warning: any stale entry fails, not
    /// just a quarter of the baseline. Applies in every output format. The
    /// verdict reaches a machine consumer as `gate_outcomes["stale-baseline"]`,
    /// which is published with or without this flag; the flag decides only
    /// whether that entry is `enforced`, and the exit code plus one stderr
    /// line. Inert when no baseline is loaded, on runs narrowed to part of
    /// the project, which cannot judge a baseline saved with another scope,
    /// on `health` and `dupes` in production mode, and under `health --report-only`, which never
    /// fails a run; such a run prints why it stood down instead of passing
    /// quietly. `fallow audit` always analyzes only changed files, so the gate
    /// never fires there.
    #[arg(hide_short_help = true, long, global = true, help_heading = "Gates")]
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
    #[arg(hide_short_help = true, long, global = true, help_heading = "Gates")]
    pub fail_on_baseline_growth: bool,

    /// The git ref that --fail-on-baseline-growth compares the baseline with.
    ///
    /// Defaults to the `fallow audit` base resolution. Needs
    /// --fail-on-baseline-growth.
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        value_name = "REF",
        help_heading = "Gates"
    )]
    pub baseline_base: Option<String>,

    /// Exit with code 1 if fallow could not parse a source file cleanly.
    ///
    /// Arms the `parse-error` gate: a file with a `source-parse-degraded` or
    /// `source-nesting-too-deep` entry in `workspace_diagnostics[]` fails the run, and the gate entry in
    /// `gate_outcomes["parse-error"]` names each such file. Off by default,
    /// because the parser also rejects valid syntax that is newer than the
    /// parser. Applies to `dead-code`, `architecture`, `health`, `audit` and
    /// the bare run, in every output format. The `failOnParseError` config key
    /// arms the same gate. `health --report-only` never fails a run.
    #[arg(hide_short_help = true, long, global = true, help_heading = "Gates")]
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
        default_value = "0",
        help_heading = "Gates"
    )]
    pub tolerance: String,

    /// Path to the regression baseline file.
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        value_name = "PATH",
        help_heading = "Baselines"
    )]
    pub regression_baseline: Option<PathBuf>,

    /// Save the current issue counts as a regression baseline. Omit PATH to
    /// update regression.baseline in the discovered fallow config, or create
    /// .fallowrc.json when none exists. Provide PATH to write a standalone file;
    /// PATH must resolve inside the project root, its Git work tree, the CI workspace or a temp directory.
    #[expect(
        clippy::option_option,
        reason = "clap pattern: None=not passed, Some(None)=flag only (write to config), Some(Some(path))=write to file"
    )]
    #[arg(hide_short_help = true, long, global = true, value_name = "PATH", num_args = 0..=1, default_missing_value = "", help_heading = "Baselines")]
    pub save_regression_baseline: Option<Option<String>>,

    /// Run only specific analyses when no subcommand is given.
    #[arg(long, value_delimiter = ',', help_heading = "Scope")]
    pub only: Vec<AnalysisKind>,

    /// Skip specific analyses when no subcommand is given.
    #[arg(long, value_delimiter = ',', help_heading = "Scope")]
    pub skip: Vec<AnalysisKind>,

    /// Override duplication detection mode in combined mode.
    #[arg(
        hide_short_help = true,
        long = "dupes-mode",
        global = true,
        help_heading = "Duplication"
    )]
    pub dupes_mode: Option<DupesMode>,

    /// Enable function-scoped near-miss clone detection in combined mode.
    #[arg(
        hide_short_help = true,
        long = "dupes-near",
        global = true,
        help_heading = "Duplication"
    )]
    pub dupes_near: bool,

    /// Override duplication threshold in combined mode.
    #[arg(
        hide_short_help = true,
        long = "dupes-threshold",
        global = true,
        help_heading = "Duplication"
    )]
    pub dupes_threshold: Option<f64>,

    /// Override the minimum token count for clones in combined mode.
    #[arg(
        hide_short_help = true,
        long = "dupes-min-tokens",
        global = true,
        help_heading = "Duplication"
    )]
    pub dupes_min_tokens: Option<usize>,

    /// Override the minimum line count for clones in combined mode.
    #[arg(
        hide_short_help = true,
        long = "dupes-min-lines",
        global = true,
        help_heading = "Duplication"
    )]
    pub dupes_min_lines: Option<usize>,

    /// Override the minimum clone occurrences in combined mode (must be >= 2).
    #[arg(hide_short_help = true, long = "dupes-min-occurrences", global = true, value_parser = parse_min_occurrences, help_heading = "Duplication")]
    pub dupes_min_occurrences: Option<usize>,

    /// Only report cross-directory duplicates in combined mode.
    #[arg(
        hide_short_help = true,
        long = "dupes-skip-local",
        global = true,
        help_heading = "Duplication"
    )]
    pub dupes_skip_local: bool,

    /// Enable cross-language duplicate detection in combined mode.
    #[arg(
        hide_short_help = true,
        long = "dupes-cross-language",
        global = true,
        help_heading = "Duplication"
    )]
    pub dupes_cross_language: bool,

    /// Exclude module wiring from duplicate detection in combined mode
    /// (default). Pass `--dupes-no-ignore-imports` to count it again.
    #[arg(
        hide_short_help = true,
        long = "dupes-ignore-imports",
        global = true,
        help_heading = "Duplication"
    )]
    pub dupes_ignore_imports: bool,

    /// Count module wiring as clone candidates in combined mode (opt out of the
    /// default exclusion).
    #[arg(
        hide_short_help = true,
        long = "dupes-no-ignore-imports",
        global = true,
        conflicts_with = "dupes_ignore_imports",
        help_heading = "Duplication"
    )]
    pub dupes_no_ignore_imports: bool,

    /// Omit clone instances whose path is a symlink, or lies under a
    /// symlinked directory, in combined mode.
    #[arg(
        hide_short_help = true,
        long = "dupes-ignore-symlinks",
        global = true,
        help_heading = "Duplication"
    )]
    pub dupes_ignore_symlinks: bool,

    /// Report symlinked clone instances in combined mode (opt out of a config
    /// `duplicates.ignoreSymlinks: true`).
    #[arg(
        hide_short_help = true,
        long = "dupes-no-ignore-symlinks",
        global = true,
        conflicts_with = "dupes_ignore_symlinks",
        help_heading = "Duplication"
    )]
    pub dupes_no_ignore_symlinks: bool,

    /// Compute health score in combined mode.
    #[arg(hide_short_help = true, long, help_heading = "Health")]
    pub score: bool,

    /// Compare current health metrics against the most recent saved snapshot.
    #[arg(hide_short_help = true, long, help_heading = "Health")]
    pub trend: bool,

    /// Compare current health metrics against this snapshot file in combined
    /// mode. Implies --trend and --score.
    #[arg(
        hide_short_help = true,
        long,
        value_name = "PATH",
        help_heading = "Health"
    )]
    pub trend_from: Option<PathBuf>,

    /// Save a vital signs snapshot for trend tracking in combined mode.
    /// Provide a path or omit for the default `.fallow/snapshots/` location.
    /// A given path must resolve inside the project root, its Git work tree, the CI workspace or a temp directory.
    #[expect(
        clippy::option_option,
        reason = "clap pattern: None=not passed, Some(None)=default path, Some(Some(path))=custom path"
    )]
    #[arg(hide_short_help = true, long, value_name = "PATH", num_args = 0..=1, default_missing_value = "", help_heading = "Health")]
    pub save_snapshot: Option<Option<String>>,

    /// Path to Istanbul or raw V8 coverage data for measured CRAP scores in combined mode.
    /// Also settable via `FALLOW_COVERAGE` or `health.coverage`.
    #[arg(
        hide_short_help = true,
        long,
        value_name = "PATH",
        help_heading = "Health"
    )]
    pub coverage: Option<PathBuf>,

    /// Absolute prefix to strip from Istanbul file paths in combined mode.
    /// Also settable via `FALLOW_COVERAGE_ROOT` or `health.coverageRoot`.
    #[arg(
        hide_short_help = true,
        long = "coverage-root",
        value_name = "PATH",
        help_heading = "Health"
    )]
    pub coverage_root: Option<PathBuf>,

    /// Compare duplication clone groups against a saved baseline in combined
    /// mode (produced by `fallow dupes --save-baseline`).
    #[arg(
        hide_short_help = true,
        long = "dupes-baseline",
        value_name = "PATH",
        help_heading = "Baselines"
    )]
    pub dupes_baseline: Option<PathBuf>,

    /// Compare health findings against a saved baseline in combined mode
    /// (produced by `fallow health --save-baseline`).
    #[arg(
        hide_short_help = true,
        long = "health-baseline",
        value_name = "PATH",
        help_heading = "Baselines"
    )]
    pub health_baseline: Option<PathBuf>,

    /// Report unused exports in entry files instead of auto-marking them as used.
    #[arg(hide_short_help = true, long, global = true, help_heading = "Scope")]
    pub include_entry_exports: bool,

    /// Also report the unused exports, types, class members and enum members
    /// of unused files. By default the report hides them, because deleting the
    /// file removes them, and counts them in `cascade_hidden`.
    #[arg(hide_short_help = true, long, global = true, help_heading = "Output")]
    pub show_cascade: bool,

    /// Opt in to TypeScript semantic analysis for project-wide symbol evidence.
    /// This does not emit compiler diagnostics or typed lint findings.
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        help_heading = "Type-aware"
    )]
    pub type_aware: bool,

    /// Disable TypeScript semantic analysis even when `typeAware.enabled` or
    /// `FALLOW_TYPE_AWARE` opts in, keeping this run fully syntactic.
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        conflicts_with = "type_aware",
        help_heading = "Type-aware"
    )]
    pub no_type_aware: bool,

    /// TypeScript project config to use for type-aware analysis (repeatable).
    #[arg(hide_short_help = true, long, global = true, value_name = "PATH", action = clap::ArgAction::Append, help_heading = "Type-aware")]
    pub type_aware_project: Vec<PathBuf>,

    /// Decide whether incomplete type-aware analysis is advisory or gating.
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        value_enum,
        help_heading = "Type-aware"
    )]
    pub type_aware_require: Option<TypeAwareRequireArg>,

    /// Disable incremental caching
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        help_heading = "Performance"
    )]
    pub no_cache: bool,

    /// Number of parser threads
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        help_heading = "Performance"
    )]
    pub threads: Option<usize>,

    /// Show pipeline performance timing breakdown
    #[arg(
        hide_short_help = true,
        long,
        global = true,
        help_heading = "Performance"
    )]
    pub performance: bool,
}
