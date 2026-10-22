//! Egg.js and tegg plugin.
//!
//! The Egg loader loads the files of a load unit (an application, a framework
//! or a plugin) by directory convention, so no source file imports them. A
//! load unit holds `app.js`, `agent.js`, `app/router.js`, the files under
//! `app/{controller,service,middleware,schedule}`, the `app/extend` files and
//! the `config` files. The tegg loader loads every source file of a module
//! directory, which a `package.json` with an `eggModule` key marks.
//!
//! The plugin seeds these files as entry points. It finds nested load units,
//! such as example apps and test fixture apps, through their
//! `config/plugin.*` and `config/config.default.*` files. It credits the
//! packages that `config/plugin.*` names with `package`, and the packages that
//! the `egg` block of a load unit names, to the `package.json` that owns the
//! load unit.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use fallow_extract::JsText;
use oxc_ast::ast::{Expression, ObjectProperty, PropertyKey};
use oxc_ast_visit::{Visit, walk};

use super::{PathRule, Plugin, PluginResult, config_parser};

const SOURCE_EXTENSION_LIST: &[&str] = &["js", "cjs", "mjs", "ts", "cts", "mts"];

const ENABLERS: &[&str] = &["egg", "@eggjs/core", "@eggjs/tegg"];

/// The files that the Egg loader loads from a load unit, relative to the unit
/// directory. Source: `packages/core/src/loader/egg_loader.ts` and the
/// schedule plugin of the Egg repository. The list is short on purpose: every
/// active workspace adds these globs to one compiled set.
const ENTRY_PATTERNS: &[&str] = &[
    "{app,agent,app/router,app/router/index,app/extend/*,config/config,config/config.*,config/plugin,config/plugin.*}.{js,cjs,mjs,ts,cts,mts}",
    "app/{controller,service,middleware,schedule}/**/*.{js,cjs,mjs,ts,cts,mts}",
];

/// The `app` directories whose files the Egg loader loads recursively.
const RECURSIVE_APP_DIRS: &[&str] = &["controller", "service", "middleware", "schedule"];

/// The maximum directory depth of a walk below a load unit or a module.
const WALK_DEPTH: usize = 32;

/// The config files that mark a load unit. The `**/` prefix also finds the
/// load units of example apps and test fixture apps.
const CONFIG_PATTERNS: &[&str] = &[
    "**/config/plugin.{js,cjs,mjs,ts,cts,mts}",
    "**/config/plugin.*.{js,cjs,mjs,ts,cts,mts}",
    "**/config/config.default.{js,cjs,mjs,ts,cts,mts}",
];

const ALWAYS_USED: &[&str] = &["config/{config.*,plugin,plugin.*}.{js,cjs,mjs,ts,cts,mts}"];

/// The maximum depth of the tegg module scan (`deep` in `ModuleConfigUtil`).
const MODULE_SCAN_DEPTH: usize = 10;

const PACKAGE_JSON: &str = "package.json";
const DEFAULT_FRAMEWORK: &str = "egg";
const DEPENDENCY_SECTIONS: &[&str] = &[
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
];

/// Built-in plugin for Egg.js applications, plugins and tegg modules.
pub struct EggPlugin;

