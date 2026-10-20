use std::path::Path;

use fallow_types::results::AnalysisResults;

use super::common::{create_config, fixture_path};

fn unlisted_sites(results: &AnalysisResults, root: &Path) -> Vec<(String, String)> {
    let mut sites: Vec<_> = results
        .unlisted_dependencies
        .iter()
        .flat_map(|finding| {
            finding.dep.imported_from.iter().map(|site| {
                (
                    finding.dep.package_name.clone(),
                    site.path
                        .strip_prefix(root)
                        .expect("project-relative site")
                        .to_string_lossy()
                        .replace('\\', "/"),
                )
            })
        })
        .collect();
    sites.sort();
    sites
}

#[test]
fn nested_standalone_manifest_owns_dependencies_without_workspace_membership() {
    let root = fixture_path("issue-3324-nested-package");
    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis succeeds");
    assert_eq!(
        unlisted_sites(&results, &root),
        vec![
            ("@repro/lib".into(), "index.ts".into()),
            ("@repro/missing".into(), "scripts/tool/index.ts".into()),
            ("@repro/owned".into(), "index.ts".into()),
            ("@repro/owned".into(), "scripts/sibling/index.ts".into()),
        ],
        "nested declarations apply only to their own files; root and sibling imports stay unlisted"
    );
    assert!(
        fallow_config::workspace_diagnostics_for(&root)
            .iter()
            .any(|diagnostic| {
                diagnostic.kind == fallow_config::WorkspaceDiagnosticKind::UndeclaredWorkspace
                    && diagnostic.path == root.join("scripts/tool")
            })
    );
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create directory");
    std::fs::write(path, contents).expect("write file");
}

fn root_project(root: &Path) {
    write(
        &root.join("package.json"),
        r#"{ "name": "root", "private": true, "workspaces": ["packages/*"], "dependencies": { "@repro/rootdep": "*" } }"#,
    );
    write(
        &root.join("packages/lib/package.json"),
        r#"{ "name": "@repro/lib" }"#,
    );
    write(
        &root.join("packages/lib/index.ts"),
        "export const lib = 1;\n",
    );
}

#[test]
fn nearest_standalone_owner_preserves_private_and_publishable_inheritance() {
    for private in [false, true] {
        let temp = tempfile::tempdir().expect("temporary project");
        let root = temp.path().canonicalize().expect("canonical project");
        root_project(&root);
        write(
            &root.join("index.ts"),
            "import './standalone/child/src/index'; import '@repro/missing';\n",
        );
        write(
            &root.join("standalone/package.json"),
            r#"{ "name": "outer", "private": true, "dependencies": { "@repro/ancestor": "*" } }"#,
        );
        write(
            &root.join("standalone/child/package.json"),
            &format!(
                r#"{{ "name": "child", "private": {private}, "dependencies": {{ "@repro/own": "*" }} }}"#
            ),
        );
        write(
            &root.join("standalone/child/src/index.ts"),
            "import '@repro/own'; import '@repro/ancestor'; import '@repro/rootdep'; import 'child'; import './vite.config';\n",
        );
        write(
            &root.join("standalone/child/src/vite.config.ts"),
            "import '@repro/ancestor'; import '@repro/rootdep';\n",
        );
        let results =
            fallow_core::analyze(&create_config(root.clone())).expect("analysis succeeds");
        let expected = if private {
            vec![("@repro/missing".into(), "index.ts".into())]
        } else {
            vec![
                (
                    "@repro/ancestor".into(),
                    "standalone/child/src/index.ts".into(),
                ),
                ("@repro/missing".into(), "index.ts".into()),
                (
                    "@repro/rootdep".into(),
                    "standalone/child/src/index.ts".into(),
                ),
            ]
        };
        assert_eq!(
            unlisted_sites(&results, &root),
            expected,
            "private={private}"
        );
    }
}

