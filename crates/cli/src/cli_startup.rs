use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::OnceLock;

use fallow_engine::validate;
use fallow_types::ANALYSIS_DIAGNOSTIC_LOG_TARGET;
use tracing_subscriber::filter::{LevelFilter, Targets};

use crate::cli_format::{Format, FormatConfig, format_from_env, parse_format_arg, resolve_format};
use crate::cli_telemetry::{TelemetryRun, record_run_epilogue};
use crate::exit_codes::{ErrorExitCode, INVALID_INPUT};
use crate::{
    Cli, Command, emit_known_failure, emit_known_failure_with_style, rayon_pool, regression,
    report, runtime_support, security_help_target, similar_code_help_target, telemetry,
};

/// Build the tracing filter for the CLI.
///
/// Human output should stay clean by default, even when stderr is redirected to a
/// file or captured by an agent. Internal INFO-level tracing is therefore opt-in
/// via `RUST_LOG`, while warnings remain visible. An explicitly empty `RUST_LOG`
/// disables tracing entirely, which keeps the test harness deterministic.
///
/// The filter reads `target=level` and `level` directives. It skips a directive
/// that it cannot read, such as a span or field filter, and keeps the others.
///
/// A quiet run (`--quiet`, `--ci` or `FALLOW_QUIET`) also hides the warnings
/// under [`ANALYSIS_DIAGNOSTIC_LOG_TARGET`]. The JSON envelope carries those
/// facts, and the stderr lines only add noise to a quiet run. Other warnings,
/// such as an unknown rule key in the config, stay visible. An explicit
/// `RUST_LOG` always wins.
pub fn build_tracing_filter(rust_log: Option<&str>, quiet: bool) -> Targets {
    match rust_log.map(str::trim) {
        Some(value) => parse_targets_lossy(value),
        None if quiet => Targets::new()
            .with_default(LevelFilter::WARN)
            .with_target(ANALYSIS_DIAGNOSTIC_LOG_TARGET, LevelFilter::OFF),
        None => Targets::new().with_default(LevelFilter::WARN),
    }
}

/// Parse comma-separated `RUST_LOG` directives, without the ones that do not
/// parse. No directive gives a filter that disables every target.
pub fn parse_targets_lossy(value: &str) -> Targets {
    let readable: Vec<&str> = value
        .split(',')
        .map(str::trim)
        .filter(|directive| !directive.is_empty() && directive.parse::<Targets>().is_ok())
        .collect();
    // An empty string parses to a filter that enables every target.
    if readable.is_empty() {
        return Targets::new();
    }
    readable.join(",").parse().unwrap_or_default()
}

/// Decide whether tracing lines on stderr carry ANSI color codes.
///
/// The fmt layer colors by default, also when stderr is a file or a pipe. An
/// agent or a log file then gets raw escape codes around each warning. Color
/// stays on only for a terminal, and `NO_COLOR` with a value turns it off.
/// `CLICOLOR_FORCE` with a value other than `0` turns it on, as it does for
/// the report output, so a forced-color pipe gets colored warnings too.
#[must_use]
pub fn tracing_uses_ansi(
    stderr_is_terminal: bool,
    no_color: Option<&OsStr>,
    clicolor_force: Option<&OsStr>,
) -> bool {
    if clicolor_force.is_some_and(|value| value != "0") {
        return true;
    }
    stderr_is_terminal && no_color.is_none_or(OsStr::is_empty)
}

/// Set up tracing for the CLI. `quiet` is the resolved quiet mode of the run.
pub fn setup_tracing(quiet: bool) {
    use std::io::IsTerminal as _;
    use tracing_subscriber::layer::SubscriberExt as _;
    use tracing_subscriber::util::SubscriberInitExt as _;

    let rust_log = std::env::var("RUST_LOG").ok();
    let ansi = tracing_uses_ansi(
        std::io::stderr().is_terminal(),
        std::env::var_os("NO_COLOR").as_deref(),
        std::env::var_os("CLICOLOR_FORCE").as_deref(),
    );
    // The builder alone keeps INFO and above. The target filter decides.
    tracing_subscriber::fmt()
        .with_max_level(tracing_subscriber::filter::LevelFilter::TRACE)
        .with_writer(std::io::stderr)
        .with_ansi(ansi)
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .finish()
        .with(build_tracing_filter(rust_log.as_deref(), quiet))
        .init();
}

