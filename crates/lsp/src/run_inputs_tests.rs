//! The rules of an editor run: the defaults fallback of a discovered config,
//! the error of an explicit config path, and the reload of a kept session.

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
use crate::initialization::{initialization_config_path, parse_initialization_options};
use crate::session_store::{EditorSessionStore, SessionKey, session_input_file};

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
    let parsed = parse_initialization_options(Some(options));
    let config_path = initialization_config_path(options, Some(root));
    let mut output = EditorAnalysisOutput::new(
        EditorAnalysisResults::default(),
        fallow_api::EditorDuplicationReport::default(),
    );
    let mut messages = Vec::new();
    let cancellation = Arc::new(AtomicBool::new(false));
    let type_aware_sessions = Arc::new(Mutex::new(FxHashMap::default()));
    let result = analyze_project_root(&mut ProjectRootAnalysisInput {
        project_root: root,
        config_path: config_path.as_deref(),
        allow_remote_extends: parsed.allow_remote_extends,
        duplication_options: parsed.duplication.as_ref(),
        production_override: parsed.production,
        inline_complexity_enabled: false,
        type_aware_options: None,
        type_aware_sessions: &type_aware_sessions,
        type_aware_changes: &fallow_api::TypeAwareFileChanges::default(),
        cancellation: &cancellation,
        run_cancellation: &cancellation,
        changed_files: None,
        global_changed_since_requested: false,
        no_package_baselines: false,
        sessions: &Arc::default(),
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
    let key = SessionKey {
        config_path: initialization_config_path(options, Some(root)),
        allow_remote_extends: parsed.allow_remote_extends,
        production_override: parsed.production,
    };
    let (_session, sources) = analysis::load_project_session(root, &key).expect("session loads");
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
        config_path: None,
        allow_remote_extends: false,
        duplication_options: None,
        production_override: None,
        no_package_baselines: false,
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
