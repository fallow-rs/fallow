use std::path::{Path, PathBuf};

use fallow_config::{
    CONFIG_FILE_NAMES, CommandKind, ConfigFallback, EnvSnapshot, EnvVar, PlannedAnalysis,
    ProductionAnalysis, RunInputs, SelectionInput, Surface,
};
use fallow_types::output_format::OutputFormat;
use fallow_types::sourced::{Given, Origin, Source};

use super::resolve::{EmptyCause, ResolveError, resolve_run};
use super::{AnalysisSlot, ResolvedRun};
use crate::project_config::{ProjectConfigOptions, config_for_project_analysis};

const ORIGIN: Origin = Origin::ApiOption("test");
const PRODUCTION_ON: &str = "1";

fn given<T>(value: T) -> Given<T> {
    Given {
        value,
        origin: ORIGIN,
    }
}

/// A project root with a `.git` marker, a `package.json` and the given files.
fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dunce::canonicalize(dir.path()).expect("canonical root");
    std::fs::create_dir(root.join(".git")).expect("git marker");
    std::fs::write(root.join("package.json"), r#"{"name":"app"}"#).expect("package.json");
    for (name, content) in files {
        std::fs::write(root.join(name), content).expect("project file");
    }
    (dir, root)
}

fn inputs(
    root: &Path,
    surface: Surface,
    env: EnvSnapshot,
    command: CommandKind,
    selection: SelectionInput,
) -> RunInputs {
    let mut inputs = RunInputs::new(surface, env, command, selection);
    inputs.root = Some(given(root.to_path_buf()));
    inputs.threads = Some(given(1));
    inputs
}

const fn planned(analysis: ProductionAnalysis) -> PlannedAnalysis {
    match analysis {
        ProductionAnalysis::DeadCode => PlannedAnalysis::DeadCode,
        ProductionAnalysis::Health => PlannedAnalysis::Health,
        ProductionAnalysis::Dupes => PlannedAnalysis::Duplication,
    }
}

const fn single_command(analysis: ProductionAnalysis) -> CommandKind {
    match analysis {
        ProductionAnalysis::DeadCode => CommandKind::DeadCode,
        ProductionAnalysis::Health => CommandKind::Health,
        ProductionAnalysis::Dupes => CommandKind::Dupes,
    }
}

fn fixture_roots() -> Vec<PathBuf> {
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures");
    let mut roots: Vec<PathBuf> = std::fs::read_dir(&fixtures)
        .expect("fixtures dir")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            CONFIG_FILE_NAMES
                .iter()
                .any(|name| path.join(name).is_file())
        })
        .map(|path| dunce::canonicalize(path).expect("canonical fixture"))
        .collect();
    roots.sort();
    roots
}

const PRODUCTION_IGNORE_CONFIG: &str =
    r#"{ "production": { "deadCode": true, "dupes": false, "ignore": ["scripts/**"] } }"#;

/// The resolver gives the config of one analysis that the old entry point
/// gives for the same inputs.
#[test]
fn resolve_run_equals_config_for_project_analysis() {
    let mut roots = fixture_roots();
    assert!(!roots.is_empty(), "no fixture with a config file");
    // The object form with `ignore` globs, which no fixture has.
    let (_split, split_root) = project(&[(".fallowrc.json", PRODUCTION_IGNORE_CONFIG)]);
    roots.push(split_root);
    let analyses = [
        ProductionAnalysis::DeadCode,
        ProductionAnalysis::Health,
        ProductionAnalysis::Dupes,
    ];
    for root in &roots {
        for analysis in analyses {
            for production_override in [None, Some(true), Some(false)] {
                let label = format!("{} {analysis:?} {production_override:?}", root.display());
                let expected = config_for_project_analysis(
                    root,
                    None,
                    ProjectConfigOptions {
                        output: OutputFormat::Human,
                        no_cache: false,
                        threads: 1,
                        production_override,
                        quiet: true,
                        analysis,
                        allow_remote_extends: false,
                    },
                );
                let mut run_inputs = inputs(
                    root,
                    Surface::Api,
                    EnvSnapshot::capture(),
                    single_command(analysis),
                    SelectionInput::Single(planned(analysis)),
                );
                run_inputs.production = production_override.map(given);
                let actual = resolve_run(&run_inputs);
                match (expected, actual) {
                    (Ok(expected), Ok(run)) => {
                        let AnalysisSlot::Runs(runnable) = run.analysis(planned(analysis)) else {
                            panic!("{label}: the analysis does not run");
                        };
                        assert_eq!(
                            format!("{:?}", runnable.config()),
                            format!("{:?}", expected.config),
                            "{label}: config"
                        );
                        assert_eq!(
                            format!("{:?}", runnable.workspaces()),
                            format!("{:?}", expected.workspaces),
                            "{label}: workspaces"
                        );
                    }
                    (Err(expected), Err(actual)) => {
                        assert_eq!(actual.message(), expected.to_string(), "{label}: error");
                    }
                    (expected, actual) => panic!(
                        "{label}: old {:?}, new {:?}",
                        expected.map(|_| ()),
                        actual.map(|_| ())
                    ),
                }
            }
        }
    }
}

