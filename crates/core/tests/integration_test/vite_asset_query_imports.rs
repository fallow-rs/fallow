use super::common::{create_config, create_config_with_cache};
use std::path::Path;

const PACKAGE_JSON: &str = r#"{"name":"vite-asset-queries","private":true,"dependencies":{"plain-pkg":"1.0.0","@scope/mapped-pkg":"1.0.0"},"devDependencies":{"vite":"*"}}"#;

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(path, source).expect("fixture file");
}

fn project(entry: &str, installed: bool) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("project");
    let root = dir.path();
    write(root, "package.json", PACKAGE_JSON);
    write(root, "src/index.ts", entry);
    if installed {
        write(
            root,
            "node_modules/plain-pkg/package.json",
            r#"{"name":"plain-pkg","version":"1.0.0","main":"index.js"}"#,
        );
        write(
            root,
            "node_modules/plain-pkg/index.js",
            "module.exports = {};",
        );
        write(
            root,
            "node_modules/plain-pkg/build/worker.min.mjs",
            "self.onmessage = () => {};",
        );
        write(
            root,
            "node_modules/@scope/mapped-pkg/package.json",
            r#"{"name":"@scope/mapped-pkg","version":"1.0.0","exports":{".":"./dist/index.js","./dist/*":"./dist/*"}}"#,
        );
        write(
            root,
            "node_modules/@scope/mapped-pkg/dist/index.js",
            "export {};",
        );
        write(root, "node_modules/@scope/mapped-pkg/dist/engine.wasm", "");
        write(
            root,
            "node_modules/@scope/mapped-pkg/dist/worker.js",
            "export {};",
        );
    }
    dir
}

struct Findings {
    unresolved: Vec<String>,
    unused_dependencies: Vec<String>,
}

fn analyze(root: &Path) -> Findings {
    let mut config = create_config(root.to_path_buf());
    config.entry_patterns = vec!["src/index.ts".to_string()];
    let results = fallow_core::analyze(&config).expect("analysis");
    Findings {
        unresolved: results
            .unresolved_imports
            .iter()
            .map(|finding| finding.import.specifier.clone())
            .collect(),
        unused_dependencies: results
            .unused_dependencies
            .iter()
            .map(|finding| finding.dep.package_name.clone())
            .collect(),
    }
}

const QUERY_IMPORTS: &str = "\
import a from 'plain-pkg/build/worker.min.mjs?url';
import b from 'plain-pkg/build/worker.min.mjs?raw';
import c from 'plain-pkg/build/worker.min.mjs?worker&inline';
import d from '@scope/mapped-pkg/dist/engine.wasm?url';
import e from '@scope/mapped-pkg/dist/engine.wasm?init';
import f from '@scope/mapped-pkg/dist/worker.js?sharedworker';
import g from '@scope/mapped-pkg/dist/engine.wasm?url&no-inline';
console.log(a, b, c, d, e, f, g);
";

#[test]
fn query_imports_of_an_installed_package_resolve_and_credit_it() {
    let dir = project(QUERY_IMPORTS, true);
    let findings = analyze(dir.path());
    assert!(findings.unresolved.is_empty(), "{:?}", findings.unresolved);
    assert!(
        findings.unused_dependencies.is_empty(),
        "{:?}",
        findings.unused_dependencies
    );
}

#[test]
fn query_imports_of_an_uninstalled_package_credit_it() {
    let dir = project(QUERY_IMPORTS, false);
    let findings = analyze(dir.path());
    assert!(
        findings.unresolved.is_empty(),
        "an uninstalled package cannot prove the file is missing: {:?}",
        findings.unresolved
    );
    assert!(
        findings.unused_dependencies.is_empty(),
        "both packages must count as used: {:?}",
        findings.unused_dependencies
    );
}

#[test]
fn missing_file_of_an_installed_package_with_query_stays_unresolved() {
    let dir = project(
        "import a from 'plain-pkg/build/missing.mjs?url';\nimport b from '@scope/mapped-pkg/dist/missing.wasm?raw';\nconsole.log(a, b);\n",
        true,
    );
    assert_eq!(
        analyze(dir.path()).unresolved,
        vec![
            "plain-pkg/build/missing.mjs?url".to_string(),
            "@scope/mapped-pkg/dist/missing.wasm?raw".to_string(),
        ]
    );
}

#[test]
fn unknown_query_on_an_uninstalled_package_stays_unresolved() {
    let dir = project(
        "import a from 'plain-pkg/build/worker.min.mjs?custom';\nconsole.log(a);\n",
        false,
    );
    assert_eq!(
        analyze(dir.path()).unresolved,
        vec!["plain-pkg/build/worker.min.mjs?custom".to_string()]
    );
}

/// Build a workspace project whose `ui` package exports its `dist` output,
/// but has only source files.
fn workspace_project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("project");
    let root = dir.path();
    write(
        root,
        "package.json",
        r#"{"name":"workspace-root","private":true,"workspaces":["apps/*","packages/*"]}"#,
    );
    write(
        root,
        "apps/web/package.json",
        r#"{"name":"web","private":true,"dependencies":{"@org/ui":"workspace:*"},"devDependencies":{"vite":"*"}}"#,
    );
    write(
        root,
        "apps/web/src/index.ts",
        "import url from '@org/ui/dist/worker.js?worker';\nconsole.log(url);\n",
    );
    write(
        root,
        "packages/ui/package.json",
        r#"{"name":"@org/ui","version":"1.0.0","exports":{"./dist/*":"./dist/*"}}"#,
    );
    write(
        root,
        "packages/ui/tsconfig.json",
        r#"{"compilerOptions":{"rootDir":"src","outDir":"dist"}}"#,
    );
    write(
        root,
        "packages/ui/src/worker.ts",
        "self.onmessage = () => {};\n",
    );
    dir
}