impl Plugin for EggPlugin {
    fn name(&self) -> &'static str {
        "egg"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_package_json(&self, pkg: &fallow_config::PackageJson, _root: &Path) -> bool {
        pkg.egg.is_some() || pkg.egg_plugin.is_some() || pkg.egg_module.is_some()
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn resolve_package_json(&self, pkg: &fallow_config::PackageJson, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        // A tegg app, also a standalone one without a `config` directory,
        // loads the modules below its base directory.
        resolve_unit_modules(root, root, &mut result);
        if let Some(exports) = pkg
            .egg_plugin
            .as_ref()
            .and_then(|plugin| plugin.get("exports"))
        {
            for key in ["import", "require", "typescript"] {
                let Some(dir) = exports.get(key).and_then(serde_json::Value::as_str) else {
                    continue;
                };
                if let Some(unit) = relative_dir(
                    &root.join(config_parser::path_from_config_string(dir)),
                    root,
                ) && !unit.is_empty()
                {
                    let prefix = globset::escape(&unit);
                    result.entry_patterns.extend(
                        ENTRY_PATTERNS
                            .iter()
                            .map(|pattern| PathRule::new(format!("{prefix}/{pattern}"))),
                    );
                }
            }
        }
        result
    }

    fn package_json_referenced_dependencies(
        &self,
        pkg: &fallow_config::PackageJson,
        _root: &Path,
    ) -> Vec<String> {
        pkg.egg.as_ref().map(egg_block_packages).unwrap_or_default()
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let Some(file_name) = config_path.file_name().and_then(OsStr::to_str) else {
            return result;
        };
        let Some(unit_dir) = config_path.parent().and_then(Path::parent) else {
            return result;
        };
        if !unit_dir.starts_with(root) {
            return result;
        }
        let is_plugin_config = file_name.starts_with("plugin.");
        if is_plugin_config {
            let packages = plugin_config_packages(source, config_path);
            if !packages.is_empty() {
                let owner = owning_manifest(unit_dir, root);
                result
                    .package_referenced_dependencies
                    .extend(packages.into_iter().map(|name| (owner.clone(), name)));
            }
        }
        if is_unit_marker(config_path, file_name) {
            resolve_load_unit(unit_dir, root, &mut result);
        }
        result
    }
}

/// Whether `config_path` is the one config file that resolves its load unit.
///
/// `config/config.default.*` marks the unit. A unit without that file is
/// marked by `config/plugin.<ext>`. Environment variants such as
/// `plugin.local.js` only credit packages, so each unit is read once.
fn is_unit_marker(config_path: &Path, file_name: &str) -> bool {
    if file_name.starts_with("config.default.") {
        return true;
    }
    let Some(stem) = file_name.strip_prefix("plugin.") else {
        return false;
    };
    if stem.contains('.') {
        return false;
    }
    let Some(config_dir) = config_path.parent() else {
        return false;
    };
    !["js", "cjs", "mjs", "ts", "cts", "mts"]
        .iter()
        .any(|ext| config_dir.join(format!("config.default.{ext}")).is_file())
}

/// Seed the entries of the load unit at `unit_dir`, read its `package.json`,
/// and seed the tegg modules that the unit loads.
fn resolve_load_unit(unit_dir: &Path, root: &Path, result: &mut PluginResult) {
    let Some(unit) = relative_dir(unit_dir, root) else {
        return;
    };
    let egg = read_manifest(&unit_dir.join(PACKAGE_JSON))
        .and_then(|manifest| manifest.get("egg").cloned());
    // The static entry patterns cover the unit at the package root, and the
    // package.json hooks credit its `egg` block and seed its modules.
    let mut packages = if unit.is_empty() {
        Vec::new()
    } else {
        let mut files = Vec::new();
        collect_unit_files(unit_dir, &mut files);
        push_file_entries(&files, root, result);
        egg.as_ref().map(egg_block_packages).unwrap_or_default()
    };
    // Without `egg.framework`, `egg-bin` and `egg-scripts` start the unit with
    // the `egg` framework, so a CommonJS app never imports it.
    if egg.as_ref().and_then(|egg| egg.get("framework")).is_none() {
        packages.push(DEFAULT_FRAMEWORK.to_string());
    }
    let owner = owning_manifest(unit_dir, root);
    result
        .package_referenced_dependencies
        .extend(packages.into_iter().map(|name| (owner.clone(), name)));
    if !unit.is_empty() {
        resolve_unit_modules(unit_dir, root, result);
    }
}

/// Seed the tegg modules of a load unit. `config/module.json` lists the
/// modules when it exists. Otherwise tegg scans the unit for `package.json`
/// files with an `eggModule` key (`ModuleConfigUtil.readModuleReference`).
fn resolve_unit_modules(unit_dir: &Path, root: &Path, result: &mut PluginResult) {
    let config_dir = unit_dir.join("config");
    if let Some(modules) = read_manifest(&config_dir.join("module.json")) {
        resolve_module_json(&modules, &config_dir, unit_dir, root, result);
        return;
    }
    let mut module_dirs = Vec::new();
    scan_module_dirs(unit_dir, 0, &mut module_dirs);
    for dir in module_dirs {
        push_module_files(&dir, root, result);
    }
}

fn resolve_module_json(
    modules: &serde_json::Value,
    config_dir: &Path,
    unit_dir: &Path,
    root: &Path,
    result: &mut PluginResult,
) {
    let Some(entries) = modules.as_array() else {
        return;
    };
    let mut packages = Vec::new();
    for entry in entries {
        if let Some(path) = entry.get("path").and_then(serde_json::Value::as_str) {
            let dir = config_dir.join(config_parser::path_from_config_string(path));
            if config_parser::is_inside_root(&dir, root) {
                push_module_files(&config_parser::lexical_normalize(&dir), root, result);
            }
        } else if let Some(package) = entry.get("package").and_then(serde_json::Value::as_str)
            && config_parser::is_package_specifier(package)
        {
            packages.push(package.to_string());
        }
    }
    if !packages.is_empty() {
        let owner = owning_manifest(unit_dir, root);
        result
            .package_referenced_dependencies
            .extend(packages.into_iter().map(|name| (owner.clone(), name)));
    }
}

/// Collect the directories below `dir` whose `package.json` has an
/// `eggModule` object. The scan skips the directories that tegg skips
/// (`node_modules`, hidden directories, `coverage`) and `test` directories,
/// whose fixture apps are load units of their own.
fn scan_module_dirs(dir: &Path, depth: usize, found: &mut Vec<PathBuf>) {
    if depth > MODULE_SCAN_DEPTH {
        return;
    }
    if read_manifest(&dir.join(PACKAGE_JSON)).is_some_and(|manifest| {
        manifest
            .get("eggModule")
            .is_some_and(serde_json::Value::is_object)
    }) {
        found.push(dir.to_path_buf());
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if !file_type.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.starts_with('.') || matches!(name, "node_modules" | "coverage" | "test") {
            continue;
        }
        scan_module_dirs(&entry.path(), depth + 1, found);
    }
}

/// Seed every file that tegg loads from the module directory `dir`. The
/// exclusions mirror `LoaderUtil.filePattern` of `tegg/core/loader`: hidden
/// directories, `node_modules`, declaration files, `vitest.config.*`, and
/// `test` and `coverage` directories.
fn push_module_files(dir: &Path, root: &Path, result: &mut PluginResult) {
    let mut files = Vec::new();
    walk_source_files(dir, 0, true, &mut files);
    files.retain(|file| {
        file.file_name()
            .and_then(OsStr::to_str)
            .is_none_or(|name| !name.starts_with("vitest.config."))
    });
    push_file_entries(&files, root, result);
}

/// Seed each file as one literal entry path.
///
/// A nested load unit or module gets literal paths instead of globs. A
/// repository can hold hundreds of test fixture apps, and one glob per unit
/// would make the compiled glob set of all plugin entries too large.
fn push_file_entries(files: &[PathBuf], root: &Path, result: &mut PluginResult) {
    for file in files {
        if let Some(relative) = relative_dir(file, root) {
            result
                .entry_patterns
                .push(PathRule::new(globset::escape(&relative)));
        }
    }
}

/// Collect the files that the Egg loader loads from the load unit at
/// `unit_dir`.
fn collect_unit_files(unit_dir: &Path, files: &mut Vec<PathBuf>) {
    let app_dir = unit_dir.join("app");
    let config_dir = unit_dir.join("config");
    let stems: [(&Path, &[&str]); 3] = [
        (unit_dir, &["app", "agent"]),
        (&app_dir, &["router"]),
        (&config_dir, &["config", "plugin"]),
    ];
    for (dir, allowed) in stems {
        files.extend(list_source_files(dir).into_iter().filter(|file| {
            file.file_name()
                .and_then(OsStr::to_str)
                .and_then(|name| name.split('.').next())
                .is_some_and(|stem| allowed.contains(&stem))
        }));
    }
    files.extend(
        list_source_files(&app_dir.join("router"))
            .into_iter()
            .filter(|file| file.file_stem().is_some_and(|stem| stem == "index")),
    );
    files.extend(list_source_files(&app_dir.join("extend")));
    for name in RECURSIVE_APP_DIRS {
        walk_source_files(&app_dir.join(name), 0, false, files);
    }
}

/// The source files directly inside `dir`, without declaration files.
fn list_source_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_dir()))
        .map(|entry| entry.path())
        .filter(|path| is_source_file(path))
        .collect();
    files.sort();
    files
}

