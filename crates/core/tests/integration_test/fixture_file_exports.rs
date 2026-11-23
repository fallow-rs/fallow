//! A test-runner fixture pattern such as `**/fixtures/**` keeps each matching
//! file alive. When a module outside the fixture directories imports a name
//! from such a file, the file is a module of the project, and each export
//! that no file imports is unused (issue #5082).

use std::fs;
use std::path::Path;

use fallow_config::{FallowConfig, OutputFormat};

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

fn analyze(root: &Path) -> fallow_types::results::AnalysisResults {
    let config = FallowConfig::default().resolve(
        root.to_path_buf(),
        OutputFormat::Human,
        1,
        true,
        true,
        None,
    );
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn unused_export_names(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    let mut names: Vec<String> = results
        .unused_exports
        .iter()
        .map(|finding| finding.export.export_name.clone())
        .collect();
    names.sort_unstable();
    names
}

fn unused_file_names(results: &fallow_types::results::AnalysisResults) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| {
            finding
                .file
                .path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

fn write_project(root: &Path, runner: &str) {
    write(
        root,
        "package.json",
        &format!(r#"{{"name":"r","private":true,"devDependencies":{{"{runner}":"*"}}}}"#),
    );
    write(root, "src/main.ts", "export const x = 1;\n");
}

#[test]
fn vitest_fixture_imported_by_name_reports_its_unused_export() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_project(root, "vitest");
    write(
        root,
        "src/main.test.ts",
        "import { used } from \"./testkit/fixtures/shared.ts\";\nconsole.log(used);\n",
    );
    write(
        root,
        "src/testkit/fixtures/shared.ts",
        "export const used = 1;\nexport const unusedFixture = 2;\n",
    );

    let results = analyze(root);

    assert_eq!(unused_export_names(&results), ["unusedFixture"]);
    assert!(unused_file_names(&results).is_empty());
}

#[test]
fn jest_fixture_imported_by_name_reports_its_unused_export() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_project(root, "jest");
    write(
        root,
        "src/main.test.ts",
        "import { used } from \"./__fixtures__/shared\";\nconsole.log(used);\n",
    );
    write(
        root,
        "src/__fixtures__/shared.ts",
        "export const used = 1;\nexport const unusedFixture = 2;\n",
    );

    assert_eq!(unused_export_names(&analyze(root)), ["unusedFixture"]);
}

#[test]
fn default_export_of_an_imported_fixture_stays_used() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_project(root, "vitest");
    write(
        root,
        "src/main.test.ts",
        "import { plugin } from \"./fixtures/vitest.config.custom\";\nconsole.log(plugin);\n",
    );
    write(
        root,
        "src/fixtures/vitest.config.custom.ts",
        "export const plugin = 1;\nexport const unusedOption = 2;\nexport default { plugin };\n",
    );

    assert_eq!(unused_export_names(&analyze(root)), ["unusedOption"]);
}

#[test]
fn fixture_that_a_checked_fixture_imports_is_checked_too() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_project(root, "vitest");
    write(
        root,
        "src/main.test.ts",
        "import { thread } from \"./fixtures/thread\";\nconsole.log(thread);\n",
    );
    write(
        root,
        "src/fixtures/thread.ts",
        "import { user } from \"./user\";\nexport const thread = { user };\n",
    );
    write(
        root,
        "src/fixtures/user.ts",
        "export const user = 1;\nexport const unusedUser = 2;\n",
    );

    assert_eq!(unused_export_names(&analyze(root)), ["unusedUser"]);
}

#[test]
fn fixture_that_no_module_imports_keeps_its_exports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_project(root, "vitest");
    write(
        root,
        "src/main.test.ts",
        "import { readFileSync } from \"node:fs\";\nreadFileSync(\"./fixtures/input.ts\");\n",
    );
    write(root, "src/fixtures/input.ts", "export const data = 1;\n");
    write(
        root,
        "src/fixtures/chain.ts",
        "import { leaf } from \"./leaf\";\nexport const chain = leaf;\n",
    );
    write(
        root,
        "src/fixtures/leaf.ts",
        "export const leaf = 1;\nexport const other = 2;\n",
    );

    let results = analyze(root);

    assert!(unused_export_names(&results).is_empty());
    assert!(unused_file_names(&results).is_empty());
}

