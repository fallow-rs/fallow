use std::path::{Path, PathBuf};

use fallow_config::{
    AnalysisSelection, CommandKind, ConfigFallback, ConfigKey, EnvSnapshot, PlannedAnalysis,
    RunInputs, SelectionInput, SkipReason, Surface,
};
use fallow_types::sourced::{Given, Origin, Source};

use super::resolve::{InputField, InputProblem, ResolveError, resolve_run};
use super::{AnalysisSlot, ResolvedRun};
use crate::project_config::ProductionModes;

const ORIGIN: Origin = Origin::Flag("--test");

const fn given<T>(value: T) -> Given<T> {
    Given {
        value,
        origin: ORIGIN,
    }
}

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

fn combined(root: &Path) -> RunInputs {
    let mut inputs = RunInputs::new(
        Surface::Cli,
        EnvSnapshot::none().with_cwd(root.to_path_buf()),
        CommandKind::Combined,
        SelectionInput::Lists {
            only: Vec::new(),
            skip: Vec::new(),
            origin: ORIGIN,
        },
    );
    inputs.threads = Some(given(2));
    inputs
}

fn resolve(inputs: &RunInputs) -> ResolvedRun {
    resolve_run(inputs).expect("the run resolves")
}

const SPLIT_CONFIG: &str = r#"{
  "production": { "deadCode": true, "health": true, "dupes": false },
  "duplicates": { "enabled": true }
}"#;

#[test]
fn the_run_keeps_the_surface_the_command_and_the_root() {
    let (_dir, root) = project(None);
    let run = resolve(&combined(&root));
    assert_eq!(run.surface(), Surface::Cli);
    assert_eq!(run.command(), CommandKind::Combined);
    assert_eq!(run.root().value(), &root);
    assert_eq!(
        run.root().source(),
        &Source::Default {
            rule: "current-dir"
        }
    );
    assert_eq!(
        run.config_file().source(),
        &Source::Default { rule: "no-config" }
    );
}

#[test]
fn each_production_mode_gets_its_own_config() {
    let (_dir, root) = project(Some(SPLIT_CONFIG));
    let run = resolve(&combined(&root));
    assert_eq!(
        run.production_modes(),
        ProductionModes {
            dead_code: true,
            health: true,
            dupes: false,
        }
    );
    let runnable = |analysis| match run.analysis(analysis) {
        AnalysisSlot::Runs(runnable) => runnable,
        AnalysisSlot::Skipped { .. } => panic!("{analysis:?} runs"),
    };
    let dead_code = runnable(PlannedAnalysis::DeadCode);
    let dupes = runnable(PlannedAnalysis::Duplication);
    let health = runnable(PlannedAnalysis::Health);
    assert_eq!(dead_code.analysis(), PlannedAnalysis::DeadCode);
    assert!(dead_code.config().production);
    assert!(!dupes.config().production);
    assert!(dead_code.shares_files_with(health));
    assert!(!dead_code.shares_files_with(dupes));
    assert_eq!(
        run.config().production,
        fallow_config::ProductionConfig::PerAnalysis(fallow_config::PerAnalysisProductionConfig {
            dead_code: true,
            health: true,
            dupes: false,
            ignore: Vec::new(),
        }),
        "the run keeps the config before the production mode is flattened"
    );
    let session = dead_code.open_session();
    assert!(session.config().production);
    assert_eq!(
        session.config_path(),
        Some(root.join(".fallowrc.json").as_path())
    );
}