/// The production decision of the dead-code analysis of one run with
/// `FALLOW_PRODUCTION=1`.
fn production_with_env(surface: Surface, command: CommandKind) -> (bool, Source) {
    let (_dir, root) = project(&[]);
    let env = EnvSnapshot::from_pairs(&[("FALLOW_PRODUCTION", PRODUCTION_ON)], Some(&root));
    let run = resolve_run(&inputs(
        &root,
        surface,
        env,
        command,
        SelectionInput::Single(PlannedAnalysis::DeadCode),
    ))
    .expect("run resolves");
    let decision = run.production(PlannedAnalysis::DeadCode);
    (*decision.value(), decision.source().clone())
}

fn assert_production_env_applies(surface: Surface, command: CommandKind, applies: bool) {
    let expected = if applies {
        (
            true,
            Source::Environment {
                name: "FALLOW_PRODUCTION",
            },
        )
    } else {
        (
            false,
            Source::Default {
                rule: "production-off",
            },
        )
    };
    assert_eq!(
        production_with_env(surface, command),
        expected,
        "{surface:?} {command:?}"
    );
}

macro_rules! production_rows {
    ($($name:ident: $surface:ident, $command:ident, $applies:expr;)*) => {
        $(
            #[test]
            fn $name() {
                assert_production_env_applies(Surface::$surface, CommandKind::$command, $applies);
            }
        )*
    };
}

production_rows! {
    env_policy_production_cli_combined: Cli, Combined, true;
    env_policy_production_mcp_combined: Mcp, Combined, true;
    env_policy_production_cli_list: Cli, List, true;
    env_policy_production_mcp_flags: Mcp, Flags, true;
    env_policy_production_cli_trace_dependency: Cli, TraceDependency, true;
    env_policy_production_mcp_trace_dependency: Mcp, TraceDependency, true;
    env_policy_production_cli_baselines_prune: Cli, BaselinesPrune, true;
    env_policy_production_cli_trace_path: Cli, TracePath, false;
    env_policy_production_mcp_trace_path: Mcp, TracePath, false;
    env_policy_production_mcp_trace_error: Mcp, TraceError, false;
    env_policy_production_cli_security: Cli, Security, false;
    env_policy_production_cli_inspect: Cli, Inspect, false;
    env_policy_production_cli_coverage_analyze: Cli, CoverageAnalyze, false;
    env_policy_production_cli_coverage_upload: Cli, CoverageUpload, false;
    env_policy_production_cli_doctor: Cli, Doctor, false;
    env_policy_production_mcp_decision_surface: Mcp, DecisionSurface, false;
    env_policy_production_lsp_editor: Lsp, Editor, false;
    env_policy_production_api_combined: Api, Combined, false;
    env_policy_production_node_combined: Node, Combined, false;
}

/// A run of `command` on `surface` with one variable set.
fn run_with_var(surface: Surface, command: CommandKind, name: &str, value: &str) -> ResolvedRun {
    let (_dir, root) = project(&[]);
    let env = EnvSnapshot::from_pairs(&[(name, value)], Some(&root));
    resolve_run(&inputs(
        &root,
        surface,
        env,
        command,
        SelectionInput::Single(PlannedAnalysis::DeadCode),
    ))
    .expect("run resolves")
}

