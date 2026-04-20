//! Nuxt framework plugin.
//!
//! Detects Nuxt projects and marks pages, layouts, middleware, server API,
//! plugins, composables, and utils as entry points. Recognizes conventional
//! server API and middleware exports. Parses nuxt.config.ts to extract modules,
//! CSS files, plugins, and other configuration.
//!
//! Also detects Nuxt **module** authoring projects (using `@nuxt/kit`) and marks
//! `src/runtime/` components, composables, plugins, and utils as entry points.

use std::path::Path;

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["nuxt"];

/// Secondary enabler for Nuxt module authoring projects.
/// `@nuxt/kit` is the standard API for building Nuxt modules.
const MODULE_AUTHORING_ENABLER: &str = "@nuxt/kit";

const ENTRY_PATTERNS: &[&str] = &[
    // Standard Nuxt directories
    "pages/**/*.{vue,ts,tsx,js,jsx}",
    "layouts/**/*.{vue,ts,tsx,js,jsx}",
    "middleware/**/*.{ts,js}",
    "server/api/**/*.{ts,js}",
    "server/routes/**/*.{ts,js}",
    "server/middleware/**/*.{ts,js}",
    "server/plugins/**/*.{ts,js}",
    "server/utils/**/*.{ts,js}",
    // Nuxt only auto-registers top-level plugins plus nested index files.
    "plugins/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "plugins/**/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    // Nuxt only scans the top level of composables/utils by default.
    "composables/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "utils/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "components/**/*.{vue,ts,tsx,js,jsx}",
    // Nuxt auto-scans modules/ for custom modules
    "modules/**/*.{ts,js}",
    // Nuxt 3 app/ directory structure
    "app/pages/**/*.{vue,ts,tsx,js,jsx}",
    "app/layouts/**/*.{vue,ts,tsx,js,jsx}",
    "app/middleware/**/*.{ts,js}",
    "app/plugins/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "app/plugins/**/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "app/composables/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "app/utils/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "app/components/**/*.{vue,ts,tsx,js,jsx}",
    "app/modules/**/*.{ts,js}",
];

const SRC_DIR_ENTRY_PATTERNS: &[&str] = &[
    "pages/**/*.{vue,ts,tsx,js,jsx}",
    "layouts/**/*.{vue,ts,tsx,js,jsx}",
    "middleware/**/*.{ts,js}",
    "plugins/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "plugins/**/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "composables/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "utils/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "components/**/*.{vue,ts,tsx,js,jsx}",
];

const CONFIG_PATTERNS: &[&str] = &[
    "nuxt.config.{ts,js}",
    // Nuxt module entry point: triggers runtime directory discovery
    "src/module.{ts,js}",
];

const ALWAYS_USED: &[&str] = &[
    "nuxt.config.{ts,js}",
    "app.vue",
    "app.config.{ts,js}",
    "error.vue",
    // Nuxt 3 app/ directory structure
    "app/app.vue",
    "app/app.config.{ts,js}",
    "app/error.vue",
    // Nuxt module entry point
    "src/module.{ts,js}",
];

const SRC_DIR_ALWAYS_USED: &[&str] = &["app.vue", "app.config.{ts,js}", "error.vue"];
const COMPONENT_ENTRY_GLOB: &str = "vue,ts,tsx,js,jsx";
const SCRIPT_ENTRY_GLOB: &str = "ts,js,mts,cts,mjs,cjs";
const SCRIPT_ENTRY_EXTENSIONS: &[&str] = &["ts", "js", "mts", "cts", "mjs", "cjs"];

/// Implicit dependencies that Nuxt provides — these should not be flagged as unlisted.
const TOOLING_DEPENDENCIES: &[&str] = &[
    "nuxt",
    "@nuxt/devtools",
    "@nuxt/test-utils",
    "@nuxt/schema",
    "@nuxt/kit",
    // Implicit Nuxt runtime dependencies (re-exported by Nuxt at build time)
    "vue",
    "vue-router",
    "ofetch",
    "h3",
    "@unhead/vue",
    "@unhead/schema",
    "nitropack",
    "defu",
    "hookable",
    "ufo",
    "unctx",
    "unenv",
    "ohash",
    "pathe",
    "scule",
    "unimport",
    "unstorage",
    "radix3",
    "cookie-es",
    "crossws",
    "consola",
];

const USED_EXPORTS_SERVER_API: &[&str] = &["default", "defineEventHandler"];
const USED_EXPORTS_MIDDLEWARE: &[&str] = &["default"];
const USED_EXPORTS_DEFAULT: &[&str] = &["default"];

const DEFAULT_EXPORT_ENTRY_PATTERNS: &[&str] = &[
    "pages/**/*.{vue,ts,tsx,js,jsx}",
    "layouts/**/*.{vue,ts,tsx,js,jsx}",
    "components/**/*.{vue,ts,tsx,js,jsx}",
    "modules/**/*.{ts,js}",
    "plugins/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "plugins/**/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "app/pages/**/*.{vue,ts,tsx,js,jsx}",
    "app/layouts/**/*.{vue,ts,tsx,js,jsx}",
    "app/components/**/*.{vue,ts,tsx,js,jsx}",
    "app/modules/**/*.{ts,js}",
    "app/plugins/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "app/plugins/**/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "server/routes/**/*.{ts,js}",
    "middleware/**/*.{ts,js}",
    "app/middleware/**/*.{ts,js}",
    "server/middleware/**/*.{ts,js}",
    "server/plugins/**/*.{ts,js}",
    "app.vue",
    "app.config.{ts,js}",
    "error.vue",
    "app/app.vue",
    "app/app.config.{ts,js}",
    "app/error.vue",
];

