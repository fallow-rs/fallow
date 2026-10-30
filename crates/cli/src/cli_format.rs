use clap::ValueEnum;
use fallow_config::OutputFormat;

use super::{Cli, Command, CoverageCli, ImpactCli, RulePackCli};
use crate::cli_hooks::HooksCli;
use crate::json_style::{JsonStyle, LeanStyle};

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Format {
    Human,
    Json,
    Sarif,
    Compact,
    #[value(alias = "md")]
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
    // Naming rationale (do not "fix" later): `github-annotations` PREFIXES the
    // provider, unlike `review-github`, because workflow-command annotations
    // are a GitHub-only concept with no GitLab twin (the `codeclimate` format
    // covers GitLab Code Quality).
    /// GitHub workflow-command annotations. Log-based `::warning` annotations render on fork PRs without a write token, unlike the PR-comment/review formats.
    #[value(name = "github-annotations")]
    GithubAnnotations,
    /// GitHub Actions job-summary markdown, for `fallow ... >> "$GITHUB_STEP_SUMMARY"`. Renders on fork PRs without a write token.
    #[value(name = "github-summary")]
    GithubSummary,
}

impl From<Format> for OutputFormat {
    fn from(format: Format) -> Self {
        match format {
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
            Format::GithubAnnotations => Self::GithubAnnotations,
            Format::GithubSummary => Self::GithubSummary,
        }
    }
}

pub struct FormatConfig {
    pub output: OutputFormat,
    pub payload_is_json: bool,
    pub quiet: bool,
    pub fail_on_issues: bool,
    pub json_style: JsonStyle,
}

/// Read `FALLOW_FORMAT` env var and parse it into a Format value.
pub fn format_from_env() -> Option<Format> {
    let val = std::env::var("FALLOW_FORMAT").ok()?;
    parse_format_arg(&val)
}

pub fn parse_format_arg(value: &str) -> Option<Format> {
    Format::from_str(value, true).ok()
}

/// Read `FALLOW_QUIET` env var: "1" or "true" means quiet.
fn quiet_from_env() -> bool {
    std::env::var("FALLOW_QUIET").is_ok_and(|v| v == "1" || v.eq_ignore_ascii_case("true"))
}

pub fn bool_from_env(name: &str) -> Option<bool> {
    let value = std::env::var(name).ok()?;
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

pub fn resolve_format(cli: &Cli) -> FormatConfig {
    let cli_format_was_explicit = std::env::args_os().any(|arg| {
        let a = arg.to_string_lossy();
        a == "--format"
            || a == "--output"
            || a.starts_with("--format=")
            || a.starts_with("--output=")
            || a == "-f"
            || (a.starts_with("-f") && a.len() > 2)
    });
    let format: Format = if cli_format_was_explicit {
        cli.format
    } else {
        format_from_env().unwrap_or(cli.format)
    };

    let quiet = cli.quiet || quiet_from_env();
    let (output, quiet, fail_on_issues) = apply_ci_defaults(
        cli.ci,
        cli.fail_on_issues,
        format.into(),
        quiet,
        cli_format_was_explicit,
    );
    let output = resolve_command_output(cli, output);
    let payload_is_json = command_payload_is_json(cli, output);

    FormatConfig {
        output,
        payload_is_json,
        quiet,
        fail_on_issues,
        json_style: resolve_json_style(cli),
    }
}

fn resolve_json_style(cli: &Cli) -> JsonStyle {
    if cli.json.lean {
        return JsonStyle::Lean(LeanStyle {
            pretty: cli.json.pretty,
            table_cap: lean_table_cap(cli),
        });
    }
    if cli.json.pretty {
        JsonStyle::Pretty
    } else {
        JsonStyle::Compact
    }
}

/// `--full-tables` keeps every row. A `--top` on `fallow health` replaces the
/// default cap, so the lean tables follow the row count that the user chose.
fn lean_table_cap(cli: &Cli) -> Option<usize> {
    if cli.json.full_tables {
        return None;
    }
    let top = match cli.command.as_ref() {
        Some(Command::Health { top, .. }) => *top,
        _ => None,
    };
    Some(top.unwrap_or(fallow_output::LEAN_TABLE_CAP))
}

/// Error text when `--pretty` or `--lean` cannot apply to this run.
pub fn json_presentation_error(cli: &Cli, fmt: &FormatConfig) -> Option<&'static str> {
    if cli.json.pretty && !fmt.payload_is_json {
        return Some(
            "--pretty requires JSON output. Use --format json --pretty, or remove --pretty.",
        );
    }
    if cli.json.lean && !fmt.payload_is_json {
        return Some("--lean requires JSON output. Use --format json --lean, or remove --lean.");
    }
    unsupported_lean_use(cli)
}

