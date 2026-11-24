use clap::{CommandFactory, FromArgMatches};

use super::*;
use crate::Cli;

fn rejection(args: &[&str]) -> Option<String> {
    let root = Cli::command();
    let argv = std::iter::once("fallow").chain(args.iter().copied());
    let matches = root
        .clone()
        .try_get_matches_from(argv)
        .expect("the command line parses");
    let cli = Cli::from_arg_matches(&matches).expect("the command line converts");
    let output = crate::cli_format::resolve_format(&cli).output;
    rejected_option(&root, &matches, output)
}

fn global_longs() -> Vec<String> {
    let mut root = Cli::command();
    root.build();
    root.get_arguments()
        .filter(|arg| arg.is_global_set())
        .map(|arg| {
            let long = arg
                .get_long()
                .unwrap_or_else(|| panic!("global argument {} has no long", arg.get_id()));
            format!("--{long}")
        })
        .collect()
}

#[test]
fn every_global_argument_has_one_row() {
    let longs = global_longs();
    for long in &longs {
        let rows = GLOBAL_OPTION_SUPPORT
            .iter()
            .filter(|row| row.option == long)
            .count();
        assert_eq!(
            rows, 1,
            "{long} needs exactly one row in GLOBAL_OPTION_SUPPORT"
        );
    }
    for row in GLOBAL_OPTION_SUPPORT {
        assert!(
            longs.iter().any(|long| long == row.option),
            "{} has a row but is no global argument",
            row.option
        );
    }
}

#[test]
fn every_row_names_existing_commands_once() {
    let root = Cli::command();
    for row in GLOBAL_OPTION_SUPPORT {
        let mut seen: Vec<&str> = Vec::new();
        for entry in row.commands.iter().flat_map(|group| group.iter()) {
            let known = entry.command == BARE_RUN
                || entry
                    .command
                    .split(' ')
                    .try_fold(&root, |cmd, name| cmd.find_subcommand(name))
                    .is_some();
            assert!(
                known,
                "{} names the unknown command {}",
                row.option, entry.command
            );
            assert!(
                !seen.contains(&entry.command),
                "{} names {} more than once",
                row.option,
                entry.command
            );
            seen.push(entry.command);
        }
    }
}

#[test]
fn every_rejected_message_uses_known_placeholders() {
    let placeholder = |text: &str| -> Vec<String> {
        text.split('<')
            .skip(1)
            .filter_map(|rest| rest.split_once('>').map(|(name, _)| name.to_owned()))
            .collect()
    };
    for row in GLOBAL_OPTION_SUPPORT {
        let supports = std::iter::once(row.default).chain(
            row.commands
                .iter()
                .flat_map(|group| group.iter())
                .map(|entry| entry.support),
        );
        for support in supports {
            if let Support::Rejected { message } | Support::SupportedWithFormats { message, .. } =
                support
            {
                for name in placeholder(message) {
                    assert!(
                        // `<file>` and `<PATH>` are value names of the text, not
                        // placeholders.
                        matches!(
                            name.as_str(),
                            "option" | "command" | "formats" | "file" | "PATH"
                        ),
                        "{} has the unknown placeholder <{name}>: {message}",
                        row.option
                    );
                }
            }
        }
    }
}

#[test]
fn accepted_command_lines_have_no_rejection() {
    for args in [
        &["--save-baseline", "b.json"][..],
        &["dead-code", "--save-baseline", "b.json"],
        &["dupes", "--ci"],
        &["health", "--group-by", "owner", "--group", "src"],
        &["security", "--sarif-file", "s.sarif"],
        &["doctor", "--format", "json", "--quiet"],
        &["init", "--root", "."],
        &["dead-code", "--tolerance", "0"],
    ] {
        assert_eq!(rejection(args), None, "{args:?}");
    }
}

