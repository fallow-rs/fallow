use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use fallow_engine::validate;

use crate::cli_format::{Format, FormatConfig, format_from_env, parse_format_arg, resolve_format};
use crate::cli_telemetry::{TelemetryRun, record_run_epilogue};
use crate::{
    Cli, Command, emit_known_failure, output_runtime, rayon_pool, regression, report,
    runtime_support, security_help_target, telemetry,
};

/// Build the tracing filter for the CLI.
///
/// Human output should stay clean by default, even when stderr is redirected to a
/// file or captured by an agent. Internal INFO-level tracing is therefore opt-in
/// via `RUST_LOG`, while warnings remain visible. An explicitly empty `RUST_LOG`
/// disables tracing entirely, which keeps the test harness deterministic.
pub fn build_tracing_filter(rust_log: Option<&str>) -> tracing_subscriber::EnvFilter {
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
pub fn setup_tracing() {
    let rust_log = std::env::var("RUST_LOG").ok();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(build_tracing_filter(rust_log.as_deref()))
        .with_target(false)
        .with_timer(tracing_subscriber::fmt::time::uptime())
        .init();
}

pub fn validate_inputs(
    cli: &Cli,
    output: fallow_config::OutputFormat,
) -> Result<(PathBuf, usize), ExitCode> {
    validate_input_flags(cli, output)?;
    let root = resolve_validated_root(cli, output)?;
    validate_input_git_refs(cli, output)?;

    let threads = cli
        .threads
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, std::num::NonZero::get));

    rayon_pool::configure_global_pool(threads);

    Ok((root, threads))
}

/// Reject unsupported security globals, control characters in path-shaped flags,
/// and the `--workspace`/`--changed-workspaces` mutual exclusion.
fn validate_input_flags(cli: &Cli, output: fallow_config::OutputFormat) -> Result<(), ExitCode> {
    let validation_failure = |message: &str| {
        emit_known_failure(message, 2, output, telemetry::FailureReason::Validation)
    };

    if matches!(&cli.command, Some(Command::Security { .. }))
        && let Some(flag) = crate::unsupported_security_global(cli)
    {
        return Err(validation_failure(&format!(
            "{flag} is not valid with `fallow security`."
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
) -> Result<PathBuf, ExitCode> {
    let raw_root = if let Some(root) = cli.root.clone() {
        root
    } else {
        std::env::current_dir().map_err(|err| {
            emit_known_failure(
                &format!("Failed to get current directory: {err}"),
                2,
                output,
                telemetry::FailureReason::Config,
            )
        })?
    };
    validate::validate_root(&raw_root)
        .map_err(|e| emit_known_failure(&e, 2, output, telemetry::FailureReason::Config))
}

/// Validate `--changed-since` / `--changed-workspaces` as well-formed git refs.
fn validate_input_git_refs(cli: &Cli, output: fallow_config::OutputFormat) -> Result<(), ExitCode> {
    let validation_failure = |message: &str| {
        emit_known_failure(message, 2, output, telemetry::FailureReason::Validation)
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
        return Some(arg);
    }
    None
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
    args_use_legacy_check_alias(std::env::args())
}

fn raw_args_invoked_review_alias() -> bool {
    args_invoked_review_alias(std::env::args())
}

fn warn_legacy_check_alias_if_needed(used_legacy_check_alias: bool, quiet: bool) {
    if used_legacy_check_alias && !quiet {
        eprintln!("fallow: `check` is deprecated; use `dead-code` instead.");
    }
}

/// Parse argv into a `Cli`, apply the legacy-alias warning and process-wide
/// overrides (legacy envelope, max-file-size, workspace PR marker), and resolve
/// the output format. Returns the parse error's exit code on failure.
pub fn parse_cli_args() -> Result<(Cli, FormatConfig), ExitCode> {
    let used_legacy_check_alias = raw_args_use_legacy_check_alias();
    let mut cli = Cli::try_parse().map_err(|err| handle_cli_parse_error(&err))?;
    warn_legacy_check_alias_if_needed(used_legacy_check_alias, cli.quiet);
    if raw_args_invoked_review_alias()
        && let Some(Command::Audit { brief, .. }) = cli.command.as_mut()
    {
        *brief = true;
    }
    output_runtime::set_legacy_envelope(cli.legacy_envelope);
    runtime_support::set_max_file_size_override(cli.max_file_size);

    if let Some(workspaces) = cli.workspace.as_ref()
        && !workspaces.is_empty()
    {
        report::ci::pr_comment::set_workspace_marker_from_list(workspaces);
    }

    let fmt = resolve_format(&cli);
    Ok((cli, fmt))
}

/// Run the pre-dispatch validation gates (diff filter, output-gate flags, global
/// filter, regression tolerance). On any failure, returns the epilogue-recorded
/// exit code so `main` can return it directly.
pub fn run_pre_dispatch_checks(
    cli: &Cli,
    root: &Path,
    output: fallow_config::OutputFormat,
    quiet: bool,
    telemetry_run: TelemetryRun,
) -> Result<regression::Tolerance, ExitCode> {
    let fail = |code: ExitCode, reason: telemetry::FailureReason| {
        record_run_epilogue(telemetry_run, code, Some(reason), cli.parent_run.as_deref())
    };

    if let Err(code) = init_cli_diff_filter(cli, root, output, quiet) {
        return Err(fail(code, telemetry::FailureReason::Diff));
    }

    if (cli.ci || cli.fail_on_issues || cli.sarif_file.is_some() || cli.output_file.is_some())
        && command_rejects_output_gate(cli.command.as_ref())
    {
        let code = emit_known_failure(
            "--ci, --fail-on-issues, --sarif-file, and --output-file are only valid with dead-code, dupes, health, security, or bare invocation",
            2,
            output,
            telemetry::FailureReason::Validation,
        );
        return Err(fail(code, telemetry::FailureReason::Validation));
    }

    if let Some(message) = global_filter_error(cli) {
        let code = emit_known_failure(message, 2, output, telemetry::FailureReason::Validation);
        return Err(fail(code, telemetry::FailureReason::Validation));
    }

    parse_cli_tolerance(cli, output)
        .map_err(|code| fail(code, telemetry::FailureReason::Validation))
}

fn handle_cli_parse_error(err: &clap::Error) -> ExitCode {
    let exit_code = u8::try_from(err.exit_code()).unwrap_or(2);
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
        return crate::error::emit_error(
            err.to_string().trim(),
            exit_code,
            fallow_config::OutputFormat::Json,
        );
    }

    let _ = err.print();
    ExitCode::from(exit_code)
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

fn command_rejects_output_gate(command: Option<&Command>) -> bool {
    matches!(
        command,
        Some(
            Command::Init { .. }
                | Command::ConfigSchema
                | Command::PluginSchema
                | Command::RulePackSchema
                | Command::Schema
                | Command::Explain { .. }
                | Command::CiTemplate { .. }
                | Command::Config { .. }
                | Command::Ci { .. }
                | Command::List { .. }
                | Command::Inspect { .. }
                | Command::Trace { .. }
                | Command::Flags { .. }
                | Command::Migrate { .. }
                | Command::License { .. }
                | Command::Coverage { .. }
                | Command::Hooks { .. }
                | Command::SetupHooks { .. }
        )
    )
}

fn global_filter_error(cli: &Cli) -> Option<&'static str> {
    if (!cli.only.is_empty() || !cli.skip.is_empty()) && cli.command.is_some() {
        return Some("--only and --skip can only be used without a subcommand");
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
            2,
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
    .map_err(|msg| emit_known_failure(&msg, 2, output, telemetry::FailureReason::Diff))?;
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
