use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use fallow_engine::validate;

use crate::cli_format::{Format, FormatConfig, format_from_env, parse_format_arg, resolve_format};
use crate::cli_telemetry::{TelemetryRun, record_run_epilogue};
use crate::{
    Cli, Command, emit_known_failure, emit_known_failure_with_style, rayon_pool, regression,
    report, runtime_support, security_help_target, telemetry,
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
    json_style: crate::json_style::JsonStyle,
) -> Result<(PathBuf, usize), ExitCode> {
    validate_input_flags(cli, output, json_style)?;
    let root = resolve_validated_root(cli, output, json_style)?;
    validate_input_git_refs(cli, output, json_style)?;

    let threads = cli
        .threads
        .unwrap_or_else(|| std::thread::available_parallelism().map_or(4, std::num::NonZero::get));

    rayon_pool::configure_global_pool(threads);

    Ok((root, threads))
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
            2,
            output,
            json_style,
            telemetry::FailureReason::Validation,
        )
    };

    if matches!(&cli.command, Some(Command::AuditCache { .. })) && cli.root.is_none() {
        return Err(validation_failure(
            "`fallow audit-cache remove` requires an explicit `--root <path>`.",
        ));
    }

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
    json_style: crate::json_style::JsonStyle,
) -> Result<PathBuf, ExitCode> {
    let raw_root = if let Some(root) = cli.root.clone() {
        root
    } else {
        std::env::current_dir().map_err(|err| {
            emit_known_failure_with_style(
                &format!("Failed to get current directory: {err}"),
                2,
                output,
                json_style,
                telemetry::FailureReason::Config,
            )
        })?
    };
    validate::validate_root(&raw_root).map_err(|e| {
        emit_known_failure_with_style(&e, 2, output, json_style, telemetry::FailureReason::Config)
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
            2,
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
    let option_name = arg.split_once('=').map_or(arg, |(name, _)| name);
    !arg.contains('=') && global_value_options().contains(&option_name)
}

fn global_value_options() -> &'static [&'static str] {
    &[
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
        "--report-path-prefix",
        "--annotations-path-prefix",
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
    ]
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
/// overrides (max-file-size, workspace PR marker), and resolve the output
/// format. Returns the parse error's exit code on failure.
pub fn parse_cli_args() -> Result<(Cli, FormatConfig), ExitCode> {
    let used_legacy_check_alias = raw_args_use_legacy_check_alias();
    let mut cli = Cli::try_parse().map_err(|err| handle_cli_parse_error(&err))?;
    warn_legacy_check_alias_if_needed(used_legacy_check_alias, cli.quiet);
    if raw_args_invoked_review_alias()
        && let Some(Command::Audit { brief, .. }) = cli.command.as_mut()
    {
        *brief = true;
    }
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
    json_style: crate::json_style::JsonStyle,
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
        let code = emit_known_failure_with_style(
            "--ci, --fail-on-issues, --sarif-file, and --output-file are only valid with dead-code, dupes, health, security, or bare invocation",
            2,
            output,
            json_style,
            telemetry::FailureReason::Validation,
        );
        return Err(fail(code, telemetry::FailureReason::Validation));
    }

    if let Some(message) = global_filter_error(cli) {
        let code = emit_known_failure_with_style(
            message,
            2,
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
                | fallow_config::OutputFormat::ReviewGithub
                | fallow_config::OutputFormat::ReviewGitlab
        )
    {
        let code = emit_known_failure_with_style(
            "--report-path-prefix is only valid with --format github-annotations, \
             github-summary, codeclimate, review-github, or review-gitlab",
            2,
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
            | fallow_config::OutputFormat::ReviewGithub
            | fallow_config::OutputFormat::ReviewGitlab
    ) {
        report::github::init_report_prefix(root);
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
        return crate::error::emit_error_with_style(
            err.to_string().trim(),
            exit_code,
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

fn command_rejects_output_gate(command: Option<&Command>) -> bool {
    matches!(
        command,
        Some(
            Command::Init { .. }
                | Command::ConfigSchema
                | Command::PluginSchema
                | Command::PluginCheck
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
                | Command::AuditCache { .. }
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

/// Directories a supplied unified diff's paths might be relative to, most
/// preferred first.
///
/// `git diff` writes paths relative to the repository toplevel, while
/// `git diff --relative` writes them relative to the invoking directory. Both
/// reach fallow through `--diff-file` / `--diff-stdin`, and a unified diff does
/// not say which one it is, so the caller offers both and the paths decide (see
/// `choose_diff_base`). The two coincide for a single-package repo, which is why
/// keying against `--root` alone went unnoticed until `--root` addressed a
/// package inside a monorepo.
///
/// The toplevel is only used to measure how far `root` sits below it; the
/// returned base is that many components popped off `root` itself, so it keeps
/// `root`'s spelling. Finding paths are built from `root`, and a canonicalized
/// base would fail to prefix them wherever the two disagree (`/tmp` vs
/// `/private/tmp` on macOS).
fn diff_base_candidates(root: &Path) -> Vec<PathBuf> {
    let Some(toplevel) = git_toplevel_base(root) else {
        return vec![root.to_path_buf()];
    };
    if toplevel == root {
        return vec![root.to_path_buf()];
    }
    vec![toplevel, root.to_path_buf()]
}

/// `root` with its offset below the git toplevel popped off, preserving
/// `root`'s spelling. `None` outside a git repo.
fn git_toplevel_base(root: &Path) -> Option<PathBuf> {
    let toplevel = crate::base_worktree::git_toplevel(root)?;
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let offset = canonical_root.strip_prefix(&toplevel).ok()?;
    let mut base = root.to_path_buf();
    for _ in offset.components() {
        if !base.pop() {
            return None;
        }
    }
    Some(base)
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
    let _ = report::ci::diff_filter::init_shared_diff(
        diff_source.as_ref(),
        root,
        &diff_base_candidates(root),
        suppress_warnings,
    );
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