#[test]
fn a_command_without_a_baseline_rejects_the_baseline_options() {
    assert_eq!(
        rejection(&["fix", "--save-baseline", "b.json"]).as_deref(),
        Some(
            "`fallow fix` does not load or save a baseline, so `--save-baseline` has no effect. Use `--save-baseline` with bare `fallow`, `fallow dead-code`, `fallow architecture`, `fallow dupes` or `fallow health`, or use `fallow audit --dead-code-baseline`, `--health-baseline` or `--dupes-baseline`."
        )
    );
    for (args, option) in [
        (&["watch", "--baseline", "b.json"][..], "--baseline"),
        (
            &["init", "--fail-on-baseline-growth"],
            "--fail-on-baseline-growth",
        ),
        (&["flags", "--baseline-base", "main"], "--baseline-base"),
    ] {
        let message = rejection(args).unwrap_or_else(|| panic!("{args:?} is rejected"));
        assert!(
            message.contains(&format!("so `{option}` has no effect")),
            "{message}"
        );
    }
}

#[test]
fn security_rejects_the_analysis_options() {
    for (args, option) in [
        (&["--baseline", "b.json"][..], "--baseline"),
        (&["--fail-on-stale-baseline"], "--fail-on-stale-baseline"),
        (&["--fail-on-parse-error"], "--fail-on-parse-error"),
        (&["--production"], "--production"),
        (&["--group-by", "owner"], "--group-by"),
        (
            &["--save-regression-baseline"],
            "--save-regression-baseline",
        ),
        (&["--dupes-mode", "semantic"], "--dupes-mode"),
        (&["--include-entry-exports"], "--include-entry-exports"),
        (&["--show-cascade"], "--show-cascade"),
        (&["--performance"], "--performance"),
    ] {
        let mut argv = vec!["security"];
        argv.extend_from_slice(args);
        assert_eq!(
            rejection(&argv),
            Some(format!("{option} is not valid with `fallow security`.")),
            "{argv:?}"
        );
    }
    assert_eq!(rejection(&["security", "--tolerance", "2"]), None);
}

#[test]
fn doctor_rejects_every_option_except_project_and_presentation() {
    for row in GLOBAL_OPTION_SUPPORT {
        let accepted = matches!(
            row.option,
            "--root" | "--config" | "--format" | "--pretty" | "--quiet"
        );
        assert_eq!(
            row.support_for("doctor") == Support::Supported,
            accepted,
            "doctor and {}",
            row.option
        );
    }
    assert_eq!(
        rejection(&["doctor", "--threads", "2"]).as_deref(),
        Some("--threads is not valid with `fallow doctor`.")
    );
    assert_eq!(
        rejection(&["doctor", "--output-file", "r.json"]).as_deref(),
        Some(
            "--output-file is not valid with `fallow doctor`; doctor is read-only and writes its report to stdout"
        )
    );
}

#[test]
fn commands_without_a_report_reject_the_output_and_gate_options() {
    let message = "--ci, --fail-on-issues, --sarif-file, and --output-file are only valid with dead-code, architecture, dupes, health, security, or bare invocation";
    for args in [
        &["init", "--ci"][..],
        &["schema", "--output-file", "x.json"],
        &["config-schema", "--fail-on-issues"],
        &["explain", "unused-files", "--sarif-file", "x.sarif"],
    ] {
        assert_eq!(rejection(args).as_deref(), Some(message), "{args:?}");
    }
}

#[test]
fn only_sarif_writers_accept_sarif_file() {
    assert_eq!(
        rejection(&["dupes", "--sarif-file", "x.sarif"]).as_deref(),
        Some(
            "`fallow dupes` does not write a SARIF file, so `--sarif-file` has no effect. Use `--format sarif` with `--output-file`, or use `--sarif-file` with bare `fallow`, `fallow dead-code`, `fallow architecture` or `fallow security`."
        )
    );
    let other = rejection(&["fix", "--sarif-file", "x.sarif"]).expect("fix rejects it");
    assert!(
        other.starts_with("This command does not write a SARIF file"),
        "{other}"
    );
    for args in [
        &["--sarif-file", "x.sarif"][..],
        &["dead-code", "--sarif-file", "x.sarif"],
        &["architecture", "--sarif-file", "x.sarif"],
    ] {
        assert_eq!(rejection(args), None, "{args:?}");
    }
}

