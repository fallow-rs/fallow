//! `ESLint` plugin.
//!
//! Detects `ESLint` projects and marks config files as always used.
//! Parses `ESLint` config to extract plugin/config imports as referenced dependencies.
//! Also covers Prettier and lint-staged config files.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use super::config_parser;
use super::{Plugin, PluginResult};

pub struct EslintPlugin;

const ENABLERS: &[&str] = &["eslint", "@eslint/js"];

const CONFIG_PATTERNS: &[&str] = &[
    "eslint.config.{js,mjs,cjs,ts,mts,cts}",
    ".eslintrc.{js,cjs,mjs,json,yaml,yml}",
];

const ALWAYS_USED: &[&str] = &[
    "eslint.config.{js,mjs,cjs,ts,mts,cts}",
    ".eslintrc.{js,cjs,mjs,json,yaml,yml}",
    ".prettierrc.{js,cjs,mjs,json,yaml,yml}",
    "prettier.config.{js,mjs,cjs,ts}",
    ".lintstagedrc.{js,cjs,mjs,json}",
    "lint-staged.config.{js,mjs,cjs}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["eslint"];

const ESLINT_CONFIG_EXPORTS: &[&str] = &["default"];

/// ESLint config filenames to check for file-based activation.
/// In monorepos, `eslint` is typically only in the root package.json, but
/// workspace packages still have their own ESLint config files.
const ESLINT_CONFIG_FILES: &[&str] = &[
    "eslint.config.js",
    "eslint.config.mjs",
    "eslint.config.cjs",
    "eslint.config.ts",
    "eslint.config.mts",
    "eslint.config.cts",
    ".eslintrc.js",
    ".eslintrc.cjs",
    ".eslintrc.json",
    ".eslintrc.yml",
    ".eslintrc.yaml",
];

impl Plugin for EslintPlugin {
    fn name(&self) -> &'static str {
        "eslint"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    /// Activate when `eslint` is in deps OR when an ESLint config file exists.
    /// In monorepos, `eslint` is usually only in the root package.json, but
    /// workspace packages have their own config files and ESLint-related devDeps.
    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        // Standard enabler check
        let enablers = self.enablers();
        if enablers.iter().any(|e| deps.iter().any(|d| d == e)) {
            return true;
        }
        // File-based activation: check for ESLint config files in the workspace root
        ESLINT_CONFIG_FILES.iter().any(|f| root.join(f).exists())
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn package_json_config_key(&self) -> Option<&'static str> {
        Some("eslintConfig")
    }

    fn used_exports(&self) -> Vec<(&'static str, &'static [&'static str])> {
        vec![(
            "eslint.config.{js,mjs,cjs,ts,mts,cts}",
            ESLINT_CONFIG_EXPORTS,
        )]
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let mut visited = FxHashSet::default();
        extract_eslint_config(config_path, source, root, &mut result, &mut visited, 0);
        result
    }
}

/// Maximum depth for following relative-path `extends` chains.
/// ESLint configs in the wild rarely chain more than 2-3 levels deep;
/// 8 is a generous ceiling that also caps pathological cases.
const MAX_EXTENDS_DEPTH: usize = 8;

/// Extract referenced dependencies from a single ESLint config file.
///
/// Recurses into relative-path `extends` entries (`./config/base.js`,
/// `../shared/eslintrc.json`) so chained-file plugins/parsers/extends
/// are credited as used. Cycle protection via canonicalized-path set;
/// depth bounded by [`MAX_EXTENDS_DEPTH`].
fn extract_eslint_config(
    config_path: &Path,
    source: &str,
    root: &Path,
    result: &mut PluginResult,
    visited: &mut FxHashSet<PathBuf>,
    depth: usize,
) {
    if depth >= MAX_EXTENDS_DEPTH {
        return;
    }
    let key = std::fs::canonicalize(config_path).unwrap_or_else(|_| config_path.to_path_buf());
    if !visited.insert(key) {
        return;
    }

    // For JSON configs, wrap in parens so Oxc can parse them
    let is_json = config_path.extension().is_some_and(|ext| ext == "json");
    let (parse_source, parse_path_buf) = if is_json {
        (format!("({source})"), config_path.with_extension("js"))
    } else {
        (source.to_string(), config_path.to_path_buf())
    };
    let parse_path: &Path = &parse_path_buf;

    // Extract import sources as referenced dependencies (eslint plugins, configs)
    let imports = config_parser::extract_imports(&parse_source, parse_path);
    for imp in &imports {
        let dep = crate::resolve::extract_package_name(imp);
        result.referenced_dependencies.push(dep);
    }

    // Follow shared config imports one level deep to discover peer deps.
    // e.g. eslint.config.js imports @sveltejs/eslint-config, which internally
    // imports typescript-eslint, eslint-plugin-svelte, @eslint/js, all peer deps
    // that the host project must install.
    //
    // We use the full import specifier (including subpath, e.g. `@scope/pkg/next`)
    // so that subpath exports are resolved correctly, and we walk up the directory
    // tree to find packages hoisted to the monorepo root node_modules.
    for imp in &imports {
        let pkg_name = crate::resolve::extract_package_name(imp);
        if let Some((entry_source, entry_path)) =
            read_package_entry_for_specifier(root, imp, &pkg_name)
        {
            let nested = config_parser::extract_imports(&entry_source, &entry_path);
            for nested_imp in &nested {
                result
                    .referenced_dependencies
                    .push(crate::resolve::extract_package_name(nested_imp));
            }
        }
    }

    // Legacy .eslintrc: extract plugins by short name
    // e.g. plugins: ["react"] → eslint-plugin-react
    let plugins =
        config_parser::extract_config_shallow_strings(&parse_source, parse_path, "plugins");
    for plugin in &plugins {
        result
            .referenced_dependencies
            .push(resolve_eslint_plugin_name(plugin));
    }

    // Legacy .eslintrc: extract extends
    // e.g. extends: ["airbnb", "plugin:react/recommended", "./shared/base.js"]
    let extends =
        config_parser::extract_config_shallow_strings(&parse_source, parse_path, "extends");
    for ext in &extends {
        process_extends_entry(ext, config_path, root, result, visited, depth);
    }

    // Legacy .eslintrc: extract parser
    // e.g. parser: "@typescript-eslint/parser"
    if let Some(parser) =
        config_parser::extract_config_string(&parse_source, parse_path, &["parser"])
    {
        let dep = crate::resolve::extract_package_name(&parser);
        result.referenced_dependencies.push(dep);
    }

    // overrides[*].parser, plugins, extends: each override entry is a sub-config
    // with the same fields as the top level. ESLint applies them when files match.
    let override_parsers = config_parser::extract_config_array_nested_string_or_array(
        &parse_source,
        parse_path,
        &["overrides"],
        &["parser"],
    );
    for parser in &override_parsers {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(parser));
    }
    let override_plugins = config_parser::extract_config_array_nested_string_or_array(
        &parse_source,
        parse_path,
        &["overrides"],
        &["plugins"],
    );
    for plugin in &override_plugins {
        result
            .referenced_dependencies
            .push(resolve_eslint_plugin_name(plugin));
    }
    let override_extends = config_parser::extract_config_array_nested_string_or_array(
        &parse_source,
        parse_path,
        &["overrides"],
        &["extends"],
    );
    for ext in &override_extends {
        process_extends_entry(ext, config_path, root, result, visited, depth);
    }

    // Flat config: extract plugin names from plugins object keys
    // e.g. plugins: { react: reactPlugin, "@typescript-eslint": tseslint }
    let plugin_keys =
        config_parser::extract_config_object_keys(&parse_source, parse_path, &["plugins"]);
    for key in &plugin_keys {
        result
            .referenced_dependencies
            .push(resolve_eslint_plugin_name(key));
    }

    // settings["import/resolver"] → resolver package dependencies
    // Handles three formats:
    //   Object: { typescript: { project: "..." } } → eslint-import-resolver-typescript
    //   String: "typescript" → eslint-import-resolver-typescript
    //   Array:  ["typescript", "node"] → eslint-import-resolver-typescript
    let resolver_path = &["settings", "import/resolver"];
    let resolver_keys =
        config_parser::extract_config_object_keys(&parse_source, parse_path, resolver_path);
    for key in &resolver_keys {
        if let Some(dep) = resolve_eslint_resolver_name(key) {
            result.referenced_dependencies.push(dep);
        }
    }
    if let Some(resolver) =
        config_parser::extract_config_string(&parse_source, parse_path, resolver_path)
        && let Some(dep) = resolve_eslint_resolver_name(&resolver)
    {
        result.referenced_dependencies.push(dep);
    }
    let resolver_strings =
        config_parser::extract_config_string_array(&parse_source, parse_path, resolver_path);
    for resolver in &resolver_strings {
        if let Some(dep) = resolve_eslint_resolver_name(resolver) {
            result.referenced_dependencies.push(dep);
        }
    }
}