/// Collect the source files below `dir`. The walk skips hidden directories
/// and `node_modules`, and with `skip_tests` also `test` and `coverage`, as
/// the tegg loader does.
fn walk_source_files(dir: &Path, depth: usize, skip_tests: bool, files: &mut Vec<PathBuf>) {
    if depth > WALK_DEPTH {
        return;
    }
    files.extend(list_source_files(dir));
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| {
            entry.file_name().to_str().is_some_and(|name| {
                !name.starts_with('.')
                    && name != "node_modules"
                    && !(skip_tests && matches!(name, "test" | "coverage"))
            })
        })
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    for child in dirs {
        walk_source_files(&child, depth + 1, skip_tests, files);
    }
}

fn is_source_file(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    SOURCE_EXTENSION_LIST.contains(&extension)
        && Path::new(stem).extension() != Some(OsStr::new("d"))
}

/// `dir` relative to `root` with `/` separators, or `None` when `dir` is not
/// below `root`. The root itself is the empty string.
fn relative_dir(dir: &Path, root: &Path) -> Option<String> {
    let relative = config_parser::lexical_normalize(dir);
    let relative = relative.strip_prefix(root).ok()?;
    Some(config_parser::path_to_config_string(relative))
}

/// The packages that an `egg` block names: `framework`, `tscompiler` and the
/// package specifiers of `require`.
fn egg_block_packages(egg: &serde_json::Value) -> Vec<String> {
    let mut packages: Vec<String> = ["framework", "tscompiler"]
        .iter()
        .filter_map(|key| egg.get(*key).and_then(serde_json::Value::as_str))
        .map(str::to_string)
        .collect();
    if let Some(required) = egg.get("require").and_then(serde_json::Value::as_array) {
        packages.extend(
            required
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_string),
        );
    }
    packages.retain(|name| config_parser::is_package_specifier(name));
    packages.sort();
    packages.dedup();
    packages
}

