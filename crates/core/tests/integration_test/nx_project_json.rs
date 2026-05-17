use super::common::create_config;

#[test]
fn nx_project_json_marks_nested_main_as_reachable_without_workspace_package_json() {
    let tmp = tempfile::tempdir().expect("tempdir should be created");
    let root = tmp.path();

    std::fs::create_dir_all(root.join("apps/query/src")).expect("app source dir should exist");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "query-monorepo",
            "dependencies": {
                "nx": "1.0.0"
            }
        }"#,
    )
    .expect("root package.json should be written");
    std::fs::write(
        root.join("apps/query/project.json"),
        r#"{
            "targets": {
                "build": {
                    "executor": "@nx/js:tsc",
                    "options": {
                        "main": "apps/query/src/main.ts"
                    }
                }
            }
        }"#,
    )
    .expect("project.json should be written");
    std::fs::write(
        root.join("apps/query/src/main.ts"),
        "import { helper } from './helper';\nconsole.log(helper);\n",
    )
    .expect("main.ts should be written");
    std::fs::write(
        root.join("apps/query/src/helper.ts"),
        "export const helper = 1;\nexport const unusedHelper = 2;\n",
    )
    .expect("helper.ts should be written");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|issue| {
            issue
                .file
                .path
                .strip_prefix(root)
                .unwrap_or(&issue.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    assert!(
        !unused_files.contains(&"apps/query/src/main.ts".to_string()),
        "main.ts should be reachable via Nx project.json, unused files: {unused_files:?}"
    );
    assert!(
        !unused_files.contains(&"apps/query/src/helper.ts".to_string()),
        "helper.ts should be reachable from main.ts, unused files: {unused_files:?}"
    );

    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|issue| issue.export.export_name.clone())
        .collect();
    assert!(
        unused_exports.contains(&"unusedHelper".to_string()),
        "unusedHelper should still be reported, unused exports: {unused_exports:?}"
    );
    assert!(
        results.unresolved_imports.is_empty(),
        "imports should resolve, found unresolved imports: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|issue| issue.import.specifier.as_str())
            .collect::<Vec<_>>()
    );
}