#[test]
fn the_remaining_command_checks_come_from_the_table() {
    assert_eq!(
        rejection(&["fix", "--fail-on-parse-error"]).as_deref(),
        Some(
            "--fail-on-parse-error is not valid with `fallow fix`. Use it with dead-code, architecture, health, audit or the bare run."
        )
    );
    assert_eq!(
        rejection(&["check", "--group-by", "owner", "--group", "src"]).as_deref(),
        Some("--group is valid with `fallow health --group-by` only.")
    );
    assert_eq!(
        rejection(&["audit", "--baseline", "b.json"]).as_deref(),
        Some(
            "audit uses per-analysis baselines. Use --dead-code-baseline, --health-baseline, or --dupes-baseline (or save them with `fallow dead-code|health|dupes --save-baseline <file>`)"
        )
    );
    assert_eq!(
        rejection(&["similar-code", "--ci"]).as_deref(),
        Some("--ci is not valid with this `fallow similar-code` command.")
    );
}

#[test]
fn an_option_before_the_subcommand_counts() {
    assert_eq!(
        rejection(&["--threads", "2", "doctor"]).as_deref(),
        Some("--threads is not valid with `fallow doctor`.")
    );
}

#[test]
fn analysis_commands_without_a_regression_baseline_reject_the_regression_options() {
    let message = rejection(&["dupes", "--save-regression-baseline"]).expect("dupes rejects it");
    assert!(
        message.starts_with("`fallow dupes` has no regression baseline, so `--save-regression-baseline` has no effect."),
        "{message}"
    );
    assert!(
        message.contains("fallow dupes --baseline <file>"),
        "{message}"
    );
    let message = rejection(&["health", "--fail-on-regression"]).expect("health rejects it");
    assert!(
        message.contains("fallow health --baseline <file>"),
        "{message}"
    );
    let message = rejection(&["fix", "--tolerance", "2"]).expect("fix rejects it");
    assert!(
        message.starts_with("This command has no regression baseline, so `--tolerance`"),
        "{message}"
    );
    assert_eq!(
        rejection(&["list", "--entry-weight", "--fail-on-regression"]),
        None
    );
    assert_eq!(rejection(&["security", "--tolerance", "2"]), None);
}

/// The pairs of a file option and a command that ignore the option on
/// purpose. `audit-cache` must clean up when the config does not load
/// (`audit_cache_prune_ignores_a_config_that_does_not_load`). The schema
/// commands and `telemetry` read no project, and accept `--root` because a
/// caller can pass it to each command
/// (`rule_pack_schema_matches_legacy_top_level_command`).
const INTENTIONALLY_IGNORED: &[(&str, &str)] = &[
    ("--config", "audit-cache"),
    ("--root", "config-schema"),
    ("--root", "plugin-schema"),
    ("--root", "rule-pack-schema"),
    ("--root", "schema"),
    ("--root", "telemetry"),
];

/// A command that ignores an option that names a file accepts the command
/// line, exits 0, and reads or writes no file (issue #4931). Such a pair is
/// supported, so the file test runs it, or it is rejected.
#[test]
fn no_command_ignores_an_option_that_names_a_file() {
    let globals = global_longs();
    for option in [
        "--save-regression-baseline",
        "--regression-baseline",
        "--save-baseline",
        "--baseline",
        "--sarif-file",
        "--output-file",
        "--diff-file",
        "--churn-file",
        "--config",
        "--root",
        "--type-aware-project",
    ] {
        let Some(row) = option_support(option) else {
            assert!(
                !globals.iter().any(|long| long == option),
                "{option} is global and needs a row"
            );
            continue;
        };
        assert!(
            !matches!(row.default, Support::Ignored { .. }),
            "{option} has an ignored default"
        );
        for entry in row.commands.iter().flat_map(|group| group.iter()) {
            if INTENTIONALLY_IGNORED.contains(&(option, entry.command)) {
                continue;
            }
            assert!(
                !matches!(entry.support, Support::Ignored { .. }),
                "{} ignores {option}",
                entry.command
            );
        }
    }
}

#[test]
fn the_baseline_hint_names_each_baseline_command() {
    let row = option_support("--baseline").expect("row");
    let Support::Rejected { message } = row.default else {
        panic!("--baseline rejects a command without a baseline");
    };
    for command in ["dead-code", "architecture", "dupes", "health"] {
        assert_eq!(row.support_for(command), Support::Supported, "{command}");
        assert!(
            message.contains(&format!("`fallow {command}`")),
            "{command}: {message}"
        );
    }
}

