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

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fallow_api::{
    DeadCodeFilters, EditorAnalysisSession, TypeAwareOptions, TypeAwareRequire, TypeAwareSession,
};
use fallow_config::DuplicatesConfig;
use fallow_types::semantic::SemanticCandidateDecisionKind;
use rustc_hash::FxHashSet;

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, content).unwrap();
}

const TEST_NAME: &str = "changed_files_scope_after_type_aware_pass_keeps_real_finding";

/// Run this test again in a child process with the sidecar path in its
/// environment. The type-aware session reads the path only from the process
/// environment, and a test must not change the environment of its own process.
fn rerun_with_type_aware_sidecar() {
    let mut sidecar = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    sidecar.pop();
    sidecar.pop();
    sidecar.push("tools/type-aware-sidecar/fallow-type-aware.mjs");

    let mut command = Command::new(std::env::current_exe().expect("test binary path"));
    command.args(["--exact", TEST_NAME, "--nocapture", "--test-threads=1"]);
    #[cfg(windows)]
    {
        let path = std::env::var_os("PATH").expect("PATH must contain the Node.js runtime");
        let node = std::env::split_paths(&path)
            .map(|entry| entry.join("node.exe"))
            .find(|candidate| candidate.is_file())
            .expect("Node.js executable must be available for type-aware tests");
        command
            .env("FALLOW_TYPE_AWARE_BIN", node)
            .env("FALLOW_TYPE_AWARE_SCRIPT", &sidecar);
    }
    #[cfg(not(windows))]
    command.env("FALLOW_TYPE_AWARE_BIN", &sidecar);

    let output = command.output().expect("run the test in a child process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success() && stdout.contains("1 passed"),
        "child run failed:\n{stdout}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

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
        rerun_with_type_aware_sidecar();
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
    session.apply_changed_files_scope(&mut output.dead_code, Some(&changed));

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