pub fn validate_inputs(
    cli: &Cli,
    output: fallow_config::OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> Result<(PathBuf, usize), ExitCode> {
    validate_input_flags(cli, output, json_style)?;
    let root = resolve_validated_root(cli, output, json_style)?;
    validate_input_git_refs(cli, output, json_style)?;
    if let Some(message) = missing_type_aware_project(cli, &root) {
        return Err(emit_known_failure_with_style(
            &message,
            INVALID_INPUT,
            output,
            json_style,
            telemetry::FailureReason::Validation,
        ));
    }

    let threads = cli
        .threads
        .unwrap_or_else(fallow_engine::resolved_run::available_threads);

    crate::process_clock::time(crate::process_clock::ProcessSpan::ThreadPool, || {
        rayon_pool::configure_global_pool(threads);
    });

    Ok((root, threads))
}

/// The error for a `--type-aware-project` path that does not exist, as given
/// or relative to the project root, when `--type-aware` is on the command
/// line. The check runs at startup, so it also fails a run in which no
/// command reads the project, such as an audit with no changed file. The
/// environment and the config can turn type-aware analysis on too; the
/// command that reads the project checks those sources when it resolves its
/// type-aware settings.
fn missing_type_aware_project(cli: &Cli, root: &Path) -> Option<String> {
    if !cli.type_aware {
        return None;
    }
    cli.type_aware_project
        .iter()
        .find(|project| !project.exists() && !root.join(project).exists())
        .map(|project| {
            format!(
                "--type-aware-project {} does not exist. Pass the path of a tsconfig file, relative to the project root.",
                project.display()
            )
        })
}

/// Reject unsupported security globals, control characters in path-shaped flags,
/// and the `--workspace`/`--changed-workspaces` mutual exclusion.
fn validate_input_flags(
    cli: &Cli,
    output: fallow_config::OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> Result<(), ExitCode> {
    let validation_failure = |message: &str| {
        emit_known_failure_with_style(
            message,
            INVALID_INPUT,
            output,
            json_style,
            telemetry::FailureReason::Validation,
        )
    };

    // Only `remove` requires an explicit root (it deletes unconditionally);
    // `prune` applies the same policy every audit applies and defaults to cwd.
    if matches!(
        &cli.command,
        Some(Command::AuditCache {
            subcommand: crate::AuditCacheCli::Remove { .. }
        })
    ) && cli.root.is_none()
    {
        return Err(validation_failure(
            "`fallow audit-cache remove` requires an explicit `--root <path>`.",
        ));
    }

    if let Some(patterns) = cli.group.as_deref() {
        for pattern in patterns {
            if let Err(e) = validate::validate_no_control_chars(pattern, "--group") {
                return Err(validation_failure(&e));
            }
        }
        if let Err(e) = fallow_engine::health::validate_group_filter(patterns) {
            return Err(validation_failure(&e));
        }
    }

    if matches!(&cli.command, Some(Command::SimilarCode { .. }))
        && let Some(flag) = crate::similar_code_help::unsupported_similar_code_option(cli)
    {
        return Err(validation_failure(&format!(
            "{flag} is not valid with this `fallow similar-code` command."
        )));
    }

    if let Some(ref config_path) = cli.config
        && let Some(s) = config_path.to_str()
        && let Err(e) = validate::validate_no_control_chars(s, "--config")
    {
        return Err(validation_failure(&e));
    }
    if let Some(ref ws_patterns) = cli.workspace {
        for ws in ws_patterns {
            if let Err(e) = validate::validate_no_control_chars(ws, "--workspace") {
                return Err(validation_failure(&e));
            }
        }
    }
    if let Some(ref git_ref) = cli.changed_since
        && let Err(e) = validate::validate_no_control_chars(git_ref, "--changed-since")
    {
        return Err(validation_failure(&e));
    }
    if let Some(ref git_ref) = cli.changed_workspaces
        && let Err(e) = validate::validate_no_control_chars(git_ref, "--changed-workspaces")
    {
        return Err(validation_failure(&e));
    }

    if cli.workspace.is_some() && cli.changed_workspaces.is_some() {
        return Err(validation_failure(
            "--workspace and --changed-workspaces are mutually exclusive. \
             Pick one: --workspace for explicit package names/globs, \
             --changed-workspaces for git-derived monorepo CI scoping.",
        ));
    }

    Ok(())
}

/// Resolve `--root` (or cwd), then validate it as an analysis root.
fn resolve_validated_root(
    cli: &Cli,
    output: fallow_config::OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> Result<PathBuf, ExitCode> {
    let raw_root = if let Some(root) = cli.root.clone() {
        root
    } else {
        std::env::current_dir().map_err(|err| {
            emit_known_failure_with_style(
                &format!("Failed to get current directory: {err}"),
                INVALID_INPUT,
                output,
                json_style,
                telemetry::FailureReason::Config,
            )
        })?
    };
    validate::validate_root(&raw_root).map_err(|e| {
        telemetry::note_failure_reason(telemetry::FailureReason::Config);
        crate::error::emit_programmatic_error(
            &fallow_api::ProgrammaticError::new(e, 2)
                .with_code("FALLOW_INVALID_ROOT")
                .with_help("Pass --root with an existing, readable project directory."),
            output,
            json_style,
        )
    })
}

/// Validate `--changed-since` / `--changed-workspaces` as well-formed git refs.
fn validate_input_git_refs(
    cli: &Cli,
    output: fallow_config::OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> Result<(), ExitCode> {
    let validation_failure = |message: &str| {
        emit_known_failure_with_style(
            message,
            INVALID_INPUT,
            output,
            json_style,
            telemetry::FailureReason::Validation,
        )
    };

    if let Some(ref git_ref) = cli.changed_since
        && let Err(e) = validate::validate_git_ref(git_ref)
    {
        return Err(validation_failure(&format!("invalid --changed-since: {e}")));
    }

    if let Some(ref git_ref) = cli.changed_workspaces
        && let Err(e) = validate::validate_git_ref(git_ref)
    {
        return Err(validation_failure(&format!(
            "invalid --changed-workspaces: {e}"
        )));
    }

    Ok(())
}

/// Find the first positional (non-flag) token in argv, which is the invoked
/// subcommand name. Skips global flags and their values so `fallow --root /p
/// review` still resolves to `review`. Returns `None` when there is no
/// positional token (bare `fallow`, or only flags).
fn first_subcommand_token<I>(args: I) -> Option<String>
where
    I: IntoIterator<Item = String>,
{
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
            skip_next = global_flag_consumes_next(&arg);
            continue;
        }
        return Some(arg);
    }
    None
}

fn global_flag_consumes_next(arg: &str) -> bool {
    !arg.contains('=') && global_value_options().iter().any(|name| name == arg)
}

/// Every spelling (long, short, and aliases) of a top-level `Cli` option that
/// takes a value. The list comes from the clap definition, so a boolean flag
/// never makes the scan skip the subcommand name that follows it.
fn global_value_options() -> &'static [String] {
    static OPTIONS: OnceLock<Vec<String>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        <Cli as clap::CommandFactory>::command()
            .get_arguments()
            .filter(|arg| !arg.is_positional() && arg.get_action().takes_values())
            .flat_map(option_spellings)
            .collect()
    })
}