/// The `package.json` that owns the files below `start_dir`: the nearest
/// manifest inside `root` that has a dependency section and is not in a test
/// or fixture directory.
///
/// Egg test fixture apps and tegg module directories hold their own
/// `package.json`. Such a manifest does not install packages: the enclosing
/// package installs the packages that the fixture app names.
fn owning_manifest(start_dir: &Path, root: &Path) -> PathBuf {
    let mut current = Some(start_dir);
    while let Some(dir) = current {
        if !dir.starts_with(root) {
            break;
        }
        let manifest_path = dir.join(PACKAGE_JSON);
        if !is_in_fixture_dir(dir, root)
            && let Some(manifest) = read_manifest(&manifest_path)
            && DEPENDENCY_SECTIONS
                .iter()
                .any(|section| manifest.get(*section).is_some())
        {
            return manifest_path;
        }
        current = dir.parent();
    }
    root.join(PACKAGE_JSON)
}

/// Whether a directory between `root` and `dir` (inclusive) is a test or
/// fixture directory. A workspace package never lives there.
fn is_in_fixture_dir(dir: &Path, root: &Path) -> bool {
    dir.strip_prefix(root).is_ok_and(|relative| {
        relative.components().any(|component| {
            matches!(
                component.as_os_str().to_str(),
                Some("test" | "tests" | "__tests__" | "fixtures" | "__fixtures__")
            )
        })
    })
}

fn read_manifest(path: &Path) -> Option<serde_json::Value> {
    let source = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&source).ok()
}

/// The package names that a `config/plugin.*` file assigns to a `package`
/// key. The walk covers `exports.x = {...}`, `module.exports = {...}` and
/// `export default {...}`, also with a `satisfies` or `as` wrapper.
fn plugin_config_packages(source: &str, path: &Path) -> Vec<String> {
    config_parser::extract_from_source(source, path, |program| {
        let mut collector = PackageValueCollector::default();
        collector.visit_program(program);
        Some(collector.packages)
    })
    .unwrap_or_default()
}

#[derive(Default)]
struct PackageValueCollector {
    packages: Vec<String>,
}