/// `--lean` changes only the dead-code, health and combined envelopes, so
/// another command rejects it instead of writing its full shape.
fn unsupported_lean_use(cli: &Cli) -> Option<&'static str> {
    if !cli.json.lean {
        return None;
    }
    match cli.command.as_ref() {
        None | Some(Command::Check { .. }) => None,
        Some(Command::Health { top, .. }) => (cli.json.full_tables && top.is_some()).then_some(
            "--full-tables keeps every table row, but --top cuts the tables. Remove one of them.",
        ),
        Some(_) => Some(
            "--lean applies to `fallow dead-code`, `fallow health` and bare `fallow`. Remove --lean.",
        ),
    }
}

/// Resolve commands that always use JSON, including for structured errors.
fn resolve_command_output(cli: &Cli, selected: OutputFormat) -> OutputFormat {
    match cli.command.as_ref() {
        Some(
            Command::Schema
            | Command::ConfigSchema
            | Command::PluginSchema
            | Command::RulePackSchema
            | Command::Ci { .. }
            | Command::RulePack {
                subcommand: RulePackCli::Schema,
            }
            | Command::Config { path: false }
            | Command::Coverage {
                subcommand:
                    CoverageCli::Setup {
                        args: crate::coverage::CoverageSetupCli { json: true, .. },
                    }
                    | CoverageCli::ReviewPacket { .. }
                    | CoverageCli::DeploymentChanges { .. },
            },
        ) => OutputFormat::Json,
        _ => selected,
    }
}

/// Whether the selected command's successful payload is JSON. Human-only
/// commands may still use `--format json` for structured errors.
fn command_payload_is_json(cli: &Cli, selected: OutputFormat) -> bool {
    match cli.command.as_ref() {
        Some(
            Command::Schema
            | Command::ConfigSchema
            | Command::PluginSchema
            | Command::RulePackSchema
            | Command::Ci { .. }
            | Command::RulePack {
                subcommand: RulePackCli::Schema,
            },
        ) => true,
        Some(Command::Config { path }) => !path,
        Some(Command::Coverage {
            subcommand: CoverageCli::Setup { args },
        }) => args.json,
        Some(
            Command::Coverage {
                subcommand:
                    CoverageCli::UploadInventory { .. }
                    | CoverageCli::UploadSourceMaps { .. }
                    | CoverageCli::UploadStaticFindings { .. },
            }
            | Command::Impact {
                subcommand:
                    Some(
                        ImpactCli::Enable
                        | ImpactCli::Disable
                        | ImpactCli::Default { .. }
                        | ImpactCli::Reset { .. }
                        | ImpactCli::Statusline,
                    ),
                ..
            }
            | Command::Init { .. }
            | Command::CiTemplate { .. }
            | Command::Migrate { .. }
            | Command::SetupHooks { .. }
            | Command::Hooks {
                subcommand: HooksCli::Install { .. } | HooksCli::Uninstall { .. },
            },
        ) => false,
        _ => matches!(selected, OutputFormat::Json),
    }
}

/// Apply CI defaults: if `--ci` is set, override format to SARIF unless the
/// output was explicit, enable fail-on-issues, and set quiet.
pub fn apply_ci_defaults(
    ci: bool,
    mut fail_on_issues: bool,
    output: OutputFormat,
    quiet: bool,
    cli_format_was_explicit: bool,
) -> (OutputFormat, bool, bool) {
    if ci {
        let ci_output = if !cli_format_was_explicit && format_from_env().is_none() {
            OutputFormat::Sarif
        } else {
            output
        };
        fail_on_issues = true;
        (ci_output, true, fail_on_issues)
    } else {
        (output, quiet, fail_on_issues)
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn lean_is_accepted_only_on_commands_with_a_lean_envelope() {
        let parse = |command: &[&str]| {
            let mut argv = vec!["fallow"];
            argv.extend_from_slice(command);
            argv.extend(["--format", "json", "--lean"]);
            Cli::try_parse_from(argv).expect("--lean parses before validation")
        };
        for accepted in [
            &[][..],
            &["dead-code"],
            &["check"],
            &["health"],
            &["health", "--top", "3"],
        ] {
            assert_eq!(unsupported_lean_use(&parse(accepted)), None, "{accepted:?}");
        }
        for rejected in [&["dupes"][..], &["audit"], &["architecture"], &["list"]] {
            assert!(
                unsupported_lean_use(&parse(rejected)).is_some(),
                "{rejected:?}"
            );
        }
        assert!(
            unsupported_lean_use(&parse(&["health", "--top", "3", "--full-tables"])).is_some(),
            "--full-tables cannot restore rows that --top cut"
        );
    }
}