#[test]
fn symlink_ignored_malformed_and_outside_manifests_do_not_credit_imports() {
    for mode in [
        "missing",
        "malformed",
        "ignored",
        "symlink-outside",
        "symlink-sibling",
    ] {
        let temp = tempfile::tempdir().expect("temporary project");
        let parent = temp.path().canonicalize().expect("canonical parent");
        let root = parent.join("project");
        root_project(&root);
        write(
            &parent.join("package.json"),
            r#"{ "dependencies": { "@repro/invalid": "*" } }"#,
        );
        write(
            &root.join("index.ts"),
            "import './valid/src/index'; import './invalid/src/index'; import '@repro/invalid';\n",
        );
        write(
            &root.join("valid/package.json"),
            r#"{ "name": "valid", "dependencies": { "@repro/own": "*" } }"#,
        );
        write(&root.join("valid/src/index.ts"), "import '@repro/own';\n");
        write(
            &root.join("invalid/src/index.ts"),
            "import '@repro/invalid';\n",
        );
        let path = root.join("invalid/package.json");
        match mode {
            "malformed" => write(&path, "{broken"),
            "ignored" => write(&path, r#"{ "dependencies": { "@repro/invalid": "*" } }"#),
            "symlink-outside" | "symlink-sibling" => {
                let target = if mode == "symlink-sibling" {
                    let sibling = root.join("sibling/package.json");
                    write(&sibling, r#"{ "dependencies": { "@repro/invalid": "*" } }"#);
                    sibling
                } else {
                    parent.join("package.json")
                };
                #[cfg(unix)]
                std::os::unix::fs::symlink(&target, &path).expect("symlink manifest");
                #[cfg(windows)]
                std::os::windows::fs::symlink_file(&target, &path).expect("symlink manifest");
            }
            _ => {}
        }
        let mut config = create_config(root.clone());
        if mode == "ignored" {
            let mut patterns = globset::GlobSetBuilder::new();
            patterns.add(globset::Glob::new("invalid/package.json").expect("valid ignore glob"));
            config.ignore_patterns =
                fallow_config::IgnorePatternSet::new(patterns.build().expect("glob set"), &[]);
        }
        let results = fallow_core::analyze(&config).expect("analysis succeeds");
        assert_eq!(
            unlisted_sites(&results, &root),
            vec![
                ("@repro/invalid".into(), "index.ts".into()),
                ("@repro/invalid".into(), "invalid/src/index.ts".into()),
            ],
            "mode={mode}"
        );
    }
}

#[test]
fn nameless_nested_manifests_do_not_block_root_declarations() {
    let temp = tempfile::tempdir().expect("temporary project");
    let root = temp.path().canonicalize().expect("canonical project");
    write(
        &root.join("package.json"),
        r#"{ "name": "root", "workspaces": ["packages/*"], "dependencies": { "react": "*" } }"#,
    );
    write(
        &root.join("index.ts"),
        "import './src/esm/util'; import './src/local/util';\n",
    );
    write(
        &root.join("src/esm/package.json"),
        r#"{ "type": "module" }"#,
    );
    write(&root.join("src/esm/util.ts"), "import 'react';\n");
    write(
        &root.join("src/local/package.json"),
        r#"{ "dependencies": { "@repro/local": "*" } }"#,
    );
    write(
        &root.join("src/local/util.ts"),
        "import 'react'; import '@repro/local';\n",
    );
    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis succeeds");
    assert_eq!(
        unlisted_sites(&results, &root),
        Vec::<(String, String)>::new(),
        "a marker manifest is not an owner, and a nameless manifest inherits root declarations"
    );
}

#[test]
fn metadata_only_nested_manifests_inherit_ancestor_declarations() {
    let temp = tempfile::tempdir().expect("temporary project");
    let root = temp.path().canonicalize().expect("canonical project");
    write(
        &root.join("package.json"),
        r#"{ "name": "root", "private": true, "workspaces": ["examples/*"], "dependencies": { "@repro/rootdep": "*" } }"#,
    );
    write(
        &root.join("examples/demo/package.json"),
        r#"{ "name": "demo", "dependencies": { "egg": "*" } }"#,
    );
    write(
        &root.join("examples/demo/index.ts"),
        "import './app/biz/Foo';\n",
    );
    write(
        &root.join("examples/demo/app/biz/package.json"),
        r#"{ "name": "biz", "eggModule": { "name": "biz" } }"#,
    );
    write(
        &root.join("examples/demo/app/biz/Foo.ts"),
        "import 'egg'; import '@repro/rootdep'; import 'biz'; import '@repro/missing';\n",
    );
    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis succeeds");
    assert_eq!(
        unlisted_sites(&results, &root),
        vec![(
            "@repro/missing".into(),
            "examples/demo/app/biz/Foo.ts".into()
        )],
        "a manifest that declares no dependencies does not stop the walk to ancestor manifests"
    );
}
