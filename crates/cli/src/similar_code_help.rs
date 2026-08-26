use std::ffi::OsStr;

use crate::similar_code_cli::{SimilarCodeCacheSubcommand, SimilarCodeSubcommand};
use crate::{Cli, Command};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimilarCodeHelpTarget {
    Discovery,
    Status,
    Setup,
    Inspect,
    Review,
    Cache,
    CacheClear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SimilarCodeInvocation {
    Discovery,
    Status,
    Setup,
    Inspect,
    Review,
    CacheClear,
}

impl SimilarCodeInvocation {
    fn from_command(command: &Command) -> Option<Self> {
        let Command::SimilarCode { subcommand, .. } = command else {
            return None;
        };
        Some(match subcommand {
            None => Self::Discovery,
            Some(SimilarCodeSubcommand::Status) => Self::Status,
            Some(SimilarCodeSubcommand::Setup { .. }) => Self::Setup,
            Some(SimilarCodeSubcommand::Inspect { .. }) => Self::Inspect,
            Some(SimilarCodeSubcommand::Review { .. }) => Self::Review,
            Some(SimilarCodeSubcommand::Cache {
                subcommand: SimilarCodeCacheSubcommand::Clear { .. },
            }) => Self::CacheClear,
        })
    }

    const fn allows_project_options(self) -> bool {
        matches!(self, Self::Discovery | Self::Inspect | Self::CacheClear)
    }

    const fn allows_analysis_options(self) -> bool {
        matches!(self, Self::Discovery)
    }

    const fn allows_output_file(self) -> bool {
        matches!(self, Self::Discovery | Self::Inspect | Self::Review)
    }
}

/// Return the first explicitly unsupported root or similar-code option.
/// Presentation options (`format`, `pretty`, and `quiet`) are valid for every
/// target. Every other option is admitted by the target-specific groups below.
pub fn unsupported_similar_code_option(cli: &Cli) -> Option<&'static str> {
    unsupported_similar_code_option_with_explicit_tolerance(
        cli,
        raw_args_contain_long_option("tolerance"),
    )
}

fn unsupported_similar_code_option_with_explicit_tolerance(
    cli: &Cli,
    explicit_tolerance: bool,
) -> Option<&'static str> {
    let command = cli.command.as_ref()?;
    let invocation = SimilarCodeInvocation::from_command(command)?;
    let project = invocation.allows_project_options();
    let analysis = invocation.allows_analysis_options();

    if cli.root.is_some() && !project {
        return Some("--root");
    }
    if cli.config.is_some() && !project {
        return Some("--config");
    }
    if cli.allow_remote_extends && !project {
        return Some("--allow-remote-extends");
    }
    if cli.no_cache && !analysis {
        return Some("--no-cache");
    }
    if cli.threads.is_some() && !analysis {
        return Some("--threads");
    }
    if cli.changed_since.is_some() && !analysis {
        return Some("--changed-since");
    }
    if cli.diff_file.is_some() && !analysis {
        return Some("--diff-file");
    }
    if cli.diff_stdin && !analysis {
        return Some("--diff-stdin");
    }
    if cli.max_file_size.is_some() && !analysis {
        return Some("--max-file-size");
    }
    if cli.workspace.is_some() && !analysis {
        return Some("--workspace");
    }
    if cli.changed_workspaces.is_some() && !analysis {
        return Some("--changed-workspaces");
    }
    if cli.explain && !analysis {
        return Some("--explain");
    }
    if cli.output_file.is_some() && !invocation.allows_output_file() {
        return Some("--output-file");
    }

    if let Some(flag) = unsupported_universal_analysis_option(cli, explicit_tolerance) {
        return Some(flag);
    }

    let Command::SimilarCode {
        threshold,
        min_lines,
        top,
        file,
        ..
    } = command
    else {
        unreachable!("invocation was derived from similar-code")
    };
    if !analysis {
        if threshold.is_some() {
            return Some("--threshold");
        }
        if min_lines.is_some() {
            return Some("--min-lines");
        }
        if top.is_some() {
            return Some("--top");
        }
        if !file.is_empty() {
            return Some("--file");
        }
    }
    None
}

