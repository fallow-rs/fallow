use super::common::{create_config, fixture_path};

#[test]
fn webpack_inline_loader_requests_resolve_resource_and_credit_loaders() {
    let root = fixture_path("webpack-inline-loaders");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(
        unresolved.is_empty(),
        "inline loader requests should resolve to their resource, got unresolved: {unresolved:?}"
    );

    let mut unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(&config.root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    unused_files.sort();
    assert_eq!(
        unused_files,
        vec![
            "src/orphan.js".to_string(),
            "src/sandbox/helper.js".to_string()
        ],
        "loader resources should be used, and an asset loader resource must not keep its own imports alive"
    );

    // `src/sandbox/helper.js` is an unused file. Its export is referenced only
    // from the unreachable raw-loader resource, so it is reported too, as for
    // any chain of unused files.
    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .filter(|export| !export.export.path.ends_with("src/sandbox/helper.js"))
        .map(|export| export.export.export_name.clone())
        .collect();
    assert!(
        unused_exports.is_empty(),
        "a loader replaces the exports of its resource, so a loader import uses the whole resource: {unused_exports:?}"
    );

    let unused_deps: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .chain(
            results
                .unused_dev_dependencies
                .iter()
                .map(|dep| dep.dep.package_name.as_str()),
        )
        .collect();
    assert_eq!(
        unused_deps,
        vec!["left-pad"],
        "inline loader packages should count as used dependencies"
    );

    let dev_in_production: Vec<&str> = results
        .dev_dependencies_in_production
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        dev_in_production.is_empty(),
        "loaders run at build time and a raw-loader resource never runs, so neither is a production import: {dev_in_production:?}"
    );

    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        unlisted.is_empty(),
        "loader requests must not report unlisted packages, got {unlisted:?}"
    );
}

#[test]
fn webpack_inline_loader_request_with_missing_resource_reports_the_full_request() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"missing-resource","main":"src/index.js","dependencies":{"raw-loader":"^4.0.2"}}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.js"),
        "export const text = require('!raw-loader!./missing.js');\n",
    )
    .expect("write index.js");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert_eq!(unresolved, vec!["!raw-loader!./missing.js"]);
    assert!(
        results.unused_dependencies.is_empty(),
        "the loader package is used even when the resource is missing: {:?}",
        results.unused_dependencies
    );
}

#[test]
fn plain_path_with_a_bang_resolves_as_a_plain_path() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"bang-path","main":"src/index.js"}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.js"),
        "import w from './we!rd.js';\nimport r from './a!b/c.js';\nexport const both = [w, r];\n",
    )
    .expect("write index.js");
    std::fs::write(root.join("src/we!rd.js"), "export default 1;\n").expect("write we!rd.js");
    std::fs::create_dir_all(root.join("src/a!b")).expect("create a!b");
    std::fs::write(root.join("src/a!b/c.js"), "export default 2;\n").expect("write c.js");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(unresolved.is_empty(), "got unresolved: {unresolved:?}");
    let unlisted: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(unlisted.is_empty(), "got unlisted: {unlisted:?}");
    let unused_files: Vec<_> = results.unused_files.iter().map(|f| &f.file.path).collect();
    assert!(
        unused_files.is_empty(),
        "got unused files: {unused_files:?}"
    );
    let unused_exports: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|export| export.export.export_name.as_str())
        .collect();
    assert!(
        unused_exports.is_empty(),
        "got unused exports: {unused_exports:?}"
    );
}

/// Write a project that imports `pkg/a!b.js`. With `installed`, the package
/// holds a file named `a!b.js`.
fn write_bang_subpath_project(root: &std::path::Path, installed: bool) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"bang-subpath","main":"src/index.js","dependencies":{"pkg":"1.0.0"}}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.js"),
        "import x from 'pkg/a!b.js';\nexport default x;\n",
    )
    .expect("write index.js");
    if installed {
        let package = root.join("node_modules/pkg");
        std::fs::create_dir_all(&package).expect("create node_modules/pkg");
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"pkg","version":"1.0.0"}"#,
        )
        .expect("write pkg package.json");
        std::fs::write(package.join("a!b.js"), "module.exports = 1;\n").expect("write a!b.js");
    }
}