/// Process a single `extends` entry: package-name resolution OR file-chain recursion.
///
/// Path-like entries (`./foo`, `../foo`, `/foo`) point at sibling config files.
/// Resolve relative to the current config's parent directory and recurse so the
/// chained file's plugins/parsers/extends are also credited.
fn process_extends_entry(
    name: &str,
    config_path: &Path,
    root: &Path,
    result: &mut PluginResult,
    visited: &mut FxHashSet<PathBuf>,
    depth: usize,
) {
    if !is_path_like_extends(name) {
        if let Some(dep) = resolve_eslint_extends_name(name) {
            result.referenced_dependencies.push(dep);
        }
        return;
    }
    let parent = config_path.parent().unwrap_or(config_path);
    let target = parent.join(name);
    let candidates: Vec<PathBuf> = if target.extension().is_some() {
        vec![target]
    } else {
        // ESLint resolves extension-less extends paths against this short list.
        // Order matches ESLint's own resolution (.js first, then .cjs, .mjs, .json).
        ["js", "cjs", "mjs", "json"]
            .iter()
            .map(|ext| target.with_extension(ext))
            .collect()
    };
    for candidate in candidates {
        if let Ok(chained_source) = std::fs::read_to_string(&candidate) {
            push_setup_file_once(result, candidate.clone());
            extract_eslint_config(
                &candidate,
                &chained_source,
                root,
                result,
                visited,
                depth + 1,
            );
            return;
        }
    }
}