const SRC_DIR_DEFAULT_EXPORT_PATTERNS: &[&str] = &[
    "pages/**/*.{vue,ts,tsx,js,jsx}",
    "layouts/**/*.{vue,ts,tsx,js,jsx}",
    "components/**/*.{vue,ts,tsx,js,jsx}",
    "modules/**/*.{ts,js}",
    "plugins/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "plugins/**/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
];

/// Virtual module prefixes provided by Nuxt at build time.
const VIRTUAL_MODULE_PREFIXES: &[&str] = &["#"];

pub struct NuxtPlugin;

impl Plugin for NuxtPlugin {
    fn name(&self) -> &'static str {
        "nuxt"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    /// Also activate for Nuxt module authoring projects that depend on `@nuxt/kit`.
    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        deps.iter()
            .any(|d| d == "nuxt" || d == MODULE_AUTHORING_ENABLER)
            || root.join("nuxt.config.ts").exists()
            || root.join("nuxt.config.js").exists()
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

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn virtual_module_prefixes(&self) -> &'static [&'static str] {
        VIRTUAL_MODULE_PREFIXES
    }

    fn path_aliases(&self, root: &Path) -> Vec<(&'static str, String)> {
        // Nuxt's srcDir defaults to `app/` when the directory exists, otherwise root.
        let src_dir = if root.join("app").is_dir() {
            "app".to_string()
        } else {
            String::new()
        };
        let mut aliases = vec![
            // ~/  → srcDir (app/ or root)
            ("~/", src_dir.clone()),
            // @/  → srcDir (Nuxt alias synonym for ~/)
            ("@/", src_dir),
            // ~~/ → rootDir (project root)
            ("~~/", String::new()),
            // @@/ → rootDir (Nuxt alias synonym for ~~/)
            ("@@/", String::new()),
            // #shared/ → shared/ directory
            ("#shared/", "shared".to_string()),
            // #server/ → server/ directory
            ("#server/", "server".to_string()),
        ];
        // Also map the bare `~` and `~~` (without trailing slash) for edge cases
        // like `import '~/composables/foo'` — already covered by `~/` prefix.
        // Map #shared (without slash) for bare imports like `import '#shared'`
        aliases.push(("#shared", "shared".to_string()));
        aliases.push(("#server", "server".to_string()));
        aliases
    }

    fn used_exports(&self) -> Vec<(&'static str, &'static [&'static str])> {
        let mut exports = Vec::with_capacity(DEFAULT_EXPORT_ENTRY_PATTERNS.len() + 3);
        exports.push(("server/api/**/*.{ts,js}", USED_EXPORTS_SERVER_API));
        exports.push(("middleware/**/*.{ts,js}", USED_EXPORTS_MIDDLEWARE));
        exports.push(("app/middleware/**/*.{ts,js}", USED_EXPORTS_MIDDLEWARE));
        exports.extend(
            DEFAULT_EXPORT_ENTRY_PATTERNS
                .iter()
                .copied()
                .map(|pattern| (pattern, USED_EXPORTS_DEFAULT)),
        );
        exports
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();

        // Nuxt module authoring: src/module.{ts,js} → add src/runtime/ patterns.
        // Nuxt modules place their runtime code (components, composables, plugins,
        // utils) in src/runtime/ and register them programmatically via @nuxt/kit
        // APIs (addComponentsDir, addImportsDir, addPlugin).
        if config_path.file_stem().is_some_and(|stem| stem == "module") {
            add_module_runtime_patterns(&mut result, root);

            // Extract import sources as referenced dependencies
            let imports = config_parser::extract_imports(source, config_path);
            for imp in &imports {
                let dep = crate::resolve::extract_package_name(imp);
                result.referenced_dependencies.push(dep);
            }

            return result;
        }

        // Nuxt aliases resolve against srcDir, which defaults to `app/` when it exists
        // and can be overridden explicitly via config.
        let default_src_dir = default_nuxt_src_dir(root);
        let configured_src_dir = extract_nuxt_src_dir(source, config_path, root);
        let src_dir = configured_src_dir
            .clone()
            .unwrap_or_else(|| default_src_dir.clone());

        if let Some(configured_src_dir) = configured_src_dir.as_deref()
            && configured_src_dir != default_src_dir.as_str()
        {
            add_src_dir_support(&mut result, configured_src_dir);
        }

        // Extract import sources as referenced dependencies
        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        // modules: [...] → referenced dependencies (Nuxt modules are npm packages)
        let modules = config_parser::extract_config_string_array(source, config_path, &["modules"]);
        for module in &modules {
            let dep = crate::resolve::extract_package_name(module);
            result.referenced_dependencies.push(dep);
        }

        // css: [...] → always-used files or referenced dependencies
        // Local paths (`~/`, `~~/`, `@/`, `@@/`, `./`, `/`) route through
        // `normalize_nuxt_path` for the same workspace-root-relative resolution
        // used by plugins/components/alias. Bare specifiers are npm package CSS.
        let css = config_parser::extract_config_string_array(source, config_path, &["css"]);
        for entry in &css {
            if is_local_css_path(entry) {
                if let Some(normalized) = normalize_nuxt_path(entry, config_path, root, &src_dir) {
                    result.always_used_files.push(normalized);
                }
            } else {
                // npm package CSS (e.g., `@unocss/reset/tailwind.css`, `floating-vue/dist/style.css`)
                let dep = crate::resolve::extract_package_name(entry);
                result.referenced_dependencies.push(dep);
            }
        }

        // postcss.plugins → referenced dependencies (object keys)
        let postcss_plugins =
            config_parser::extract_config_object_keys(source, config_path, &["postcss", "plugins"]);
        for plugin in &postcss_plugins {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(plugin));
        }

        // plugins: [...] → entry patterns with default-export coverage
        let mut plugins =
            config_parser::extract_config_string_array(source, config_path, &["plugins"]);
        plugins.extend(config_parser::extract_config_array_object_strings(
            source,
            config_path,
            &["plugins"],
            "src",
        ));
        for plugin in plugins {
            if let Some(normalized) = normalize_nuxt_path(&plugin, config_path, root, &src_dir) {
                let pattern = script_entry_pattern(&normalized);
                add_default_used_export(&mut result, &pattern);
                result.push_entry_pattern(pattern);
            }
        }

        // alias: { "@shared": "./shared" } → resolver path aliases
        for (find, replacement) in
            config_parser::extract_config_aliases(source, config_path, &["alias"])
        {
            if let Some(normalized) = normalize_nuxt_path(&replacement, config_path, root, &src_dir)
            {
                result.path_aliases.push((find, normalized));
            }
        }

        // imports.dirs: ["~/custom/composables"] → auto-import roots
        for dir in
            config_parser::extract_config_string_array(source, config_path, &["imports", "dirs"])
        {
            if let Some(pattern) = normalize_imports_dir_pattern(&dir, config_path, root, &src_dir)
            {
                result.push_entry_pattern(pattern);
            }
        }

        // components config supports string arrays, object arrays, and object.dirs arrays.
        let mut component_dirs =
            config_parser::extract_config_string_array(source, config_path, &["components"]);
        component_dirs.extend(config_parser::extract_config_array_object_strings(
            source,
            config_path,
            &["components"],
            "path",
        ));
        component_dirs.extend(config_parser::extract_config_array_object_strings(
            source,
            config_path,
            &["components", "dirs"],
            "path",
        ));
        component_dirs.extend(config_parser::extract_config_string_array(
            source,
            config_path,
            &["components", "dirs"],
        ));
        for dir in component_dirs {
            if let Some(normalized) = normalize_nuxt_path(&dir, config_path, root, &src_dir) {
                let pattern = component_dir_pattern(&normalized);
                add_default_used_export(&mut result, &pattern);
                result.push_entry_pattern(pattern);
            }
        }

        // extends: [...] → referenced dependencies
        let extends = config_parser::extract_config_string_array(source, config_path, &["extends"]);
        for ext in &extends {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(ext));
        }

        result
    }
}