fn unsupported_universal_analysis_option(
    cli: &Cli,
    explicit_tolerance: bool,
) -> Option<&'static str> {
    [
        (cli.churn_file.is_some(), "--churn-file"),
        (cli.baseline.is_some(), "--baseline"),
        (cli.baseline_mode.is_some(), "--baseline-mode"),
        (cli.parent_run.is_some(), "--parent-run"),
        (cli.save_baseline.is_some(), "--save-baseline"),
        (cli.production, "--production"),
        (cli.no_production, "--no-production"),
        (cli.production_dead_code, "--production-dead-code"),
        (cli.production_health, "--production-health"),
        (cli.production_dupes, "--production-dupes"),
        (cli.group_by.is_some(), "--group-by"),
        (cli.performance, "--performance"),
        (cli.explain_skipped, "--explain-skipped"),
        (cli.summary, "--summary"),
        (cli.ci, "--ci"),
        (cli.fail_on_issues, "--fail-on-issues"),
        (cli.sarif_file.is_some(), "--sarif-file"),
        (cli.report_path_prefix.is_some(), "--report-path-prefix"),
        (cli.fail_on_regression, "--fail-on-regression"),
        (cli.tolerance != "0" || explicit_tolerance, "--tolerance"),
        (cli.regression_baseline.is_some(), "--regression-baseline"),
        (
            cli.save_regression_baseline.is_some(),
            "--save-regression-baseline",
        ),
        (cli.dupes_mode.is_some(), "--dupes-mode"),
        (cli.dupes_near, "--dupes-near"),
        (cli.dupes_threshold.is_some(), "--dupes-threshold"),
        (cli.dupes_min_tokens.is_some(), "--dupes-min-tokens"),
        (cli.dupes_min_lines.is_some(), "--dupes-min-lines"),
        (
            cli.dupes_min_occurrences.is_some(),
            "--dupes-min-occurrences",
        ),
        (cli.dupes_skip_local, "--dupes-skip-local"),
        (cli.dupes_cross_language, "--dupes-cross-language"),
        (cli.dupes_ignore_imports, "--dupes-ignore-imports"),
        (cli.dupes_no_ignore_imports, "--dupes-no-ignore-imports"),
        (!cli.only.is_empty(), "--only"),
        (!cli.skip.is_empty(), "--skip"),
        (cli.score, "--score"),
        (cli.trend, "--trend"),
        (cli.save_snapshot.is_some(), "--save-snapshot"),
        (cli.coverage.is_some(), "--coverage"),
        (cli.coverage_root.is_some(), "--coverage-root"),
        (cli.include_entry_exports, "--include-entry-exports"),
        (cli.type_aware, "--type-aware"),
        (cli.no_type_aware, "--no-type-aware"),
        (!cli.type_aware_project.is_empty(), "--type-aware-project"),
        (cli.type_aware_require.is_some(), "--type-aware-require"),
    ]
    .into_iter()
    .find_map(|(used, flag)| used.then_some(flag))
}

pub fn similar_code_help_target<I, S>(args: I) -> Option<SimilarCodeHelpTarget>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| arg.as_ref().to_string_lossy().into_owned())
        .collect();

    if args.first().is_some_and(|arg| arg == "help") {
        if args.get(1).is_none_or(|arg| arg != "similar-code") {
            return None;
        }
        let path = command_tokens(&args[2..])
            .into_iter()
            .map(|(_, arg)| arg)
            .collect::<Vec<_>>();
        return Some(target_from_path(&path));
    }

    let command_tokens = command_tokens(&args);
    let similar_code_token_index = command_tokens
        .iter()
        .position(|(_, arg)| *arg == "similar-code")?;
    let similar_code_arg_index = command_tokens[similar_code_token_index].0;
    if !args[similar_code_arg_index + 1..]
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        return None;
    }
    let path = command_tokens[similar_code_token_index + 1..]
        .iter()
        .map(|(_, arg)| *arg)
        .collect::<Vec<_>>();
    Some(target_from_path(&path))
}

fn target_from_path(args: &[&str]) -> SimilarCodeHelpTarget {
    let first_subcommand = args
        .iter()
        .position(|arg| matches!(*arg, "status" | "setup" | "inspect" | "review" | "cache"));
    match first_subcommand.map(|index| (args[index], &args[index + 1..])) {
        Some(("status", _)) => SimilarCodeHelpTarget::Status,
        Some(("setup", _)) => SimilarCodeHelpTarget::Setup,
        Some(("inspect", _)) => SimilarCodeHelpTarget::Inspect,
        Some(("review", _)) => SimilarCodeHelpTarget::Review,
        Some(("cache", rest)) if rest.contains(&"clear") => SimilarCodeHelpTarget::CacheClear,
        Some(("cache", _)) => SimilarCodeHelpTarget::Cache,
        _ => SimilarCodeHelpTarget::Discovery,
    }
}

