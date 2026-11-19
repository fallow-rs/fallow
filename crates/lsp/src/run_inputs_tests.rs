//! The translator of the editor run inputs, and the rules of an editor run:
//! the defaults fallback of a discovered config, the error of an explicit
//! config path, the reload of a kept session, and the plan of duplication.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use fallow_api::{EditorAnalysisOutput, EditorAnalysisResults};
use ls_types::MessageType;
use rustc_hash::FxHashMap;
use serde_json::{Value, json};

use crate::analysis::{
    self, BlockingAnalysisInput, ProjectRootAnalysisInput, RunParseWork, SharedSessionStore,
    analyze_project_root, run_blocking_analysis,
};
use crate::initialization::parse_initialization_options;
use crate::session_store::{EditorSessionStore, session_input_file};

use super::run_inputs;

/// A config with a boundary rule that names a zone that does not exist.
const INVALID_BOUNDARY_CONFIG: &str = r#"{
    "boundaries": {
        "zones": [{ "name": "ui", "patterns": ["src/ui/**"] }],
        "rules": [{ "from": "ui", "allow": ["typo-zone"] }]
    }
}"#;

/// The result of one project root analysis.
struct Outcome {
    result: Result<(), String>,
    output: EditorAnalysisOutput,
    messages: Vec<(MessageType, String)>,
}

impl Outcome {
    fn warnings(&self) -> Vec<&str> {
        self.messages
            .iter()
            .filter(|(kind, _)| *kind == MessageType::WARNING)
            .map(|(_, message)| message.as_str())
            .collect()
    }

    fn reports_unused_file(&self, name: &str) -> bool {
        self.output
            .results
            .unused_files
            .iter()
            .any(|finding| finding.file.path.ends_with(name))
    }
}

/// A project with an entry, an unused source file and an unused test file.
/// Production mode leaves the test file out of the analysis.
fn write_project(root: &Path) {
    std::fs::create_dir_all(root.join(".git")).expect("git marker");
    std::fs::create_dir_all(root.join("src")).expect("source dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"editor-run","private":true,"main":"src/index.ts"}"#,
    )
    .expect("manifest");
    std::fs::write(root.join("src/index.ts"), "export const used = 1;\n").expect("entry");
    std::fs::write(root.join("src/orphan.ts"), "export const orphan = 2;\n").expect("orphan");
    std::fs::write(
        root.join("src/orphan.test.ts"),
        "export const helper = 3;\n",
    )
    .expect("test file");
}

fn project() -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dunce::canonicalize(dir.path()).expect("canonical root");
    write_project(&root);
    (dir, root)
}

/// Analyze `root` with the initialization options `options`.
fn analyze(root: &Path, options: &Value) -> Outcome {
    analyze_in(root, options, &Arc::default())
}

/// Analyze `root` with `options` and the session store `sessions`.
fn analyze_in(root: &Path, options: &Value, sessions: &SharedSessionStore) -> Outcome {
    let parsed = parse_initialization_options(Some(options));
    let mut output = EditorAnalysisOutput::new(
        EditorAnalysisResults::default(),
        fallow_api::EditorDuplicationReport::default(),
    );
    let mut messages = Vec::new();
    let cancellation = Arc::new(AtomicBool::new(false));
    let type_aware_sessions = Arc::new(Mutex::new(FxHashMap::default()));
    let result = analyze_project_root(&mut ProjectRootAnalysisInput {
        project_root: root,
        run_options: &parsed,
        duplication_options: parsed.duplication.as_ref(),
        inline_complexity_enabled: false,
        type_aware_options: None,
        type_aware_sessions: &type_aware_sessions,
        type_aware_changes: &fallow_api::TypeAwareFileChanges::default(),
        cancellation: &cancellation,
        run_cancellation: &cancellation,
        changed_files: None,
        global_changed_since_requested: false,
        sessions,
        parse_work: &mut RunParseWork::default(),
        merged_analysis: &mut output,
        merged_inline_complexity: &mut Vec::new(),
        config_messages: &mut messages,
        package_scopes: &mut Vec::new(),
        unmatched_config_patterns: &mut Vec::new(),
    })
    .map_err(|err| err.to_string());
    Outcome {
        result,
        output,
        messages,
    }
}

