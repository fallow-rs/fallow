use std::path::{Path, PathBuf};

use fallow_config::{
    CommandKind, EnvSnapshot, PlannedAnalysis, RunInputs, SelectionInput, Surface,
};
use fallow_types::sourced::{Given, Origin, Source};

use super::resolve::resolve_run;
use super::{BaseConfig, BaseSide, ResolvedRun, resolve_base_run};

const ORIGIN: Origin = Origin::Flag("--changed-since");

fn given<T>(value: T) -> Given<T> {
    Given {
        value,
        origin: ORIGIN,
    }
}

/// A root with a `.git` marker, a `package.json` and the given files.
fn root_with(dir: &Path, files: &[(&str, &str)]) -> PathBuf {
    std::fs::create_dir_all(dir.join(".git")).expect("git marker");
    std::fs::write(dir.join("package.json"), r#"{"name":"app"}"#).expect("package.json");
    for (name, content) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("file dir");
        std::fs::write(path, content).expect("project file");
    }
    dunce::canonicalize(dir).expect("canonical root")
}

/// A head root and a base root in one temporary directory.
fn head_and_base(
    head: &[(&str, &str)],
    base: &[(&str, &str)],
) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let head = root_with(&dir.path().join("head"), head);
    let base = root_with(&dir.path().join("base"), base);
    (dir, head, base)
}

fn head_run(root: &Path, config: Option<&str>) -> ResolvedRun {
    let mut inputs = RunInputs::new(
        Surface::Cli,
        EnvSnapshot::none(),
        CommandKind::Security,
        SelectionInput::Single(PlannedAnalysis::DeadCode),
    );
    inputs.root = Some(given(root.to_path_buf()));
    inputs.config_path = config.map(|path| given(root.join(path)));
    inputs.threads = Some(given(1));
    resolve_run(&inputs).expect("head run")
}

fn base_run(head: &ResolvedRun, base_root: &Path, config: BaseConfig) -> ResolvedRun {
    resolve_base_run(
        head,
        BaseSide {
            root: given(base_root.to_path_buf()),
            config,
            cache_dir: None,
            snapshot: fallow_config::AnalysisSnapshot::Base,
        },
    )
    .expect("base run")
}

fn dead_code_production(run: &ResolvedRun) -> bool {
    *run.production(PlannedAnalysis::DeadCode).value()
}

const PRODUCTION_ON: &str = r#"{"production": true}"#;
const PRODUCTION_OFF: &str = r#"{"production": false}"#;

#[test]
fn base_copy_uses_the_base_config() {
    let (_dir, head, base) = head_and_base(
        &[(".fallowrc.json", PRODUCTION_ON)],
        &[(".fallowrc.json", PRODUCTION_OFF)],
    );
    let head_run = head_run(&head, None);
    assert!(dead_code_production(&head_run));
    let run = base_run(&head_run, &base, BaseConfig::BaseCopy);
    assert!(!dead_code_production(&run));
    assert_eq!(
        run.config_file().value().as_deref(),
        Some(base.join(".fallowrc.json").as_path())
    );
    assert_eq!(run.root().value(), &base);
    assert_eq!(run.command(), CommandKind::Security);
}

#[test]
fn head_reuses_the_head_config() {
    let (_dir, head, base) = head_and_base(
        &[(".fallowrc.json", PRODUCTION_ON)],
        &[(".fallowrc.json", PRODUCTION_OFF)],
    );
    let head_run = head_run(&head, None);
    let run = base_run(&head_run, &base, BaseConfig::Head);
    assert!(dead_code_production(&run));
    assert_eq!(
        run.config_file().value().as_deref(),
        Some(head.join(".fallowrc.json").as_path())
    );
    assert_eq!(
        run.config_file().source(),
        &Source::Derived {
            from: "head-config"
        }
    );
}

#[test]
fn base_copy_of_an_explicit_config_reads_the_file_under_the_base_root() {
    let (_dir, head, base) = head_and_base(
        &[("conf/fallow.json", PRODUCTION_ON)],
        &[
            ("conf/fallow.json", PRODUCTION_OFF),
            (".fallowrc.json", PRODUCTION_ON),
        ],
    );
    let head_run = head_run(&head, Some("conf/fallow.json"));
    let run = base_run(&head_run, &base, BaseConfig::BaseCopy);
    assert!(!dead_code_production(&run));
    assert_eq!(
        run.config_file().value().as_deref(),
        Some(base.join("conf/fallow.json").as_path())
    );
}

#[test]
fn base_copy_without_a_base_version_discovers_the_base_config() {
    let (_dir, head, base) = head_and_base(
        &[("conf/fallow.json", PRODUCTION_OFF)],
        &[(".fallowrc.json", PRODUCTION_ON)],
    );
    let head_run = head_run(&head, Some("conf/fallow.json"));
    let run = base_run(&head_run, &base, BaseConfig::BaseCopy);
    assert!(dead_code_production(&run));
    assert_eq!(
        run.config_file().source(),
        &Source::Derived { from: "discovery" }
    );
}

#[test]
fn head_without_a_config_file_discovers_the_base_config() {
    let (_dir, head, base) = head_and_base(&[], &[(".fallowrc.json", PRODUCTION_ON)]);
    let head_run = head_run(&head, None);
    let run = base_run(&head_run, &base, BaseConfig::Head);
    assert!(dead_code_production(&run));
}

#[test]
fn a_base_run_keeps_the_given_inputs_and_is_quiet() {
    let (_dir, head, base) = head_and_base(&[], &[(".fallowrc.json", PRODUCTION_OFF)]);
    let mut inputs = RunInputs::new(
        Surface::Cli,
        EnvSnapshot::none(),
        CommandKind::Audit,
        SelectionInput::Lists {
            only: vec![PlannedAnalysis::DeadCode],
            skip: Vec::new(),
            origin: Origin::Flag("--only"),
        },
    );
    inputs.root = Some(given(head));
    inputs.threads = Some(given(3));
    inputs.production = Some(given(true));
    let head_run = resolve_run(&inputs).expect("head run");
    let cache = base.join(".fallow");
    let run = resolve_base_run(
        &head_run,
        BaseSide {
            root: given(base),
            config: BaseConfig::BaseCopy,
            cache_dir: Some(given(cache.clone())),
            snapshot: fallow_config::AnalysisSnapshot::Base,
        },
    )
    .expect("base run");
    assert!(
        dead_code_production(&run),
        "the flag wins over the base config"
    );
    assert_eq!(*run.settings().threads.value(), 3);
    assert_eq!(run.settings().cache_dir.value(), &cache);
    assert!(run.settings().quiet);
    assert_eq!(run.command(), CommandKind::Audit);
    assert!(matches!(
        run.analysis(PlannedAnalysis::Duplication),
        super::AnalysisSlot::Skipped { .. }
    ));
}