/// Add entry patterns for the Nuxt module authoring convention.
///
/// Nuxt modules use `src/runtime/` for components, composables, plugins, and
/// utils that are programmatically registered via `@nuxt/kit` APIs. We detect
/// two common layouts:
///   - `src/runtime/{components,composables,plugins,utils,locale}/`
///   - `runtime/{components,composables,plugins,utils,locale}/` (less common)
fn add_module_runtime_patterns(result: &mut PluginResult, root: &Path) {
    let runtime_dir = if root.join("src/runtime").is_dir() {
        "src/runtime"
    } else if root.join("runtime").is_dir() {
        "runtime"
    } else {
        return;
    };

    // Components (Vue SFCs and TS/JS)
    let components = format!("{runtime_dir}/components/**/*.{{{COMPONENT_ENTRY_GLOB}}}");
    add_default_used_export(result, &components);
    result.push_entry_pattern(components);

    // Composables (top-level only, matching Nuxt convention)
    let composables = format!("{runtime_dir}/composables/*.{{{SCRIPT_ENTRY_GLOB}}}");
    result.push_entry_pattern(composables);

    // Utils (top-level only)
    let utils = format!("{runtime_dir}/utils/*.{{{SCRIPT_ENTRY_GLOB}}}");
    result.push_entry_pattern(utils);

    // Plugins
    let plugins = format!("{runtime_dir}/plugins/*.{{{SCRIPT_ENTRY_GLOB}}}");
    add_default_used_export(result, &plugins);
    result.push_entry_pattern(plugins);

    // Locale files (common in i18n-aware modules like Nuxt UI)
    let locale_dir = root.join(runtime_dir).join("locale");
    if locale_dir.is_dir() {
        let locale = format!("{runtime_dir}/locale/*.{{{SCRIPT_ENTRY_GLOB}}}");
        result.push_entry_pattern(locale);
    }

    // Types directory (re-exported types)
    let types_dir = root.join(runtime_dir).join("types");
    if types_dir.is_dir() {
        let types = format!("{runtime_dir}/types/*.{{{SCRIPT_ENTRY_GLOB}}}");
        result.push_entry_pattern(types);
    }

    // Vue-specific runtime directory: mirrors the main runtime structure with
    // its own components, composables, plugins, and stubs subdirectories.
    let vue_dir = root.join(runtime_dir).join("vue");
    if vue_dir.is_dir() {
        let vue_components = format!("{runtime_dir}/vue/**/*.{{{COMPONENT_ENTRY_GLOB}}}");
        add_default_used_export(result, &vue_components);
        result.push_entry_pattern(vue_components);
    }
}