/// Whether the config inputs of a session that loaded with `options` count
/// as changed after `edit`.
fn changed_after(root: &Path, options: &Value, edit: impl FnOnce()) -> bool {
    let parsed = parse_initialization_options(Some(options));
    let inputs = run_inputs(&parsed, root, fallow_config::EnvSnapshot::none());
    let sources = analysis::load_project_session(root, &inputs)
        .expect("session loads")
        .sources;
    assert!(!sources.changed(), "nothing changed yet");
    edit();
    sources.changed()
}

/// One run of `root` through the session store `sessions`.
fn blocking_run(
    root: &Path,
    sessions: &SharedSessionStore,
) -> (EditorAnalysisOutput, RunParseWork) {
    let output = run_blocking_analysis(&BlockingAnalysisInput {
        project_roots: vec![root.to_path_buf()],
        run_options: crate::initialization::LspInitializationOptions::default(),
        duplication_options: None,
        inline_complexity_enabled: false,
        type_aware_options: None,
        type_aware_sessions: Arc::new(Mutex::new(FxHashMap::default())),
        type_aware_changes: fallow_api::TypeAwareFileChanges::default(),
        root: root.to_path_buf(),
        toplevel: None,
        changed_since: None,
        cancellation: Arc::new(AtomicBool::new(false)),
        run_cancellation: Arc::new(AtomicBool::new(false)),
        sessions: Arc::clone(sessions),
    })
    .expect("the run succeeds");
    (output.analysis, output.parse_work)
}

#[test]
fn discovered_invalid_boundary_falls_back_with_editor_settings() {
    let (_dir, root) = project();
    std::fs::write(root.join(".fallowrc.json"), INVALID_BOUNDARY_CONFIG).expect("config");

    let outcome = analyze(&root, &json!({ "production": true }));

    assert_eq!(outcome.result, Ok(()));
    let warnings = outcome.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("invalid boundary configuration"),
        "{warnings:?}"
    );
    assert!(
        outcome.reports_unused_file("orphan.ts"),
        "the default config still gives findings"
    );
    assert!(
        !outcome.reports_unused_file("orphan.test.ts"),
        "the fallback keeps fallow.production, so the test file is out"
    );
}

/// An external plugin whose entry pattern leaves the project root.
const INVALID_PLUGIN: &str = r#"{"name":"p","entryPoints":["../escape/**"]}"#;

/// Analyze `root` in production mode and assert the defaults fallback for an
/// invalid external plugin file.
fn assert_invalid_plugin_falls_back(root: &Path) {
    let outcome = analyze(root, &json!({ "production": true }));

    assert_eq!(outcome.result, Ok(()));
    let warnings = outcome.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("invalid external plugin definition"),
        "{warnings:?}"
    );
    assert!(
        outcome.reports_unused_file("orphan.ts"),
        "the default config still gives findings"
    );
    assert!(
        !outcome.reports_unused_file("orphan.test.ts"),
        "the fallback keeps fallow.production, so the test file is out"
    );
}

#[test]
fn discovered_config_with_an_invalid_listed_plugin_falls_back() {
    let (_dir, root) = project();
    std::fs::create_dir_all(root.join("tools")).expect("plugin dir");
    std::fs::write(root.join("tools/p.json"), INVALID_PLUGIN).expect("plugin");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"plugins":["tools/p.json"]}"#,
    )
    .expect("config");

    assert_invalid_plugin_falls_back(&root);
}

#[test]
fn discovered_config_with_an_invalid_local_plugin_falls_back() {
    let (_dir, root) = project();
    std::fs::create_dir_all(root.join(".fallow/plugins")).expect("plugin dir");
    std::fs::write(root.join(".fallow/plugins/p.json"), INVALID_PLUGIN).expect("plugin");
    std::fs::write(root.join(".fallowrc.json"), "{}").expect("config");

    assert_invalid_plugin_falls_back(&root);
}