fn assert_workspace_source_is_credited(root: &Path) {
    let mut config = create_config(root.to_path_buf());
    config.entry_patterns = vec!["apps/web/src/index.ts".to_string()];
    let results = fallow_core::analyze(&config).expect("analysis");
    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|finding| finding.file.path.display().to_string())
        .collect();
    assert!(
        unused_files.is_empty(),
        "the query import must credit the workspace source: {unused_files:?}"
    );
    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    assert!(unresolved.is_empty(), "{unresolved:?}");
}

#[test]
fn query_import_of_an_unlinked_workspace_package_credits_its_source() {
    let dir = workspace_project();
    assert_workspace_source_is_credited(dir.path());
}

/// A package manager links a workspace package into `node_modules`. A
/// directory symlink on the Windows runner needs extra privileges, so this
/// case runs on Unix only.
#[cfg(unix)]
#[test]
fn query_import_of_a_linked_workspace_package_credits_its_source() {
    let dir = workspace_project();
    let root = dir.path();
    std::fs::create_dir_all(root.join("node_modules/@org")).expect("node_modules");
    std::os::unix::fs::symlink("../../packages/ui", root.join("node_modules/@org/ui"))
        .expect("symlink");
    assert_workspace_source_is_credited(root);
}

#[test]
fn install_after_a_cached_run_reports_a_missing_query_file() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("project");
    write(&root, "package.json", PACKAGE_JSON);
    write(
        &root,
        "src/index.ts",
        "import a from 'plain-pkg/build/missing.mjs?url';\nconsole.log(a);\n",
    );
    let mut config = create_config_with_cache(root.clone(), temp.path().join("cache"));
    config.entry_patterns = vec!["src/index.ts".to_string()];
    let before = fallow_core::analyze(&config).expect("cold analysis");
    assert!(before.unresolved_imports.is_empty());

    write(
        &root,
        "node_modules/plain-pkg/package.json",
        r#"{"name":"plain-pkg","version":"1.0.0","main":"index.js"}"#,
    );
    write(
        &root,
        "node_modules/plain-pkg/index.js",
        "module.exports = {};",
    );
    let after = fallow_core::analyze(&config).expect("analysis after install");
    let unresolved: Vec<&str> = after
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    assert_eq!(unresolved, vec!["plain-pkg/build/missing.mjs?url"]);
}

/// Run a cached analysis on a project whose `node_modules` holds only
/// `other-pkg`. Then install `plain-pkg` the way a package manager does: the
/// install adds the package and writes the lockfile and the install marker.
/// The warm run must see the installed package and report the missing file.
fn assert_install_into_existing_node_modules_misses_cache(lockfile: &str, marker: &str) {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("project");
    write(&root, "package.json", PACKAGE_JSON);
    write(
        &root,
        "src/index.ts",
        "import a from 'plain-pkg/build/missing.mjs?url';\nconsole.log(a);\n",
    );
    write(
        &root,
        "node_modules/other-pkg/package.json",
        r#"{"name":"other-pkg","version":"1.0.0"}"#,
    );
    write(&root, lockfile, "other-pkg@1.0.0\n");
    write(&root, marker, "other-pkg@1.0.0\n");
    let mut config = create_config_with_cache(root.clone(), temp.path().join("cache"));
    config.entry_patterns = vec!["src/index.ts".to_string()];
    let before = fallow_core::analyze(&config).expect("cold analysis");
    assert!(before.unresolved_imports.is_empty());

    write(
        &root,
        "node_modules/plain-pkg/package.json",
        r#"{"name":"plain-pkg","version":"1.0.0","main":"index.js"}"#,
    );
    write(
        &root,
        "node_modules/plain-pkg/index.js",
        "module.exports = {};",
    );
    write(&root, lockfile, "other-pkg@1.0.0\nplain-pkg@1.0.0\n");
    write(&root, marker, "other-pkg@1.0.0\nplain-pkg@1.0.0\n");
    let after = fallow_core::analyze(&config).expect("analysis after install");
    let unresolved: Vec<&str> = after
        .unresolved_imports
        .iter()
        .map(|finding| finding.import.specifier.as_str())
        .collect();
    assert_eq!(
        unresolved,
        vec!["plain-pkg/build/missing.mjs?url"],
        "{lockfile} and {marker} changed, so the graph cache must miss"
    );
}

#[test]
fn npm_install_into_existing_node_modules_misses_graph_cache() {
    assert_install_into_existing_node_modules_misses_cache(
        "package-lock.json",
        "node_modules/.package-lock.json",
    );
}

#[test]
fn pnpm_install_into_existing_node_modules_misses_graph_cache() {
    assert_install_into_existing_node_modules_misses_cache(
        "pnpm-lock.yaml",
        "node_modules/.pnpm/lock.yaml",
    );
}

#[test]
fn yarn_install_into_existing_node_modules_misses_graph_cache() {
    assert_install_into_existing_node_modules_misses_cache(
        "yarn.lock",
        "node_modules/.yarn-state.yml",
    );
}
