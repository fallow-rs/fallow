use std::path::Path;

use super::common::create_config;

#[derive(Clone, Copy)]
enum TsconfigShape {
    Root,
    Workspace,
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

fn create_issue_1032_project(root: &Path, shape: TsconfigShape) {
    write(
        &root.join("package.json"),
        r#"{
            "private": true,
            "workspaces": ["modules/*"]
        }"#,
    );
    write(
        &root.join("modules/App/package.json"),
        r#"{
            "name": "app",
            "private": true,
            "type": "module",
            "main": "src/index.ts",
            "dependencies": {
                "@radix-ui/react-checkbox": "1.0.0"
            }
        }"#,
    );
    write(
        &root.join("modules/Firlefanz/package.json"),
        r#"{
            "name": "@darkaura/firlefanz",
            "private": true,
            "type": "module"
        }"#,
    );
    write(
        &root.join("modules/App/src/index.ts"),
        r#"import { checkboxRoot } from "@radix-ui/react-checkbox";
           import { componentPatternTypes } from "@darkaura/firlefanz/lib/ecs/componentPatternTypes";

           console.log(checkboxRoot, componentPatternTypes);"#,
    );
    write(
        &root.join("modules/Firlefanz/src/lib/ecs/componentPatternTypes.ts"),
        r#"export const componentPatternTypes = ["position", "renderable"] as const;"#,
    );
    write(
        &root.join("modules/Firlefanz/src/lib/ecs/dead.ts"),
        r#"export const unusedComponentPattern = "dead";"#,
    );

    match shape {
        TsconfigShape::Root => write(
            &root.join("tsconfig.json"),
            r#"{
                "compilerOptions": {
                    "baseUrl": "modules/App",
                    "paths": {
                        "@darkaura/firlefanz/*": ["../Firlefanz/src/*"]
                    }
                }
            }"#,
        ),
        TsconfigShape::Workspace => write(
            &root.join("modules/App/tsconfig.json"),
            r#"{
                "compilerOptions": {
                    "baseUrl": ".",
                    "paths": {
                        "@darkaura/firlefanz/*": ["../Firlefanz/src/*"]
                    }
                }
            }"#,
        ),
    }
}

fn assert_issue_1032_project_resolves(root: &Path) {
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.to_string_lossy().replace('\\', "/"))
        .collect();

    assert!(
        !unused_files
            .iter()
            .any(|path| path.ends_with("modules/Firlefanz/src/lib/ecs/componentPatternTypes.ts")),
        "sibling source file imported through tsconfig paths should be reachable, got {unused_files:?}"
    );
    assert!(
        unused_files
            .iter()
            .any(|path| path.ends_with("modules/Firlefanz/src/lib/ecs/dead.ts")),
        "unreferenced sibling source file should still report unused, got {unused_files:?}"
    );

    let unresolved_imports: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    for specifier in [
        "@darkaura/firlefanz/lib/ecs/componentPatternTypes",
        "@radix-ui/react-checkbox",
    ] {
        assert!(
            !unresolved_imports.contains(&specifier),
            "{specifier} should resolve or remain package-shaped, got {unresolved_imports:?}"
        );
    }

    let unlisted_dependencies: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    for package_name in ["@darkaura/firlefanz", "@radix-ui/react-checkbox"] {
        assert!(
            !unlisted_dependencies.contains(&package_name),
            "{package_name} should not be reported as unlisted, got {unlisted_dependencies:?}"
        );
    }

    let unused_dependencies: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dependencies.contains(&"@radix-ui/react-checkbox"),
        "scoped npm dependency should stay package-shaped and credited, got {unused_dependencies:?}"
    );
}

#[test]
fn issue_1032_root_tsconfig_paths_resolve_sibling_src_package_alias() {
    let dir = tempfile::tempdir().expect("temp dir");
    create_issue_1032_project(dir.path(), TsconfigShape::Root);

    assert_issue_1032_project_resolves(dir.path());
}

#[test]
fn issue_1032_workspace_tsconfig_paths_resolve_sibling_src_package_alias() {
    let dir = tempfile::tempdir().expect("temp dir");
    create_issue_1032_project(dir.path(), TsconfigShape::Workspace);

    assert_issue_1032_project_resolves(dir.path());
}