fn option_spellings(arg: &clap::Arg) -> Vec<String> {
    let longs = arg
        .get_long()
        .into_iter()
        .chain(arg.get_all_aliases().unwrap_or_default())
        .map(|long| format!("--{long}"));
    let shorts = arg
        .get_short()
        .into_iter()
        .chain(arg.get_all_short_aliases().unwrap_or_default())
        .map(|short| format!("-{short}"));
    longs.chain(shorts).collect()
}

fn args_use_legacy_check_alias<I>(args: I) -> bool
where
    I: IntoIterator<Item = String>,
{
    first_subcommand_token(args).as_deref() == Some("check")
}

/// Whether argv invoked the `review` alias of the audit command. The clap
/// `visible_alias` routes `review` to `Command::Audit` but does NOT set
/// `--brief`; the alias implies the brief, so we detect it from raw argv and
/// force `brief = true` post-parse.
fn args_invoked_review_alias<I>(args: I) -> bool
where
    I: IntoIterator<Item = String>,
{
    first_subcommand_token(args).as_deref() == Some("review")
}

fn raw_args_use_legacy_check_alias() -> bool {
    args_use_legacy_check_alias(std::env::args_os().map(|arg| arg.to_string_lossy().into_owned()))
}

/// Set `--brief` on the audit of `cli` when `args` invoke the `review` alias.
pub fn apply_review_alias<I>(cli: &mut Cli, args: I)
where
    I: IntoIterator<Item = String>,
{
    if args_invoked_review_alias(args)
        && let Some(Command::Audit { brief, .. }) = cli.command.as_mut()
    {
        *brief = true;
    }
}

fn warn_legacy_check_alias_if_needed(used_legacy_check_alias: bool, quiet: bool) {
    if used_legacy_check_alias && !quiet {
        eprintln!("fallow: `check` is deprecated; use `dead-code` instead.");
    }
}

/// A parsed command line.
struct ParsedCommandLine {
    cli: Cli,
    /// The deprecated options that the command line sets.
    deprecated: Vec<&'static crate::schema::deprecations::Deprecation>,
    /// The clap command tree and the matches with their value sources, for
    /// the global option check after the output format is resolved.
    root: clap::Command,
    matches: clap::ArgMatches,
}