fn command_tokens(args: &[String]) -> Vec<(usize, &str)> {
    let mut tokens = Vec::new();
    let mut skip_value = false;
    for (index, arg) in args.iter().enumerate() {
        if skip_value {
            skip_value = false;
            continue;
        }
        if option_consumes_next(arg) {
            skip_value = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        tokens.push((index, arg.as_str()));
    }
    tokens
}

fn option_consumes_next(arg: &str) -> bool {
    if arg.contains('=') {
        return false;
    }
    matches!(
        arg,
        "-r" | "--root"
            | "-c"
            | "--config"
            | "-f"
            | "--format"
            | "--output"
            | "--threads"
            | "--changed-since"
            | "--base"
            | "--diff-file"
            | "--churn-file"
            | "--max-file-size"
            | "--baseline"
            | "--baseline-mode"
            | "--parent-run"
            | "--save-baseline"
            | "-w"
            | "--workspace"
            | "--changed-workspaces"
            | "--group-by"
            | "--sarif-file"
            | "-o"
            | "--output-file"
            | "--report-path-prefix"
            | "--annotations-path-prefix"
            | "--tolerance"
            | "--regression-baseline"
            | "--save-regression-baseline"
            | "--only"
            | "--skip"
            | "--dupes-mode"
            | "--dupes-threshold"
            | "--dupes-min-tokens"
            | "--dupes-min-lines"
            | "--dupes-min-occurrences"
            | "--save-snapshot"
            | "--coverage"
            | "--coverage-root"
            | "--type-aware-project"
            | "--type-aware-require"
            | "--threshold"
            | "--min-lines"
            | "--top"
            | "--file"
            | "--candidates"
            | "--verdicts"
    )
}

fn raw_args_contain_long_option(long: &str) -> bool {
    args_contain_long_option(std::env::args_os(), long)
}

fn args_contain_long_option<I, S>(args: I, long: &str) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    let exact = format!("--{long}");
    let prefix = format!("--{long}=");
    args.into_iter().any(|arg| {
        let arg = arg.as_ref().to_string_lossy();
        arg == exact || arg.starts_with(&prefix)
    })
}

pub fn render_similar_code_help(target: SimilarCodeHelpTarget) -> &'static str {
    match target {
        SimilarCodeHelpTarget::Discovery => DISCOVERY_HELP,
        SimilarCodeHelpTarget::Status => STATUS_HELP,
        SimilarCodeHelpTarget::Setup => SETUP_HELP,
        SimilarCodeHelpTarget::Inspect => INSPECT_HELP,
        SimilarCodeHelpTarget::Review => REVIEW_HELP,
        SimilarCodeHelpTarget::Cache => CACHE_HELP,
        SimilarCodeHelpTarget::CacheClear => CACHE_CLEAR_HELP,
    }
}

const DISCOVERY_HELP: &str = "\
Find semantically similar functions with a pinned local model (opt-in).

Usage: fallow similar-code [OPTIONS] [COMMAND]

Commands:
  status   Show exact companion and pinned-model readiness
  setup    Download and verify the pinned local model
  inspect  Reproduce and inspect one unverified candidate
  review   Join candidate JSON with a separate verdict document
  cache    Manage the user-local, project-namespaced vector cache
  help     Print this message or the help of a command

Options:
  -r, --root <ROOT>                       Project root directory
  -c, --config <CONFIG>                   Path to config file
      --allow-remote-extends              Allow trusted config files to extend HTTPS URLs
  -f, --format <FORMAT>                   Output format: human or json [default: human]
      --pretty                            Indent JSON output
  -q, --quiet                             Suppress progress and cold-run guidance
      --no-cache                          Disable incremental vector caching
      --threads <THREADS>                 Number of parser threads
      --changed-since <REF>               Report pairs touching files changed since this git ref
      --diff-file <PATH>                  Unified diff for line-level scoping
      --diff-stdin                        Read the unified diff from stdin
  -w, --workspace <WORKSPACE>             Scope output to selected workspaces
      --changed-workspaces <REF>          Scope output to workspaces touched since this git ref
      --max-file-size <MB>                Skip source files larger than this many megabytes
      --explain                           Include metric and model interpretation metadata
      --threshold <0..1>                  Minimum cosine similarity retained
      --min-lines <N>                     Minimum source lines per extracted function
      --top <N>                           Cap displayed candidates after comparison
      --file <PATH>                       Report pairs touching these project-relative files
  -o, --output-file <PATH>                Write the report to a file instead of stdout
  -h, --help                              Print help