fn default_nuxt_src_dir(root: &Path) -> String {
    if root.join("app").is_dir() {
        "app".to_string()
    } else {
        String::new()
    }
}

fn is_local_css_path(entry: &str) -> bool {
    entry.starts_with("~/")
        || entry.starts_with("~~/")
        || entry.starts_with("@/")
        || entry.starts_with("@@/")
        || entry.starts_with('.')
        || entry.starts_with('/')
}

fn extract_nuxt_src_dir(source: &str, config_path: &Path, root: &Path) -> Option<String> {
    let raw = config_parser::extract_config_string(source, config_path, &["srcDir"])?;
    normalize_nuxt_src_dir(&raw, config_path, root)
}

fn normalize_nuxt_src_dir(raw: &str, config_path: &Path, root: &Path) -> Option<String> {
    let trimmed = raw.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Some(String::new());
    }
    config_parser::normalize_config_path(trimmed, config_path, root)
}

fn add_src_dir_support(result: &mut PluginResult, src_dir: &str) {
    result
        .path_aliases
        .push(("~/".to_string(), src_dir.to_string()));
    result
        .path_aliases
        .push(("@/".to_string(), src_dir.to_string()));

    if src_dir.is_empty() {
        return;
    }

    result.extend_entry_patterns(
        SRC_DIR_ENTRY_PATTERNS
            .iter()
            .map(|pattern| prefix_with_src_dir(src_dir, pattern)),
    );
    extend_prefixed_patterns(&mut result.always_used_files, src_dir, SRC_DIR_ALWAYS_USED);
    add_prefixed_default_used_exports(result, src_dir, SRC_DIR_DEFAULT_EXPORT_PATTERNS);
    add_default_used_export(
        result,
        prefix_with_src_dir(src_dir, "middleware/**/*.{ts,js}"),
    );
    add_prefixed_default_used_exports(result, src_dir, SRC_DIR_ALWAYS_USED);
}

fn add_default_used_export(result: &mut PluginResult, pattern: impl Into<String>) {
    result.push_used_export_rule(pattern, ["default"]);
}

fn add_prefixed_default_used_exports(result: &mut PluginResult, prefix: &str, patterns: &[&str]) {
    for pattern in patterns {
        add_default_used_export(result, prefix_with_src_dir(prefix, pattern));
    }
}

fn extend_prefixed_patterns(target: &mut Vec<String>, prefix: &str, patterns: &[&str]) {
    target.extend(
        patterns
            .iter()
            .map(|pattern| prefix_with_src_dir(prefix, pattern)),
    );
}

fn component_dir_pattern(dir: &str) -> String {
    format!("{dir}/**/*.{{{COMPONENT_ENTRY_GLOB}}}")
}

fn script_entry_pattern(path: &str) -> String {
    if has_supported_extension(path, SCRIPT_ENTRY_EXTENSIONS) {
        path.to_string()
    } else {
        format!("{path}.{{{SCRIPT_ENTRY_GLOB}}}")
    }
}

fn has_supported_extension(path: &str, supported_extensions: &[&str]) -> bool {
    Path::new(path)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| supported_extensions.contains(&ext))
}

fn normalize_nuxt_path(
    raw: &str,
    config_path: &Path,
    root: &Path,
    src_dir: &str,
) -> Option<String> {
    if let Some(stripped) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("@/")) {
        return Some(prefix_with_src_dir(src_dir, stripped));
    }

    if let Some(stripped) = raw.strip_prefix("~~/").or_else(|| raw.strip_prefix("@@/")) {
        return Some(stripped.to_string());
    }

    config_parser::normalize_config_path(raw, config_path, root)
}

fn normalize_imports_dir_pattern(
    raw: &str,
    config_path: &Path,
    root: &Path,
    src_dir: &str,
) -> Option<String> {
    let normalized = normalize_nuxt_path(raw, config_path, root, src_dir)?;
    Some(imports_dir_pattern(&normalized))
}

fn imports_dir_pattern(normalized: &str) -> String {
    let normalized = normalized.trim_end_matches('/');
    if normalized.is_empty() {
        return "*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}".to_string();
    }

    if has_glob_syntax(normalized) {
        if path_looks_like_file_pattern(normalized) {
            normalized.to_string()
        } else {
            format!("{normalized}/*.{{ts,tsx,js,jsx,mts,cts,mjs,cjs}}")
        }
    } else {
        format!("{normalized}/*.{{ts,tsx,js,jsx,mts,cts,mjs,cjs}}")
    }
}