/// Parse argv like `Cli::try_parse`, and also read the deprecated options of
/// the command line.
fn parse_command_line() -> Result<ParsedCommandLine, clap::Error> {
    use clap::{CommandFactory, FromArgMatches};
    let root = Cli::command();
    let mut matches = root.clone().try_get_matches()?;
    let deprecated = crate::schema::deprecations::options_on_command_line(&root, &matches);
    let checked = matches.clone();
    let cli =
        Cli::from_arg_matches_mut(&mut matches).map_err(|err| err.format(&mut root.clone()))?;
    Ok(ParsedCommandLine {
        cli,
        deprecated,
        root,
        matches: checked,
    })
}

fn warn_deprecated_options(entries: &[&crate::schema::deprecations::Deprecation], quiet: bool) {
    if quiet {
        return;
    }
    for entry in entries {
        eprintln!("{}", crate::schema::deprecations::option_warning(entry));
    }
}

/// Parse argv into a `Cli`, apply the legacy-alias warning and process-wide
/// overrides (max-file-size, workspace PR marker), and resolve the output
/// format. Returns the parse error's exit code on failure. The third value is
/// the error message for a global option that the command rejects.
pub fn parse_cli_args() -> Result<(Cli, FormatConfig, Option<String>), ExitCode> {
    let used_legacy_check_alias = raw_args_use_legacy_check_alias();
    let ParsedCommandLine {
        mut cli,
        deprecated: deprecated_options,
        root,
        matches,
    } = parse_command_line().map_err(|err| handle_cli_parse_error(&err))?;
    warn_legacy_check_alias_if_needed(used_legacy_check_alias, cli.quiet);
    warn_deprecated_options(&deprecated_options, cli.quiet);
    apply_review_alias(
        &mut cli,
        std::env::args_os().map(|arg| arg.to_string_lossy().into_owned()),
    );
    runtime_support::set_max_file_size_override(cli.max_file_size);

    if let Some(workspaces) = cli.workspace.as_ref()
        && !workspaces.is_empty()
    {
        report::ci::pr_comment::set_workspace_marker_from_list(workspaces);
    }

    let fmt = resolve_format(&cli);
    let rejected_option =
        crate::global_option_support::rejected_option(&root, &matches, fmt.output);
    Ok((cli, fmt, rejected_option))
}

/// Emit the error of a global option that the command rejects. The commands
/// that run before the telemetry run starts (the schema commands, `telemetry`,
/// `doctor` and the Impact statusline) record no run, as for their other
/// errors.
pub fn reject_global_option(cli: &Cli, fmt: &FormatConfig, message: &str) -> ExitCode {
    let records_no_run = crate::is_impact_statusline(cli)
        || matches!(
            cli.command,
            Some(
                Command::Schema(_)
                    | Command::ConfigSchema
                    | Command::PluginSchema
                    | Command::RulePackSchema
                    | Command::Telemetry { .. }
                    | Command::Doctor
            )
        );
    if records_no_run {
        return crate::error::emit_input_error_with_style(message, fmt.output, fmt.json_style);
    }
    let telemetry_run = crate::cli_telemetry::start_telemetry_run(cli, fmt);
    let code = emit_known_failure_with_style(
        message,
        INVALID_INPUT,
        fmt.output,
        fmt.json_style,
        telemetry::FailureReason::Validation,
    );
    record_run_epilogue(
        telemetry_run,
        code,
        Some(telemetry::FailureReason::Validation),
        cli.parent_run.as_deref(),
        None,
    )
}