fn unlisted_and_unused(root: &std::path::Path) -> (Vec<String>, Vec<String>) {
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unlisted = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.clone())
        .collect();
    let unused = results
        .unused_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.clone())
        .collect();
    (unlisted, unused)
}

#[test]
fn installed_package_file_with_a_bang_resolves_as_a_plain_path() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    write_bang_subpath_project(tmp.path(), true);
    let (unlisted, unused) = unlisted_and_unused(tmp.path());
    assert!(
        unlisted.is_empty(),
        "`pkg/a!b.js` is the installed file `a!b.js` of `pkg`, not the loader `pkg/a` for `b.js`: {unlisted:?}"
    );
    assert!(unused.is_empty(), "pkg is imported: {unused:?}");
}

#[test]
fn uninstalled_bare_request_with_a_bang_is_read_as_webpack_reads_it() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    write_bang_subpath_project(tmp.path(), false);
    let (unlisted, unused) = unlisted_and_unused(tmp.path());
    assert_eq!(
        unlisted,
        ["b.js"],
        "without the installed file, webpack reads `pkg/a!b.js` as the loader `pkg/a` for `b.js`"
    );
    assert!(
        unused.is_empty(),
        "the loader `pkg/a` credits pkg: {unused:?}"
    );
}

#[test]
fn webpack_1_short_loader_name_credits_the_loader_package() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"short-loader","main":"src/index.js","devDependencies":{"raw-loader":"^0.5.1"}}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.js"),
        "export const text = require('raw!./template.js');\n",
    )
    .expect("write index.js");
    std::fs::write(root.join("src/template.js"), "export const t = 1;\n")
        .expect("write template.js");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_dev_dependencies.is_empty(),
        "webpack 1 resolved `raw` to `raw-loader`: {:?}",
        results.unused_dev_dependencies
    );
    assert!(results.unresolved_imports.is_empty());
    assert!(results.unused_files.is_empty());
}

/// Write a project whose entry re-exports resources through inline loaders,
/// with named and star re-exports through an asset loader and a code loader.
fn write_loader_re_export_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src/sandbox")).expect("create src/sandbox");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"loader-re-export","main":"src/index.js","devDependencies":{"dev-only":"^1.0.0","raw-loader":"^4.0.2","worker-loader":"^3.0.8"}}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.js"),
        "export { default as source } from 'raw-loader!./sandbox/reex.js';\n\
         export { default as Worker } from 'worker-loader!./worker.js';\n\
         export * from 'raw-loader!./sandbox/star.js';\n\
         export * from 'worker-loader!./star-worker.js';\n",
    )
    .expect("write index.js");
    std::fs::write(
        root.join("src/sandbox/reex.js"),
        "import 'dev-only';\nimport { local } from './local.js';\n\
         export default local;\nexport const extra = 2;\n",
    )
    .expect("write reex.js");
    std::fs::write(
        root.join("src/sandbox/local.js"),
        "export const local = 1;\n",
    )
    .expect("write local.js");
    std::fs::write(
        root.join("src/worker.js"),
        "import { job } from './job.js';\n\
         export const first = 1;\nexport const second = 2;\n\
         self.onmessage = () => job();\n",
    )
    .expect("write worker.js");
    std::fs::write(root.join("src/job.js"), "export const job = () => 1;\n").expect("write job.js");
    std::fs::write(
        root.join("src/sandbox/star.js"),
        "import { starLocal } from './star-local.js';\n\
         export const alpha = starLocal;\nexport default 3;\n",
    )
    .expect("write star.js");
    std::fs::write(
        root.join("src/sandbox/star-local.js"),
        "export const starLocal = 1;\n",
    )
    .expect("write star-local.js");
    std::fs::write(
        root.join("src/star-worker.js"),
        "import { starJob } from './star-job.js';\n\
         export const beta = 1;\nself.onmessage = () => starJob();\n",
    )
    .expect("write star-worker.js");
    std::fs::write(
        root.join("src/star-job.js"),
        "export const starJob = () => 1;\n",
    )
    .expect("write star-job.js");
}