fn has_glob_syntax(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?') || pattern.contains('[') || pattern.contains('{')
}

fn path_looks_like_file_pattern(pattern: &str) -> bool {
    pattern
        .rsplit('/')
        .next()
        .is_some_and(|segment| segment.contains('.'))
}

fn prefix_with_src_dir(src_dir: &str, path: &str) -> String {
    if src_dir.is_empty() {
        path.to_string()
    } else {
        format!("{src_dir}/{path}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_entry_pattern(result: &PluginResult, pattern: &str) -> bool {
        result
            .entry_patterns
            .iter()
            .any(|entry_pattern| entry_pattern.pattern == pattern)
    }

    fn has_used_export_rule(result: &PluginResult, pattern: &str, exports: &[&str]) -> bool {
        result.used_exports.iter().any(|rule| {
            rule.path.pattern == pattern
                && exports
                    .iter()
                    .all(|expected| rule.exports.iter().any(|actual| actual == expected))
        })
    }

    #[test]
    fn enabler_is_nuxt() {
        let plugin = NuxtPlugin;
        assert_eq!(plugin.enablers(), &["nuxt"]);
    }

    #[test]
    fn is_enabled_with_nuxt_dep() {
        let plugin = NuxtPlugin;
        let deps = vec!["nuxt".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn is_enabled_with_nuxt_kit_dep() {
        let plugin = NuxtPlugin;
        let deps = vec!["@nuxt/kit".to_string()];
        assert!(
            plugin.is_enabled_with_deps(&deps, Path::new("/project")),
            "@nuxt/kit should activate the Nuxt plugin for module authoring"
        );
    }

    #[test]
    fn is_not_enabled_without_nuxt() {
        let plugin = NuxtPlugin;
        let deps = vec!["vue".to_string()];
        assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn entry_patterns_include_nuxt_conventions() {
        let plugin = NuxtPlugin;
        let patterns = plugin.entry_patterns();
        assert!(patterns.iter().any(|p| p.starts_with("pages/")));
        assert!(patterns.iter().any(|p| p.starts_with("layouts/")));
        assert!(patterns.iter().any(|p| p.starts_with("server/api/")));
        assert!(patterns.contains(&"composables/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}"));
        assert!(patterns.iter().any(|p| p.starts_with("components/")));
    }

    #[test]
    fn entry_patterns_include_app_dir_variants() {
        let plugin = NuxtPlugin;
        let patterns = plugin.entry_patterns();
        assert!(
            patterns.iter().any(|p| p.starts_with("app/pages/")),
            "should include Nuxt 3 app/ directory variants"
        );
    }

    #[test]
    fn virtual_module_prefixes_includes_hash() {
        let plugin = NuxtPlugin;
        assert_eq!(plugin.virtual_module_prefixes(), &["#"]);
    }

    #[test]
    fn path_aliases_include_nuxt_at_variants() {
        let plugin = NuxtPlugin;
        let aliases = plugin.path_aliases(Path::new("/project"));
        assert!(aliases.iter().any(|(prefix, _)| *prefix == "@/"));
        assert!(aliases.iter().any(|(prefix, _)| *prefix == "@@/"));
    }

    #[test]
    fn used_exports_for_server_api() {
        let plugin = NuxtPlugin;
        let exports = plugin.used_exports();
        let api_entry = exports
            .iter()
            .find(|(pat, _)| *pat == "server/api/**/*.{ts,js}");
        assert!(api_entry.is_some());
        let (_, names) = api_entry.unwrap();
        assert!(names.contains(&"default"));
        assert!(names.contains(&"defineEventHandler"));
    }

    #[test]
    fn used_exports_cover_runtime_default_exports() {
        let plugin = NuxtPlugin;
        let exports = plugin.used_exports();

        for pattern in [
            "pages/**/*.{vue,ts,tsx,js,jsx}",
            "layouts/**/*.{vue,ts,tsx,js,jsx}",
            "components/**/*.{vue,ts,tsx,js,jsx}",
            "plugins/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
            "plugins/**/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
            "modules/**/*.{ts,js}",
            "server/routes/**/*.{ts,js}",
            "server/plugins/**/*.{ts,js}",
            "app/components/**/*.{vue,ts,tsx,js,jsx}",
            "app.vue",
            "app.config.{ts,js}",
            "app/app.vue",
        ] {
            let entry = exports
                .iter()
                .find(|(candidate, _)| *candidate == pattern)
                .unwrap_or_else(|| panic!("missing used_exports rule for {pattern}"));
            assert!(
                entry.1.contains(&"default"),
                "{pattern} should keep the default export alive"
            );
        }
    }

    // ── resolve_config tests ─────────────────────────────────────

    #[test]
    fn resolve_config_modules_as_deps() {
        let source = r#"
            export default defineNuxtConfig({
                modules: ["@nuxtjs/tailwindcss", "@pinia/nuxt"]
            });
        "#;
        let plugin = NuxtPlugin;
        let result =
            plugin.resolve_config(Path::new("nuxt.config.ts"), source, Path::new("/project"));
        assert!(
            result
                .referenced_dependencies
                .contains(&"@nuxtjs/tailwindcss".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@pinia/nuxt".to_string())
        );
    }

    #[test]
    fn resolve_config_css_tilde_resolves_to_root() {
        // Without an `app/` dir, `~/` resolves to project root
        let source = r#"
            export default defineNuxtConfig({
                css: ["~/assets/main.css"]
            });
        "#;
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(
            Path::new("nuxt.config.ts"),
            source,
            Path::new("/nonexistent"),
        );
        assert!(
            result
                .always_used_files
                .contains(&"assets/main.css".to_string()),
            "~/assets/main.css should resolve to assets/main.css without app/ dir: {:?}",
            result.always_used_files
        );
    }

    #[test]
    fn resolve_config_css_double_tilde_always_root() {
        let source = r#"
            export default defineNuxtConfig({
                css: ["~~/shared/global.css"]
            });
        "#;
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(
            Path::new("nuxt.config.ts"),
            source,
            Path::new("/nonexistent"),
        );
        assert!(
            result
                .always_used_files
                .contains(&"shared/global.css".to_string()),
            "~~/shared/global.css should resolve to shared/global.css"
        );
    }

    #[test]
    fn resolve_config_css_npm_package() {
        let source = r#"
            export default defineNuxtConfig({
                css: ["@unocss/reset/tailwind.css"]
            });
        "#;
        let plugin = NuxtPlugin;
        let result =
            plugin.resolve_config(Path::new("nuxt.config.ts"), source, Path::new("/project"));
        assert!(
            result
                .referenced_dependencies
                .contains(&"@unocss/reset".to_string()),
            "npm package CSS should be tracked as referenced dependency"
        );
    }

    #[test]
    fn resolve_config_postcss_plugins_as_deps() {
        let source = r#"
            export default defineNuxtConfig({
                postcss: {
                    plugins: {
                        autoprefixer: {},
                        "postcss-nested": {}
                    }
                }
            });
        "#;
        let plugin = NuxtPlugin;
        let result =
            plugin.resolve_config(Path::new("nuxt.config.ts"), source, Path::new("/project"));
        assert!(
            result
                .referenced_dependencies
                .contains(&"autoprefixer".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"postcss-nested".to_string())
        );
    }

    #[test]
    fn resolve_config_extends_as_deps() {
        let source = r#"
            export default defineNuxtConfig({
                extends: ["@nuxt/ui-pro"]
            });
        "#;
        let plugin = NuxtPlugin;
        let result =
            plugin.resolve_config(Path::new("nuxt.config.ts"), source, Path::new("/project"));
        assert!(
            result
                .referenced_dependencies
                .contains(&"@nuxt/ui-pro".to_string())
        );
    }

    #[test]
    fn resolve_config_import_sources_as_deps() {
        let source = r#"
            import { defineNuxtConfig } from "nuxt/config";
            export default defineNuxtConfig({});
        "#;
        let plugin = NuxtPlugin;
        let result =
            plugin.resolve_config(Path::new("nuxt.config.ts"), source, Path::new("/project"));
        assert!(
            result.referenced_dependencies.contains(&"nuxt".to_string()),
            "import source should be extracted as a referenced dependency"
        );
    }

    #[test]
    fn resolve_config_empty_source() {
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(Path::new("nuxt.config.ts"), "", Path::new("/project"));
        assert!(result.referenced_dependencies.is_empty());
        assert!(result.always_used_files.is_empty());
        assert!(result.entry_patterns.is_empty());
    }

    #[test]
    fn resolve_config_css_relative_path() {
        let source = r#"
            export default defineNuxtConfig({
                css: ["./assets/global.css"]
            });
        "#;
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/nuxt.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(
            result
                .always_used_files
                .contains(&"assets/global.css".to_string()),
            "relative CSS path should resolve to a workspace-root-relative always-used file: {:?}",
            result.always_used_files
        );
    }

    #[test]
    fn resolve_config_css_relative_with_nested_config() {
        // `./assets/global.css` in docs/nuxt.config.ts is config-relative, so it
        // resolves to docs/assets/global.css at the workspace root.
        let tmp = tempfile::tempdir().expect("create tempdir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("docs")).expect("create docs");
        let config_path = root.join("docs/nuxt.config.ts");

        let source = r#"
            export default defineNuxtConfig({
                css: ["./assets/global.css"]
            });
        "#;
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(&config_path, source, root);

        let expected = "docs/assets/global.css";
        assert!(
            result
                .always_used_files
                .iter()
                .any(|p| p.replace('\\', "/") == expected),
            "./assets/global.css should resolve relative to config dir: {:?}",
            result.always_used_files
        );
    }

    #[test]
    fn resolve_config_css_tilde_with_srcdir_app() {
        // Root-level config with explicit srcDir: 'app' plus `css: ['~/assets/main.css']`.
        // Previously untested combination — covers the tests/904–1128 gap.
        let tmp = tempfile::tempdir().expect("create tempdir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("app")).expect("create app");
        let config_path = root.join("nuxt.config.ts");

        let source = r#"
            export default defineNuxtConfig({
                srcDir: "app/",
                css: ["~/assets/main.css"]
            });
        "#;
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(&config_path, source, root);

        let expected = "app/assets/main.css";
        assert!(
            result
                .always_used_files
                .iter()
                .any(|p| p.replace('\\', "/") == expected),
            "~/assets/main.css with srcDir:'app' should resolve to {expected}: {:?}",
            result.always_used_files
        );
    }

    #[test]
    fn resolve_config_extracts_custom_aliases_and_dirs() {
        let source = r#"
            export default defineNuxtConfig({
                srcDir: "app/",
                alias: {
                    "@shared": "./app/shared"
                },
                imports: {
                    dirs: ["~/custom/composables"]
                },
                components: [
                    { path: "@/feature-components" }
                ]
            });
        "#;
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/nuxt.config.ts"),
            source,
            Path::new("/project"),
        );

        assert!(
            result
                .path_aliases
                .contains(&("@shared".to_string(), "app/shared".to_string()))
        );
        assert!(
            result
                .path_aliases
                .contains(&("~/".to_string(), "app".to_string()))
        );
        assert!(
            result
                .path_aliases
                .contains(&("@/".to_string(), "app".to_string()))
        );
        assert!(has_entry_pattern(
            &result,
            "app/custom/composables/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}"
        ));
        assert!(has_entry_pattern(
            &result,
            "app/feature-components/**/*.{vue,ts,tsx,js,jsx}"
        ));
        assert!(
            has_used_export_rule(
                &result,
                "app/feature-components/**/*.{vue,ts,tsx,js,jsx}",
                &["default"],
            ),
            "custom component dirs should contribute default-export used rules"
        );
        assert!(
            result
                .always_used_files
                .contains(&"app/app.config.{ts,js}".to_string())
        );
    }

    #[test]
    fn resolve_config_plugins_supports_string_and_object_entries() {
        let source = r#"
            export default defineNuxtConfig({
                srcDir: "app/",
                plugins: [
                    "~/runtime/plain-plugin",
                    { src: "@/runtime/object-plugin", mode: "client" }
                ]
            });
        "#;
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/nuxt.config.ts"),
            source,
            Path::new("/project"),
        );

        for pattern in [
            "app/runtime/plain-plugin.{ts,js,mts,cts,mjs,cjs}",
            "app/runtime/object-plugin.{ts,js,mts,cts,mjs,cjs}",
        ] {
            assert!(
                has_entry_pattern(&result, pattern),
                "expected configured plugin entry pattern {pattern}, got {:?}",
                result.entry_patterns
            );
            assert!(
                has_used_export_rule(&result, pattern, &["default"]),
                "configured plugin pattern {pattern} should keep default exports alive"
            );
        }
    }

    #[test]
    fn resolve_config_components_dirs_supports_nested_object_entries() {
        let source = r#"
            export default defineNuxtConfig({
                srcDir: "app/",
                components: {
                    dirs: [
                        { path: "~/feature/ui" }
                    ]
                }
            });
        "#;
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/nuxt.config.ts"),
            source,
            Path::new("/project"),
        );

        let expected = "app/feature/ui/**/*.{vue,ts,tsx,js,jsx}".to_string();
        assert!(
            has_entry_pattern(&result, &expected),
            "nested components.dirs object entries should add entry patterns"
        );
        assert!(
            has_used_export_rule(&result, &expected, &["default"]),
            "nested components.dirs object entries should keep default component exports alive"
        );
    }

    #[test]
    fn resolve_config_src_dir_overrides_default_app_aliases() {
        let source = r#"
            export default defineNuxtConfig({
                srcDir: "."
            });
        "#;
        let plugin = NuxtPlugin;
        let temp = tempfile::tempdir().expect("temp dir should be created");
        std::fs::create_dir(temp.path().join("app")).expect("app dir should exist");
        let config_path = temp.path().join("nuxt.config.ts");
        let result = plugin.resolve_config(&config_path, source, temp.path());

        assert!(
            result
                .path_aliases
                .contains(&("~/".to_string(), String::new())),
            "srcDir='.' should remap ~/ to the project root"
        );
        assert!(
            result
                .path_aliases
                .contains(&("@/".to_string(), String::new())),
            "srcDir='.' should remap @/ to the project root"
        );
    }

    #[test]
    fn resolve_config_src_dir_adds_custom_source_roots() {
        let source = r#"
            export default defineNuxtConfig({
                srcDir: "src/",
                imports: {
                    dirs: ["~/custom/composables"]
                },
                components: [
                    { path: "@/feature-components" }
                ]
            });
        "#;
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/nuxt.config.ts"),
            source,
            Path::new("/project"),
        );

        assert!(
            result
                .path_aliases
                .contains(&("~/".to_string(), "src".to_string())),
            "srcDir should remap ~/ to the configured source root"
        );
        assert!(
            result
                .path_aliases
                .contains(&("@/".to_string(), "src".to_string())),
            "srcDir should remap @/ to the configured source root"
        );
        assert!(has_entry_pattern(
            &result,
            "src/custom/composables/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}"
        ));
        assert!(has_entry_pattern(
            &result,
            "src/feature-components/**/*.{vue,ts,tsx,js,jsx}"
        ));
        for expected in [
            "src/middleware/**/*.{ts,js}",
            "src/plugins/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
            "src/plugins/**/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
            "src/components/**/*.{vue,ts,tsx,js,jsx}",
        ] {
            assert!(
                has_used_export_rule(&result, expected, &["default"]),
                "{expected} should keep default exports alive under srcDir"
            );
        }
        assert!(
            result
                .always_used_files
                .contains(&"src/app.vue".to_string()),
            "srcDir should add app.vue under the configured source root"
        );
        assert!(
            result
                .always_used_files
                .contains(&"src/app.config.{ts,js}".to_string()),
            "srcDir should add app.config under the configured source root"
        );
        assert!(
            result
                .always_used_files
                .contains(&"src/error.vue".to_string()),
            "srcDir should add error.vue under the configured source root"
        );
    }

    #[test]
    fn imports_dirs_glob_can_scan_nested_files() {
        assert_eq!(
            imports_dir_pattern("app/composables"),
            "app/composables/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}"
        );
        assert_eq!(
            imports_dir_pattern("app/composables/**"),
            "app/composables/**/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}"
        );
        assert_eq!(
            imports_dir_pattern("app/composables/*/index.{ts,js,mjs,mts}"),
            "app/composables/*/index.{ts,js,mjs,mts}"
        );
    }

    #[test]
    fn entry_patterns_keep_nested_plugin_index_only() {
        let plugin = NuxtPlugin;
        let patterns = plugin.entry_patterns();
        assert!(patterns.contains(&"plugins/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}"));
        assert!(patterns.contains(&"plugins/**/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}"));
        assert!(!patterns.contains(&"plugins/**/*.{ts,js}"));
    }

    // ── Nuxt module authoring tests ──────────────────────────────

    #[test]
    fn module_authoring_resolve_config_adds_runtime_patterns() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = temp.path().join("src/runtime");
        std::fs::create_dir_all(runtime.join("components")).unwrap();
        std::fs::create_dir_all(runtime.join("composables")).unwrap();
        std::fs::create_dir_all(runtime.join("plugins")).unwrap();
        std::fs::create_dir_all(runtime.join("utils")).unwrap();

        let source = r"
            import { defineNuxtModule, addComponentsDir } from '@nuxt/kit';
            export default defineNuxtModule({ setup() {} });
        ";
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(&temp.path().join("src/module.ts"), source, temp.path());

        assert!(
            result
                .entry_patterns
                .iter()
                .any(|p| p.starts_with("src/runtime/components/")),
            "should add runtime components: {:?}",
            result.entry_patterns
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|p| p.starts_with("src/runtime/composables/")),
            "should add runtime composables: {:?}",
            result.entry_patterns
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|p| p.starts_with("src/runtime/plugins/")),
            "should add runtime plugins: {:?}",
            result.entry_patterns
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|p| p.starts_with("src/runtime/utils/")),
            "should add runtime utils: {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn module_authoring_detects_locale_and_types_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let runtime = temp.path().join("src/runtime");
        std::fs::create_dir_all(runtime.join("components")).unwrap();
        std::fs::create_dir_all(runtime.join("locale")).unwrap();
        std::fs::create_dir_all(runtime.join("types")).unwrap();

        let source = "";
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(&temp.path().join("src/module.ts"), source, temp.path());

        assert!(
            result
                .entry_patterns
                .iter()
                .any(|p| p.starts_with("src/runtime/locale/")),
            "should detect locale dir: {:?}",
            result.entry_patterns
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|p| p.starts_with("src/runtime/types/")),
            "should detect types dir: {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn module_authoring_no_runtime_dir_is_noop() {
        let temp = tempfile::tempdir().unwrap();
        // No src/runtime/ directory
        let source = "";
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(&temp.path().join("src/module.ts"), source, temp.path());
        assert!(
            result.entry_patterns.is_empty(),
            "no runtime dir should produce no patterns"
        );
    }

    #[test]
    fn module_authoring_extracts_import_deps() {
        let temp = tempfile::tempdir().unwrap();
        let source = r"
            import { defineNuxtModule, addComponentsDir } from '@nuxt/kit';
            import defu from 'defu';
        ";
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(&temp.path().join("src/module.ts"), source, temp.path());
        assert!(
            result
                .referenced_dependencies
                .contains(&"@nuxt/kit".to_string()),
            "@nuxt/kit should be a referenced dependency"
        );
        assert!(
            result.referenced_dependencies.contains(&"defu".to_string()),
            "defu should be a referenced dependency"
        );
    }

    #[test]
    fn nuxt_config_not_treated_as_module() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp.path().join("src/runtime/components")).unwrap();

        let source = r#"
            export default defineNuxtConfig({
                modules: ["@nuxtjs/tailwindcss"]
            });
        "#;
        let plugin = NuxtPlugin;
        let result =
            plugin.resolve_config(&temp.path().join("nuxt.config.ts"), source, temp.path());

        // nuxt.config.ts should NOT add runtime patterns
        assert!(
            !result.entry_patterns.iter().any(|p| p.contains("runtime")),
            "nuxt.config.ts should not add runtime patterns: {:?}",
            result.entry_patterns
        );
    }
}
