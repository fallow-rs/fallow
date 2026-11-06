//! Credit for files that a config or a manifest names but no import reaches.
//!
//! - An ESLint `extends` entry with a subpath loads a file of a workspace
//!   package. The file is used also when no install links the package into
//!   `node_modules`.
//! - A Jest `preset` that is a relative path names a config file. That file is
//!   used, and the packages that it names are credited.
//!   Manual mocks in `__mocks__` stay used when the preset sets `testMatch`.
//! - Each condition target of a package.json `imports` entry is used when an
//!   import reaches the entry, not only the target for the active conditions.
//!   The targets export the same names by design, so they are not duplicate
//!   exports.

use std::path::Path;

use super::common::create_config;

fn write(root: &Path, rel: &str, contents: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("parent dir")).expect("create dir");
    std::fs::write(path, contents).expect("write file");
}

fn unused_file_paths(results: &fallow_core::results::AnalysisResults, root: &Path) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .strip_prefix(root)
                .unwrap_or(&f.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn unused_dependency_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.clone())
        .chain(
            results
                .unused_dev_dependencies
                .iter()
                .map(|d| d.dep.package_name.clone()),
        )
        .collect()
}

fn duplicate_export_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .duplicate_exports
        .iter()
        .map(|d| d.export.export_name.clone())
        .collect()
}

fn unused_export_names(results: &fallow_core::results::AnalysisResults) -> Vec<String> {
    results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.clone())
        .collect()
}

#[test]
fn extends_subpath_credits_the_files_of_an_unlinked_workspace_config_package() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"root","private":true,"workspaces":["apps/*","packages/*"],"devDependencies":{"eslint":"8.57.1"}}"#,
    );
    write(
        root,
        "apps/api/package.json",
        r#"{"name":"api","main":"index.js","devDependencies":{"eslint-config-shared":"workspace:*"}}"#,
    );
    write(root, "apps/api/index.js", "module.exports = 1;\n");
    write(
        root,
        "apps/api/.eslintrc.cjs",
        "module.exports = { root: true, extends: ['eslint-config-shared/back', 'shared/back/typescript'] };\n",
    );
    write(
        root,
        "packages/eslint-config-shared/package.json",
        r#"{"name":"eslint-config-shared","private":true,"main":"index.js"}"#,
    );
    write(
        root,
        "packages/eslint-config-shared/back/index.js",
        "module.exports = { rules: {} };\n",
    );
    write(
        root,
        "packages/eslint-config-shared/back/typescript.js",
        "module.exports = { extends: ['./index.js'] };\n",
    );
    write(
        root,
        "packages/eslint-config-shared/unused.js",
        "module.exports = {};\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused = unused_file_paths(&results, root);
    assert!(
        !unused
            .iter()
            .any(|path| path.starts_with("packages/eslint-config-shared/back/")),
        "files that an extends entry loads must be used: {unused:?}"
    );
    assert!(
        unused
            .iter()
            .any(|path| path == "packages/eslint-config-shared/unused.js"),
        "a file that no extends entry loads must stay unused: {unused:?}"
    );
}

#[test]
fn relative_jest_preset_is_read_as_jest_config() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"root","private":true,"workspaces":["packages/*"],"devDependencies":{"jest":"29.7.0","jest-shared-setup":"1.0.0","jest-shared-transform":"1.0.0"}}"#,
    );
    write(
        root,
        "jest-preset.front.js",
        "module.exports = {\n  setupFiles: ['jest-shared-setup/setup'],\n  transform: { '\\\\.js$': 'jest-shared-transform' },\n  setupFilesAfterEnv: ['<rootDir>/test/after-env.js'],\n  testMatch: ['**/tests/**/*.test.js'],\n};\n",
    );
    write(
        root,
        "packages/web/src/hooks/useThing.js",
        "export const useThing = () => 1;\n",
    );
    write(
        root,
        "packages/web/src/hooks/__mocks__/useThing.js",
        "export const useThing = () => 2;\n",
    );
    write(
        root,
        "packages/web/src/tests/index.test.js",
        "import { useThing } from '../hooks/useThing';\njest.mock('../hooks/useThing');\ntest('x', () => useThing());\n",
    );
    write(
        root,
        "packages/web/test/after-env.js",
        "module.exports = {};\n",
    );
    write(
        root,
        "packages/web/package.json",
        r#"{"name":"web","main":"src/index.js","scripts":{"test":"jest"}}"#,
    );
    write(root, "packages/web/src/index.js", "module.exports = 1;\n");
    write(
        root,
        "packages/web/jest.config.js",
        "module.exports = { preset: '../../jest-preset.front.js' };\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results, root);
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "jest-preset.front.js"),
        "a relative Jest preset file must be used: {unused_files:?}"
    );
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "packages/web/test/after-env.js"),
        "a <rootDir> setup file of a preset resolves against the consumer: {unused_files:?}"
    );
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "packages/web/src/hooks/__mocks__/useThing.js"),
        "a manual mock must stay used when the preset sets testMatch: {unused_files:?}"
    );
    let duplicates = duplicate_export_names(&results);
    assert!(
        !duplicates.iter().any(|name| name == "useThing"),
        "a manual mock must not be a duplicate export of its module: {duplicates:?}"
    );
    let unused_deps = unused_dependency_names(&results);
    for name in ["jest-shared-setup", "jest-shared-transform"] {
        assert!(
            !unused_deps.iter().any(|dep| dep == name),
            "a package that the preset names must be credited, {name}: {unused_deps:?}"
        );
    }
}