/// True when an `extends` entry references a sibling file rather than a package.
fn is_path_like_extends(name: &str) -> bool {
    name.starts_with("./") || name.starts_with("../") || name.starts_with('/')
}

fn push_setup_file_once(result: &mut PluginResult, path: PathBuf) {
    if !result.setup_files.iter().any(|existing| existing == &path) {
        result.setup_files.push(path);
    }
}

/// Maximum directory depth the node_modules walk is allowed to climb.
/// Real monorepos rarely exceed 4-5 levels (apps/foo, packages/foo/sub); 8 is a
/// generous ceiling that also bounds pathological inputs (e.g. an absolute path
/// rooted near `/`) so the walk cannot traverse the entire filesystem.
const MAX_NODE_MODULES_WALK_DEPTH: usize = 8;

/// Find a package directory by walking up from `start` through ancestor directories,
/// checking `node_modules/<pkg_name>` at each level. Bounded by
/// [`MAX_NODE_MODULES_WALK_DEPTH`] so the walk cannot escape into the host filesystem.
///
/// Mirrors Node.js module resolution and handles monorepos where dependencies are
/// hoisted to the monorepo root rather than installed per-workspace.
fn find_package_dir(start: &Path, pkg_name: &str) -> Option<PathBuf> {
    for dir in start.ancestors().take(MAX_NODE_MODULES_WALK_DEPTH) {
        let candidate = dir.join("node_modules").join(pkg_name);
        if candidate.join("package.json").exists() {
            return Some(candidate);
        }
    }
    None
}

/// Read a package's entry point source from node_modules, walking up the directory
/// tree to find it (handles hoisted deps in monorepos).
///
/// When `specifier` contains a subpath (e.g., `@scope/pkg/next`), resolves that
/// subpath via the package's `exports` map, falling back to extension probing.
/// Returns `None` if the package is not found, the subpath cannot be resolved,
/// or the entry file is unreadable.
fn read_package_entry_for_specifier(
    workspace_dir: &Path,
    specifier: &str,
    pkg_name: &str,
) -> Option<(String, PathBuf)> {
    let pkg_dir = find_package_dir(workspace_dir, pkg_name)?;
    let pkg_json_str = std::fs::read_to_string(pkg_dir.join("package.json")).ok()?;
    let pkg_json: serde_json::Value = serde_json::from_str(&pkg_json_str).ok()?;

    // Extract the subpath from the specifier (e.g., "@scope/pkg/next" → "./next").
    // If there is no subpath the specifier equals the package name and we use ".".
    let subpath_key = if specifier.len() > pkg_name.len() {
        let raw = &specifier[pkg_name.len()..]; // e.g., "/next"
        format!(".{raw}") // e.g., "./next"
    } else {
        ".".to_string()
    };

    let entry_rel = resolve_package_entry(&pkg_json, &subpath_key, &pkg_dir)?;
    let entry_path = pkg_dir.join(entry_rel);
    let source = std::fs::read_to_string(&entry_path).ok()?;
    Some((source, entry_path))
}

/// Resolve a package entry path from its `package.json` for the given subpath key.
///
/// Subpath key is `"."` for the main entry or `"./subpath"` for named subpaths.
/// Returns `None` when the subpath cannot be resolved against the exports map and
/// no `.js`/`.mjs`/`.cjs` file exists for the bare subpath name. Prefer this over
/// returning a guessed-but-unverified path so the caller can skip the subpath
/// cleanly rather than `read_to_string` silently swallowing the error.
fn resolve_package_entry(
    pkg_json: &serde_json::Value,
    subpath_key: &str,
    pkg_dir: &Path,
) -> Option<String> {
    if let Some(exports) = pkg_json.get("exports")
        && let Some(rel) = resolve_exports_subpath(exports, subpath_key)
    {
        return Some(rel);
    }

    if subpath_key == "." {
        // Main entry: module → main → index.js
        if let Some(v) = pkg_json.get("module").and_then(|v| v.as_str()) {
            return Some(v.to_string());
        }
        if let Some(v) = pkg_json.get("main").and_then(|v| v.as_str()) {
            return Some(v.to_string());
        }
        return Some("index.js".to_string());
    }

    // For subpath keys like "./next", strip the leading "./" and try as a direct
    // file within the package directory. This handles packages that expose
    // sub-files without an exports map (e.g., `eslint-config-foo/next` → `next.js`).
    let bare = subpath_key.strip_prefix("./").unwrap_or(subpath_key);
    if std::path::Path::new(bare).extension().is_some() {
        return Some(bare.to_string());
    }
    for ext in &["js", "mjs", "cjs"] {
        let candidate = format!("{bare}.{ext}");
        if pkg_dir.join(&candidate).exists() {
            return Some(candidate);
        }
    }
    None
}