#[test]
fn lean_applies_to_the_commands_with_a_lean_envelope() {
    for accepted in [
        &["--lean"][..],
        &["dead-code", "--lean"],
        &["check", "--lean"],
        &["health", "--lean"],
    ] {
        assert_eq!(rejection(accepted), None, "{accepted:?}");
    }
    for command in ["dupes", "audit", "architecture", "list", "doctor"] {
        assert_eq!(
            rejection(&[command, "--lean"]).as_deref(),
            Some(
                "--lean applies to `fallow dead-code`, `fallow health` and bare `fallow`. Remove --lean."
            ),
            "{command}"
        );
    }
}

#[test]
fn the_impact_statusline_rejects_the_output_and_gate_options() {
    let gate = "--ci, --fail-on-issues, --sarif-file, and --output-file are only valid with dead-code, architecture, dupes, health, security, or bare invocation";
    for option in ["--ci", "--fail-on-issues"] {
        assert_eq!(
            rejection(&["impact", "statusline", option]).as_deref(),
            Some(gate),
            "{option}"
        );
        assert_eq!(rejection(&["impact", "status", option]), None, "{option}");
    }
    assert_eq!(
        rejection(&["impact", "statusline", "--output-file", "s.out"]).as_deref(),
        Some(
            "`fallow impact` prints its output to stdout, so `--output-file` has no effect. Redirect stdout to a file instead."
        )
    );
}

#[test]
fn commands_that_print_to_stdout_reject_output_file() {
    for args in [
        &["recommend"][..],
        &["workspaces"],
        &["suppressions"],
        &["rule-pack", "schema"],
        &["impact", "status"],
    ] {
        let mut argv = args.to_vec();
        argv.extend(["--output-file", "o.out"]);
        let message = rejection(&argv).unwrap_or_else(|| panic!("{argv:?} is rejected"));
        assert!(
            message.contains(&format!("`fallow {}` prints its output to stdout", args[0])),
            "{message}"
        );
    }
    assert_eq!(
        rejection(&["mutation", "tasks", "--output-file", "o.out"]),
        None
    );
    assert_eq!(
        rejection(&["type-aware", "status", "--output-file", "o.out"]),
        None
    );
}

#[test]
fn a_subcommand_entry_names_the_subcommand_in_its_message() {
    assert_eq!(
        rejection(&["security", "blind-spots", "--sarif-file", "s.sarif"]).as_deref(),
        Some("--sarif-file is not valid with `fallow security blind-spots`.")
    );
    assert_eq!(
        rejection(&[
            "security",
            "survivors",
            "--candidates",
            "c.json",
            "--verdicts",
            "v.json",
            "--summary"
        ])
        .as_deref(),
        Some("--summary is not valid with `fallow security survivors`.")
    );
    assert_eq!(
        rejection(&["security", "blind-spots", "--baseline", "b.json"]).as_deref(),
        Some("--baseline is not valid with `fallow security`.")
    );
    assert_eq!(rejection(&["security", "--sarif-file", "s.sarif"]), None);
}

#[test]
fn the_regression_options_keep_their_error_order() {
    let first = |args: &[&str]| -> String {
        let message = rejection(args).expect("rejected");
        [
            "--fail-on-regression",
            "--regression-baseline",
            "--save-regression-baseline",
            "--tolerance",
        ]
        .into_iter()
        .find(|option| message.contains(&format!("`{option}`")))
        .expect("names a regression option")
        .to_owned()
    };
    assert_eq!(
        first(&[
            "dupes",
            "--tolerance",
            "2",
            "--regression-baseline",
            "r.json"
        ]),
        "--regression-baseline"
    );
    assert_eq!(
        first(&[
            "dupes",
            "--tolerance",
            "2",
            "--save-regression-baseline",
            "--fail-on-regression"
        ]),
        "--fail-on-regression"
    );
    assert_eq!(
        first(&["health", "--tolerance", "2", "--save-regression-baseline"]),
        "--save-regression-baseline"
    );
    assert_eq!(first(&["fix", "--tolerance", "2"]), "--tolerance");
}