impl<'a> Visit<'a> for PackageValueCollector {
    fn visit_object_property(&mut self, property: &ObjectProperty<'a>) {
        let is_package_key = match &property.key {
            PropertyKey::StaticIdentifier(identifier) => identifier.name == "package",
            PropertyKey::StringLiteral(literal) => literal.value == "package",
            _ => false,
        };
        if is_package_key
            && let Expression::StringLiteral(literal) = &property.value
            && config_parser::is_package_specifier(&literal.value.text())
        {
            let name = literal.value.text().into_owned();
            if !self.packages.contains(&name) {
                self.packages.push(name);
            }
        }
        walk::walk_object_property(self, property);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packages(source: &str, file: &str) -> Vec<String> {
        plugin_config_packages(source, Path::new(file))
    }

    #[test]
    fn credits_package_values_of_every_module_shape() {
        assert_eq!(
            packages(
                "exports.tracer = { enable: true, package: '@eggjs/tracer' };",
                "plugin.js"
            ),
            vec!["@eggjs/tracer"]
        );
        assert_eq!(
            packages(
                "module.exports = { redis: { enable: true, package: '@eggjs/redis' } };",
                "plugin.js"
            ),
            vec!["@eggjs/redis"]
        );
        assert_eq!(
            packages(
                "export default { cors: { enable: true, package: '@eggjs/cors' } };",
                "plugin.ts"
            ),
            vec!["@eggjs/cors"]
        );
        assert_eq!(
            packages(
                "import type { EggPlugin } from 'egg';\nexport default { view: { enable: true, 'package': '@eggjs/view' } } satisfies EggPlugin;",
                "plugin.ts"
            ),
            vec!["@eggjs/view"]
        );
    }

    #[test]
    fn skips_spreads_non_literals_and_relative_values() {
        assert!(
            packages(
                "import tracerPlugin from '@eggjs/tracer';\nexport default { ...tracerPlugin(), a: { package: name } };",
                "plugin.ts"
            )
            .is_empty()
        );
        assert!(packages("exports.local = { package: './lib/local' };", "plugin.js").is_empty());
        assert!(
            packages(
                "exports.local = { enable: true, path: '/abs/plugin' };",
                "plugin.js"
            )
            .is_empty()
        );
    }

    #[test]
    fn egg_block_names_framework_tscompiler_and_required_packages() {
        let egg = serde_json::json!({
            "framework": "yadan",
            "typescript": true,
            "tscompiler": "esbuild-register",
            "require": ["egg-ts-helper/register", "./local/setup.js"]
        });
        assert_eq!(
            egg_block_packages(&egg),
            vec!["egg-ts-helper/register", "esbuild-register", "yadan"]
        );
        assert!(egg_block_packages(&serde_json::json!(true)).is_empty());
    }

    #[test]
    fn static_patterns_match_the_loader_conventions() {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in ENTRY_PATTERNS {
            builder.add(
                globset::GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .build()
                    .expect("valid glob"),
            );
        }
        let set = builder.build().expect("glob set");
        for path in [
            "app.js",
            "agent.ts",
            "app/router.ts",
            "app/router/index.js",
            "app/extend/context.js",
            "app/extend/helper.unittest.ts",
            "app/controller/home.js",
            "app/controller/admin/user.ts",
            "app/service/user.mts",
            "app/middleware/gzip.js",
            "app/schedule/tick.js",
            "config/config.default.ts",
            "config/config.prod.js",
            "config/plugin.js",
            "config/plugin.local.ts",
        ] {
            assert!(set.is_match(path), "{path} must match");
        }
        for path in [
            "lib/app.js",
            "app/router/users.js",
            "app/extend/nested/x.js",
            "app/model/user.js",
            "config/other.js",
        ] {
            assert!(!set.is_match(path), "{path} must not match");
        }
    }

    #[test]
    fn activation_by_package_json_key_and_enabler() {
        let parse = |raw: &str| -> fallow_config::PackageJson {
            serde_json::from_str(raw).expect("package json")
        };
        let root = Path::new("/repo");
        for raw in [
            r#"{"egg":{"framework":"yadan"}}"#,
            r#"{"eggPlugin":{"name":"x"}}"#,
            r#"{"eggModule":{"name":"x"}}"#,
        ] {
            assert!(
                EggPlugin.is_enabled_with_package_json(&parse(raw), root),
                "{raw}"
            );
        }
        assert!(!EggPlugin.is_enabled_with_package_json(&parse(r#"{"name":"x"}"#), root));
        for enabler in ["egg", "@eggjs/core", "@eggjs/tegg"] {
            assert!(EggPlugin.is_enabled_with_deps(&[enabler.to_string()], root));
        }
        assert!(!EggPlugin.is_enabled_with_deps(&["express".to_string()], root));
    }

    #[test]
    fn package_json_hook_seeds_the_modules_of_the_package() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let raw = r#"{"name":"mod","eggModule":{"name":"mod"}}"#;
        write(&root.join("package.json"), raw);
        write(&root.join("src/a.ts"), "export class A {}");
        write(&root.join("src/types.d.ts"), "export {};");
        write(&root.join("test/a.test.ts"), "export {};");
        write(&root.join("vitest.config.ts"), "export default {};");
        let pkg: fallow_config::PackageJson = serde_json::from_str(raw).expect("package json");
        let result = EggPlugin.resolve_package_json(&pkg, root);
        let patterns: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect();
        assert_eq!(patterns, vec!["src/a.ts"]);
    }

    #[test]
    fn standalone_app_without_config_seeds_its_module_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let raw = r#"{"name":"standalone","dependencies":{"@eggjs/tegg":"4"}}"#;
        write(&root.join("package.json"), raw);
        write(&root.join("main.ts"), "export {};");
        write(
            &root.join("app/package.json"),
            r#"{"eggModule":{"name":"app"}}"#,
        );
        write(
            &root.join("app/HelloService.ts"),
            "export class HelloService {}",
        );
        let pkg: fallow_config::PackageJson = serde_json::from_str(raw).expect("package json");
        let result = EggPlugin.resolve_package_json(&pkg, root);
        let patterns: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect();
        assert_eq!(patterns, vec!["app/HelloService.ts"]);

        let plain = tempfile::tempdir().expect("tempdir");
        write(&plain.path().join("package.json"), r#"{"name":"x"}"#);
        let pkg: fallow_config::PackageJson =
            serde_json::from_str(r#"{"name":"x"}"#).expect("package json");
        assert!(
            EggPlugin
                .resolve_package_json(&pkg, plain.path())
                .is_empty()
        );
    }

    #[test]
    fn egg_plugin_exports_seed_the_plugin_unit() {
        let pkg: fallow_config::PackageJson = serde_json::from_str(
            r#"{"eggPlugin":{"name":"x","exports":{"import":"./dist/esm","typescript":"./src"}}}"#,
        )
        .expect("package json");
        let result = EggPlugin.resolve_package_json(&pkg, Path::new("/repo"));
        let patterns: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect();
        assert!(patterns.contains(
            &"src/app/{controller,service,middleware,schedule}/**/*.{js,cjs,mjs,ts,cts,mts}"
        ));
        assert_eq!(patterns.len(), 2 * ENTRY_PATTERNS.len());
        assert!(
            patterns
                .iter()
                .any(|pattern| pattern.starts_with("dist/esm/{app,agent,"))
        );
    }

    #[test]
    fn package_json_credits_the_egg_block() {
        let pkg: fallow_config::PackageJson =
            serde_json::from_str(r#"{"egg":{"framework":"yadan"}}"#).expect("package json");
        assert_eq!(
            EggPlugin.package_json_referenced_dependencies(&pkg, Path::new("/repo")),
            vec!["yadan"]
        );
    }

    fn write(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create dir");
        std::fs::write(path, contents).expect("write file");
    }

    #[test]
    fn nested_unit_credits_the_owning_manifest_and_seeds_entries_and_modules() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(
            &root.join("package.json"),
            r#"{"name":"app","devDependencies":{"@eggjs/redis":"1"}}"#,
        );
        let unit = root.join("test/fixtures/apps/a");
        write(
            &unit.join("package.json"),
            r#"{"name":"a","egg":{"framework":"yadan"}}"#,
        );
        write(&unit.join("app/router.js"), "module.exports = () => {};");
        write(
            &unit.join("app/controller/admin/user.js"),
            "module.exports = {};",
        );
        write(&unit.join("lib/helper.js"), "module.exports = {};");
        write(
            &unit.join("app/biz/package.json"),
            r#"{"eggModule":{"name":"biz"}}"#,
        );
        write(
            &unit.join("app/biz/BizService.ts"),
            "export class BizService {}",
        );
        write(&unit.join("app/biz/types.d.ts"), "export {};");
        write(&unit.join("app/biz/test/biz.test.ts"), "export {};");
        write(
            &unit.join("test/mod/package.json"),
            r#"{"eggModule":{"name":"t"}}"#,
        );
        write(&unit.join("test/mod/index.ts"), "export {};");
        let config = unit.join("config/plugin.js");
        let source = "module.exports = { redis: { enable: true, package: '@eggjs/redis' } };";
        write(&config, source);

        let result = EggPlugin.resolve_config(&config, source, root);
        let owner = root.join("package.json");
        assert!(
            result
                .package_referenced_dependencies
                .contains(&(owner.clone(), "@eggjs/redis".to_string()))
        );
        assert!(
            result
                .package_referenced_dependencies
                .contains(&(owner, "yadan".to_string()))
        );
        let patterns: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect();
        let mut sorted = patterns.clone();
        sorted.sort_unstable();
        assert_eq!(
            sorted,
            vec![
                "test/fixtures/apps/a/app/biz/BizService.ts",
                "test/fixtures/apps/a/app/controller/admin/user.js",
                "test/fixtures/apps/a/app/router.js",
                "test/fixtures/apps/a/config/plugin.js",
            ],
            "declaration files, module tests, unit tests and other files are not entries"
        );
    }

    #[test]
    fn root_unit_seeds_no_prefixed_entries_and_env_variants_only_credit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(
            &root.join("package.json"),
            r#"{"name":"app","dependencies":{"egg":"4"}}"#,
        );
        write(
            &root.join("config/config.default.js"),
            "module.exports = {};",
        );
        let plugin = root.join("config/plugin.js");
        let source = "exports.tracer = { package: '@eggjs/tracer' };";
        write(&plugin, source);
        let result = EggPlugin.resolve_config(&plugin, source, root);
        assert!(result.entry_patterns.is_empty());
        assert_eq!(
            result.package_referenced_dependencies,
            vec![(root.join("package.json"), "@eggjs/tracer".to_string())]
        );

        let local = root.join("config/plugin.local.js");
        let result = EggPlugin.resolve_config(&local, source, root);
        assert!(result.entry_patterns.is_empty());
        assert_eq!(result.package_referenced_dependencies.len(), 1);
    }

    #[test]
    fn module_json_lists_paths_and_packages() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(
            &root.join("package.json"),
            r#"{"name":"app","dependencies":{"egg":"4"}}"#,
        );
        write(&root.join("modules/foo/index.ts"), "export {};");
        write(
            &root.join("config/module.json"),
            r#"[{"path":"../modules/foo"},{"package":"@scope/mod"},{"path":"../../outside"}]"#,
        );
        let config = root.join("config/config.default.ts");
        write(&config, "export default {};");
        let owner = root.join("package.json");
        let unit = EggPlugin.resolve_config(&config, "export default {};", root);
        assert!(unit.entry_patterns.is_empty());
        assert_eq!(
            unit.package_referenced_dependencies,
            vec![(owner.clone(), "egg".to_string())],
            "a unit without egg.framework credits the default framework"
        );

        let pkg: fallow_config::PackageJson =
            serde_json::from_str(r#"{"name":"app","dependencies":{"egg":"4"}}"#)
                .expect("package json");
        let result = EggPlugin.resolve_package_json(&pkg, root);
        let patterns: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.as_str())
            .collect();
        assert_eq!(patterns, vec!["modules/foo/index.ts"]);
        assert_eq!(
            result.package_referenced_dependencies,
            vec![(owner, "@scope/mod".to_string())]
        );
    }

    #[test]
    fn owning_manifest_skips_manifests_without_dependencies() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write(&root.join("package.json"), r#"{"name":"r"}"#);
        write(
            &root.join("packages/a/package.json"),
            r#"{"name":"a","devDependencies":{"x":"1"}}"#,
        );
        write(
            &root.join("packages/a/test/fixtures/app/package.json"),
            r#"{"name":"fixture"}"#,
        );
        write(
            &root.join("packages/a/test/fixtures/deps-app/package.json"),
            r#"{"name":"fixture","dependencies":{"x":"workspace:*"}}"#,
        );
        assert_eq!(
            owning_manifest(&root.join("packages/a/test/fixtures/deps-app"), root),
            root.join("packages/a/package.json"),
            "a fixture manifest with dependencies is not a workspace package"
        );
        assert_eq!(
            owning_manifest(&root.join("packages/a/test/fixtures/app"), root),
            root.join("packages/a/package.json")
        );
        assert_eq!(
            owning_manifest(&root.join("other"), root),
            root.join("package.json")
        );
    }
}
