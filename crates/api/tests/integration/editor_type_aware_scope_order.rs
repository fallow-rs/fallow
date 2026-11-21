//! The editor narrows findings after semantic reconciliation. The reachable
//! analysis scope survives that report filter, so an orphan consumer outside
//! the changed files cannot credit a finding in a changed file.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::path::{Path, PathBuf};

use fallow_api::run::{CommandKind, EnvSnapshot, SelectionInput, Surface};
use fallow_api::{
    DeadCodeFilters, EditorAnalysisSession, TypeAwareOptions, TypeAwareRequire, TypeAwareSession,
};
use fallow_config::DuplicatesConfig;
use fallow_types::semantic::SemanticCandidateDecisionKind;
use fallow_types::sourced::{Given, Origin};
use rustc_hash::FxHashSet;

use crate::common::{rerun_with_type_aware_sidecar, write};

/// The full test name in the `integration` binary. The child run filters on
/// it with `--exact`, so it includes the module path.
const TEST_NAME: &str =
    "editor_type_aware_scope_order::changed_files_scope_after_type_aware_pass_keeps_real_finding";

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    // The session reports paths below the dunce spelling of the root. On
    // Windows, `Path::canonicalize` gives the `\\?\` spelling, which is not a
    // prefix of those paths.
    let root = dunce::canonicalize(dir.path()).unwrap();
    write(
        &root,
        "package.json",
        r#"{"name":"scope-order","private":true,"main":"src/index.ts"}"#,
    );
    write(
        &root,
        "tsconfig.json",
        r#"{"compilerOptions":{"strict":true,"module":"esnext","moduleResolution":"bundler","target":"es2022"},"include":["src"]}"#,
    );
    // `src/lonely.ts` is an unused file, so the rule pass hides its export by
    // default. The test reads the type-aware decision for that export.
    write(&root, ".fallowrc.json", r#"{"showCascade":true}"#);
    write(
        &root,
        "src/index.ts",
        "export const main = (): number => 2;\n",
    );
    write(
        &root,
        "src/lonely.ts",
        "export const helper = (): number => 1;\n",
    );
    write(
        &root,
        "src/orphan.ts",
        "import { helper } from \"./lonely\";\nexport const orphanValue = helper();\n",
    );
    (dir, root)
}

/// The run inputs of an editor session of `root`, as the language server
/// gives them: dead code and duplication, no health.
fn editor_run_inputs(root: &Path) -> fallow_config::RunInputs {
    let origin = Origin::ApiOption("test");
    let mut inputs = fallow_config::RunInputs::new(
        Surface::Api,
        EnvSnapshot::capture().with_cwd(root.to_path_buf()),
        CommandKind::Editor,
        SelectionInput::Booleans {
            dead_code: true,
            duplication: true,
            health: false,
            origin,
        },
    );
    inputs.root = Some(Given {
        value: root.to_path_buf(),
        origin,
    });
    inputs
}

#[test]
fn changed_files_scope_after_type_aware_pass_keeps_real_finding() {
    if std::env::var_os("FALLOW_TYPE_AWARE_BIN").is_none() {
        rerun_with_type_aware_sidecar(TEST_NAME);
        return;
    }
    let (_dir, root) = fixture();
    let changed: FxHashSet<PathBuf> = std::iter::once(root.join("src/lonely.ts")).collect();

    let run = fallow_api::run::resolve_run(&editor_run_inputs(&root)).expect("resolve the run");
    let session = EditorAnalysisSession::open(&run).expect("open the editor session");
    let mut output = session
        .analyze_project_with_changed_files(&DuplicatesConfig::default(), false, Some(&changed))
        .expect("analyze project");
    let mut semantic = TypeAwareSession::new(&root).expect("start type-aware session");
    session
        .refine_type_aware_dead_code_in_session(
            &mut semantic,
            None,
            &TypeAwareOptions {
                enabled: Some(true),
                projects: Vec::new(),
                require: TypeAwareRequire::Complete,
            },
            &DeadCodeFilters::default(),
            &mut output.dead_code,
        )
        .expect("type-aware pass completes");
    session.apply_change_scope(
        &mut output,
        &fallow_api::ChangeScope::changed_files(&changed),
    );

    let results = &output.dead_code.results;
    let relative = |path: &Path| -> String {
        path.strip_prefix(&root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let helper = results
        .unused_exports
        .iter()
        .find(|finding| {
            relative(&finding.export.path) == "src/lonely.ts"
                && finding.export.export_name == "helper"
        })
        .unwrap_or_else(|| {
            panic!(
                "a reference in an unused file outside the changed files must not credit `helper`: {:?}",
                results.unused_exports
            )
        });
    let decision = helper
        .semantic
        .as_ref()
        .expect("the type-aware pass records a decision for `helper`");
    assert_eq!(
        decision.decision,
        SemanticCandidateDecisionKind::RetainedAbstained,
        "the only evidence for `helper` comes from an unreachable file: {decision:?}"
    );
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| relative(&finding.file.path))
        .collect();
    assert_eq!(
        unused_files,
        ["src/lonely.ts"],
        "the scope keeps only the unused files in the changed files"
    );
}