/// Run the pre-dispatch validation gates (diff filter, output-gate flags, global
/// filter, regression tolerance). On any failure, returns the epilogue-recorded
/// exit code so `main` can return it directly.
pub fn run_pre_dispatch_checks(
    cli: &Cli,
    root: &Path,
    output: fallow_config::OutputFormat,
    json_style: crate::json_style::JsonStyle,
    quiet: bool,
    telemetry_run: TelemetryRun,
) -> Result<regression::Tolerance, ExitCode> {
    let fail = |code: ExitCode, reason: telemetry::FailureReason| {
        record_run_epilogue(
            telemetry_run,
            code,
            Some(reason),
            cli.parent_run.as_deref(),
            None,
        )
    };

    if let Err(code) = init_cli_diff_filter(cli, root, output, quiet) {
        return Err(fail(code, telemetry::FailureReason::Diff));
    }

    if let Some(message) = flag_scope_error(cli, root) {
        let code = emit_known_failure_with_style(
            &message,
            INVALID_INPUT,
            output,
            json_style,
            telemetry::FailureReason::Validation,
        );
        return Err(fail(code, telemetry::FailureReason::Validation));
    }

    if let Some(message) = global_filter_error(cli) {
        let code = emit_known_failure_with_style(
            message,
            INVALID_INPUT,
            output,
            json_style,
            telemetry::FailureReason::Validation,
        );
        return Err(fail(code, telemetry::FailureReason::Validation));
    }

    if cli.report_path_prefix.is_some()
        && !matches!(
            output,
            fallow_config::OutputFormat::GithubAnnotations
                | fallow_config::OutputFormat::GithubSummary
                | fallow_config::OutputFormat::CodeClimate
                | fallow_config::OutputFormat::PrCommentGithub
                | fallow_config::OutputFormat::PrCommentGitlab
                | fallow_config::OutputFormat::ReviewGithub
                | fallow_config::OutputFormat::ReviewGitlab
        )
    {
        let code = emit_known_failure_with_style(
            "--report-path-prefix is only valid with --format github-annotations, \
             github-summary, codeclimate, pr-comment-github, pr-comment-gitlab, \
             review-github, or review-gitlab",
            INVALID_INPUT,
            output,
            json_style,
            telemetry::FailureReason::Validation,
        );
        return Err(fail(code, telemetry::FailureReason::Validation));
    }
    report::github::set_report_path_prefix(cli.report_path_prefix.clone());
    // `init_report_prefix` shells out to `git rev-parse --show-toplevel`. Only
    // the formats that read the resolved global (`report_prefix()`) need it:
    // codeclimate applies it at the wire boundary, and review-{github,gitlab}
    // apply it in the renderer, which has no `root` to re-derive it from. The
    // github-native formats compute their rebase from `root` directly, so skip
    // the probe for every other format.
    if matches!(
        output,
        fallow_config::OutputFormat::CodeClimate
            | fallow_config::OutputFormat::PrCommentGithub
            | fallow_config::OutputFormat::PrCommentGitlab
            | fallow_config::OutputFormat::ReviewGithub
            | fallow_config::OutputFormat::ReviewGitlab
    ) {
        report::github::init_report_prefix(root);
    }

    if defers_tolerance_parse(cli.command.as_ref()) {
        return Ok(regression::Tolerance::Absolute(0));
    }
    parse_cli_tolerance(cli, output)
        .map_err(|code| fail(code, telemetry::FailureReason::Validation))
}

/// `fallow list` reads `--tolerance` only for the `--entry-weight` gate, so
/// the list dispatcher parses the value there. A plain listing never fails on
/// a value that it does not use.
fn defers_tolerance_parse(command: Option<&Command>) -> bool {
    matches!(command, Some(Command::List { .. } | Command::Workspaces))
}

fn handle_cli_parse_error(err: &clap::Error) -> ExitCode {
    let exit_code = u8::try_from(err.exit_code()).unwrap_or(INVALID_INPUT.get());
    if err.kind() == clap::error::ErrorKind::DisplayHelp
        && let Some(target) = similar_code_help_target(std::env::args_os().skip(1))
    {
        print!("{}", crate::render_similar_code_help(target));
        return ExitCode::SUCCESS;
    }
    if err.kind() == clap::error::ErrorKind::DisplayHelp
        && let Some(target) = security_help_target(std::env::args_os().skip(1))
    {
        print!("{}", crate::render_security_help(target));
        return ExitCode::SUCCESS;
    }

    if matches!(
        parse_error_output_format(std::env::args_os().skip(1)),
        fallow_config::OutputFormat::Json
    ) {
        if exit_code != 0 {
            return crate::error::emit_programmatic_error(
                &fallow_api::ProgrammaticError::new(err.to_string().trim(), exit_code)
                    .with_code("FALLOW_INVALID_CLI_ARGUMENTS")
                    .with_help(
                        "Run `fallow --help` or `fallow <command> --help` to check accepted arguments and required values.",
                    ),
                fallow_config::OutputFormat::Json,
                parse_error_json_style(std::env::args_os().skip(1)),
            );
        }
        return crate::error::emit_error_with_style(
            err.to_string().trim(),
            ErrorExitCode::passthrough(exit_code),
            fallow_config::OutputFormat::Json,
            parse_error_json_style(std::env::args_os().skip(1)),
        );
    }

    let _ = err.print();
    ExitCode::from(exit_code)
}

fn parse_error_json_style<I, S>(args: I) -> crate::json_style::JsonStyle
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    if args
        .into_iter()
        .any(|arg| arg.as_ref() == OsStr::new("--pretty"))
    {
        crate::json_style::JsonStyle::Pretty
    } else {
        crate::json_style::JsonStyle::Compact
    }
}

