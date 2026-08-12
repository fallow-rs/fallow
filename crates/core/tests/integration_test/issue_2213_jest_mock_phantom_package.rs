//! Regression tests for issue #2213: `jest.mock('@scope/pkg')` without a
//! factory synthesized the speculative `__mocks__` sibling candidate
//! `@scope/__mocks__/pkg`. Bare specifiers classify as npm packages instead of
//! `Unresolvable`, so the candidate bypassed the speculative drop guard and
//! surfaced as a phantom `unlisted-dependency` finding for `@scope/__mocks__`,
//! blocking CI in gated audits. The resolver now drops speculative candidates
//! that land in package space; only internal manual-mock files count as hits.

use std::path::Path;

use super::common::create_config;

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create parent dir");
    }
    std::fs::write(path, contents).expect("write file");
}

/// The reporter's shape: a listed scoped dependency, a Jest manual mock under
/// the root `__mocks__/` directory, and a test that both imports the real
/// specifier and registers the factory-less automock.
fn create_project(root: &Path) {
    write(
        root.join("package.json").as_path(),
        r#"{
            "name": "issue-2213-repro",
            "private": true,
            "dependencies": { "@bacons/apple-targets": "^5.0.0" },
            "devDependencies": { "jest": "^29.7.0" }
        }"#,
    );
    write(
        root.join("__mocks__/@bacons/apple-targets.ts").as_path(),
        r#"export const ExtensionStorage = { get: (): string => "mock" };"#,
    );
    write(
        root.join("src/index.ts").as_path(),
        r#"import { ExtensionStorage } from "@bacons/apple-targets";
           export const read = (): string => ExtensionStorage.get();"#,
    );
    // The second import is the positive control: a genuinely unlisted package
    // must keep surfacing, so a regression that suppresses every unlisted
    // dependency cannot make this test pass.
    write(
        root.join("src/__tests__/widget.test.ts").as_path(),
        r#"import { ExtensionStorage } from "@bacons/apple-targets";
           import { helper } from "definitely-not-installed";
           jest.mock("@bacons/apple-targets");
           test("reads", () => {
             expect(ExtensionStorage.get()).toBe("mock");
             expect(helper).toBeDefined();
           });"#,
    );
    let pkg = root.join("node_modules/@bacons/apple-targets");
    write(
        pkg.join("package.json").as_path(),
        r#"{ "name": "@bacons/apple-targets", "version": "5.0.0", "main": "index.js" }"#,
    );
    write(
        pkg.join("index.js").as_path(),
        "module.exports = { ExtensionStorage: { get: () => 'real' } };",
    );
}

#[test]
fn issue_2213_jest_mock_does_not_fabricate_phantom_unlisted_dependency() {
    let dir = tempfile::tempdir().expect("temp dir");
    create_project(dir.path());

    let config = create_config(dir.path().to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|finding| finding.dep.package_name.as_str())
        .collect();
    assert!(
        !unlisted.contains(&"@bacons/__mocks__"),
        "factory-less jest.mock of a scoped package must not fabricate a \
         phantom `@scope/__mocks__` unlisted dependency, got {unlisted:?}"
    );
    assert!(
        unlisted.contains(&"definitely-not-installed"),
        "a genuinely unlisted dependency must still be reported, got {unlisted:?}"
    );

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    assert!(
        !unresolved
            .iter()
            .any(|specifier| specifier.contains("__mocks__")),
        "the speculative mock candidate must not surface as unresolved-import, got {unresolved:?}"
    );
}
