use std::path::{Path, PathBuf};

use fallow_config::{CommandKind, EnvSnapshot, PlannedAnalysis, SelectionInput, Surface};
use fallow_engine::resolved_run::{EmptyCause, InputField, InputProblem, ResolveError};
use fallow_types::sourced::{Origin, Source};

use super::*;

fn project(config: Option<&str>) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dunce::canonicalize(dir.path()).expect("canonical root");
    std::fs::create_dir(root.join(".git")).expect("git marker");
    std::fs::write(root.join("package.json"), r#"{"name":"app"}"#).expect("package.json");
    if let Some(config) = config {
        std::fs::write(root.join(".fallowrc.json"), config).expect("config");
    }
    (dir, root)
}

fn options(root: &Path, surface: Surface, env: &[(&str, &str)]) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(root.to_path_buf()),
        threads: Some(1),
        host: Some(HostInputs {
            surface,
            env: EnvSnapshot::from_pairs(env, Some(root)),
        }),
        ..AnalysisOptions::default()
    }
}

const fn combined() -> ProgrammaticEntry {
    ProgrammaticEntry::Combined {
        dead_code: true,
        duplication: true,
        health: true,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
    }
}

fn audit(production: bool) -> ProgrammaticEntry {
    ProgrammaticEntry::Audit {
        dead_code: None,
        duplication: None,
        health: None,
        production,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
    }
}

fn error_pair(err: &ResolveError) -> (Option<String>, Option<String>) {
    let err = programmatic_error(err);
    assert_eq!(err.exit_code, 2);
    (err.code, err.context)
}

fn pair(code: &str, context: &str) -> (Option<String>, Option<String>) {
    (Some(code.to_owned()), Some(context.to_owned()))
}

#[test]
fn each_resolve_error_maps_to_the_code_of_the_runners() {
    let input = |field, problem| ResolveError::Input {
        field,
        problem,
        message: "message".to_owned(),
    };
    assert_eq!(
        error_pair(&input(InputField::Root, InputProblem::CwdUnavailable)),
        pair("FALLOW_CWD_UNAVAILABLE", "analysis.root")
    );
    assert_eq!(
        error_pair(&input(InputField::Root, InputProblem::Invalid)),
        pair("FALLOW_INVALID_ROOT", "analysis.root")
    );
    assert_eq!(
        error_pair(&input(InputField::ConfigPath, InputProblem::Invalid)),
        pair("FALLOW_INVALID_CONFIG_PATH", "analysis.configPath")
    );
    assert_eq!(
        error_pair(&input(InputField::Threads, InputProblem::Zero)),
        pair("FALLOW_INVALID_THREADS", "analysis.threads")
    );
    for err in [
        ResolveError::ConfigInvalid {
            file: None,
            message: "invalid rule pack".to_owned(),
        },
        ResolveError::Workspace {
            message: "bad manifest".to_owned(),
        },
    ] {
        assert_eq!(
            error_pair(&err),
            pair("FALLOW_CONFIG_LOAD_FAILED", "analysis.configPath")
        );
    }
    assert_eq!(
        error_pair(&ResolveError::NothingSelected {
            command: CommandKind::Combined,
            cause: EmptyCause::Request,
        }),
        pair("FALLOW_COMBINED_EMPTY", "combined")
    );
    assert_eq!(
        error_pair(&ResolveError::NothingSelected {
            command: CommandKind::Audit,
            cause: EmptyCause::Request,
        }),
        pair("FALLOW_AUDIT_NO_ANALYSIS", "audit")
    );
}