";

const STATUS_HELP: &str = "\
Show exact companion and pinned-model readiness without reading project source.

Usage: fallow similar-code status [OPTIONS]

Options:
  -f, --format <FORMAT>  Output format: human or json [default: human]
      --pretty           Indent JSON output
  -q, --quiet            Suppress progress output
  -h, --help             Print help
";

const SETUP_HELP: &str = "\
Download and verify the pinned local model after explicit confirmation.

Usage: fallow similar-code setup --local [OPTIONS]

Options:
      --local            Confirm setup for the first-party pinned local model
      --yes              Confirm non-interactively
  -f, --format <FORMAT>  Output format: human or json [default: human]
      --pretty           Indent JSON output
  -q, --quiet            Suppress progress output
  -h, --help             Print help
";

const INSPECT_HELP: &str = "\
Reproduce and inspect one unverified candidate with bounded source evidence.

Usage: fallow similar-code [OPTIONS] inspect <CANDIDATE_ID> --candidates <PATH>

Arguments:
  <CANDIDATE_ID>  Snapshot-stable candidate identity from fallow similar-code

Options:
  -r, --root <ROOT>                       Project root directory
  -c, --config <CONFIG>                   Path to config file
      --allow-remote-extends              Allow trusted config files to extend HTTPS URLs
      --candidates <PATH>                 Raw discovery JSON containing this candidate
  -f, --format <FORMAT>                   Output format: human or json [default: human]
      --pretty                            Indent JSON output
  -q, --quiet                             Suppress progress output
  -o, --output-file <PATH>                Write the report to a file instead of stdout
  -h, --help                              Print help
";

const REVIEW_HELP: &str = "\
Join candidate JSON with a separate human or agent verdict document.

Usage: fallow similar-code review --candidates <PATH> --verdicts <PATH> [OPTIONS]

Options:
      --candidates <PATH>                     Raw fallow similar-code JSON document
      --verdicts <PATH>                       Versioned verdict JSON document
      --require-verdict-for-each-candidate    Fail unless every candidate has a safe verdict match
  -f, --format <FORMAT>                       Output format: human or json [default: human]
      --pretty                                Indent JSON output
  -q, --quiet                                 Suppress progress output
  -o, --output-file <PATH>                    Write the report to a file instead of stdout
  -h, --help                                  Print help
";

const CACHE_HELP: &str = "\
Manage the user-local, project-namespaced vector cache. Model artifacts are unaffected.

Usage: fallow similar-code cache <COMMAND>

Commands:
  clear  Remove cached vectors after explicit confirmation
  help   Print this message or the help of a command

Options:
  -h, --help  Print help
";

const CACHE_CLEAR_HELP: &str = "\
Remove cached vectors after explicit confirmation.

Usage: fallow similar-code cache clear --yes [OPTIONS]

Options:
  -r, --root <ROOT>          Project root directory
  -c, --config <CONFIG>      Path to config file
      --allow-remote-extends Allow trusted config files to extend HTTPS URLs
      --yes                  Confirm deletion of derived vectors
  -f, --format <FORMAT>      Output format: human or json [default: human]
      --pretty               Indent JSON output
  -q, --quiet                Suppress progress output
  -h, --help                 Print help