#[test]
fn webpack_inline_loader_re_export_credits_the_whole_resource() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_loader_re_export_project(root);
    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unresolved: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|u| u.import.specifier.as_str())
        .collect();
    assert!(unresolved.is_empty(), "got unresolved: {unresolved:?}");

    let dev_in_production: Vec<&str> = results
        .dev_dependencies_in_production
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        dev_in_production.is_empty(),
        "a raw-loader resource never runs, so its imports are not production imports: {dev_in_production:?}"
    );

    let mut unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(&config.root)
                .unwrap_or(&file.file.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    unused_files.sort();
    assert_eq!(
        unused_files,
        vec![
            "src/sandbox/local.js".to_string(),
            "src/sandbox/star-local.js".to_string(),
        ],
        "a re-exported raw-loader resource must not keep its own imports in use, a worker-loader resource must"
    );

    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .filter(|export| {
            !export.export.path.ends_with("src/sandbox/local.js")
                && !export.export.path.ends_with("src/sandbox/star-local.js")
        })
        .map(|export| export.export.export_name.clone())
        .collect();
    assert!(
        unused_exports.is_empty(),
        "a loader re-export uses the whole resource: {unused_exports:?}"
    );

    let unused_deps: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.dep.package_name.as_str())
        .collect();
    assert!(
        !unused_deps.contains(&"raw-loader") && !unused_deps.contains(&"worker-loader"),
        "inline loader packages in a re-export are used: {unused_deps:?}"
    );
}

/// Write a project whose entry loads resources through loaders that run the
/// resource in another thread. Each resource imports the entry back, so each
/// loader edge closes a cycle. The entry also loads a package file through a
/// thread loader and through an asset loader.
fn write_thread_loader_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"thread-loaders","main":"src/index.ts","dependencies":{"heavy-worker-pkg":"^1.0.0","notes-pkg":"^1.0.0","plain-pkg":"^1.0.0"},"devDependencies":{"raw-loader":"^4.0.2","worker-loader":"^3.0.8","sharedworker-loader":"^2.1.1","worklet-loader":"^2.0.0"}}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import Work from 'worker-loader!./work.ts';\n\
         import Shared from 'sharedworker-loader?name=s!./shared.ts';\n\
         export { default as workletUrl } from 'worklet-loader!./worklet.ts';\n\
         import HeavyWorker from 'worker-loader!heavy-worker-pkg/worker';\n\
         import notes from 'raw-loader!notes-pkg/notes.txt';\n\
         import 'plain-pkg';\n\
         export const value = 1;\n\
         export const started = [new Work(), new Shared('s'), new HeavyWorker(), notes];\n",
    )
    .expect("write index.ts");
    for name in ["work", "shared", "worklet"] {
        std::fs::write(
            root.join(format!("src/{name}.ts")),
            "import { value } from './index.ts';\nexport const seen = value;\n",
        )
        .expect("write thread resource");
    }
}