/// Resolve a subpath key against a package's `exports` field value.
///
/// Handles three export formats:
/// - String shorthand: `"exports": "./index.js"`
/// - Object with dot key: `"exports": { ".": "./index.js" }`
/// - Condition object: `"exports": { ".": { "import": "./index.mjs", "default": "./index.cjs" } }`
/// - Subpath exports: `"exports": { "./next": "./next.js" }`
fn resolve_exports_subpath(exports: &serde_json::Value, subpath_key: &str) -> Option<String> {
    if subpath_key == "." {
        // String shorthand: "exports": "./index.js"
        if let Some(s) = exports.as_str() {
            return Some(s.to_string());
        }
        // Object: look up "."
        if let Some(dot) = exports.get(".") {
            return resolve_condition_object(dot);
        }
        return None;
    }

    // Named subpath: look up the key in the exports object.
    if let Some(entry) = exports.get(subpath_key) {
        return resolve_condition_object(entry);
    }

    None
}

/// Resolve a condition object (or plain string) to a file path string.
fn resolve_condition_object(value: &serde_json::Value) -> Option<String> {
    if let Some(s) = value.as_str() {
        return Some(s.to_string());
    }
    // Prefer "import", then "default", then "require" for ESM-first packages.
    for key in &["import", "default", "require"] {
        if let Some(s) = value.get(key).and_then(|v| v.as_str()) {
            return Some(s.to_string());
        }
    }
    None
}

#[cfg(test)]
fn read_package_entry(root: &Path, pkg_name: &str) -> Option<(String, PathBuf)> {
    read_package_entry_for_specifier(root, pkg_name, pkg_name)
}

/// Resolve `ESLint` plugin short name to full package name.
///
/// - `"react"` → `"eslint-plugin-react"`
/// - `"@typescript-eslint"` → `"@typescript-eslint/eslint-plugin"`
/// - `"eslint-plugin-react"` → `"eslint-plugin-react"` (already full)
fn resolve_eslint_plugin_name(name: &str) -> String {
    if name.starts_with("eslint-plugin-") || name.contains("/eslint-plugin") {
        name.to_string()
    } else if let Some(scope) = name.strip_prefix('@') {
        if scope.contains('/') {
            // Already scoped with subpath, push as-is
            name.to_string()
        } else {
            // "@typescript-eslint" → "@typescript-eslint/eslint-plugin"
            format!("{name}/eslint-plugin")
        }
    } else {
        format!("eslint-plugin-{name}")
    }
}

/// Resolve `ESLint` extends name to a package dependency.
///
/// - `"airbnb"` → `"eslint-config-airbnb"`
/// - `"plugin:react/recommended"` → `"eslint-plugin-react"`
/// - `"eslint:recommended"` → `None` (built-in)
fn resolve_eslint_extends_name(name: &str) -> Option<String> {
    if name.starts_with("eslint:") {
        // Built-in ESLint config
        None
    } else if let Some(rest) = name.strip_prefix("plugin:") {
        // "plugin:react/recommended" → extract plugin name
        let plugin_name = rest.split('/').next()?;
        Some(resolve_eslint_plugin_name(plugin_name))
    } else if name.starts_with("eslint-config-") || name.contains("/eslint-config") {
        Some(name.to_string())
    } else if name.starts_with('@') {
        // Scoped package, push as-is
        Some(name.to_string())
    } else {
        Some(format!("eslint-config-{name}"))
    }
}

