//! The mapping of [`command_run_inputs`] from a parsed command line to the
//! fields of `RunInputs` that the run parity cases do not set: the
//! per-analysis production flags, `--only` and `--skip`, `--brief`, and the
//! config and setting flags.

use std::path::PathBuf;

use clap::Parser as _;
use fallow_config::{EnvSnapshot, PlannedAnalysis, RunInputs, SelectionInput};
use fallow_types::sourced::{Given, Origin};

use super::command_run_inputs;
use crate::cli_args::Cli;

fn inputs(argv: &[&str]) -> RunInputs {
    let cli = Cli::try_parse_from(argv).expect("argv parses");
    command_run_inputs(&cli, EnvSnapshot::none()).expect("the command loads a config")
}

const fn flag<T>(value: T, name: &'static str) -> Given<T> {
    Given {
        value,
        origin: Origin::Flag(name),
    }
}

/// The per-analysis production inputs as `[dead_code, health, dupes]`.
fn per_analysis(inputs: &RunInputs) -> [Option<Given<bool>>; 3] {
    [
        inputs.production_dead_code,
        inputs.production_health,
        inputs.production_dupes,
    ]
}

#[test]
fn each_per_analysis_production_flag_sets_its_own_analysis() {
    let cases = [
        (
            "--production-dead-code",
            [Some(flag(true, "--production-dead-code")), None, None],
        ),
        (
            "--production-health",
            [None, Some(flag(true, "--production-health")), None],
        ),
        (
            "--production-dupes",
            [None, None, Some(flag(true, "--production-dupes"))],
        ),
    ];
    for (name, expected) in cases {
        assert_eq!(
            per_analysis(&inputs(&["fallow", "audit", name])),
            expected,
            "audit {name}"
        );
        assert_eq!(
            per_analysis(&inputs(&["fallow", name])),
            expected,
            "bare {name}"
        );
    }
    assert_eq!(
        per_analysis(&inputs(&["fallow", "health", "--production"])),
        [None, None, None]
    );
}

fn lists(
    only: &[PlannedAnalysis],
    skip: &[PlannedAnalysis],
    origin: &'static str,
) -> SelectionInput {
    SelectionInput::Lists {
        only: only.to_vec(),
        skip: skip.to_vec(),
        origin: Origin::Flag(origin),
    }
}

#[test]
fn only_and_skip_select_the_analyses_of_the_bare_run_and_of_audit() {
    use PlannedAnalysis::{DeadCode, Duplication, Health};
    let cases: [(&[&str], SelectionInput); 6] = [
        (
            &["fallow", "--only", "dupes"],
            lists(&[Duplication], &[], "--only"),
        ),
        (
            &["fallow", "--skip", "health"],
            lists(&[], &[Health], "--skip"),
        ),
        (
            &["fallow", "--only", "architecture"],
            lists(&[DeadCode], &[], "--only"),
        ),
        (
            &["fallow", "audit", "--only", "health"],
            lists(&[Health], &[], "--only"),
        ),
        (
            &["fallow", "audit", "--skip", "dupes"],
            lists(&[], &[Duplication], "--skip"),
        ),
        (&["fallow", "audit"], lists(&[], &[], "--skip")),
    ];
    for (argv, expected) in cases {
        assert_eq!(inputs(argv).selection, expected, "{argv:?}");
    }
}

#[test]
fn the_audit_brief_names_every_analysis() {
    use PlannedAnalysis::{DeadCode, Duplication, Health};
    let every = lists(&[DeadCode, Duplication, Health], &[], "--brief");
    for argv in [
        ["fallow", "audit", "--brief"].as_slice(),
        &["fallow", "audit", "--brief", "--skip", "dupes"],
        &["fallow", "audit", "--walkthrough-guide"],
    ] {
        assert_eq!(inputs(argv).selection, every, "{argv:?}");
    }
}

#[test]
fn the_config_and_setting_flags_reach_the_run_inputs() {
    let given = inputs(&[
        "fallow",
        "dead-code",
        "--root",
        "/project",
        "--config",
        "custom.json",
        "--allow-remote-extends",
        "--no-cache",
        "--max-file-size",
        "7",
        "--no-package-baselines",
        "--include-entry-exports",
    ]);
    assert_eq!(given.root, Some(flag(PathBuf::from("/project"), "--root")));
    assert_eq!(
        given.config_path,
        Some(flag(PathBuf::from("custom.json"), "--config"))
    );
    assert_eq!(
        given.allow_remote_extends,
        Some(flag(true, "--allow-remote-extends"))
    );
    assert_eq!(given.no_cache, Some(flag(true, "--no-cache")));
    assert_eq!(given.max_file_size_mb, Some(flag(7, "--max-file-size")));
    assert_eq!(
        given.package_baselines,
        Some(flag(false, "--no-package-baselines"))
    );
    assert_eq!(
        given.include_entry_exports,
        Some(flag(true, "--include-entry-exports"))
    );

    let default = inputs(&["fallow", "dead-code"]);
    assert_eq!(default.config_path, None);
    assert_eq!(default.allow_remote_extends, None);
    assert_eq!(default.no_cache, None);
    assert_eq!(default.max_file_size_mb, None);
    assert_eq!(default.package_baselines, None);
    assert_eq!(default.include_entry_exports, None);
}

#[test]
fn workspaces_resolves_the_run_of_list() {
    let workspaces = inputs(&["fallow", "workspaces", "--no-production"]);
    assert_eq!(workspaces.command, fallow_config::CommandKind::List);
    assert_eq!(workspaces.production, Some(flag(false, "--no-production")));
}

/// `--include-entry-exports` reaches the resolved config of each analysis of
/// a bare run, not only the dead-code config: the resolver applies the given
/// settings to each mode entry.
#[test]
fn include_entry_exports_reaches_every_analysis_config() {
    let dir = tempfile::tempdir().expect("tempdir");
    std::fs::write(dir.path().join("package.json"), "{}").expect("package.json");
    std::fs::write(
        dir.path().join(".fallowrc.json"),
        r#"{"production": {"deadCode": false, "health": true, "dupes": true}}"#,
    )
    .expect("config");
    let root = dir.path().to_string_lossy().into_owned();
    let mut inputs = inputs(&["fallow", "--include-entry-exports", "--root", &root]);
    inputs.threads = Some(flag(1, "--threads"));
    let run = fallow_engine::resolved_run::resolve_run(&inputs).expect("run resolves");
    for analysis in [
        PlannedAnalysis::DeadCode,
        PlannedAnalysis::Duplication,
        PlannedAnalysis::Health,
    ] {
        let runnable = crate::cli_production::runnable(&run, analysis).expect("analysis runs");
        assert!(
            runnable.config().include_entry_exports,
            "{analysis:?} config keeps the flag"
        );
    }
}