fn parse_error_output_format<I, S>(args: I) -> fallow_config::OutputFormat
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string_lossy().into_owned())
        .collect();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if matches!(arg.as_str(), "--format" | "--output" | "-f") {
            return iter
                .next()
                .and_then(|value| parse_format_arg(value))
                .unwrap_or(Format::Human)
                .into();
        }
        let short_format_value = if arg.starts_with("--") {
            None
        } else {
            arg.strip_prefix("-f")
        };
        if let Some(value) = arg
            .strip_prefix("--format=")
            .or_else(|| arg.strip_prefix("--output="))
            .or(short_format_value)
            .filter(|value| !value.is_empty())
        {
            return parse_format_arg(value).unwrap_or(Format::Human).into();
        }
    }
    format_from_env().unwrap_or(Format::Human).into()
}

pub fn cli_has_bare_coverage_input(cli: &Cli) -> bool {
    cli.coverage.is_some() || cli.coverage_root.is_some()
}

pub fn bare_coverage_subcommand_error_message() -> &'static str {
    "`--coverage` and `--coverage-root` are bare combined-mode flags. Use `fallow health --coverage <coverage-final.json>` for standalone health analysis, or omit the subcommand to run combined mode."
}

/// Return the first combined-mode baseline flag on the command line.
///
/// These flags configure bare `fallow` only. Before a subcommand they would
/// have no effect, so the caller rejects them.
pub fn cli_bare_combined_baseline_flag(cli: &Cli) -> Option<&'static str> {
    if cli.dupes_baseline.is_some() {
        return Some("--dupes-baseline");
    }
    if cli.health_baseline.is_some() {
        return Some("--health-baseline");
    }
    None
}

pub fn bare_combined_baseline_subcommand_error_message(flag: &str) -> String {
    format!(
        "`{flag}` is a bare combined-mode flag and has no effect before a subcommand. Use `fallow audit {flag} <file>`, `fallow dupes --baseline <file>` or `fallow health --baseline <file>`, or omit the subcommand to run combined mode."
    )
}

/// The first error of a flag that a bare run cannot use: `--sarif-file` or a
/// regression flag without the dead-code analysis, or a save path outside the
/// allowed directories. [`crate::global_option_support`] decides each flag
/// for each subcommand.
fn flag_scope_error(cli: &Cli, root: &Path) -> Option<String> {
    sarif_file_without_sarif_error(cli)
        .map(str::to_owned)
        .or_else(|| regression_flag_without_baseline_error(cli))
        .or_else(|| analysis_flag_without_analysis_error(cli))
        .or_else(|| crate::write_scope::write_path_error(cli, root))
}

/// Reject a global `--dupes-*` option on a bare run without the duplication
/// analysis, `--churn-file` on a bare run without the health analysis, and
/// `--type-aware-project` on a bare run without the dead-code and the health
/// analysis. Only these analyses read the options. `--only` and `--skip`
/// decide here; `duplicates.enabled: false` in the config gives an
/// inert-flag notice when the run starts (see `combined::bare_analyses`).
fn analysis_flag_without_analysis_error(cli: &Cli) -> Option<String> {
    if cli.command.is_some() {
        return None;
    }
    let (run_check, run_dupes, run_health) =
        crate::combined::resolve_analyses(&cli.only, &cli.skip);
    if !run_check && !run_health && !cli.type_aware_project.is_empty() {
        return Some(
            "This run does not include the dead-code or the health analysis, which read `--type-aware-project`, so the option has no effect. Include dead-code or health in the run, or remove `--type-aware-project`.".to_owned(),
        );
    }
    if !run_dupes && let Some(flag) = cli.dupes_overrides().first_global_flag() {
        return Some(format!(
            "This run does not include the duplication analysis, which reads `{flag}`, so the option has no effect. Include dupes in the run, or remove `{flag}`."
        ));
    }
    (!run_health && cli.churn_file.is_some()).then(|| {
        "This run does not include the health analysis, which reads the churn file, so `--churn-file` has no effect. Include health in the run, or use `fallow health --churn-file`.".to_owned()
    })
}

/// Reject `--sarif-file` on a bare run without the dead-code analysis.
///
/// Only the dead-code analysis writes the SARIF file of a bare run.
fn sarif_file_without_sarif_error(cli: &Cli) -> Option<&'static str> {
    cli.sarif_file.as_ref()?;
    if cli.command.is_some() {
        return None;
    }
    let (run_check, _, _) = crate::combined::resolve_analyses(&cli.only, &cli.skip);
    (!run_check).then_some(
        "This run does not include the dead-code analysis, which writes the SARIF file, so `--sarif-file` has no effect. Use `--format sarif` with `--output-file`, or include dead-code in the run.",
    )
}

