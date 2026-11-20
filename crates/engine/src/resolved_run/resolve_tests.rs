use std::path::{Path, PathBuf};

use fallow_config::{
    CONFIG_FILE_NAMES, CommandKind, ConfigFallback, EnvSnapshot, EnvVar, PlannedAnalysis,
    ProductionAnalysis, RunInputs, SelectionInput, Surface,
};
use fallow_types::output_format::OutputFormat;
use fallow_types::sourced::{Given, Origin, Source};

use super::resolve::{EmptyCause, ResolveError, resolve_run};
use super::{AnalysisSlot, ResolvedRun};
use crate::project_config::{ProjectConfigOptions, config_for_project_analysis_with_env};

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
    let fixtures =
        Path::new(&std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures");
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

/// The variables that every host applies to a resolved config.
const HOST_ENV: [(&str, &str); 4] = [
    ("FALLOW_MAX_FILE_SIZE", "3"),
    ("FALLOW_CACHE_DIR", "env-cache"),
    ("FALLOW_CACHE_MAX_SIZE", "9"),
    ("FALLOW_PACKAGE_BASELINES", "off"),
];

const MB: u64 = 1024 * 1024;

/// The config file of a fixture root.
fn config_file_of(root: &Path) -> Option<PathBuf> {
    CONFIG_FILE_NAMES
        .iter()
        .map(|name| root.join(name))
        .find(|path| path.is_file())
}

/// The resolver gives the config of one analysis that the old entry point
/// gives for the same inputs, with a discovered and an explicit config path.
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
    let env = |root: &Path| EnvSnapshot::from_pairs(&HOST_ENV, Some(root));
    for root in &roots {
        for config_path in [None, config_file_of(root)] {
            for analysis in analyses {
                for production_override in [None, Some(true), Some(false)] {
                    let label = format!(
                        "{} {config_path:?} {analysis:?} {production_override:?}",
                        root.display()
                    );
                    let expected = config_for_project_analysis_with_env(
                        root,
                        config_path.as_deref(),
                        ProjectConfigOptions {
                            output: OutputFormat::Human,
                            no_cache: false,
                            threads: 1,
                            production_override,
                            quiet: true,
                            analysis,
                            allow_remote_extends: false,
                        },
                        &env(root),
                    );
                    let mut run_inputs = inputs(
                        root,
                        Surface::Api,
                        env(root),
                        single_command(analysis),
                        SelectionInput::Single(planned(analysis)),
                    );
                    run_inputs.production = production_override.map(given);
                    run_inputs.config_path = config_path.clone().map(given);
                    assert_same_result(&label, analysis, expected, resolve_run(&run_inputs));
                }
            }
        }
    }
}

fn assert_same_result(
    label: &str,
    analysis: ProductionAnalysis,
    expected: crate::EngineResult<crate::project_config::ProjectConfig>,
    actual: Result<ResolvedRun, ResolveError>,
) {
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

/// A config with production `ignore` globs and a cache size, at a path that
/// discovery does not find.
const FIXED_CONFIG: &str = r#"{
  "production": { "deadCode": true, "dupes": false, "ignore": ["scripts/**"] },
  "cache": { "maxSizeMb": 4 }
}"#;

