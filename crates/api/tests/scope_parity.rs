//! Scope flags of `fallow_api`, the runtime behind the MCP typed route and the
//! Node bindings. A scope must narrow the same way as on the CLI.

#![expect(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;
use std::process::Command;

use fallow_api::{
    AnalysisOptions, DeadCodeOptions, DuplicationOptions, run_dead_code, run_duplication,
    serialize_dead_code_programmatic_json, serialize_duplication_programmatic_json,
};
use serde_json::Value;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "commit.gpgsign=false",
            "-c",
            "user.name=fallow",
            "-c",
            "user.email=fallow@example.invalid",
        ])
        .args(args)
        .current_dir(root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn write(root: &Path, path: &str, content: &str) {
    let target = root.join(path);
    fs::create_dir_all(target.parent().unwrap()).unwrap();
    fs::write(target, content).unwrap();
}

fn commit(root: &Path, message: &str) {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", message]);
}

fn analysis(root: &Path) -> AnalysisOptions {
    AnalysisOptions {
        root: Some(root.to_path_buf()),
        no_cache: true,
        ..AnalysisOptions::default()
    }
}

fn dead_code(analysis: AnalysisOptions) -> Value {
    let options = DeadCodeOptions {
        analysis,
        ..DeadCodeOptions::default()
    };
    run_dead_code(&options)
        .and_then(serialize_dead_code_programmatic_json)
        .expect("run the programmatic dead-code analysis")
}

/// The locations of each `duplicate_exports` finding, as relative paths.
fn duplicate_export_owners(report: &Value) -> Vec<Vec<String>> {
    report["duplicate_exports"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|finding| {
            finding["locations"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|location| location["path"].as_str().unwrap().to_string())
                .collect()
        })
        .collect()
}

/// `dup` is exported by three files. `ignoreFindings` matches two of them, so
/// the full run reports the finding: not every owner is ignored.
fn duplicate_export_repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"scope-parity","version":"1.0.0","main":"src/index.ts"}"#,
    );
    write(
        root,
        ".fallowrc.json",
        r#"{"ignoreFindings":["src/x.ts","src/y.ts"]}"#,
    );
    write(
        root,
        "src/index.ts",
        "export * from \"./x\";\nexport * from \"./y\";\nexport * from \"./z\";\n",
    );
    for name in ["x", "y", "z"] {
        write(root, &format!("src/{name}.ts"), "export const dup = 1;\n");
    }
    git(root, &["init", "-q"]);
    commit(root, "base");
    write(root, "src/x.ts", "export const dup = 2;\n");
    write(root, "src/y.ts", "export const dup = 3;\n");
    commit(root, "head");
    dir
}

#[test]
fn a_full_run_reports_a_duplicate_export_with_one_owner_not_ignored() {
    let dir = duplicate_export_repository();
    let report = dead_code(analysis(dir.path()));
    assert_eq!(
        duplicate_export_owners(&report),
        vec![vec!["src/x.ts", "src/y.ts", "src/z.ts"]]
    );
}

#[test]
fn a_duplicate_export_that_only_ignored_owners_hold_after_the_scope_is_hidden() {
    let dir = duplicate_export_repository();
    let report = dead_code(AnalysisOptions {
        changed_since: Some("HEAD~1".to_string()),
        ..analysis(dir.path())
    });
    assert_eq!(
        duplicate_export_owners(&report),
        Vec::<Vec<String>>::new(),
        "`--changed-since` keeps the owners src/x.ts and src/y.ts, which \
         `ignoreFindings` both match, so the finding is hidden as on the CLI"
    );
}

/// The same 13-line function in `packages/a` (`pkg-a`) and `packages/b`
/// (`pkg-b`): one clone group with one instance in each workspace.
fn cross_workspace_clone_repository() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"root","private":true,"workspaces":["packages/*"]}"#,
    );
    let function = "export function compute(input: number): number {\n  let total = input;\n  \
                    for (let index = 0; index < 10; index += 1) {\n    total += index * 2;\n    \
                    if (total > 100) {\n      total -= 7;\n    }\n  }\n  const scaled = total * 3;\n  \
                    const shifted = scaled - 11;\n  const clamped = Math.min(shifted, 999);\n  \
                    return clamped + input;\n}\n";
    for (dir_name, package) in [("a", "pkg-a"), ("b", "pkg-b")] {
        write(
            root,
            &format!("packages/{dir_name}/package.json"),
            &format!(r#"{{"name":"{package}","version":"1.0.0","main":"src/index.ts"}}"#),
        );
        write(
            root,
            &format!("packages/{dir_name}/src/index.ts"),
            "export { compute } from \"./clone\";\n",
        );
        write(root, &format!("packages/{dir_name}/src/clone.ts"), function);
    }
    git(root, &["init", "-q"]);
    commit(root, "base");
    dir
}

/// The instance paths of each clone group, relative to the root.
fn clone_group_files(report: &Value) -> Vec<Vec<String>> {
    report["clone_groups"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|group| {
            let mut files: Vec<String> = group["instances"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|instance| instance["file"].as_str().unwrap().to_string())
                .collect();
            files.sort();
            files.dedup();
            files
        })
        .collect()
}

fn duplication(analysis: AnalysisOptions) -> Value {
    let options = DuplicationOptions {
        analysis,
        ..DuplicationOptions::default()
    };
    run_duplication(&options)
        .and_then(serialize_duplication_programmatic_json)
        .expect("run the programmatic duplication analysis")
}

#[test]
fn a_workspace_scope_keeps_a_clone_group_with_one_instance_in_the_workspace() {
    let dir = cross_workspace_clone_repository();
    let unscoped = clone_group_files(&duplication(analysis(dir.path())));
    let cross = vec![
        "packages/a/src/clone.ts".to_string(),
        "packages/b/src/clone.ts".to_string(),
    ];
    assert!(
        unscoped.contains(&cross),
        "the fixture must hold a clone group across the two workspaces: {unscoped:?}"
    );

    let scoped = clone_group_files(&duplication(AnalysisOptions {
        workspace: Some(vec!["pkg-a".to_string()]),
        ..analysis(dir.path())
    }));
    assert!(
        scoped.contains(&cross),
        "`workspace: pkg-a` keeps the whole group, as `fallow dupes --workspace pkg-a` \
         does: a clone group is in scope when one of its instances is. Got {scoped:?}"
    );
}