/// Reject the regression flags on a bare run without the dead-code analysis,
/// which owns the regression baseline.
fn regression_flag_without_baseline_error(cli: &Cli) -> Option<String> {
    let flag = crate::dispatch::first_regression_flag(cli)?;
    if cli.command.is_some() {
        return None;
    }
    let (run_check, _, _) = crate::combined::resolve_analyses(&cli.only, &cli.skip);
    (!run_check).then(|| {
        format!(
            "This run does not include the dead-code analysis, which owns the regression baseline, so `{flag}` has no effect. Include dead-code in the run, or use `fallow dead-code`."
        )
    })
}

fn global_filter_error(cli: &Cli) -> Option<&'static str> {
    if (!cli.only.is_empty() || !cli.skip.is_empty()) && cli.command.is_some() {
        return Some(
            "--only and --skip can only be used without a subcommand. For audit, pass them after `audit`",
        );
    }
    if (cli.production_dead_code || cli.production_health || cli.production_dupes)
        && cli.command.is_some()
    {
        return Some(
            "--production-dead-code, --production-health, and --production-dupes can only be used without a subcommand. For audit, pass them after `audit`",
        );
    }
    if !cli.only.is_empty() && !cli.skip.is_empty() {
        return Some("--only and --skip are mutually exclusive");
    }
    None
}

fn parse_cli_tolerance(
    cli: &Cli,
    output: fallow_config::OutputFormat,
) -> Result<regression::Tolerance, ExitCode> {
    regression::Tolerance::parse(&cli.tolerance).map_err(|e| {
        emit_known_failure(
            &format!("invalid --tolerance: {e}"),
            INVALID_INPUT,
            output,
            telemetry::FailureReason::Validation,
        )
    })
}