/// The resolver gives fixed values for an explicit config path and set
/// variables, so a step that both the resolver and the old entry point call
/// cannot drift without a failure.
#[test]
fn resolve_run_gives_fixed_values() {
    let (_dir, root) = project(&[("custom.json", FIXED_CONFIG)]);
    let env = EnvSnapshot::from_pairs(&HOST_ENV, Some(&root));
    let ignore = vec!["scripts/**".to_owned()];
    for (analysis, production) in [
        (ProductionAnalysis::DeadCode, true),
        (ProductionAnalysis::Health, false),
        (ProductionAnalysis::Dupes, false),
    ] {
        let mut run_inputs = inputs(
            &root,
            Surface::Api,
            env.clone(),
            single_command(analysis),
            SelectionInput::Single(planned(analysis)),
        );
        run_inputs.config_path = Some(given(root.join("custom.json")));
        let run = resolve_run(&run_inputs).expect("the run resolves");
        assert_eq!(
            run.config_file().value(),
            &Some(root.join("custom.json")),
            "{analysis:?}"
        );
        let AnalysisSlot::Runs(runnable) = run.analysis(planned(analysis)) else {
            panic!("{analysis:?} runs");
        };
        let config = runnable.config();
        assert_eq!(config.production, production, "{analysis:?}");
        assert_eq!(config.production_ignore, ignore, "{analysis:?}");
        assert_eq!(config.max_file_size_bytes, Some(3 * MB), "{analysis:?}");
        assert_eq!(config.cache_dir, root.join("env-cache"), "{analysis:?}");
        assert_eq!(config.cache_max_size_mb, Some(9), "{analysis:?}");
        assert_eq!(config.threads, 1, "{analysis:?}");
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
    env_policy_production_cli_trace_chain: Cli, TraceChain, false;
    env_policy_production_mcp_trace_chain: Mcp, TraceChain, true;
    env_policy_production_cli_security: Cli, Security, false;
    env_policy_production_cli_inspect: Cli, Inspect, true;
    env_policy_production_mcp_inspect: Mcp, Inspect, false;
    env_policy_production_cli_coverage_analyze: Cli, CoverageAnalyze, false;
    env_policy_production_cli_coverage_upload: Cli, CoverageUpload, false;
    env_policy_production_cli_doctor: Cli, Doctor, false;
    env_policy_production_mcp_decision_surface: Mcp, DecisionSurface, false;
    env_policy_production_mcp_mutation: Mcp, Mutation, false;
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

/// The resolved config of the dead-code analysis of a run.
fn dead_code_config(run: &ResolvedRun) -> fallow_config::ResolvedConfig {
    let AnalysisSlot::Runs(runnable) = run.analysis(PlannedAnalysis::DeadCode) else {
        panic!("dead code runs");
    };
    runnable.config().clone()
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
    assert_eq!(*setting.value(), Some(7 * MB));
    assert_eq!(
        setting.source(),
        &Source::Environment {
            name: "FALLOW_MAX_FILE_SIZE"
        }
    );
    assert_eq!(dead_code_config(&run).max_file_size_bytes, Some(7 * MB));
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
    assert_eq!(dead_code_config(&run).cache_max_size_mb, Some(64));
}

#[test]
fn env_policy_cache_dir_node_combined() {
    let run = run_with_var(
        Surface::Node,
        CommandKind::Combined,
        "FALLOW_CACHE_DIR",
        "env-cache",
    );
    let expected = run.root().value().join("env-cache");
    assert_eq!(run.settings().cache_dir.value(), &expected);
    assert_eq!(dead_code_config(&run).cache_dir, expected);
}

#[test]
fn env_policy_package_baselines_mcp_combined() {
    let (_dir, root) = project(&[(
        ".fallowrc.json",
        r#"{ "workspaces": { "changedSince": { "packages/web": "main" } } }"#,
    )]);
    let run_for = |pairs: &[(&str, &str)]| {
        resolve_run(&inputs(
            &root,
            Surface::Mcp,
            EnvSnapshot::from_pairs(pairs, Some(&root)),
            CommandKind::Combined,
            SelectionInput::Single(PlannedAnalysis::DeadCode),
        ))
        .expect("run resolves")
    };
    let control = run_for(&[]);
    assert_eq!(dead_code_config(&control).workspace_changed_since.len(), 1);
    let run = run_for(&[("FALLOW_PACKAGE_BASELINES", "off")]);
    assert!(!*run.settings().package_baselines.value());
    assert!(dead_code_config(&run).workspace_changed_since.is_empty());
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

macro_rules! var_rows {
    ($($name:ident: $surface:ident, $command:ident, $var:ident, $read:expr;)*) => {
        $(
            #[test]
            fn $name() {
                assert_var_read(
                    Surface::$surface,
                    CommandKind::$command,
                    EnvVar::$var,
                    $read,
                );
            }
        )*
    };
}

var_rows! {
    env_policy_extends_timeout_secs_node_combined: Node, Combined, ExtendsTimeoutSecs, true;
    env_policy_changed_since_cli_combined: Cli, Combined, ChangedSince, false;
    env_policy_changed_since_mcp_combined: Mcp, Combined, ChangedSince, true;
    env_policy_diff_file_cli_dead_code: Cli, DeadCode, DiffFile, true;
    env_policy_diff_file_cli_inspect: Cli, Inspect, DiffFile, true;
    env_policy_diff_file_mcp_combined: Mcp, Combined, DiffFile, true;
    env_policy_diff_file_api_combined: Api, Combined, DiffFile, false;
    env_policy_coverage_api_health: Api, Health, Coverage, false;
    env_policy_coverage_cli_health: Cli, Health, Coverage, true;
    env_policy_coverage_cli_combined: Cli, Combined, Coverage, true;
    env_policy_coverage_cli_audit: Cli, Audit, CoverageRoot, true;
    env_policy_coverage_cli_viz: Cli, Viz, Coverage, true;
    env_policy_coverage_cli_dead_code: Cli, DeadCode, Coverage, false;
    env_policy_coverage_mcp_audit: Mcp, Audit, Coverage, true;
    env_policy_coverage_mcp_health: Mcp, Health, CoverageRoot, true;
    env_policy_coverage_mcp_combined: Mcp, Combined, Coverage, true;
    env_policy_coverage_mcp_viz: Mcp, Viz, Coverage, false;
    env_policy_audit_base_node_audit: Node, Audit, AuditBase, true;
    env_policy_audit_base_cli_audit: Cli, Audit, AuditBase, true;
    env_policy_audit_base_cli_dead_code: Cli, DeadCode, AuditBase, false;
    env_policy_audit_base_mcp_combined: Mcp, Combined, AuditBase, false;
    env_policy_changed_since_mcp_audit: Mcp, Audit, ChangedSince, true;
    env_policy_changed_since_api_audit: Api, Audit, ChangedSince, false;
    env_policy_changed_since_cli_audit: Cli, Audit, ChangedSince, false;
    env_policy_type_aware_lsp_editor: Lsp, Editor, TypeAware, false;
    env_policy_type_aware_api_combined: Api, Combined, TypeAware, true;
    env_policy_type_aware_projects_cli_dead_code: Cli, DeadCode, TypeAwareProjects, true;
    env_policy_type_aware_projects_api_combined: Api, Combined, TypeAwareProjects, false;
    env_policy_type_aware_projects_node_combined: Node, Combined, TypeAwareProjects, false;
    env_policy_type_aware_projects_mcp_combined: Mcp, Combined, TypeAwareProjects, false;
    env_policy_type_aware_require_cli_health: Cli, Health, TypeAwareRequire, true;
    env_policy_type_aware_require_api_doctor: Api, Doctor, TypeAwareRequire, true;
    env_policy_type_aware_require_api_combined: Api, Combined, TypeAwareRequire, false;
    env_policy_type_aware_require_node_combined: Node, Combined, TypeAwareRequire, false;
    env_policy_type_aware_require_mcp_audit: Mcp, Audit, TypeAwareRequire, false;
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
    assert_eq!(
        err.config_file(),
        Some(root.join(".fallowrc.json").as_path())
    );
}

#[test]
fn fallback_keeps_the_error_of_the_discovered_config() {
    let (_dir, root) = project(&[(".fallowrc.json", INVALID_BOUNDARY_CONFIG)]);
    let run = resolve_run(&editor_inputs(&root)).expect("the defaults apply");
    let err = run.fallback_error().expect("the run fell back");
    assert!(
        err.message().contains("invalid boundary configuration"),
        "{err:?}"
    );
    assert_eq!(
        err.config_file(),
        Some(root.join(".fallowrc.json").as_path())
    );
    let clean = project(&[(".fallowrc.json", "{}")]);
    let run = resolve_run(&editor_inputs(&clean.1)).expect("the config loads");
    assert!(run.fallback_error().is_none());
}

#[test]
fn sources_change_with_a_file_of_the_extends_chain() {
    let (_dir, root) = project(&[
        (".fallowrc.json", r#"{"extends":"./base.json"}"#),
        ("base.json", r#"{"entry":["a.ts"]}"#),
    ]);
    let run = resolve_run(&editor_inputs(&root)).expect("the config loads");
    assert!(!run.sources_changed());
    std::fs::write(root.join("base.json"), r#"{"entry":["b.ts"]}"#).expect("same-size edit");
    assert!(run.sources_changed());
}

#[test]
fn sources_change_with_a_plugin_file() {
    let (_dir, root) = project(&[(".fallowrc.json", r#"{"plugins":["entries.json"]}"#)]);
    let plugin = root.join("entries.json");
    std::fs::write(&plugin, r#"{"name":"entries","entryPoints":["a.ts"]}"#).expect("plugin");
    let run = resolve_run(&editor_inputs(&root)).expect("the config loads");
    assert!(!run.sources_changed());
    std::fs::write(&plugin, r#"{"name":"entries","entryPoints":["b.ts"]}"#).expect("edit");
    assert!(run.sources_changed());
}

#[test]
fn the_editor_scopes_a_shared_cache_dir_to_the_root() {
    let (_dir, root) = project(&[]);
    let shared = tempfile::tempdir().expect("shared cache dir");
    let shared = dunce::canonicalize(shared.path()).expect("canonical cache dir");
    let shared_text = shared.to_string_lossy().into_owned();
    let env =
        || EnvSnapshot::from_pairs(&[("FALLOW_CACHE_DIR", shared_text.as_str())], Some(&root));
    let editor = inputs(
        &root,
        Surface::Lsp,
        env(),
        CommandKind::Editor,
        SelectionInput::Single(PlannedAnalysis::DeadCode),
    );
    let api = inputs(
        &root,
        Surface::Api,
        env(),
        CommandKind::DeadCode,
        SelectionInput::Single(PlannedAnalysis::DeadCode),
    );

    let editor_run = resolve_run(&editor).expect("the editor run resolves");
    let api_run = resolve_run(&api).expect("the API run resolves");

    let editor_dir = editor_run.settings().cache_dir.value();
    assert_ne!(editor_dir, &shared);
    assert!(editor_dir.starts_with(&shared), "{}", editor_dir.display());
    assert_eq!(
        editor_run.settings().cache_dir.source(),
        &Source::Derived {
            from: "editor-cache-per-root"
        }
    );
    let AnalysisSlot::Runs(runnable) = editor_run.analysis(PlannedAnalysis::DeadCode) else {
        panic!("dead code runs");
    };
    assert_eq!(&runnable.config().cache_dir, editor_dir);
    assert_eq!(api_run.settings().cache_dir.value(), &shared);

    let own = resolve_run(&editor_inputs(&root)).expect("the editor run resolves");
    assert_eq!(
        own.settings().cache_dir.source(),
        &Source::Default { rule: "cache-dir" },
        "a cache dir inside the root stays"
    );
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

#[test]
fn thread_fallback_is_one() {
    let unavailable = Err(std::io::Error::other("no parallelism"));
    assert_eq!(super::threads_or_one(unavailable), 1);
    let four = std::num::NonZeroUsize::new(4).expect("non-zero");
    assert_eq!(super::threads_or_one(Ok(four)), 4);
}
