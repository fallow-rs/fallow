//! The base side of an audit keeps the plan and the production modes of the
//! head run, and keeps its caches in the worktree.

use std::path::{Path, PathBuf};

use fallow_config::{
    CommandKind, EnvSnapshot, PlannedAnalysis, RunInputs, SelectionInput, Surface,
};
use fallow_engine::resolved_run::{AnalysisSlot, ResolvedRun, resolve_run};
use fallow_types::sourced::{Given, Origin};

use super::{own_config_run, worktree_side_run};

/// A root with a `.git` marker, a `package.json` and the given files.
fn root_with(dir: &Path, files: &[(&str, &str)]) -> PathBuf {
    std::fs::create_dir_all(dir.join(".git")).expect("git marker");
    std::fs::write(dir.join("package.json"), r#"{"name":"app"}"#).expect("package.json");
    for (name, content) in files {
        std::fs::write(dir.join(name), content).expect("project file");
    }
    dunce::canonicalize(dir).expect("canonical root")
}

/// The head run of `fallow audit` without `--only` or `--skip`.
fn audit_head(root: &Path, env: &[(&str, &str)]) -> ResolvedRun {
    let mut inputs = RunInputs::new(
        Surface::Cli,
        EnvSnapshot::from_pairs(env, Some(root)),
        CommandKind::Audit,
        SelectionInput::Lists {
            only: Vec::new(),
            skip: Vec::new(),
            origin: Origin::Flag("--only"),
        },
    );
    inputs.root = Some(Given {
        value: root.to_path_buf(),
        origin: Origin::Flag("--root"),
    });
    inputs.threads = Some(Given {
        value: 1,
        origin: Origin::Flag("--threads"),
    });
    resolve_run(&inputs).expect("head run")
}

fn runs(run: &ResolvedRun, analysis: PlannedAnalysis) -> bool {
    matches!(run.analysis(analysis), AnalysisSlot::Runs(_))
}

fn production(run: &ResolvedRun, analysis: PlannedAnalysis) -> bool {
    *run.production(analysis).value()
}

#[test]
fn the_worktree_side_keeps_the_head_plan_and_production_modes() {
    let dir = tempfile::tempdir().expect("temp dir");
    let head = root_with(&dir.path().join("head"), &[]);
    let base = root_with(
        &dir.path().join("base"),
        &[(
            ".fallowrc.json",
            r#"{"duplicates":{"enabled":false},"production":true}"#,
        )],
    );
    let head_run = audit_head(&head, &[]);
    assert!(runs(&head_run, PlannedAnalysis::Duplication));

    let run = worktree_side_run(
        &head_run,
        &base,
        true,
        "--base",
        fallow_config::AnalysisSnapshot::Base,
    )
    .expect("base run");

    assert_eq!(
        run.config_file().value().as_deref(),
        Some(base.join(".fallowrc.json").as_path()),
        "a head run without a config file discovers the base config"
    );
    for analysis in [
        PlannedAnalysis::DeadCode,
        PlannedAnalysis::Duplication,
        PlannedAnalysis::Health,
    ] {
        assert_eq!(
            runs(&run, analysis),
            runs(&head_run, analysis),
            "{analysis:?}"
        );
        assert_eq!(
            production(&run, analysis),
            production(&head_run, analysis),
            "{analysis:?}"
        );
    }
}

#[test]
fn the_own_config_side_runs_with_the_head_dead_code_mode() {
    let dir = tempfile::tempdir().expect("temp dir");
    let head = root_with(
        &dir.path().join("head"),
        &[(".fallowrc.json", r#"{"production":false}"#)],
    );
    let base = root_with(
        &dir.path().join("base"),
        &[(".fallowrc.json", r#"{"production":true}"#)],
    );
    let head_run = audit_head(&head, &[]);

    let run = own_config_run(&head_run, &base, true).expect("the own-config pass runs");

    assert_eq!(
        run.config_file().value().as_deref(),
        Some(base.join(".fallowrc.json").as_path())
    );
    assert!(!production(&run, PlannedAnalysis::DeadCode));
}

#[test]
fn the_own_config_side_keeps_its_caches_in_the_worktree() {
    let dir = tempfile::tempdir().expect("temp dir");
    let head = root_with(&dir.path().join("head"), &[(".fallowrc.json", "{}")]);
    let base = root_with(&dir.path().join("base"), &[(".fallowrc.json", "{}")]);
    let shared = dir.path().join("shared-cache");
    let shared_text = shared.to_str().expect("utf-8 path");
    let head_run = audit_head(&head, &[("FALLOW_CACHE_DIR", shared_text)]);
    assert_eq!(head_run.settings().cache_dir.value(), &shared);

    let run = own_config_run(&head_run, &base, false).expect("the own-config pass runs");

    assert_eq!(
        run.settings().cache_dir.value(),
        &super::super::worktree_cache::cache_dir(&base)
    );
}
