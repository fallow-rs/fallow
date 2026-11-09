use super::common::{create_config, create_config_with_cache};
use fallow_types::cache_rejection::CacheRejection;
use std::path::Path;

const PRE_SCRIPT_EXTENSION_CACHE_VERSION: u32 = 67;
const VITE_PACKAGE: &str =
    r#"{"name":"directory-resolution","private":true,"devDependencies":{"vite":"*"}}"#;

fn write(root: &Path, path: &str, source: &str) {
    let path = root.join(path);
    std::fs::create_dir_all(path.parent().expect("parent")).expect("directory");
    std::fs::write(path, source).expect("fixture file");
}

fn project(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("project");
    write(
        dir.path(),
        "package.json",
        r#"{"name":"directory-resolution","private":true}"#,
    );
    for (path, source) in files {
        write(dir.path(), path, source);
    }
    dir
}

fn unused_files(root: &Path) -> Vec<String> {
    let mut config = create_config(root.to_path_buf());
    config.entry_patterns = vec!["src/index.*".to_string()];
    fallow_core::analyze(&config)
        .expect("analysis")
        .unused_files
        .iter()
        .map(|file| {
            file.file
                .path
                .strip_prefix(root)
                .expect("project-relative finding")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

fn assert_edge(root: &Path, from: &str, target: &str, expected: bool) {
    let mut config = create_config(root.to_path_buf());
    config.entry_patterns = vec!["src/index.*".to_string()];
    let graph = fallow_core::analyze_with_trace(&config)
        .expect("analysis with graph")
        .graph
        .expect("retained graph");
    let source = graph
        .modules
        .iter()
        .find(|module| module.path.ends_with(from))
        .expect("importer discovered");
    assert!(source.is_reachable(), "importer must be live: {from}");
    let target = graph
        .modules
        .iter()
        .find(|module| module.path.ends_with(target))
        .expect("target discovered");
    assert_eq!(
        graph.edges_for(source.file_id).contains(&target.file_id),
        expected,
        "unexpected edge from {from} to {}",
        target.path.display()
    );
}

fn assert_vite_alias(root: &Path, prefix: &str, replacement: &str) {
    let package: fallow_config::PackageJson = serde_json::from_str(
        &std::fs::read_to_string(root.join("package.json")).expect("package source"),
    )
    .expect("package JSON");
    let plugins = fallow_core::plugins::PluginRegistry::default()
        .try_run(&package, root, &[root.join("vite.config.ts")])
        .expect("plugin configuration");
    assert!(
        plugins.active_plugins.iter().any(|plugin| plugin == "vite"),
        "Vite must be active for this alias fixture"
    );
    assert!(
        plugins
            .path_aliases
            .iter()
            .any(|(find, target)| { find == prefix && Path::new(target).ends_with(replacement) }),
        "Vite must produce the requested alias: {:?}",
        plugins.path_aliases
    );
}

#[test]
fn explicit_alias_and_package_maps_keep_asset_targets() {
    for extension in ["vue", "css"] {
        for mapping in ["tsconfig", "imports", "exports"] {
            let component = format!("src/Widget.{extension}");
            let (package, tsconfig, specifier) = match mapping {
                "tsconfig" => (
                    r#"{"name":"directory-resolution","private":true}"#.to_string(),
                    format!(
                        r#"{{"compilerOptions":{{"paths":{{"@app/Widget":["./{component}"]}}}},"include":["src"]}}"#
                    ),
                    "@app/Widget",
                ),
                "imports" => (
                    format!(
                        r##"{{"name":"directory-resolution","private":true,"imports":{{"#components/Widget":"./{component}"}}}}"##
                    ),
                    "{}".to_string(),
                    "#components/Widget",
                ),
                _ => (
                    format!(
                        r#"{{"name":"directory-resolution","private":true,"exports":{{"./Widget":"./{component}"}}}}"#
                    ),
                    "{}".to_string(),
                    "directory-resolution/Widget",
                ),
            };
            let entry = format!("import '{specifier}';\n");
            let dir = project(&[
                ("package.json", &package),
                ("tsconfig.json", &tsconfig),
                ("src/index.ts", &entry),
                (&component, "<template><div>widget</div></template>"),
                ("src/Widget/index.ts", "export const unused = 1;"),
            ]);
            let unused = unused_files(dir.path());
            assert!(
                unused.contains(&"src/Widget/index.ts".to_string()),
                "{mapping} must keep its explicit .{extension} target: {unused:?}"
            );
            assert!(
                !unused.contains(&component),
                "mapped asset must remain reachable: {unused:?}"
            );
            assert_edge(dir.path(), "src/index.ts", &component, true);
            assert_edge(dir.path(), "src/index.ts", "src/Widget/index.ts", false);
        }
    }
}

#[test]
fn commonjs_json_sibling_keeps_priority_over_directory() {
    let dir = project(&[
        ("src/index.cjs", "console.log(require('./Widget'));"),
        ("src/Widget.json", r#"{"name":"json-target"}"#),
        ("src/Widget/index.js", "module.exports = 'directory';"),
    ]);
    let unused = unused_files(dir.path());
    assert!(
        unused.contains(&"src/Widget/index.js".to_string()),
        "require('./Widget') must choose Widget.json: {unused:?}"
    );
    assert_edge(dir.path(), "src/index.cjs", "src/Widget/index.js", false);
}

#[test]
fn suffixes_and_renamed_aliases_prefer_script_directories() {
    for specifier in [
        "./Other?raw",
        "./Other#fragment",
        "@app/Widget",
        "/src/Other",
    ] {
        let entry = format!("import {{ used }} from '{specifier}'; console.log(used);");
        let dir = project(&[
            ("src/index.ts", &entry),
            ("src/Other.css", "body {}"),
            ("src/Other/index.ts", "export const used = 1;"),
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"paths":{"@app/Widget":["./src/Other"]}},"include":["src"]}"#,
            ),
        ]);
        let unused = unused_files(dir.path());
        assert!(
            !unused.contains(&"src/Other/index.ts".to_string()),
            "{specifier} must resolve to the directory module: {unused:?}"
        );
        assert_edge(dir.path(), "src/index.ts", "src/Other/index.ts", true);
    }
}

#[test]
fn import_map_and_bundler_aliases_prefer_script_directories() {
    for (mapping_file, mapping) in [
        ("deno.json", r#"{"imports":{"@app/Widget":"./src/Other"}}"#),
        (
            "vite.config.ts",
            "export default { resolve: { alias: { '@app/Widget': './src/Other' } } };",
        ),
        (
            "tsconfig.json",
            r#"{"extends":"./missing-base.json","compilerOptions":{"paths":{"@app/Widget":["./src/Other"]}},"include":["src"]}"#,
        ),
    ] {
        let dir = project(&[
            (
                "src/index.ts",
                "import { used } from '@app/Widget'; console.log(used);",
            ),
            ("src/Other.css", "body {}"),
            ("src/Other/index.ts", "export const used = 1;"),
            (mapping_file, mapping),
        ]);
        if mapping_file == "vite.config.ts" {
            write(dir.path(), "package.json", VITE_PACKAGE);
            assert_vite_alias(dir.path(), "@app/Widget", "src/Other");
        }
        assert_edge(dir.path(), "src/index.ts", "src/Other/index.ts", true);
    }
}

#[test]
fn asset_fallbacks_and_explicit_imports_remain_reachable() {
    for extension in [
        "vue", "svelte", "astro", "mdx", "css", "scss", "graphql", "gql",
    ] {
        for explicit in [false, true] {
            let asset = format!("src/Widget.{extension}");
            let specifier = if explicit {
                format!("./Widget.{extension}")
            } else {
                "./Widget".to_string()
            };
            let entry = format!("import '{specifier}';");
            let dir = project(&[("src/index.ts", &entry), (&asset, "")]);
            let unused = unused_files(dir.path());
            assert!(!unused.contains(&asset), "{specifier}: {unused:?}");
            assert_edge(dir.path(), "src/index.ts", &asset, true);
        }
    }
}

#[test]
fn stylesheet_import_keeps_stylesheet_sibling() {
    let dir = project(&[
        ("src/index.ts", "import './main.scss';"),
        ("src/main.scss", "@use './Widget';"),
        ("src/Widget.scss", ".widget { color: red; }"),
        ("src/Widget/index.ts", "export const unused = 1;"),
    ]);
    let unused = unused_files(dir.path());
    assert!(unused.contains(&"src/Widget/index.ts".to_string()));
    assert!(!unused.contains(&"src/Widget.scss".to_string()));
    assert_edge(dir.path(), "src/main.scss", "src/Widget.scss", true);
    assert_edge(dir.path(), "src/main.scss", "src/Widget/index.ts", false);
}

#[test]
fn bundler_aliases_keep_standalone_and_embedded_stylesheet_context() {
    for embedded in [false, true] {
        let (importer, source) = if embedded {
            (
                "src/main.vue",
                "<script>export default {};</script><style lang=\"scss\">@use '@style/Widget';</style>",
            )
        } else {
            ("src/main.scss", "@use '@style/Widget';")
        };
        let entry = if embedded {
            "import './main.vue';"
        } else {
            "import './main.scss';"
        };
        let dir = project(&[
            ("package.json", VITE_PACKAGE),
            ("src/index.ts", entry),
            (importer, source),
            (
                "vite.config.ts",
                "export default { resolve: { alias: { '@style/Widget': './src/Widget' } } };",
            ),
            ("src/Widget.scss", ".widget { color: red; }"),
            ("src/Widget/index.ts", "export const unused = 1;"),
        ]);
        assert_vite_alias(dir.path(), "@style/Widget", "src/Widget");
        assert_edge(dir.path(), importer, "src/Widget.scss", true);
        assert_edge(dir.path(), importer, "src/Widget/index.ts", false);
    }
}

#[test]
fn uninstalled_workspace_subpath_prefers_script_directory() {
    let dir = project(&[
        (
            "package.json",
            r#"{"name":"workspace-collision","private":true,"workspaces":["packages/*"],"dependencies":{"@fixture/widgets":"workspace:*"}}"#,
        ),
        (
            "src/index.ts",
            "import { used } from '@fixture/widgets/Widget'; console.log(used);",
        ),
        (
            "packages/widgets/package.json",
            r#"{"name":"@fixture/widgets","private":true}"#,
        ),
        ("packages/widgets/Widget.css", ".widget {}"),
        (
            "packages/widgets/Widget/index.ts",
            "export const used = 1; export const unused = 2;",
        ),
    ]);
    assert_edge(
        dir.path(),
        "src/index.ts",
        "packages/widgets/Widget/index.ts",
        true,
    );
}

#[test]
fn directory_package_main_keeps_explicit_asset_target() {
    let dir = project(&[
        ("src/index.ts", "import './Widget';"),
        ("src/Widget.css", ""),
        ("src/Widget/package.json", r#"{"main":"./entry.vue"}"#),
        ("src/Widget/entry.vue", "<template><div /></template>"),
        ("src/Widget/entry/index.ts", "export const unused = 1;"),
    ]);
    let unused = unused_files(dir.path());
    assert!(!unused.contains(&"src/Widget/entry.vue".to_string()));
    assert!(unused.contains(&"src/Widget/entry/index.ts".to_string()));
    assert_edge(dir.path(), "src/index.ts", "src/Widget/entry.vue", true);
}

#[test]
fn cache_from_before_script_extension_policy_is_rejected_on_unchanged_files() {
    let dir = project(&[
        (
            "src/index.ts",
            "import { used } from './Widget'; console.log(used);",
        ),
        ("src/Widget.css", ".widget {}"),
        (
            "src/Widget/index.ts",
            "export const used = 1; export const unused = 2;",
        ),
    ]);
    let cache = tempfile::tempdir().expect("cache");
    let config = create_config_with_cache(dir.path().to_path_buf(), cache.path().to_path_buf());
    let cold = fallow_core::analyze(&config).expect("cold analysis");
    let mut store = fallow_core::graph_cache::GraphCacheStore::load(cache.path()).expect("cache");
    store.version = PRE_SCRIPT_EXTENSION_CACHE_VERSION;
    store.manifest.version = PRE_SCRIPT_EXTENSION_CACHE_VERSION;
    // Model an old resolver graph without touching any project input fingerprint.
    store.graph.modules.clear();
    store.save(fallow_types::path_guard::CacheTarget::inside(cache.path()));
    assert_eq!(
        fallow_core::graph_cache::GraphCacheStore::load(cache.path()).err(),
        Some(CacheRejection::VersionMismatch),
        "the previous graph version must never replay after this resolution change"
    );
    let warm = fallow_core::analyze(&config).expect("upgraded analysis");
    assert_eq!(
        serde_json::to_value(&cold).expect("cold JSON"),
        serde_json::to_value(&warm).expect("warm JSON"),
        "unchanged source files must get the fresh resolver result after upgrade"
    );
}