fn init_cli_diff_filter(
    cli: &Cli,
    root: &Path,
    output: fallow_config::OutputFormat,
    quiet: bool,
) -> Result<(), ExitCode> {
    let diff_source = report::ci::diff_filter::resolve_diff_source(
        cli.diff_file.as_deref(),
        cli.diff_stdin,
        root,
    )
    .map_err(|msg| {
        emit_known_failure(&msg, INVALID_INPUT, output, telemetry::FailureReason::Diff)
    })?;
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
    // The candidates cost a `git rev-parse`, and only placing a diff reads
    // them, so a run without a diff source skips the call.
    let candidate_bases = diff_source
        .as_ref()
        .map(|_| fallow_engine::diff_source::diff_base_candidates(root))
        .unwrap_or_default();
    let _ = report::ci::diff_filter::init_shared_diff(
        diff_source.as_ref(),
        root,
        &candidate_bases,
        suppress_warnings,
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracing_filter_defaults_to_warn_without_env() {
        let filter = build_tracing_filter(None, false);
        assert!(filter.would_enable("fallow_core", &tracing::Level::WARN));
        assert!(!filter.would_enable("fallow_core", &tracing::Level::INFO));
        assert!(filter.would_enable(ANALYSIS_DIAGNOSTIC_LOG_TARGET, &tracing::Level::WARN));
    }

    #[test]
    fn a_quiet_run_hides_only_analysis_diagnostic_warnings() {
        let filter = build_tracing_filter(None, true);
        assert!(!filter.would_enable(ANALYSIS_DIAGNOSTIC_LOG_TARGET, &tracing::Level::WARN));
        assert!(!filter.would_enable(ANALYSIS_DIAGNOSTIC_LOG_TARGET, &tracing::Level::ERROR));
        assert!(filter.would_enable("fallow_config", &tracing::Level::WARN));
        assert!(!filter.would_enable("fallow_core", &tracing::Level::INFO));
    }

    #[test]
    fn an_explicit_env_filter_wins_over_quiet() {
        let filter = build_tracing_filter(Some("warn"), true);
        assert!(filter.would_enable(ANALYSIS_DIAGNOSTIC_LOG_TARGET, &tracing::Level::WARN));
    }

    #[test]
    fn tracing_filter_respects_explicit_env_directives() {
        let filter = build_tracing_filter(Some("info"), false);
        assert!(filter.would_enable("fallow_core", &tracing::Level::INFO));
        assert!(!filter.would_enable("fallow_core", &tracing::Level::DEBUG));

        let filter = build_tracing_filter(Some("fallow=debug"), false);
        assert!(filter.would_enable("fallow_core::lib", &tracing::Level::DEBUG));
        assert!(!filter.would_enable("oxc_resolver", &tracing::Level::ERROR));
    }

    #[test]
    fn tracing_filter_skips_a_directive_that_it_cannot_read() {
        let directives = "fallow[span{id=1}]=trace,fallow_graph=debug,warn";
        let filter = build_tracing_filter(Some(directives), false);
        assert!(filter.would_enable("fallow_graph", &tracing::Level::DEBUG));
        assert!(filter.would_enable("oxc_resolver", &tracing::Level::WARN));
        assert!(!filter.would_enable("fallow_core", &tracing::Level::TRACE));
    }

    #[test]
    fn tracing_filter_treats_empty_env_as_off() {
        for value in ["", "   "] {
            let filter = build_tracing_filter(Some(value), false);
            assert!(!filter.would_enable("fallow_core", &tracing::Level::ERROR));
        }
    }

    /// Tracing color follows the terminal, so a pipe or a log file gets
    /// plain warnings.
    #[test]
    fn tracing_color_needs_a_terminal_and_no_no_color() {
        assert!(tracing_uses_ansi(true, None, None));
        assert!(tracing_uses_ansi(true, Some(OsStr::new("")), None));
        assert!(!tracing_uses_ansi(false, None, None));
        assert!(!tracing_uses_ansi(true, Some(OsStr::new("1")), None));
        // CLICOLOR_FORCE wins, as it does for the report output.
        assert!(tracing_uses_ansi(false, None, Some(OsStr::new("1"))));
        assert!(tracing_uses_ansi(
            true,
            Some(OsStr::new("1")),
            Some(OsStr::new("1"))
        ));
        assert!(!tracing_uses_ansi(false, None, Some(OsStr::new("0"))));
    }

    fn analysis_flag_error(args: &[&str]) -> Option<String> {
        use clap::Parser;
        let argv = std::iter::once("fallow").chain(args.iter().copied());
        analysis_flag_without_analysis_error(&Cli::try_parse_from(argv).expect("parses"))
    }

    #[test]
    fn a_bare_run_without_the_reading_analysis_rejects_its_option() {
        for args in [
            &["--only", "dead-code", "--dupes-min-lines", "40"][..],
            &["--skip", "dupes", "--dupes-min-lines", "40"],
        ] {
            let message = analysis_flag_error(args).unwrap_or_else(|| panic!("{args:?}"));
            assert!(
                message.starts_with("This run does not include the duplication analysis, which reads `--dupes-min-lines`"),
                "{message}"
            );
        }
        for args in [
            &["--only", "dupes", "--churn-file", "c.json"][..],
            &["--skip", "health", "--churn-file", "c.json"],
        ] {
            let message = analysis_flag_error(args).unwrap_or_else(|| panic!("{args:?}"));
            assert!(
                message.starts_with("This run does not include the health analysis"),
                "{message}"
            );
        }
        for args in [
            &["--only", "dupes", "--type-aware-project", "tsconfig.json"][..],
            &[
                "--skip",
                "dead-code,health",
                "--type-aware-project",
                "tsconfig.json",
            ],
        ] {
            let message = analysis_flag_error(args).unwrap_or_else(|| panic!("{args:?}"));
            assert!(
                message
                    .starts_with("This run does not include the dead-code or the health analysis"),
                "{message}"
            );
        }
        for args in [
            &["--dupes-min-lines", "40"][..],
            &["--only", "dupes", "--dupes-min-lines", "40"],
            &["--only", "health", "--type-aware-project", "tsconfig.json"],
            &["--skip", "dead-code", "--churn-file", "c.json"],
            &["health", "--churn-file", "c.json"],
        ] {
            assert_eq!(analysis_flag_error(args), None, "{args:?}");
        }
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

    fn argv(args: &[&str]) -> Vec<String> {
        std::iter::once("fallow")
            .chain(args.iter().copied())
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn boolean_global_flags_do_not_hide_the_subcommand() {
        for flag in [
            "--dupes-skip-local",
            "--dupes-cross-language",
            "--dupes-ignore-imports",
        ] {
            assert!(
                args_use_legacy_check_alias(argv(&[flag, "check"])),
                "{flag} check"
            );
            assert!(
                args_invoked_review_alias(argv(&[flag, "review"])),
                "{flag} review"
            );
        }
    }

    #[test]
    fn subcommand_scan_matches_every_top_level_clap_option() {
        let command = <Cli as clap::CommandFactory>::command();
        for arg in command.get_arguments().filter(|arg| !arg.is_positional()) {
            for flag in option_spellings(arg) {
                if arg.get_action().takes_values() {
                    assert!(
                        !args_use_legacy_check_alias(argv(&[&flag, "check", "dead-code"])),
                        "{flag} takes a value, so `check` after it is that value"
                    );
                    assert!(
                        args_use_legacy_check_alias(argv(&[&flag, "value", "check"])),
                        "{flag} value check"
                    );
                } else {
                    assert!(
                        args_use_legacy_check_alias(argv(&[&flag, "check"])),
                        "{flag} takes no value, so `check` after it is the subcommand"
                    );
                }
            }
        }
    }
}