#[test]
fn a_config_load_error_has_the_text_of_the_api_loader() {
    let (_dir, root) = project(Some(r#"{ "rules": "#));
    let err = resolve_programmatic_run(&options(&root, Surface::Api, &[]), &combined())
        .expect_err("broken config");
    let detail = fallow_config::FallowConfig::find_and_load(&root).expect_err("broken config");
    assert_eq!(err.message, format!("failed to load config: {detail}"));
    assert_eq!(err.code.as_deref(), Some("FALLOW_CONFIG_LOAD_FAILED"));
}

#[test]
fn each_entry_has_one_command_and_selection() {
    let single = |analysis| SelectionInput::Single(analysis);
    let cases = [
        (
            ProgrammaticEntry::DeadCode,
            CommandKind::DeadCode,
            single(PlannedAnalysis::DeadCode),
        ),
        (
            ProgrammaticEntry::Duplication,
            CommandKind::Dupes,
            single(PlannedAnalysis::Duplication),
        ),
        (
            ProgrammaticEntry::Health,
            CommandKind::Health,
            single(PlannedAnalysis::Health),
        ),
        (
            ProgrammaticEntry::TracePath,
            CommandKind::TracePath,
            single(PlannedAnalysis::DeadCode),
        ),
        (
            ProgrammaticEntry::Doctor,
            CommandKind::Doctor,
            single(PlannedAnalysis::DeadCode),
        ),
    ];
    for (entry, command, selection) in cases {
        assert_eq!(entry.command(), command, "{entry:?}");
        assert_eq!(entry.selection(), selection, "{entry:?}");
    }
    assert!(matches!(
        audit(false).selection(),
        SelectionInput::Options {
            origin: Origin::ApiOption("audit"),
            ..
        }
    ));
}

#[test]
fn the_host_names_the_surface_and_the_origin() {
    let (_dir, root) = project(None);
    let mut node = options(&root, Surface::Node, &[]);
    node.production_override = Some(true);
    let run = resolve_programmatic_run(&node, &ProgrammaticEntry::DeadCode).expect("run");
    assert_eq!(run.surface(), Surface::Node);
    assert_eq!(
        run.production(PlannedAnalysis::DeadCode).source(),
        &Source::Given {
            origin: Origin::NodeOption("production")
        }
    );

    let mut api = options(&root, Surface::Api, &[]);
    api.production = true;
    let run = resolve_programmatic_run(&api, &ProgrammaticEntry::DeadCode).expect("run");
    assert_eq!(run.surface(), Surface::Api);
    assert_eq!(
        run.production(PlannedAnalysis::DeadCode).source(),
        &Source::Given {
            origin: Origin::ApiOption("analysis.production")
        }
    );

    let mut mcp = options(&root, Surface::Mcp, &[]);
    mcp.production_override = Some(false);
    let run = resolve_programmatic_run(&mcp, &ProgrammaticEntry::DeadCode).expect("run");
    assert_eq!(run.surface(), Surface::Mcp);
    assert_eq!(
        run.production(PlannedAnalysis::DeadCode).source(),
        &Source::Given {
            origin: Origin::ToolParam("production")
        }
    );
}

/// The run of `run_audit` for `audit`, through the same steps.
fn audit_run(audit: &crate::AuditOptions) -> ResolvedRun {
    let analysis = crate::runtime::analysis_options_for_audit(audit, "HEAD");
    crate::runtime::resolve_audit_run(audit, &analysis).expect("run")
}

#[test]
fn audit_reads_audit_production_and_not_the_legacy_option() {
    let (_dir, root) = project(None);
    let mut audit = crate::AuditOptions {
        analysis: options(&root, Surface::Api, &[]),
        ..crate::AuditOptions::default()
    };
    audit.analysis.production = true;
    let run = audit_run(&audit);
    assert!(!*run.production(PlannedAnalysis::DeadCode).value());

    audit.production = true;
    let run = audit_run(&audit);
    assert!(*run.production(PlannedAnalysis::DeadCode).value());
    assert_eq!(
        run.production(PlannedAnalysis::DeadCode).source(),
        &Source::Given {
            origin: Origin::ApiOption("audit.production")
        }
    );

    audit.analysis.production_override = Some(false);
    let run = audit_run(&audit);
    assert!(!*run.production(PlannedAnalysis::DeadCode).value());
    assert_eq!(
        run.production(PlannedAnalysis::DeadCode).source(),
        &Source::Given {
            origin: Origin::ApiOption("analysis.production_override")
        }
    );
}

#[test]
fn the_config_only_entries_ignore_the_production_options() {
    let (_dir, root) = project(None);
    let mut analysis = options(&root, Surface::Api, &[]);
    analysis.production_override = Some(true);
    for entry in [
        ProgrammaticEntry::Mutation,
        ProgrammaticEntry::SimilarCode,
        ProgrammaticEntry::Doctor,
    ] {
        let run = resolve_programmatic_run(&analysis, &entry).expect("run");
        assert!(
            !*run.production(PlannedAnalysis::DeadCode).value(),
            "{entry:?}"
        );
    }
}

#[test]
fn the_api_reads_no_production_variable() {
    let (_dir, root) = project(None);
    let analysis = options(&root, Surface::Api, &[("FALLOW_PRODUCTION", "1")]);
    let run = resolve_programmatic_run(&analysis, &ProgrammaticEntry::DeadCode).expect("run");
    assert!(!*run.production(PlannedAnalysis::DeadCode).value());
    assert_eq!(run.env_ignored().len(), 1);
}

#[test]
fn a_combined_call_with_duplicates_off_skips_duplication_by_config() {
    let (_dir, root) = project(Some(r#"{ "duplicates": { "enabled": false } }"#));
    let run =
        resolve_programmatic_run(&options(&root, Surface::Api, &[]), &combined()).expect("run");
    let AnalysisSlot::Skipped { reason } = run.analysis(PlannedAnalysis::Duplication) else {
        panic!("duplication is off");
    };
    assert!(matches!(
        reason.source(),
        Source::ConfigFile {
            key: "duplicates.enabled",
            ..
        }
    ));
}

#[test]
fn production_modes_follow_the_per_analysis_config() {
    let (_dir, root) = project(Some(
        r#"{"production":{"deadCode":false,"health":true,"dupes":false}}"#,
    ));
    let run =
        resolve_programmatic_run(&options(&root, Surface::Api, &[]), &combined()).expect("run");
    let modes = run.production_modes();
    assert!(!modes.dead_code);
    assert!(modes.health);
    assert!(!modes.dupes);
}