#[test]
fn every_condition_target_of_a_package_imports_entry_is_used() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r##"{
  "name": "app",
  "private": true,
  "main": "./src/index.ts",
  "imports": {
    "#validate": {
      "types": "./src/validate.ts",
      "browser": "./src/validate.browser.ts",
      "worker": { "import": "./src/validate.worker.ts" },
      "default": "./src/validate.ts"
    },
    "#unused": { "browser": "./src/unused.browser.ts", "default": "./src/unused.ts" }
  }
}"##,
    );
    write(
        root,
        "src/index.ts",
        "import { validate } from '#validate'\nexport const run = () => validate()\n",
    );
    write(
        root,
        "src/validate.ts",
        "export const validate = () => true\nexport const nodeOnly = 1\n",
    );
    write(
        root,
        "src/validate.browser.ts",
        "export const validate = () => false\nexport const browserOnly = 1\n",
    );
    write(
        root,
        "src/validate.worker.ts",
        "export const validate = () => false\n",
    );
    write(root, "src/unused.browser.ts", "export const x = 1\n");
    write(root, "src/unused.ts", "export const x = 1\n");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results, root);
    for path in ["src/validate.browser.ts", "src/validate.worker.ts"] {
        assert!(
            !unused_files.iter().any(|unused| unused == path),
            "a condition target of an imported entry must be used, {path}: {unused_files:?}"
        );
    }
    for path in ["src/unused.browser.ts", "src/unused.ts"] {
        assert!(
            unused_files.iter().any(|unused| unused == path),
            "the targets of an entry that no file imports must stay unused, {path}: {unused_files:?}"
        );
    }
    let unused_exports = unused_export_names(&results);
    assert!(
        !unused_exports.iter().any(|name| name == "validate"),
        "the imported name must be used in every condition target: {unused_exports:?}"
    );
    let duplicates = duplicate_export_names(&results);
    assert!(
        !duplicates.iter().any(|name| name == "validate"),
        "condition targets of one entry are not duplicate exports: {duplicates:?}"
    );
    for name in ["nodeOnly", "browserOnly"] {
        assert!(
            unused_exports.iter().any(|unused| unused == name),
            "a name that no file imports must stay unused, {name}: {unused_exports:?}"
        );
    }
}

#[test]
fn a_re_export_of_a_package_imports_entry_uses_every_condition_target() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r##"{
  "name": "app",
  "private": true,
  "main": "./src/index.ts",
  "imports": {
    "#validate": { "browser": "./src/validate.browser.ts", "default": "./src/validate.ts" }
  }
}"##,
    );
    write(
        root,
        "src/index.ts",
        "import { validate } from './barrel'\nexport const run = () => validate()\n",
    );
    write(
        root,
        "src/barrel.ts",
        "export { validate } from '#validate'\n",
    );
    write(
        root,
        "src/validate.ts",
        "export const validate = () => true\nexport const nodeOnly = 1\n",
    );
    write(
        root,
        "src/validate.browser.ts",
        "export const validate = () => false\nexport const browserOnly = 1\n",
    );

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files = unused_file_paths(&results, root);
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "src/validate.browser.ts"),
        "a re-exported condition target must be used: {unused_files:?}"
    );
    let unused_exports = unused_export_names(&results);
    assert!(
        !unused_exports.iter().any(|name| name == "validate"),
        "the re-exported name must be used in every condition target: {unused_exports:?}"
    );
    for name in ["nodeOnly", "browserOnly"] {
        assert!(
            unused_exports.iter().any(|unused| unused == name),
            "a name that no file re-exports must stay unused, {name}: {unused_exports:?}"
        );
    }
    let duplicates = duplicate_export_names(&results);
    assert!(
        !duplicates.iter().any(|name| name == "validate"),
        "condition targets of one re-exported entry are not duplicate exports: {duplicates:?}"
    );
}
