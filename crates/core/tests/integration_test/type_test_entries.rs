use std::path::Path;

use super::common::{create_config, create_production_config};

fn write_type_test_project(root: &Path, production_import: bool) {
    std::fs::create_dir_all(root.join("source")).expect("source directory");
    std::fs::create_dir_all(root.join("test-d")).expect("type-test directory");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "expect-type-project",
            "main": "./source/index.ts",
            "devDependencies": {"expect-type": "^1.0.0"}
        }"#,
    )
    .expect("package manifest");
    let production_source = if production_import {
        "import { expectTypeOf } from 'expect-type';\nexport interface Options { value: string }\nexport const productionCheck = expectTypeOf<Options>();\n"
    } else {
        "export interface Options { value: string }\n"
    };
    std::fs::write(root.join("source/index.ts"), production_source).expect("source entry");
    std::fs::write(
        root.join("test-d/options.test-d.ts"),
        "import { expectTypeOf } from 'expect-type';\nimport type { Options } from '../source/index';\nexpectTypeOf<Options>().toEqualTypeOf<{ value: string }>();\n",
    )
    .expect("type-test file");
}

#[test]
fn expect_type_convention_reaches_test_d_without_crediting_runtime_dependencies() {
    let directory = tempfile::tempdir().expect("temporary project directory");
    let root = directory.path();
    write_type_test_project(root, false);
    std::fs::write(root.join("source/orphan.ts"), "export const unused = 1;\n")
        .expect("unreachable source file");

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");
    assert!(
        !results
            .unused_files
            .iter()
            .any(|finding| finding.file.path.ends_with("test-d/options.test-d.ts")),
        "expect-type declaration tests should be reached as test roots"
    );
    assert!(
        results
            .unused_files
            .iter()
            .any(|finding| finding.file.path.ends_with("source/orphan.ts")),
        "discovering type tests must not protect unrelated source files"
    );

    let production_results = fallow_core::analyze(&create_production_config(root.to_path_buf()))
        .expect("production analysis should succeed");
    assert!(
        !production_results
            .dev_dependencies_in_production
            .iter()
            .any(|finding| finding.dep.package_name == "expect-type"),
        "a dependency imported only by test-d must not count as production usage"
    );

    write_type_test_project(root, true);
    let production_misuse = fallow_core::analyze(&create_production_config(root.to_path_buf()))
        .expect("production analysis should succeed");
    assert!(
        production_misuse
            .dev_dependencies_in_production
            .iter()
            .any(|finding| finding.dep.package_name == "expect-type"),
        "a runtime import of expect-type must remain a production dependency finding"
    );
}