#[test]
fn env_policy_max_file_size_lsp_editor() {
    let run = run_with_var(
        Surface::Lsp,
        CommandKind::Editor,
        "FALLOW_MAX_FILE_SIZE",
        "7",
    );
    let setting = &run.settings().max_file_size_bytes;
    assert_eq!(*setting.value(), Some(7 * 1024 * 1024));
    assert_eq!(
        setting.source(),
        &Source::Environment {
            name: "FALLOW_MAX_FILE_SIZE"
        }
    );
}

#[test]
fn env_policy_cache_max_size_api_combined() {
    let run = run_with_var(
        Surface::Api,
        CommandKind::Combined,
        "FALLOW_CACHE_MAX_SIZE",
        "64",
    );
    let setting = &run.settings().cache_max_size_mb;
    assert_eq!(*setting.value(), Some(64));
    assert_eq!(
        setting.source(),
        &Source::Environment {
            name: "FALLOW_CACHE_MAX_SIZE"
        }
    );
}

/// Whether one run reads `var`, by the lists of read and ignored variables.
fn assert_var_read(surface: Surface, command: CommandKind, var: EnvVar, read: bool) {
    let run = run_with_var(surface, command, var.name(), "value");
    assert_eq!(run.env_read().contains(&var), read, "{surface:?} {var:?}");
    assert_eq!(
        run.env_ignored().contains(&var),
        !read,
        "{surface:?} {var:?}"
    );
}

#[test]
fn env_policy_extends_timeout_secs_node_combined() {
    assert_var_read(
        Surface::Node,
        CommandKind::Combined,
        EnvVar::ExtendsTimeoutSecs,
        true,
    );
}

#[test]
fn env_policy_changed_since_cli_combined() {
    assert_var_read(
        Surface::Cli,
        CommandKind::Combined,
        EnvVar::ChangedSince,
        false,
    );
}

#[test]
fn env_policy_changed_since_mcp_combined() {
    assert_var_read(
        Surface::Mcp,
        CommandKind::Combined,
        EnvVar::ChangedSince,
        true,
    );
}

#[test]
fn env_policy_coverage_api_health() {
    assert_var_read(Surface::Api, CommandKind::Health, EnvVar::Coverage, false);
}

#[test]
fn env_policy_coverage_cli_health() {
    assert_var_read(Surface::Cli, CommandKind::Health, EnvVar::Coverage, true);
}

#[test]
fn env_policy_audit_base_node_audit() {
    assert_var_read(Surface::Node, CommandKind::Audit, EnvVar::AuditBase, true);
}

#[test]
fn env_policy_type_aware_lsp_editor() {
    assert_var_read(Surface::Lsp, CommandKind::Editor, EnvVar::TypeAware, false);
}

/// A rule whose `from` names a zone that the config does not define.
const INVALID_BOUNDARY_CONFIG: &str = r#"{
  "boundaries": {
    "zones": [{ "name": "app", "patterns": ["src/**"] }],
    "rules": [{ "from": "missing", "allow": ["app"] }]
  }
}"#;

fn editor_inputs(root: &Path) -> RunInputs {
    let mut run_inputs = inputs(
        root,
        Surface::Lsp,
        EnvSnapshot::none().with_cwd(root.to_path_buf()),
        CommandKind::Editor,
        SelectionInput::Single(PlannedAnalysis::DeadCode),
    );
    run_inputs.config_fallback = ConfigFallback::DefaultsOnDiscoveredError;
    run_inputs
}

#[test]
fn fallback_on_discovered_validation_error() {
    let (_dir, root) = project(&[(".fallowrc.json", INVALID_BOUNDARY_CONFIG)]);
    let run = resolve_run(&editor_inputs(&root)).expect("the defaults apply");
    assert!(run.fell_back());
    assert_eq!(run.config_file().value(), &None);
}

#[test]
fn fallback_on_workspace_error() {
    let (_dir, root) = project(&[(".fallowrc.json", "{}")]);
    std::fs::write(root.join("package.json"), "{").expect("malformed package.json");
    let run = resolve_run(&editor_inputs(&root)).expect("the defaults apply");
    assert!(run.fell_back());
    let AnalysisSlot::Runs(runnable) = run.analysis(PlannedAnalysis::DeadCode) else {
        panic!("dead code runs");
    };
    assert!(runnable.workspaces().is_empty());
}