#[test]
fn a_skipped_analysis_has_its_config_key() {
    let (_dir, root) = project(Some(r#"{ "duplicates": { "enabled": false } }"#));
    let run = resolve(&combined(&root));
    let AnalysisSlot::Skipped { reason } = run.analysis(PlannedAnalysis::Duplication) else {
        panic!("duplication is off");
    };
    assert_eq!(
        reason.value(),
        &AnalysisSelection::Skipped(SkipReason::Config(ConfigKey::DuplicatesEnabled))
    );
    assert_eq!(
        reason.source(),
        &Source::ConfigFile {
            file: root.join(".fallowrc.json"),
            key: "duplicates.enabled",
        }
    );
}

#[test]
fn an_adjustment_of_a_decided_field_fails() {
    let (_dir, root) = project(None);
    let run = resolve(&combined(&root));
    let AnalysisSlot::Runs(runnable) = run.analysis(PlannedAnalysis::DeadCode) else {
        panic!("dead code runs");
    };
    let session = runnable
        .open_session_adjusted(|config| config.show_cascade = true)
        .expect("a rule setting can change");
    assert!(session.config().show_cascade);
    let err = runnable
        .open_session_adjusted(|config| config.production = !config.production)
        .expect_err("the production mode is decided");
    assert!(err.message().contains("`production`"), "{err}");
    let err = runnable
        .open_session_adjusted(|config| config.threads += 1)
        .expect_err("the thread count is decided");
    assert!(err.message().contains("`threads`"), "{err}");
}

#[test]
fn the_view_names_each_decision_and_no_env_value() {
    let (_dir, root) = project(Some(SPLIT_CONFIG));
    let env = EnvSnapshot::from_pairs(
        &[
            ("FALLOW_PRODUCTION_DUPES", "true"),
            ("FALLOW_CHANGED_SINCE", "secret-ref"),
        ],
        Some(&root),
    );
    let mut with_env = RunInputs::new(
        Surface::Cli,
        env,
        CommandKind::Combined,
        combined(&root).selection,
    );
    with_env.threads = Some(given(2));
    with_env.include_entry_exports = Some(given(true));
    let view = serde_json::to_value(resolve(&with_env).view()).expect("view serializes");
    assert_eq!(view["schema_version"], 1);
    assert_eq!(view["surface"], "cli");
    assert_eq!(view["command"], "combined");
    assert_eq!(
        view["analyses"]["duplication"]["production"]["source"],
        serde_json::json!({ "kind": "environment", "name": "FALLOW_PRODUCTION_DUPES" })
    );
    assert_eq!(
        view["analyses"]["dead_code"]["production"]["source"]["key"],
        "production.deadCode"
    );
    assert_eq!(view["settings"]["threads"]["value"], 2);
    assert_eq!(
        view["settings"]["include_entry_exports"]["source"],
        serde_json::json!({ "kind": "given", "origin": { "kind": "flag", "name": "--test" } })
    );
    assert_eq!(view["settings"]["allow_remote_extends"]["value"], false);
    assert_eq!(view["settings"]["package_baselines"]["value"], true);
    assert_eq!(
        view["env"],
        serde_json::json!({
            "read": ["FALLOW_PRODUCTION_DUPES"],
            "ignored": ["FALLOW_CHANGED_SINCE"],
        })
    );
    assert!(!view.to_string().contains("secret-ref"));
    assert_eq!(view["fell_back"], false);
}

#[test]
fn plugin_readiness_names_a_missing_plugin_file() {
    let (_dir, root) = project(Some(r#"{ "plugins": ["plugins/missing.json"] }"#));
    let mut inputs = combined(&root);
    inputs.config_fallback = ConfigFallback::Fail;
    let result = resolve_run(&inputs);
    let run = match result {
        Ok(run) => run,
        Err(ResolveError::ConfigInvalid { file, message }) => {
            panic!("{file:?}: {message}")
        }
        Err(err) => panic!("{err:?}"),
    };
    assert_eq!(run.plugin_readiness().len(), 1);
}

#[test]
fn input_errors_name_the_field() {
    let (_dir, root) = project(None);
    let mut inputs = combined(&root);
    inputs.threads = Some(given(0));
    assert!(matches!(
        resolve_run(&inputs),
        Err(ResolveError::Input {
            field: InputField::Threads,
            problem: InputProblem::Zero,
            ..
        })
    ));

    let inputs = RunInputs::new(
        Surface::Api,
        EnvSnapshot::none(),
        CommandKind::Combined,
        SelectionInput::Single(PlannedAnalysis::DeadCode),
    );
    assert!(matches!(
        resolve_run(&inputs),
        Err(ResolveError::Input {
            field: InputField::Root,
            problem: InputProblem::CwdUnavailable,
            ..
        })
    ));

    let mut inputs = combined(&root);
    inputs.root = Some(given(root.join("missing")));
    assert!(matches!(
        resolve_run(&inputs),
        Err(ResolveError::Input {
            field: InputField::Root,
            problem: InputProblem::Invalid,
            ..
        })
    ));

    let mut inputs = RunInputs::new(
        Surface::Api,
        EnvSnapshot::none(),
        CommandKind::Combined,
        SelectionInput::Single(PlannedAnalysis::DeadCode),
    );
    inputs.root = Some(given(root));
    inputs.config_path = Some(given(PathBuf::from("custom.json")));
    assert!(matches!(
        resolve_run(&inputs),
        Err(ResolveError::Input {
            field: InputField::ConfigPath,
            problem: InputProblem::CwdUnavailable,
            ..
        })
    ));
}

const MB: u64 = 1024 * 1024;

/// A combined CLI run of `root` with the variables of `pairs`.
fn combined_with_env(root: &Path, pairs: &[(&str, &str)]) -> RunInputs {
    let mut inputs = RunInputs::new(
        Surface::Cli,
        EnvSnapshot::from_pairs(pairs, Some(root)),
        CommandKind::Combined,
        combined(root).selection,
    );
    inputs.threads = Some(given(2));
    inputs
}

fn runnable(run: &ResolvedRun, analysis: PlannedAnalysis) -> super::Runnable<'_> {
    match run.analysis(analysis) {
        AnalysisSlot::Runs(runnable) => runnable,
        AnalysisSlot::Skipped { .. } => panic!("{analysis:?} runs"),
    }
}

/// The config that a dead-code session resolves for duplication, which runs
/// in another production mode.
fn own_duplication_config(run: &ResolvedRun) -> fallow_config::ResolvedConfig {
    let session = runnable(run, PlannedAnalysis::DeadCode).open_session();
    session
        .own_duplication_config()
        .expect("duplication has its own production mode")
        .clone()
}

#[test]
fn the_own_duplication_scope_keeps_the_given_settings() {
    let (_dir, root) = project(Some(SPLIT_CONFIG));
    let mut inputs = combined_with_env(&root, &[("FALLOW_MAX_FILE_SIZE", "5")]);
    inputs.max_file_size_mb = Some(given(1));
    inputs.cache_dir = Some(given(PathBuf::from("given-cache")));
    inputs.include_entry_exports = Some(given(true));
    let run = resolve(&inputs);
    let dupes = runnable(&run, PlannedAnalysis::Duplication);
    assert_eq!(dupes.config().max_file_size_bytes, Some(MB));
    let own = own_duplication_config(&run);
    assert!(!own.production);
    assert_eq!(own.max_file_size_bytes, Some(MB));
    assert_eq!(own.cache_dir, root.join("given-cache"));
    assert!(own.include_entry_exports);
    assert_eq!(own.cache_dir, dupes.config().cache_dir);
}

#[test]
fn the_own_duplication_scope_reads_the_environment() {
    let (_dir, root) = project(Some(SPLIT_CONFIG));
    let run = resolve(&combined_with_env(
        &root,
        &[
            ("FALLOW_MAX_FILE_SIZE", "5"),
            ("FALLOW_CACHE_MAX_SIZE", "33"),
        ],
    ));
    let own = own_duplication_config(&run);
    assert_eq!(own.max_file_size_bytes, Some(5 * MB));
    assert_eq!(own.cache_max_size_mb, Some(33));
}

/// A split production config with a package baseline.
const BASELINE_CONFIG: &str = r#"{
  "production": { "deadCode": true, "health": true, "dupes": false },
  "workspaces": { "changedSince": { "packages/web": "main" } }
}"#;

fn baselines(run: &ResolvedRun) -> Vec<String> {
    runnable(run, PlannedAnalysis::DeadCode)
        .config()
        .workspace_changed_since
        .keys()
        .cloned()
        .collect()
}

#[test]
fn a_given_package_baselines_wins_over_the_environment() {
    let web = vec!["packages/web".to_owned()];
    let (_dir, root) = project(Some(BASELINE_CONFIG));
    let off = [("FALLOW_PACKAGE_BASELINES", "off")];

    let run = resolve(&combined_with_env(&root, &off));
    assert!(!*run.settings().package_baselines.value());
    assert!(baselines(&run).is_empty(), "the variable clears the map");

    let mut inputs = combined_with_env(&root, &off);
    inputs.package_baselines = Some(given(true));
    let run = resolve(&inputs);
    assert!(*run.settings().package_baselines.value());
    assert_eq!(baselines(&run), web);
    assert_eq!(
        own_duplication_config(&run)
            .workspace_changed_since
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        web
    );

    let mut inputs = combined_with_env(&root, &[]);
    inputs.package_baselines = Some(given(false));
    let run = resolve(&inputs);
    assert!(baselines(&run).is_empty());
    assert!(
        own_duplication_config(&run)
            .workspace_changed_since
            .is_empty()
    );
}

#[test]
fn a_selection_error_does_not_fall_back() {
    let (_dir, root) = project(None);
    let mut inputs = combined(&root);
    inputs.config_fallback = ConfigFallback::DefaultsOnDiscoveredError;
    inputs.selection = SelectionInput::Lists {
        only: Vec::new(),
        skip: vec![
            PlannedAnalysis::DeadCode,
            PlannedAnalysis::Duplication,
            PlannedAnalysis::Health,
        ],
        origin: ORIGIN,
    };
    assert!(matches!(
        resolve_run(&inputs),
        Err(ResolveError::NothingSelected { .. })
    ));
}

#[test]
fn a_given_include_entry_exports_wins_over_a_workspace_false() {
    let (_dir, root) = project(Some(
        r#"{ "workspaces": { "settings": { "packages/a": { "includeEntryExports": false } } } }"#,
    ));
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"app","workspaces":["packages/*"]}"#,
    )
    .expect("root manifest");
    std::fs::create_dir_all(root.join("packages/a")).expect("workspace dir");
    std::fs::write(root.join("packages/a/package.json"), r#"{"name":"a"}"#)
        .expect("workspace manifest");
    let file = root.join("packages/a/index.ts");
    let include_for = |given_include: Option<bool>| {
        let mut inputs = combined(&root);
        inputs.include_entry_exports = given_include.map(given);
        let run = resolve(&inputs);
        let AnalysisSlot::Runs(dead_code) = run.analysis(PlannedAnalysis::DeadCode) else {
            panic!("dead code runs");
        };
        let config = dead_code.config();
        config
            .workspace_settings
            .bind(&root, dead_code.workspaces());
        config.include_entry_exports_for(&file)
    };
    assert!(!include_for(None));
    assert!(include_for(Some(true)));
}

/// The workspace discovery of the resolver matches the ignore globs. An
/// adjustment that writes only rule settings still opens a session after it.
#[test]
fn a_rule_adjustment_opens_a_session_after_the_workspace_discovery() {
    let (_dir, root) = project(Some(r#"{ "ignorePatterns": ["gen/**"] }"#));
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"app","workspaces":["packages/*"]}"#,
    )
    .expect("root manifest");
    std::fs::create_dir_all(root.join("packages/empty")).expect("workspace dir");
    let run = resolve(&combined(&root));
    let AnalysisSlot::Runs(dead_code) = run.analysis(PlannedAnalysis::DeadCode) else {
        panic!("dead code runs");
    };
    assert!(
        dead_code
            .open_session_adjusted(|config| config.show_cascade = true)
            .is_ok()
    );
    assert!(
        dead_code
            .open_session_adjusted(
                |config| config.ignore_patterns = fallow_config::IgnorePatternSet::empty()
            )
            .is_err()
    );
}

/// The run resolves with a type-aware value that it cannot use, and returns
/// the error only to a reader of the type-aware settings. The language server
/// reads no type-aware variable.
#[test]
fn an_unknown_type_aware_value_fails_only_the_type_aware_reader() {
    let (_dir, root) = project(Some(r#"{ "typeAware": { "enabled": true } }"#));
    let env = EnvSnapshot::from_pairs(&[("FALLOW_TYPE_AWARE", "maybe")], Some(&root));
    let inputs = RunInputs::new(
        Surface::Cli,
        env.clone(),
        CommandKind::Dupes,
        SelectionInput::Single(PlannedAnalysis::Duplication),
    );
    let run = resolve(&inputs);
    assert!(matches!(
        run.type_aware(),
        Err(ResolveError::Input {
            field: InputField::Env(fallow_config::EnvVar::TypeAware),
            problem: InputProblem::InvalidValue,
            ..
        })
    ));
    let lsp = RunInputs::new(
        Surface::Lsp,
        env,
        CommandKind::Editor,
        SelectionInput::Single(PlannedAnalysis::DeadCode),
    );
    let settings = resolve(&lsp).type_aware().cloned().expect("settings");
    assert!(settings.enabled.value());
    assert!(matches!(
        settings.enabled.source(),
        Source::ConfigFile {
            key: "typeAware.enabled",
            ..
        }
    ));
}
