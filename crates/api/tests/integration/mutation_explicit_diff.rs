#![expect(clippy::unwrap_used, reason = "test fixture assertions")]

use super::common::{commit, git, write};
use fallow_api::{
    AnalysisOptions, MutationRunOptions, MutationRunScope, plan_mutation, run_mutation,
};

#[test]
fn explicit_unplaceable_diff_refuses_generated_mutation_scope() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(root, "package.json", r#"{"name":"p","main":"src/math.ts"}"#);
    write(
        root,
        "src/math.ts",
        "export function add(a: number, b: number) { return a + b; }\n",
    );
    git(root, &["init", "-q"]);
    commit(root, "init");
    write(
        root,
        "src/math.ts",
        "export function add(a: number, b: number) { return a - b; }\n",
    );
    let patch = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        patch.path(),
        "--- a/nowhere/x.ts\n+++ b/nowhere/x.ts\n@@ -1 +1 @@\n-a\n+b\n",
    )
    .unwrap();
    let mut options = MutationRunOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        scope: MutationRunScope::Changed {
            base: Some("HEAD".into()),
        },
        allow_dirty: true,
        ..MutationRunOptions::default()
    };
    let generated = plan_mutation(&options).unwrap();
    assert!(
        generated
            .mutants
            .iter()
            .any(|m| m.location.file == "src/math.ts")
    );
    options.analysis.diff_file = Some(patch.path().to_path_buf());
    let error = plan_mutation(&options).unwrap_err();
    assert_eq!(error.exit_code, 2);
    assert_eq!(
        error.code.as_deref(),
        Some("FALLOW_MUTATION_DIFF_NOT_APPLIED")
    );
    assert!(error.message.contains("foreign-namespace"), "{error}");
    let error = run_mutation(&options).unwrap_err();
    assert_eq!(
        error.code.as_deref(),
        Some("FALLOW_MUTATION_DIFF_NOT_APPLIED")
    );
}

#[test]
fn supplied_empty_placed_and_ambient_diff_scopes_remain_distinct() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(root, "package.json", r#"{"name":"p","main":"src/math.ts"}"#);
    let source = "export function add(a: number, b: number) { return a + b; }\n";
    write(root, "src/math.ts", source);
    write(root, "src/other.ts", source);
    git(root, &["init", "-q"]);
    commit(root, "init");
    write(root, "src/math.ts", &source.replace("a + b", "a - b"));
    let patch = tempfile::NamedTempFile::new().unwrap();
    let mut options = MutationRunOptions {
        analysis: AnalysisOptions {
            root: Some(root.to_path_buf()),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        scope: MutationRunScope::Changed {
            base: Some("HEAD".into()),
        },
        ..MutationRunOptions::default()
    };
    let generated = plan_mutation(&options).unwrap();
    assert!(
        generated
            .mutants
            .iter()
            .any(|m| m.location.file == "src/math.ts")
    );
    options.analysis.diff_file = Some(patch.path().into());
    let empty = plan_mutation(&options).unwrap();
    assert!(empty.mutants.is_empty());
    std::fs::write(
        patch.path(),
        "--- a/src/other.ts\n+++ b/src/other.ts\n@@ -1 +1 @@\n-a\n+b\n",
    )
    .unwrap();
    let placed = plan_mutation(&options).unwrap();
    assert!(
        placed
            .mutants
            .iter()
            .any(|m| m.location.file == "src/other.ts")
    );
    assert!(
        placed
            .mutants
            .iter()
            .all(|m| m.location.file == "src/other.ts")
    );
    std::fs::write(
        patch.path(),
        "--- a/nowhere/x.ts\n+++ b/nowhere/x.ts\n@@ -1 +1 @@\n-a\n+b\n",
    )
    .unwrap();
    options.analysis.diff_file = None;
    options.analysis.ambient_diff_file = Some(patch.path().into());
    let ambient = plan_mutation(&options).unwrap();
    assert_eq!(
        serde_json::to_value(ambient.mutants).unwrap(),
        serde_json::to_value(generated.mutants).unwrap()
    );
    options.analysis.diff_file = Some(patch.path().into());
    for scope in [
        MutationRunScope::All,
        MutationRunScope::Paths(vec!["src/math.ts".into()]),
    ] {
        options.scope = scope;
        assert!(
            plan_mutation(&options)
                .unwrap()
                .mutants
                .iter()
                .any(|m| m.location.file == "src/math.ts")
        );
    }
}

#[test]
fn ambiguous_patch_refuses_mutation_from_a_nested_root() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let source = "export function add(a: number, b: number) { return a + b; }\n";
    for prefix in ["", "packages/app/"] {
        write(
            root,
            &format!("{prefix}package.json"),
            r#"{"name":"p","main":"src/math.ts"}"#,
        );
        write(root, &format!("{prefix}src/math.ts"), source);
    }
    git(root, &["init", "-q"]);
    commit(root, "init");
    write(
        root,
        "packages/app/src/math.ts",
        &source.replace("a + b", "a - b"),
    );
    let patch = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        patch.path(),
        "--- a/src/math.ts\n+++ b/src/math.ts\n@@ -1 +1 @@\n-a\n+b\n",
    )
    .unwrap();
    let mut options = MutationRunOptions {
        analysis: AnalysisOptions {
            root: Some(root.join("packages/app")),
            no_cache: true,
            ..AnalysisOptions::default()
        },
        scope: MutationRunScope::All,
        ..MutationRunOptions::default()
    };
    assert!(
        plan_mutation(&options)
            .unwrap()
            .mutants
            .iter()
            .any(|m| m.location.file == "src/math.ts")
    );
    options.scope = MutationRunScope::Changed {
        base: Some("HEAD".into()),
    };
    options.analysis.diff_file = Some(patch.path().into());
    let error = plan_mutation(&options).unwrap_err();
    assert_eq!(
        error.code.as_deref(),
        Some("FALLOW_MUTATION_DIFF_NOT_APPLIED")
    );
    assert!(error.message.contains("ambiguous-base"), "{error}");
}
