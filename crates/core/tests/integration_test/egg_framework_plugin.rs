use super::common::{create_config, fixture_path};

fn relative(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn unused_file_paths(
    root: &std::path::Path,
    results: &fallow_types::results::AnalysisResults,
) -> Vec<String> {
    results
        .unused_files
        .iter()
        .map(|finding| relative(root, &finding.file.path))
        .collect()
}

/// `(package.json path, package name)` of every unused dependency and unused
/// devDependency.
fn unused_dependency_keys(
    root: &std::path::Path,
    results: &fallow_types::results::AnalysisResults,
) -> Vec<(String, String)> {
    let key = |dep: &fallow_types::results::UnusedDependency| {
        (relative(root, &dep.path), dep.package_name.clone())
    };
    results
        .unused_dependencies
        .iter()
        .map(|finding| key(&finding.dep))
        .chain(
            results
                .unused_dev_dependencies
                .iter()
                .map(|finding| key(&finding.dep)),
        )
        .collect()
}

/// The built-in Egg plugin seeds the files that the Egg loader and the tegg
/// loader load by convention.
#[test]
fn egg_load_units_and_tegg_modules_are_entry_points() {
    let root = fixture_path("egg-framework-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_file_paths(&root, &results);

    for reachable in [
        "packages/app-cjs/app.js",
        "packages/app-cjs/agent.js",
        "packages/app-cjs/app/router.js",
        "packages/app-cjs/app/controller/home.js",
        "packages/app-cjs/app/service/user.js",
        "packages/app-cjs/app/middleware/gzip.js",
        "packages/app-cjs/app/extend/helper.js",
        "packages/app-cjs/app/schedule/tick.js",
        "packages/app-cjs/config/config.default.js",
        "packages/app-cjs/config/config.prod.js",
        "packages/app-cjs/config/plugin.js",
        // A nested test fixture app is a load unit of its own.
        "packages/app-cjs/test/fixtures/apps/a/app/controller/x.js",
        "packages/app-cjs/test/fixtures/apps/a/config/plugin.js",
        "packages/app-classic/app/router.js",
        "packages/app-ts/config/plugin.ts",
        // A directory with an `eggModule` package.json is a tegg module.
        "packages/app-ts/app/biz/BizService.ts",
        "packages/tegg-mod/src/a.ts",
        // A standalone tegg app without a `config` directory.
        "packages/standalone/app/HelloService.ts",
    ] {
        assert!(
            !unused.contains(&reachable.to_string()),
            "{reachable} must be reachable by Egg convention, unused files: {unused:?}"
        );
    }

    for still_unused in [
        "packages/app-cjs/lib/dead.js",
        // The tegg loader skips test directories of a module.
        "packages/tegg-mod/test/x.test.ts",
    ] {
        assert!(
            unused.contains(&still_unused.to_string()),
            "{still_unused} must stay unused, unused files: {unused:?}"
        );
    }
}

/// `config/plugin.*` credits each `package` value, and the `egg` block
/// credits its framework, to the package.json that owns the load unit. A
/// package that only another workspace names stays reported.
#[test]
fn egg_plugin_packages_are_credited_to_the_owning_manifest() {
    let root = fixture_path("egg-framework-plugin");
    let config = create_config(root.clone());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused = unused_dependency_keys(&root, &results);
    let key = |manifest: &str, name: &str| (manifest.to_string(), name.to_string());

    for credited in [
        key("packages/app-cjs/package.json", "@eggjs/tracer"),
        // Named by the plugin config of a fixture app without dependencies.
        key("packages/app-cjs/package.json", "@eggjs/redis"),
        key("packages/app-cjs/package.json", "yadan"),
        key("packages/app-ts/package.json", "@eggjs/view"),
        // The default framework of an app without `egg.framework`.
        key("packages/app-classic/package.json", "egg"),
    ] {
        assert!(
            !unused.contains(&credited),
            "{credited:?} is named by Egg config, unused dependencies: {unused:?}"
        );
    }

    for still_unused in [
        key("packages/app-cjs/package.json", "unused-dev"),
        key("packages/other/package.json", "@eggjs/tracer"),
    ] {
        assert!(
            unused.contains(&still_unused),
            "{still_unused:?} must stay reported, unused dependencies: {unused:?}"
        );
    }
}