";

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn detects_parent_and_nested_help_forms() {
        assert_eq!(
            similar_code_help_target(["similar-code", "--help"]),
            Some(SimilarCodeHelpTarget::Discovery)
        );
        assert_eq!(
            similar_code_help_target(["help", "similar-code", "review"]),
            Some(SimilarCodeHelpTarget::Review)
        );
        assert_eq!(
            similar_code_help_target(["similar-code", "cache", "clear", "-h"]),
            Some(SimilarCodeHelpTarget::CacheClear)
        );
        assert_eq!(similar_code_help_target(["security", "--help"]), None);
        assert_eq!(
            similar_code_help_target(["similar-code", "--file", "status", "--help"]),
            Some(SimilarCodeHelpTarget::Discovery)
        );
        assert_eq!(
            similar_code_help_target(["--config", "similar-code", "security", "--help"]),
            None
        );
        assert_eq!(similar_code_help_target(["--help", "similar-code"]), None);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_argv_never_panics_during_explicit_option_detection() {
        use std::os::unix::ffi::OsStringExt as _;

        let invalid = std::ffi::OsString::from_vec(vec![b'b', b'a', b'd', 0xff]);
        assert!(!args_contain_long_option([invalid], "tolerance"));
    }

    #[test]
    fn every_help_surface_has_a_closed_format_set() {
        for target in [
            SimilarCodeHelpTarget::Discovery,
            SimilarCodeHelpTarget::Status,
            SimilarCodeHelpTarget::Setup,
            SimilarCodeHelpTarget::Inspect,
            SimilarCodeHelpTarget::Review,
            SimilarCodeHelpTarget::CacheClear,
        ] {
            let help = render_similar_code_help(target);
            assert!(help.contains("human or json"));
            for unsupported in [
                "sarif",
                "codeclimate",
                "markdown",
                "--ci",
                "--baseline",
                "--dupes",
            ] {
                assert!(
                    !help.contains(unsupported),
                    "unexpected {unsupported} in:\n{help}"
                );
            }
        }
    }

    #[test]
    fn rejects_irrelevant_analysis_options_for_each_target() {
        for (argv, expected) in [
            (
                vec![
                    "fallow",
                    "similar-code",
                    "review",
                    "--candidates",
                    "c.json",
                    "--verdicts",
                    "v.json",
                    "--dupes-near",
                ],
                "--dupes-near",
            ),
            (
                vec![
                    "fallow",
                    "similar-code",
                    "review",
                    "--candidates",
                    "c.json",
                    "--verdicts",
                    "v.json",
                    "--baseline",
                    "base.json",
                ],
                "--baseline",
            ),
            (
                vec!["fallow", "similar-code", "status", "--root", "."],
                "--root",
            ),
            (
                vec!["fallow", "similar-code", "setup", "--local", "--no-cache"],
                "--no-cache",
            ),
            (
                vec![
                    "fallow",
                    "similar-code",
                    "cache",
                    "clear",
                    "--yes",
                    "--threads",
                    "2",
                ],
                "--threads",
            ),
            (
                vec!["fallow", "--score", "similar-code", "status"],
                "--score",
            ),
            (
                vec![
                    "fallow",
                    "similar-code",
                    "review",
                    "--candidates",
                    "c.json",
                    "--verdicts",
                    "v.json",
                    "--tolerance",
                    "0",
                ],
                "--tolerance",
            ),
        ] {
            let cli = Cli::try_parse_from(&argv).expect("global option parses before validation");
            assert_eq!(
                unsupported_similar_code_option_with_explicit_tolerance(
                    &cli,
                    args_contain_long_option(&argv, "tolerance"),
                ),
                Some(expected)
            );
        }
    }

    #[test]
    fn accepts_each_targets_explicit_option_set() {
        for argv in [
            vec![
                "fallow",
                "similar-code",
                "--root",
                ".",
                "--config",
                "fallow.toml",
                "--no-cache",
                "--threads",
                "2",
                "--changed-since",
                "main",
                "--workspace",
                "app",
                "--threshold",
                "0.8",
                "--min-lines",
                "3",
                "--top",
                "10",
                "--file",
                "src/a.ts",
                "--format",
                "json",
                "--quiet",
                "--output-file",
                "out.json",
            ],
            vec![
                "fallow",
                "similar-code",
                "--root",
                ".",
                "inspect",
                "candidate-id",
                "--candidates",
                "candidates.json",
            ],
            vec![
                "fallow",
                "similar-code",
                "review",
                "--candidates",
                "c.json",
                "--verdicts",
                "v.json",
                "--output-file",
                "review.txt",
            ],
            vec!["fallow", "similar-code", "status", "--format", "json"],
            vec!["fallow", "similar-code", "setup", "--local", "--yes"],
            vec![
                "fallow",
                "similar-code",
                "--root",
                ".",
                "cache",
                "clear",
                "--yes",
            ],
        ] {
            let cli = Cli::try_parse_from(argv).expect("supported invocation parses");
            assert_eq!(unsupported_similar_code_option(&cli), None);
        }
    }
}