/// Resolve ESLint import resolver name to a package dependency.
///
/// - `"typescript"` → `"eslint-import-resolver-typescript"`
/// - `"node"` → `None` (built-in to eslint-plugin-import)
/// - `"eslint-import-resolver-typescript"` → `"eslint-import-resolver-typescript"` (already full)
fn resolve_eslint_resolver_name(name: &str) -> Option<String> {
    if matches!(name, "node" | "webpack") {
        // Built-in resolvers provided by eslint-plugin-import
        None
    } else if name.starts_with("eslint-import-resolver-") {
        Some(name.to_string())
    } else {
        Some(format!("eslint-import-resolver-{name}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ESLint plugin name resolution ───────────────────────────────

    #[test]
    fn plugin_short_name() {
        assert_eq!(resolve_eslint_plugin_name("react"), "eslint-plugin-react");
    }

    #[test]
    fn plugin_scoped_short_name() {
        assert_eq!(
            resolve_eslint_plugin_name("@typescript-eslint"),
            "@typescript-eslint/eslint-plugin"
        );
    }

    #[test]
    fn plugin_already_full_name() {
        assert_eq!(
            resolve_eslint_plugin_name("eslint-plugin-react"),
            "eslint-plugin-react"
        );
    }

    #[test]
    fn plugin_scoped_with_subpath() {
        assert_eq!(
            resolve_eslint_plugin_name("@scope/some-plugin"),
            "@scope/some-plugin"
        );
    }

    // ── ESLint extends name resolution ──────────────────────────────

    #[test]
    fn extends_short_name() {
        assert_eq!(
            resolve_eslint_extends_name("airbnb"),
            Some("eslint-config-airbnb".to_string())
        );
    }

    #[test]
    fn extends_plugin_rule() {
        assert_eq!(
            resolve_eslint_extends_name("plugin:react/recommended"),
            Some("eslint-plugin-react".to_string())
        );
    }

    #[test]
    fn extends_plugin_scoped() {
        assert_eq!(
            resolve_eslint_extends_name("plugin:@typescript-eslint/recommended"),
            Some("@typescript-eslint/eslint-plugin".to_string())
        );
    }

    #[test]
    fn extends_builtin() {
        assert_eq!(resolve_eslint_extends_name("eslint:recommended"), None);
    }

    #[test]
    fn extends_already_full_config_name() {
        assert_eq!(
            resolve_eslint_extends_name("eslint-config-prettier"),
            Some("eslint-config-prettier".to_string())
        );
    }

    #[test]
    fn extends_scoped_package() {
        assert_eq!(
            resolve_eslint_extends_name("@vue/eslint-config-typescript"),
            Some("@vue/eslint-config-typescript".to_string())
        );
    }

    // ── ESLint resolve_config integration ───────────────────────────

    #[test]
    fn resolve_config_legacy_eslintrc() {
        let source = r#"
            module.exports = {
                parser: "@typescript-eslint/parser",
                plugins: ["react", "@typescript-eslint"],
                extends: ["airbnb", "plugin:react/recommended", "eslint:recommended"]
            };
        "#;
        let plugin = EslintPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".eslintrc.js"),
            source,
            std::path::Path::new("/project"),
        );

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"@typescript-eslint/parser".to_string()));
        assert!(deps.contains(&"eslint-plugin-react".to_string()));
        assert!(deps.contains(&"@typescript-eslint/eslint-plugin".to_string()));
        assert!(deps.contains(&"eslint-config-airbnb".to_string()));
        // eslint:recommended should NOT be in deps
        assert!(!deps.iter().any(|d| d.contains("eslint:recommended")));
    }

    #[test]
    fn resolve_config_json_eslintrc() {
        let source = r#"{"plugins": ["react"], "extends": ["airbnb"]}"#;
        let plugin = EslintPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".eslintrc.json"),
            source,
            std::path::Path::new("/project"),
        );

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-react".to_string()));
        assert!(deps.contains(&"eslint-config-airbnb".to_string()));
    }

    #[test]
    fn resolve_config_flat_config_imports() {
        let source = r"
            import react from 'eslint-plugin-react';
            import tseslint from 'typescript-eslint';
            export default [{}];
        ";
        let plugin = EslintPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("eslint.config.js"),
            source,
            std::path::Path::new("/project"),
        );

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-plugin-react".to_string()));
        assert!(deps.contains(&"typescript-eslint".to_string()));
    }

    // ── Shared config following ─────────────────────────────────────

    #[test]
    fn shared_config_following_discovers_peer_deps() {
        // Create a temp dir with a mock shared config in node_modules
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        // Create node_modules/@mock/eslint-config with a package.json and index.js
        let pkg_dir = root.join("node_modules/@mock/eslint-config");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name": "@mock/eslint-config", "main": "index.js"}"#,
        )
        .unwrap();
        std::fs::write(
            pkg_dir.join("index.js"),
            r"
                import js from '@eslint/js';
                import ts from 'typescript-eslint';
                import svelte from 'eslint-plugin-svelte';
                export default [js.configs.recommended, ...ts.configs.recommended];
            ",
        )
        .unwrap();

        let source = r"
            import config from '@mock/eslint-config';
            export default [...config];
        ";
        let plugin = EslintPlugin;
        let result = plugin.resolve_config(std::path::Path::new("eslint.config.js"), source, root);

        let deps = &result.referenced_dependencies;
        // Direct import
        assert!(
            deps.contains(&"@mock/eslint-config".to_string()),
            "should find direct import"
        );
        // Peer deps from shared config's entry point
        assert!(
            deps.contains(&"@eslint/js".to_string()),
            "should find @eslint/js from shared config"
        );
        assert!(
            deps.contains(&"typescript-eslint".to_string()),
            "should find typescript-eslint from shared config"
        );
        assert!(
            deps.contains(&"eslint-plugin-svelte".to_string()),
            "should find eslint-plugin-svelte from shared config"
        );
    }

    #[test]
    fn shared_config_missing_node_modules_graceful() {
        // When node_modules doesn't exist, should not panic
        let source = r"
            import config from 'some-nonexistent-config';
            export default [...config];
        ";
        let plugin = EslintPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("eslint.config.js"),
            source,
            std::path::Path::new("/nonexistent"),
        );

        // Should still find the direct import
        assert!(
            result
                .referenced_dependencies
                .contains(&"some-nonexistent-config".to_string())
        );
    }

    #[test]
    fn find_package_dir_finds_local_install_at_depth_zero() {
        // Confirms the walk does not skip a package that is co-located with
        // the workspace's own node_modules (the default before this PR).
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path();
        let pkg_dir = workspace.join("node_modules/local-pkg");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(pkg_dir.join("package.json"), r#"{"name":"local-pkg"}"#).unwrap();

        let resolved = super::find_package_dir(workspace, "local-pkg");
        assert_eq!(resolved.as_deref(), Some(pkg_dir.as_path()));
    }

    #[test]
    fn find_package_dir_returns_none_when_walk_finds_nothing() {
        // Walk-up failure must return None cleanly, not panic. Uses a deep
        // workspace path with no node_modules anywhere along the chain.
        let dir = tempfile::tempdir().unwrap();
        let deep = dir.path().join("apps/foo/src");
        std::fs::create_dir_all(&deep).unwrap();
        // Sanity: no node_modules exists on this path.
        assert!(super::find_package_dir(&deep, "missing-pkg").is_none());
    }

    #[test]
    fn read_package_entry_exports_field() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let pkg_dir = root.join("node_modules/modern-pkg");
        std::fs::create_dir_all(&pkg_dir).unwrap();
        std::fs::write(
            pkg_dir.join("package.json"),
            r#"{"name": "modern-pkg", "exports": { ".": { "import": "./dist/index.mjs" } }}"#,
        )
        .unwrap();
        std::fs::create_dir_all(pkg_dir.join("dist")).unwrap();
        std::fs::write(
            pkg_dir.join("dist/index.mjs"),
            "import foo from 'some-dep'; export default foo;",
        )
        .unwrap();

        let (source, path) = super::read_package_entry(root, "modern-pkg").unwrap();
        assert!(source.contains("some-dep"));
        assert!(path.ends_with("dist/index.mjs"));
    }

    // ── Workspace-package flat-config dep tracing ──────────────────

    /// Regression test: apps/foo/eslint.config.mjs imports @scope/eslint-config,
    /// which itself imports eslint-plugin-react. Both packages list eslint-plugin-react
    /// in devDependencies. Fallow must NOT flag eslint-plugin-react as unused-devdep.
    ///
    /// The shared config lives at packages/eslint-config in the monorepo root, but
    /// its node_modules symlink is hoisted to monorepo-root/node_modules. The workspace
    /// package's eslint.config.mjs is analysed with root=apps/foo, so read_package_entry
    /// must walk up to the monorepo root to find the package.
    #[test]
    fn flat_config_workspace_package_dep_chain_traced() {
        // Set up a minimal monorepo layout:
        //   <root>/
        //     node_modules/@scope/eslint-config/
        //       package.json  (main: "index.js")
        //       index.js      (imports eslint-plugin-react)
        //     apps/foo/
        //       eslint.config.mjs  (imports @scope/eslint-config)
        let dir = tempfile::tempdir().unwrap();
        let monorepo_root = dir.path();

        // Shared config package in hoisted node_modules
        let shared_pkg_dir = monorepo_root.join("node_modules/@scope/eslint-config");
        std::fs::create_dir_all(&shared_pkg_dir).unwrap();
        std::fs::write(
            shared_pkg_dir.join("package.json"),
            r#"{"name": "@scope/eslint-config", "main": "index.js"}"#,
        )
        .unwrap();
        std::fs::write(
            shared_pkg_dir.join("index.js"),
            r"
                import reactPlugin from 'eslint-plugin-react';
                import storybook from 'eslint-plugin-storybook';
                export default [{ plugins: { react: reactPlugin } }];
            ",
        )
        .unwrap();

        // App workspace with its own eslint.config.mjs
        let app_dir = monorepo_root.join("apps/foo");
        std::fs::create_dir_all(&app_dir).unwrap();
        let eslint_config_path = app_dir.join("eslint.config.mjs");
        let source = r"
            import sharedConfig from '@scope/eslint-config';
            export default [...sharedConfig];
        ";
        std::fs::write(&eslint_config_path, source).unwrap();

        let plugin = EslintPlugin;
        // root = apps/foo — mirrors how run_workspace_fast calls resolve_config
        let result = plugin.resolve_config(&eslint_config_path, source, &app_dir);

        let deps = &result.referenced_dependencies;
        assert!(
            deps.contains(&"@scope/eslint-config".to_string()),
            "direct workspace package import must be listed as used: {deps:?}"
        );
        assert!(
            deps.contains(&"eslint-plugin-react".to_string()),
            "transitive dep from workspace config package must not be flagged unused: {deps:?}"
        );
        assert!(
            deps.contains(&"eslint-plugin-storybook".to_string()),
            "second transitive dep from workspace config package must not be flagged unused: {deps:?}"
        );
    }

    /// When the import uses a subpath (e.g. `@scope/eslint-config/next`),
    /// the /next file's imports must be discovered, not the package's main entry.
    #[test]
    fn flat_config_workspace_package_subpath_dep_chain_traced() {
        let dir = tempfile::tempdir().unwrap();
        let monorepo_root = dir.path();

        let shared_pkg_dir = monorepo_root.join("node_modules/@scope/eslint-config");
        std::fs::create_dir_all(&shared_pkg_dir).unwrap();
        std::fs::write(
            shared_pkg_dir.join("package.json"),
            r#"{"name": "@scope/eslint-config", "main": "index.js"}"#,
        )
        .unwrap();
        // main entry does NOT import eslint-plugin-react
        std::fs::write(shared_pkg_dir.join("index.js"), r"export default [];").unwrap();
        // /next subpath DOES import eslint-plugin-react
        std::fs::write(
            shared_pkg_dir.join("next.js"),
            r"
                import nextConfig from 'eslint-config-next';
                import reactPlugin from 'eslint-plugin-react';
                export default [{ plugins: { react: reactPlugin }, ...nextConfig }];
            ",
        )
        .unwrap();

        let app_dir = monorepo_root.join("apps/foo");
        std::fs::create_dir_all(&app_dir).unwrap();
        let eslint_config_path = app_dir.join("eslint.config.mjs");
        let source = r"
            import nextConfig from '@scope/eslint-config/next';
            export default [...nextConfig];
        ";
        std::fs::write(&eslint_config_path, source).unwrap();

        let plugin = EslintPlugin;
        let result = plugin.resolve_config(&eslint_config_path, source, &app_dir);

        let deps = &result.referenced_dependencies;
        assert!(
            deps.contains(&"@scope/eslint-config".to_string()),
            "scoped package name must be listed: {deps:?}"
        );
        assert!(
            deps.contains(&"eslint-plugin-react".to_string()),
            "transitive dep via subpath import must be traced: {deps:?}"
        );
        assert!(
            deps.contains(&"eslint-config-next".to_string()),
            "transitive dep via subpath import must be traced: {deps:?}"
        );
    }

    // ── ESLint resolver name resolution ────────────────────────────

    #[test]
    fn resolver_short_name() {
        assert_eq!(
            resolve_eslint_resolver_name("typescript"),
            Some("eslint-import-resolver-typescript".to_string())
        );
    }

    #[test]
    fn resolver_node_builtin() {
        assert_eq!(resolve_eslint_resolver_name("node"), None);
    }

    #[test]
    fn resolver_webpack_builtin() {
        assert_eq!(resolve_eslint_resolver_name("webpack"), None);
    }

    #[test]
    fn resolver_already_full_name() {
        assert_eq!(
            resolve_eslint_resolver_name("eslint-import-resolver-typescript"),
            Some("eslint-import-resolver-typescript".to_string())
        );
    }

    // ── ESLint resolver config integration ─────────────────────────

    #[test]
    fn resolve_config_resolver_object_keys() {
        let source = r#"
            module.exports = {
                settings: {
                    "import/resolver": {
                        typescript: { project: "./tsconfig.json" },
                        node: { extensions: [".js", ".ts"] }
                    }
                }
            };
        "#;
        let plugin = EslintPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".eslintrc.js"),
            source,
            std::path::Path::new("/project"),
        );

        let deps = &result.referenced_dependencies;
        assert!(
            deps.contains(&"eslint-import-resolver-typescript".to_string()),
            "should resolve typescript resolver"
        );
        // "node" is built-in, should NOT be added
        assert!(
            !deps.iter().any(|d| d == "eslint-import-resolver-node"),
            "node resolver is built-in"
        );
    }

    #[test]
    fn resolve_config_resolver_string() {
        let source = r#"
            module.exports = {
                settings: {
                    "import/resolver": "typescript"
                }
            };
        "#;
        let plugin = EslintPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".eslintrc.js"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"eslint-import-resolver-typescript".to_string())
        );
    }

    #[test]
    fn resolve_config_resolver_json() {
        let source =
            r#"{"settings": {"import/resolver": {"typescript": {"project": "./tsconfig.json"}}}}"#;
        let plugin = EslintPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".eslintrc.json"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"eslint-import-resolver-typescript".to_string())
        );
    }

    #[test]
    fn resolve_config_resolver_flat_config() {
        // Flat configs that use defineConfig() can have settings extracted
        let source = r#"
            import { defineConfig } from 'eslint/config';
            export default defineConfig({
                settings: {
                    "import/resolver": {
                        typescript: { alwaysTryTypes: true }
                    }
                }
            });
        "#;
        let plugin = EslintPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new("eslint.config.mjs"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"eslint-import-resolver-typescript".to_string())
        );
    }

    // ── Overrides[*] sub-config extraction ──────────────────────────

    #[test]
    fn resolve_config_overrides_parser_json() {
        // Issue #198 bug 1: parser inside overrides[*] in .eslintrc.json
        let source = r#"{
            "root": true,
            "overrides": [{ "files": ["*.ts"], "parser": "@typescript-eslint/parser" }]
        }"#;
        let plugin = EslintPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".eslintrc.json"),
            source,
            std::path::Path::new("/project"),
        );

        assert!(
            result
                .referenced_dependencies
                .contains(&"@typescript-eslint/parser".to_string()),
            "expected parser inside overrides to be detected, got: {:?}",
            result.referenced_dependencies
        );
    }

    #[test]
    fn resolve_config_overrides_plugins_and_extends_js() {
        let source = r#"
            module.exports = {
                overrides: [
                    {
                        files: ["*.ts"],
                        plugins: ["react", "@typescript-eslint"],
                        extends: ["plugin:react/recommended", "airbnb"]
                    }
                ]
            };
        "#;
        let plugin = EslintPlugin;
        let result = plugin.resolve_config(
            std::path::Path::new(".eslintrc.js"),
            source,
            std::path::Path::new("/project"),
        );

        let deps = &result.referenced_dependencies;
        assert!(
            deps.contains(&"eslint-plugin-react".to_string()),
            "plugins inside overrides should resolve to eslint-plugin-*"
        );
        assert!(
            deps.contains(&"@typescript-eslint/eslint-plugin".to_string()),
            "scoped plugins inside overrides should resolve to @scope/eslint-plugin"
        );
        assert!(
            deps.contains(&"eslint-config-airbnb".to_string()),
            "extends inside overrides should resolve to eslint-config-*"
        );
    }

    // ── Relative-path extends chain following ───────────────────────

    #[test]
    fn resolve_config_relative_extends_to_js_file() {
        // Issue #198 bug 2: .eslintrc.json extends a JS file in a subdirectory.
        // Plugins/extends/parser referenced in the JS file must be credited.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(
            root.join("config/eslintrc.base.js"),
            r"
                module.exports = {
                    extends: ['prettier'],
                    overrides: [
                        { files: ['*.ts'], parser: '@typescript-eslint/parser', rules: {} }
                    ]
                };
            ",
        )
        .unwrap();

        let root_config = root.join(".eslintrc.json");
        let source = r#"{ "root": true, "extends": ["./config/eslintrc.base.js"] }"#;
        std::fs::write(&root_config, source).unwrap();

        let plugin = EslintPlugin;
        let result = plugin.resolve_config(&root_config, source, root);

        let deps = &result.referenced_dependencies;
        assert!(
            deps.contains(&"eslint-config-prettier".to_string()),
            "chained extends should be followed: {deps:?}"
        );
        assert!(
            deps.contains(&"@typescript-eslint/parser".to_string()),
            "parser in chained file's overrides should be detected: {deps:?}"
        );
        assert!(
            result.setup_files.iter().any(|path| path.ends_with(
                std::path::Path::new("config").join("eslintrc.base.js")
            )),
            "chained config file should be treated as used: {:?}",
            result.setup_files
        );
    }

    #[test]
    fn resolve_config_relative_extends_to_json_file() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(
            root.join("config/base.json"),
            r#"{ "extends": ["airbnb"], "parser": "@typescript-eslint/parser" }"#,
        )
        .unwrap();

        let root_config = root.join(".eslintrc.json");
        let source = r#"{ "extends": ["./config/base.json"] }"#;
        std::fs::write(&root_config, source).unwrap();

        let plugin = EslintPlugin;
        let result = plugin.resolve_config(&root_config, source, root);

        let deps = &result.referenced_dependencies;
        assert!(
            deps.contains(&"eslint-config-airbnb".to_string()),
            "chained JSON extends should be followed: {deps:?}"
        );
        assert!(
            deps.contains(&"@typescript-eslint/parser".to_string()),
            "parser in chained JSON file should be detected: {deps:?}"
        );
    }

    #[test]
    fn resolve_config_relative_extends_extensionless_resolves_to_cjs() {
        // ESLint accepts extension-less paths like "./config/base"; we probe
        // the same short list as ESLint (.js, .cjs, .mjs, .json).
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        std::fs::create_dir_all(root.join("config")).unwrap();
        std::fs::write(
            root.join("config/base.cjs"),
            r"module.exports = { extends: ['airbnb'] };",
        )
        .unwrap();

        let root_config = root.join(".eslintrc.json");
        let source = r#"{ "extends": ["./config/base"] }"#;
        std::fs::write(&root_config, source).unwrap();

        let plugin = EslintPlugin;
        let result = plugin.resolve_config(&root_config, source, root);

        assert!(
            result
                .referenced_dependencies
                .contains(&"eslint-config-airbnb".to_string()),
            "extension-less chained extends should resolve to .cjs: {:?}",
            result.referenced_dependencies
        );
        assert!(
            result
                .setup_files
                .iter()
                .any(|path| path.ends_with(std::path::Path::new("config").join("base.cjs"))),
            "resolved extension-less config should be treated as used: {:?}",
            result.setup_files
        );
    }

    #[test]
    fn resolve_config_relative_extends_cycle_protected() {
        // A.js extends B.js extends A.js: must not infinite-loop.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let a = root.join("a.js");
        let b = root.join("b.js");
        std::fs::write(&a, r"module.exports = { extends: ['./b.js', 'airbnb'] };").unwrap();
        std::fs::write(&b, r"module.exports = { extends: ['./a.js', 'prettier'] };").unwrap();

        let plugin = EslintPlugin;
        let result = plugin.resolve_config(&a, &std::fs::read_to_string(&a).unwrap(), root);

        let deps = &result.referenced_dependencies;
        assert!(deps.contains(&"eslint-config-airbnb".to_string()));
        assert!(deps.contains(&"eslint-config-prettier".to_string()));
    }

    #[test]
    fn resolve_config_relative_extends_missing_target_graceful() {
        // Pointing at a non-existent file must not panic, silently skip.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();

        let root_config = root.join(".eslintrc.json");
        let source = r#"{ "extends": ["./nope/missing.js", "airbnb"] }"#;
        std::fs::write(&root_config, source).unwrap();

        let plugin = EslintPlugin;
        let result = plugin.resolve_config(&root_config, source, root);

        // The non-path entry is still resolved
        assert!(
            result
                .referenced_dependencies
                .contains(&"eslint-config-airbnb".to_string())
        );
    }
}