#[test]
fn output_file_works_with_the_formats_that_write_the_file() {
    for (args, format) in [
        (&["agent", "status"][..], "json"),
        (&["agent", "install"], "json"),
        (&["rule-pack", "list"], "json"),
        (&["fix", "--dry-run"], "github-summary"),
    ] {
        let mut argv = args.to_vec();
        argv.extend(["--output-file", "o.out", "--format", format]);
        assert_eq!(rejection(&argv), None, "{argv:?}");

        let mut argv = args.to_vec();
        argv.extend(["--output-file", "o.out", "--format", "markdown"]);
        let message = rejection(&argv).unwrap_or_else(|| panic!("{argv:?} is rejected"));
        assert_eq!(
            message,
            format!(
                "`fallow {}` writes the file of `--output-file` only with `--format {format}`. With another format it prints its output to stdout, so `--output-file` has no effect.",
                if args[0] == "rule-pack" {
                    "rule-pack list"
                } else {
                    args[0]
                }
            )
        );
    }
    assert!(rejection(&["agent", "status", "--output-file", "o.out"]).is_some());
    assert_eq!(
        rejection(&["agent", "status", "--format", "markdown"]),
        None
    );
}

#[test]
fn a_command_that_does_not_read_a_file_option_rejects_it() {
    let not_read = |command: &str, option: &str| {
        format!("`fallow {command}` does not read `{option}`, so the option has no effect.")
    };
    assert_eq!(
        rejection(&["dead-code", "--churn-file", "c.json"]),
        Some(not_read("dead-code", "--churn-file"))
    );
    assert_eq!(rejection(&["health", "--churn-file", "c.json"]), None);
    assert_eq!(rejection(&["--churn-file", "c.json"]), None);
    assert_eq!(
        rejection(&["explain", "unused-files", "--config", "f.json"]),
        Some(not_read("explain", "--config"))
    );
    assert_eq!(rejection(&["schema", "--root", "."]), None);
    assert_eq!(
        rejection(&["schema", "--config", "f.json"]),
        Some(not_read("schema", "--config"))
    );
    assert_eq!(
        rejection(&["audit-cache", "prune", "--config", "f.json"]),
        None
    );
    assert_eq!(
        rejection(&["impact", "statusline", "--diff-file", "d.patch"]),
        Some(not_read("impact statusline", "--diff-file"))
    );
    assert_eq!(
        rejection(&["dupes", "--type-aware-project", "tsconfig.json"]),
        Some(not_read("dupes", "--type-aware-project"))
    );
    assert_eq!(
        rejection(&["dead-code", "--type-aware-project", "tsconfig.json"]),
        None
    );
}

#[test]
fn only_the_bare_run_reads_the_global_duplication_overrides() {
    assert_eq!(rejection(&["--dupes-min-lines", "40"]), None);
    assert_eq!(
        rejection(&["audit", "--dupes-min-lines", "40"]).as_deref(),
        Some(
            "`fallow audit` does not read `--dupes-min-lines`. Set `duplicates.minLines` in the config, or pass `--dupes-min-lines` to bare `fallow`."
        )
    );
    assert_eq!(
        rejection(&["dupes", "--dupes-no-ignore-imports"]).as_deref(),
        Some(
            "`fallow dupes` does not read `--dupes-no-ignore-imports`. Use `fallow dupes --no-ignore-imports`, or set `duplicates.ignoreImports` in the config."
        )
    );
    for command in ["health", "dead-code", "fix", "watch"] {
        let message = rejection(&[command, "--dupes-mode", "semantic"])
            .unwrap_or_else(|| panic!("{command} rejects --dupes-mode"));
        assert!(message.contains("`duplicates.mode`"), "{message}");
    }
}

#[test]
fn ci_template_and_migrate_read_no_config_and_no_diff() {
    for command in [&["ci-template", "gitlab"][..], &["migrate", "--dry-run"]] {
        for option in ["--config", "--diff-file"] {
            let mut argv = command.to_vec();
            argv.extend([option, "x.json"]);
            assert_eq!(
                rejection(&argv),
                Some(format!(
                    "`fallow {}` does not read `{option}`, so the option has no effect.",
                    command[0]
                )),
                "{argv:?}"
            );
        }
    }
}

#[test]
fn viz_names_its_out_option_for_output_file() {
    assert_eq!(
        rejection(&["viz", "--no-open", "--output-file", "out.html"]).as_deref(),
        Some(
            "`fallow viz` writes the map to the path of `--out`, so `--output-file` has no effect. Use `fallow viz --out <PATH>`."
        )
    );
}