#[test]
fn the_fallback_keeps_the_editor_duplication_options() {
    let (_dir, root) = project();
    std::fs::write(root.join(".fallowrc.json"), INVALID_BOUNDARY_CONFIG).expect("config");
    let clone = "export function total(items: number[]) {\n  let sum = 0;\n  for (const item of items) {\n    sum += item;\n  }\n  return sum;\n}\n";
    std::fs::write(root.join("src/a.ts"), clone).expect("first copy");
    std::fs::write(root.join("src/b.ts"), clone).expect("second copy");

    let outcome = analyze(
        &root,
        &json!({ "duplication": { "minTokens": 5, "minLines": 2 } }),
    );

    assert_eq!(outcome.result, Ok(()));
    assert_eq!(outcome.warnings().len(), 1, "{:?}", outcome.messages);
    assert!(
        !outcome.output.duplication.clone_groups.is_empty(),
        "the editor options find the short clone"
    );
}

#[test]
fn dupes_do_not_run_when_the_plan_skips_them() {
    let (_on_dir, on_root) = project();
    assert_eq!(analyze(&on_root, &json!({})).result, Ok(()));
    assert_eq!(
        fallow_api::run::testing::duplication_runs::count(&on_root),
        1,
        "the counter sees the duplication run of a default plan"
    );

    let (_dir, root) = project();
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"duplicates":{"enabled":false}}"#,
    )
    .expect("config");

    let outcome = analyze(&root, &json!({}));

    assert_eq!(outcome.result, Ok(()));
    assert!(outcome.output.duplication.clone_groups.is_empty());
    assert_eq!(
        fallow_api::run::testing::duplication_runs::count(&root),
        0,
        "the plan skips duplication, so no session runs it"
    );
}

#[test]
fn discovered_broken_workspace_glob_falls_back() {
    let (_dir, root) = project();
    std::fs::write(root.join("package.json"), r#"{"workspaces": ["#).expect("manifest");
    std::fs::write(root.join(".fallowrc.json"), "{}").expect("config");

    let outcome = analyze(&root, &json!({}));

    assert_eq!(outcome.result, Ok(()));
    let warnings = outcome.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(warnings[0].contains("config error for"), "{warnings:?}");
    assert!(outcome.reports_unused_file("orphan.ts"));
}

#[test]
fn a_shadowed_config_file_logs_a_warning_notice() {
    let (_dir, root) = project();
    std::fs::write(root.join(".fallowrc.json"), "{}").expect("config");
    std::fs::write(root.join("fallow.toml"), "").expect("shadowed config");

    let outcome = analyze(&root, &json!({}));

    assert_eq!(outcome.result, Ok(()));
    let warnings = outcome.warnings();
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    assert!(
        warnings[0].contains("Two fallow config files exist"),
        "{warnings:?}"
    );
}

#[test]
fn a_run_that_reuses_a_kept_session_logs_its_notices() {
    let (_dir, root) = project();
    std::fs::write(root.join(".fallowrc.json"), "{}").expect("config");
    std::fs::write(root.join("fallow.toml"), "").expect("shadowed config");
    let sessions: SharedSessionStore = Arc::new(Mutex::new(EditorSessionStore::new(true)));

    let first = analyze_in(&root, &json!({}), &sessions);
    let second = analyze_in(&root, &json!({}), &sessions);

    for outcome in [&first, &second] {
        assert_eq!(outcome.result, Ok(()));
        let warnings = outcome.warnings();
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("Two fallow config files exist"),
            "{warnings:?}"
        );
    }
}

#[test]
fn explicit_invalid_config_is_an_error() {
    let (_dir, root) = project();
    std::fs::write(root.join(".fallowrc.json"), INVALID_BOUNDARY_CONFIG).expect("config");

    let outcome = analyze(
        &root,
        &json!({ "configPath": ".fallowrc.json", "production": true }),
    );

    let Err(message) = &outcome.result else {
        panic!("an explicit config path that does not load fails the run");
    };
    assert!(message.contains("fallow.configPath"), "{message}");
    assert!(
        message.contains("invalid boundary configuration"),
        "{message}"
    );
    assert!(outcome.output.results.unused_files.is_empty());
    assert!(outcome.warnings().is_empty(), "{:?}", outcome.messages);
}

