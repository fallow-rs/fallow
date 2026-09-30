//! The editor narrows dead-code findings to the changed files after the
//! type-aware pass. That pass reads `unused_files` as its set of unreachable
//! files. A scope before the pass removes an unused file outside the changed
//! files from that set, and a reference in that file then credits a real
//! finding in a changed file.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::path::{Path, PathBuf};

use fallow_api::{
    DeadCodeFilters, EditorAnalysisSession, TypeAwareOptions, TypeAwareRequire, TypeAwareSession,
};
use fallow_config::DuplicatesConfig;
use fallow_types::semantic::SemanticCandidateDecisionKind;
use rustc_hash::FxHashSet;

use crate::common::{rerun_with_type_aware_sidecar, write};

/// The full test name in the `integration` binary. The child run filters on
/// it with `--exact`, so it includes the module path.
const TEST_NAME: &str =
    "editor_type_aware_scope_order::changed_files_scope_after_type_aware_pass_keeps_real_finding";

fn fixture() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().canonicalize().unwrap();
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

#[test]
fn changed_files_scope_after_type_aware_pass_keeps_real_finding() {
    if std::env::var_os("FALLOW_TYPE_AWARE_BIN").is_none() {
        rerun_with_type_aware_sidecar(TEST_NAME);
        return;
    }
    let (_dir, root) = fixture();
    let changed: FxHashSet<PathBuf> = std::iter::once(root.join("src/lonely.ts")).collect();

    let session = EditorAnalysisSession::load(&root, None).expect("load editor session");
    let mut output = session
        .analyze_project_with_changed_files(&DuplicatesConfig::default(), false, Some(&changed))
        .expect("analyze project");
    let mut semantic = TypeAwareSession::new(&root).expect("start type-aware session");
    session
        .refine_type_aware_dead_code_in_session(
            &mut semantic,
            None,
            &TypeAwareOptions {
                enabled: true,
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