#[test]
fn explicit_path_never_falls_back() {
    let (_dir, root) = project(&[(".fallowrc.json", INVALID_BOUNDARY_CONFIG)]);
    let mut run_inputs = editor_inputs(&root);
    run_inputs.config_path = Some(given(root.join(".fallowrc.json")));
    let err = resolve_run(&run_inputs).expect_err("an explicit path fails");
    assert!(matches!(err, ResolveError::ConfigInvalid { .. }), "{err:?}");
}

fn nothing_selected(result: Result<ResolvedRun, ResolveError>) -> Option<EmptyCause> {
    match result {
        Err(ResolveError::NothingSelected { cause, .. }) => Some(cause),
        Err(err) => panic!("unexpected error {err:?}"),
        Ok(_) => None,
    }
}

fn selection_run(
    files: &[(&str, &str)],
    surface: Surface,
    command: CommandKind,
    selection: SelectionInput,
) -> Option<EmptyCause> {
    let (_dir, root) = project(files);
    nothing_selected(resolve_run(&inputs(
        &root,
        surface,
        EnvSnapshot::none().with_cwd(root.clone()),
        command,
        selection,
    )))
}

fn skip(analyses: &[PlannedAnalysis]) -> SelectionInput {
    SelectionInput::Lists {
        only: Vec::new(),
        skip: analyses.to_vec(),
        origin: Origin::Flag("--skip"),
    }
}

const DUPES_OFF: &str = r#"{ "duplicates": { "enabled": false } }"#;

#[test]
fn empty_selection_rules() {
    let all = [
        PlannedAnalysis::DeadCode,
        PlannedAnalysis::Duplication,
        PlannedAnalysis::Health,
    ];
    let no_dupes = [PlannedAnalysis::DeadCode, PlannedAnalysis::Health];
    let config = [(".fallowrc.json", DUPES_OFF)];

    // CLI bare: an empty plan fails, from the request and from the config.
    assert_eq!(
        selection_run(&[], Surface::Cli, CommandKind::Combined, skip(&all)),
        Some(EmptyCause::Request)
    );
    assert_eq!(
        selection_run(
            &config,
            Surface::Cli,
            CommandKind::Combined,
            skip(&no_dupes)
        ),
        Some(EmptyCause::Config)
    );

    // CLI audit: `duplicates.enabled` also empties the plan.
    assert_eq!(
        selection_run(&config, Surface::Cli, CommandKind::Audit, skip(&no_dupes)),
        Some(EmptyCause::Config)
    );

    // API combined: only three false bools fail.
    let booleans = |dead_code, duplication, health| SelectionInput::Booleans {
        dead_code,
        duplication,
        health,
        origin: ORIGIN,
    };
    assert_eq!(
        selection_run(
            &[],
            Surface::Api,
            CommandKind::Combined,
            booleans(false, false, false)
        ),
        Some(EmptyCause::Request)
    );
    assert_eq!(
        selection_run(
            &config,
            Surface::Api,
            CommandKind::Combined,
            booleans(true, true, true)
        ),
        None
    );

    // API audit: only three `Some(false)` options fail. A plan that the
    // config empties is not an error.
    let options = |dead_code, duplication, health| SelectionInput::Options {
        dead_code,
        duplication,
        health,
        origin: ORIGIN,
    };
    assert_eq!(
        selection_run(
            &[],
            Surface::Api,
            CommandKind::Audit,
            options(Some(false), Some(false), Some(false))
        ),
        Some(EmptyCause::Request)
    );
    assert_eq!(
        selection_run(
            &config,
            Surface::Api,
            CommandKind::Audit,
            options(Some(false), None, Some(false))
        ),
        None
    );
}

#[test]
fn relative_config_path_joins_snapshot_cwd() {
    let (_dir, root) = project(&[("custom.json", r#"{ "production": true }"#)]);
    let mut run_inputs = inputs(
        &root,
        Surface::Api,
        EnvSnapshot::none().with_cwd(root.clone()),
        CommandKind::DeadCode,
        SelectionInput::Single(PlannedAnalysis::DeadCode),
    );
    run_inputs.config_path = Some(given(PathBuf::from("custom.json")));
    let run = resolve_run(&run_inputs).expect("the config loads from the snapshot cwd");
    let file = root.join("custom.json");
    assert_eq!(run.config_file().value(), &Some(file.clone()));
    assert_eq!(
        run.production(PlannedAnalysis::DeadCode).source(),
        &Source::ConfigFile {
            file,
            key: "production"
        }
    );
}