/// Each reported cycle of a project as its sorted list of project-relative
/// paths.
fn project_cycles(root: &std::path::Path, ignore_lazy_imports: bool) -> Vec<Vec<String>> {
    let mut config = create_config(root.to_path_buf());
    config.circular_dependencies.ignore_lazy_imports = ignore_lazy_imports;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let mut cycles: Vec<Vec<String>> = results
        .circular_dependencies
        .iter()
        .map(|finding| {
            let mut files: Vec<String> = finding
                .cycle
                .files
                .iter()
                .map(|path| {
                    path.strip_prefix(&config.root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            files.sort();
            files
        })
        .collect();
    cycles.sort();
    cycles
}

#[test]
fn thread_loader_cycles_are_lazy_like_a_worker_url() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_thread_loader_project(root);

    let default_cycles = project_cycles(root, false);
    for resource in ["src/shared.ts", "src/work.ts", "src/worklet.ts"] {
        let mut expected = vec!["src/index.ts".to_string(), resource.to_string()];
        expected.sort();
        assert!(
            default_cycles.contains(&expected),
            "a thread loader cycle is reported by default, like `new Worker(new URL(...))`: {default_cycles:#?}"
        );
    }
    let lazy_ignored = project_cycles(root, true);
    assert!(
        lazy_ignored.is_empty(),
        "`ignoreLazyImports` drops a thread loader edge, like `new Worker(new URL(...))`: {lazy_ignored:#?}"
    );
}

#[test]
fn thread_loader_resources_load_out_of_thread() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    write_thread_loader_project(root);
    let config = create_config(root.to_path_buf());
    let output = fallow_core::analyze_with_trace(&config).expect("analysis should succeed");
    let graph = output.graph.as_ref().expect("graph is retained");
    let module_path = |id: fallow_types::discover::FileId| -> String {
        let path = &graph.modules[id.0 as usize].path;
        path.strip_prefix(&config.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let entry = graph
        .modules
        .iter()
        .find(|module| module_path(module.file_id) == "src/index.ts")
        .expect("entry in the graph")
        .file_id;

    let mut targets = Vec::new();
    for (target, symbols) in graph.outgoing_symbol_edges(entry) {
        targets.push(module_path(target));
        for symbol in symbols {
            assert_eq!(
                symbol.load_kind(),
                fallow_types::extract::ImportLoadKind::OutOfThread,
                "{} loads through a thread loader",
                module_path(target)
            );
        }
    }
    targets.sort();
    assert_eq!(targets, ["src/shared.ts", "src/work.ts", "src/worklet.ts"]);

    let results = &output.results;
    assert!(
        results.unresolved_imports.is_empty(),
        "{:?}",
        results.unresolved_imports
    );
    assert!(
        results.unused_dependencies.is_empty(),
        "the package behind a thread loader is used: {:?}",
        results.unused_dependencies
    );

    let closure = graph.entry_load_closure(entry);
    let names = |ids: &[fallow_types::discover::FileId]| -> Vec<String> {
        let mut names: Vec<String> = ids.iter().map(|id| module_path(*id)).collect();
        names.sort();
        names
    };
    assert_eq!(names(&closure.eager), ["src/index.ts"]);
    assert_eq!(closure.deferred, [] as [fallow_types::discover::FileId; 0]);
    assert_eq!(
        names(&closure.out_of_thread),
        ["src/shared.ts", "src/work.ts", "src/worklet.ts"],
        "a thread loader resource is not same-thread startup code"
    );

    let eager_packages: Vec<&str> = graph
        .eager_package_imports
        .get(&entry)
        .map(|imports| {
            imports
                .iter()
                .map(|import| import.specifier.as_str())
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(
        eager_packages,
        ["plain-pkg"],
        "a package loaded through a thread loader or an asset loader is not startup weight"
    );
}

#[test]
fn loader_resource_type_exports_follow_the_dynamic_import_pattern_rule() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    let root = tmp.path();
    std::fs::create_dir_all(root.join("src/pages")).expect("create src/pages");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"loader-types","main":"src/index.ts","devDependencies":{"worker-loader":"^3.0.8"}}"#,
    )
    .expect("write package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "import Work from 'worker-loader!./work.ts';\n\
         import type { Named } from './work.ts';\n\
         export const load = (name: string) => import(`./pages/${name}.ts`);\n\
         export const started: Named = new Work();\n",
    )
    .expect("write index.ts");
    let resource = "export const value = 1;\nexport default 2;\n\
                    export type Alias = number;\nexport interface Shape { x: number }\n\
                    export type Named = string;\n";
    std::fs::write(root.join("src/work.ts"), resource).expect("write work.ts");
    std::fs::write(root.join("src/pages/home.ts"), resource).expect("write home.ts");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let findings = |names: Vec<(String, String)>| -> Vec<(String, String)> {
        let mut names = names;
        names.sort();
        names
    };
    let relative = |path: &std::path::Path| -> String {
        path.strip_prefix(&config.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    };
    let unused_exports = findings(
        results
            .unused_exports
            .iter()
            .map(|finding| {
                (
                    relative(&finding.export.path),
                    finding.export.export_name.clone(),
                )
            })
            .collect(),
    );
    assert!(
        unused_exports.is_empty(),
        "a loader import and a pattern match credit every value export: {unused_exports:?}"
    );
    let unused_types = findings(
        results
            .unused_types
            .iter()
            .map(|finding| {
                (
                    relative(&finding.export.path),
                    finding.export.export_name.clone(),
                )
            })
            .collect(),
    );
    assert_eq!(
        unused_types,
        findings(vec![
            ("src/pages/home.ts".into(), "Alias".into()),
            ("src/pages/home.ts".into(), "Named".into()),
            ("src/pages/home.ts".into(), "Shape".into()),
            ("src/work.ts".into(), "Alias".into()),
            ("src/work.ts".into(), "Shape".into()),
        ]),
        "a type export needs an import that names it, for a loader resource and for a pattern match"
    );
}