#[test]
fn plugin_file_changed_during_load_reloads() {
    let (_dir, root) = project();
    std::fs::create_dir_all(root.join("tools")).expect("plugin dir");
    let plugin = root.join("tools/entries.json");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{"plugins":["tools/entries.json"]}"#,
    )
    .expect("config");
    std::fs::write(&plugin, r#"{"name":"entries","entryPoints":["src/a.ts"]}"#).expect("plugin");

    let changed = changed_after(&root, &json!({}), || {
        std::fs::write(&plugin, r#"{"name":"entries","entryPoints":["src/b.ts"]}"#)
            .expect("edit the plugin");
    });

    assert!(
        changed,
        "the session holds the older plugin, so it loads again"
    );
}

#[test]
fn config_created_after_first_run_reloads() {
    let (_dir, root) = project();
    let sessions: SharedSessionStore = Arc::new(Mutex::new(EditorSessionStore::new(true)));

    let (first, first_work) = blocking_run(&root, &sessions);
    assert_eq!(first_work.sessions_loaded, 1);
    assert!(
        first
            .results
            .unused_files
            .iter()
            .any(|finding| finding.file.path.ends_with("orphan.ts"))
    );

    let config = root.join(".fallowrc.json");
    std::fs::write(&config, r#"{"rules":{"unused-files":"off"}}"#).expect("create the config");
    // The client reports the new file, and the server marks the store stale
    // for each session input file.
    assert!(session_input_file(&config));
    sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .mark_stale();
    let (second, second_work) = blocking_run(&root, &sessions);

    assert_eq!(
        second_work.sessions_loaded, 1,
        "the next run loads the config"
    );
    assert!(
        second.results.unused_files.is_empty(),
        "{:?}",
        second.results.unused_files
    );
}

fn translated(options: &Value, root: &Path) -> fallow_config::RunInputs {
    run_inputs(
        &parse_initialization_options(Some(options)),
        root,
        fallow_config::EnvSnapshot::none(),
    )
}

#[test]
fn the_translator_gives_only_the_settings_that_the_client_sets() {
    let root = Path::new("/workspace");
    let none = translated(&json!({}), root);
    assert_eq!(none.surface(), fallow_config::Surface::Lsp);
    assert_eq!(none.command, fallow_config::CommandKind::Editor);
    assert_eq!(none.env().cwd(), Some(root));
    assert_eq!(
        none.config_fallback,
        fallow_config::ConfigFallback::DefaultsOnDiscoveredError
    );
    assert!(none.config_path.is_none());
    assert!(none.allow_remote_extends.is_none());
    assert!(none.production.is_none());
    assert!(none.package_baselines.is_none());

    let defaults = translated(
        &json!({ "configPath": "   ", "allowRemoteExtends": false, "packageBaselines": true }),
        root,
    );
    assert!(defaults.config_path.is_none(), "a blank path is no path");
    assert!(defaults.allow_remote_extends.is_none());
    assert!(defaults.package_baselines.is_none());

    let given = translated(
        &json!({
            "configPath": " config/fallow.json ",
            "allowRemoteExtends": true,
            "production": false,
            "packageBaselines": false
        }),
        root,
    );
    let value = |given: Option<&fallow_types::sourced::Given<bool>>| given.map(|given| given.value);
    assert_eq!(
        given.config_path.map(|given| given.value),
        Some(std::path::PathBuf::from("config/fallow.json"))
    );
    assert_eq!(value(given.allow_remote_extends.as_ref()), Some(true));
    assert_eq!(value(given.production.as_ref()), Some(false));
    assert_eq!(value(given.package_baselines.as_ref()), Some(false));
}

#[test]
fn a_relative_config_path_resolves_from_the_workspace_root() {
    let (_dir, root) = project();
    std::fs::create_dir_all(root.join("config")).expect("config dir");
    std::fs::write(root.join("config/fallow.json"), "{}").expect("config");
    let run = fallow_api::run::resolve_run(&translated(
        &json!({ "configPath": "config/fallow.json" }),
        &root,
    ))
    .expect("the run resolves");
    assert_eq!(
        run.config_file().value().as_deref(),
        Some(root.join("config/fallow.json").as_path())
    );
}