#[test]
fn fixture_with_its_own_package_json_keeps_its_exports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_project(root, "vitest");
    write(
        root,
        "src/main.test.ts",
        "import { entry } from \"./fixtures/app/index\";\nconsole.log(entry);\n",
    );
    write(root, "src/fixtures/app/package.json", r#"{"name":"app"}"#);
    write(
        root,
        "src/fixtures/app/index.ts",
        "export const entry = 1;\nexport const extra = 2;\n",
    );

    assert!(unused_export_names(&analyze(root)).is_empty());
}

#[test]
fn fixture_with_only_a_side_effect_import_keeps_its_exports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_project(root, "vitest");
    write(
        root,
        "src/main.test.ts",
        "import \"./fixtures/register\";\n",
    );
    write(
        root,
        "src/fixtures/register.ts",
        "export const handler = 1;\n",
    );

    assert!(unused_export_names(&analyze(root)).is_empty());
}

#[test]
fn fixture_that_an_entry_rule_also_matches_keeps_its_exports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"r","private":true,"devDependencies":{"vitest":"*"}}"#,
    );
    write(
        root,
        ".fallowrc.json",
        r#"{"entry":["src/main.ts","src/fixtures/public.ts"]}"#,
    );
    write(
        root,
        "src/main.ts",
        "import { used } from \"./fixtures/public\";\nconsole.log(used);\n",
    );
    write(
        root,
        "src/fixtures/public.ts",
        "export const used = 1;\nexport const api = 2;\n",
    );

    let config = fallow_config::FallowConfig::load(&root.join(".fallowrc.json"))
        .expect("config")
        .resolve(root.to_path_buf(), OutputFormat::Human, 1, true, true, None);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(unused_export_names(&results).is_empty());
}

#[test]
fn fixture_that_a_dynamic_import_loads_keeps_the_exports_behind_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_project(root, "vitest");
    write(
        root,
        "src/main.test.ts",
        "export const load = async () => await import(\"./fixtures/barrel\");\n",
    );
    write(
        root,
        "src/fixtures/barrel.ts",
        "export * from \"./leaf\";\n",
    );
    write(
        root,
        "src/fixtures/leaf.ts",
        "export const x = 1;\nexport const y = 2;\n",
    );

    assert!(unused_export_names(&analyze(root)).is_empty());
}

#[test]
fn fixture_that_require_loads_keeps_the_exports_of_its_imports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_project(root, "vitest");
    write(
        root,
        "src/main.test.js",
        "const data = require(\"./fixtures/a.js\");\nconsole.log(data);\n",
    );
    write(
        root,
        "src/fixtures/a.js",
        "import { leaf } from \"./b.js\";\nexport const a = leaf;\n",
    );
    write(
        root,
        "src/fixtures/b.js",
        "export const leaf = 1;\nexport const other = 2;\n",
    );

    assert!(unused_export_names(&analyze(root)).is_empty());
}

#[test]
fn nested_fixture_inside_a_fixture_project_keeps_its_exports() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_project(root, "vitest");
    write(
        root,
        "src/main.test.ts",
        "import { x } from \"./fixtures/app/test/fixtures/x\";\nconsole.log(x);\n",
    );
    write(root, "src/fixtures/app/package.json", r#"{"name":"app"}"#);
    write(
        root,
        "src/fixtures/app/test/fixtures/x.ts",
        "export const x = 1;\nexport const extra = 2;\n",
    );

    assert!(unused_export_names(&analyze(root)).is_empty());
}

#[test]
fn fixture_barrel_reports_the_unused_exports_and_types_behind_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    write_project(root, "vitest");
    write(
        root,
        "src/main.test.ts",
        "import { used, type Used } from \"./fixtures/index\";\nconst u: Used = used;\nconsole.log(u);\n",
    );
    write(root, "src/fixtures/index.ts", "export * from \"./data\";\n");
    write(
        root,
        "src/fixtures/data.ts",
        "export const used = 1;\nexport type Used = number;\nexport const unusedData = 2;\nexport type UnusedShape = string;\n",
    );

    let results = analyze(root);

    assert_eq!(unused_export_names(&results), ["unusedData"]);
    let types: Vec<&str> = results
        .unused_types
        .iter()
        .map(|finding| finding.export.export_name.as_str())
        .collect();
    assert_eq!(types, ["UnusedShape"]);
}
