//! Nuxt framework plugin.
//!
//! Detects Nuxt projects and marks pages, layouts, middleware, server API,
//! plugins, composables, and utils as entry points. Recognizes conventional
//! server API and middleware exports. Parses nuxt.config.ts to extract modules,
//! CSS files, plugins, and other configuration.
//!
//! Also detects Nuxt **module** authoring projects (using `@nuxt/kit`) and marks
//! `src/runtime/` components, composables, plugins, and utils as entry points.
//!
//! When `@nuxt/content` is registered in the nuxt.config `modules:` array, the
//! adjacent `content.config.{ts,js,mts,mjs,cts,cjs}` (which `@nuxt/content` reads
//! at build time and nothing imports) is credited as a default-export entry.

use std::path::{Path, PathBuf};

use fallow_config::{AutoImportKind, AutoImportRule};

use super::config_parser;
use super::{Plugin, PluginResult};

const ENABLERS: &[&str] = &["nuxt"];

/// Directories Nuxt auto-imports components from, relative to the project root.
/// Nuxt 4 uses `app/components`; Nuxt 3 uses top-level `components`. Both are
/// scanned when present. Custom `components: [...]` dirs in `nuxt.config` are out
/// of scope (the entry-pattern fallback keeps those files alive). See issue #704.
const COMPONENT_DIRS: &[&str] = &["components", "app/components"];

/// File extensions Nuxt treats as components, matching the component entry glob.
const COMPONENT_EXTS: &[&str] = &["vue", "ts", "tsx", "js", "jsx"];

/// Filename suffixes Nuxt strips before deriving the component name
/// (`Comments.client.vue` and `Comments.server.vue` both become `<Comments>`;
/// `Foo.global.vue` becomes `<Foo>`).
const COMPONENT_NAME_SUFFIXES: &[&str] = &["client", "server", "global"];

/// Secondary enabler for Nuxt module authoring projects.
/// `@nuxt/kit` is the standard API for building Nuxt modules.
const MODULE_AUTHORING_ENABLER: &str = "@nuxt/kit";

/// First-party module whose root `content.config.*` file is read at build time.
/// When registered in `modules:`, its config file is credited as an entry point.
const CONTENT_MODULE: &str = "@nuxt/content";

const ENTRY_PATTERNS: &[&str] = &[
    "pages/**/*.{vue,ts,tsx,js,jsx}",
    "layouts/**/*.{vue,ts,tsx,js,jsx}",
    "middleware/**/*.{ts,js}",
    "server/api/**/*.{ts,js}",
    "server/routes/**/*.{ts,js}",
    "server/middleware/**/*.{ts,js}",
    "server/plugins/**/*.{ts,js}",
    "server/utils/**/*.{ts,js}",
    "plugins/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "plugins/**/index.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "composables/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "utils/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "shared/utils/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "shared/types/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
    "components/**/*.{vue,ts,tsx,js,jsx}",
    "modules/**/*.{ts,js}",
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

const CONFIG_PATTERNS: &[&str] = &["nuxt.config.{ts,js}", "src/module.{ts,js}"];

const ALWAYS_USED: &[&str] = &[
    "nuxt.config.{ts,js}",
    "app.vue",
    "app.config.{ts,js}",
    "error.vue",
    "app/app.vue",
    "app/app.config.{ts,js}",
    "app/error.vue",
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
        let src_dir = if root.join("app").is_dir() {
            "app".to_string()
        } else {
            String::new()
        };
        let mut aliases = vec![
            ("~/", src_dir.clone()),
            ("@/", src_dir),
            ("~~/", String::new()),
            ("@@/", String::new()),
            ("#shared/", "shared".to_string()),
            ("#server/", "server".to_string()),
        ];
        aliases.push(("#shared", "shared".to_string()));
        aliases.push(("#server", "server".to_string()));
        aliases
    }

    fn auto_imports(&self, root: &Path) -> Vec<AutoImportRule> {
        let mut rules = Vec::new();
        for dir in COMPONENT_DIRS {
            let base = root.join(dir);
            if base.is_dir() {
                collect_component_auto_imports(&base, &base, &mut rules);
            }
        }
        rules
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

        if config_path.file_stem().is_some_and(|stem| stem == "module") {
            add_module_runtime_patterns(&mut result, root);

            let imports = config_parser::extract_imports(source, config_path);
            for imp in &imports {
                let dep = crate::resolve::extract_package_name(imp);
                result.referenced_dependencies.push(dep);
            }

            return result;
        }

        let default_src_dir = default_nuxt_src_dir(root);
        let configured_src_dir = extract_nuxt_src_dir(source, config_path, root);
        let src_dir = configured_src_dir
            .clone()
            .unwrap_or_else(|| default_src_dir.clone());

        if let Some(configured_src_dir) = configured_src_dir.as_deref()
            && configured_src_dir != default_src_dir.as_path()
        {
            add_src_dir_support(&mut result, configured_src_dir);
        }

        let imports = config_parser::extract_imports(source, config_path);
        for imp in &imports {
            let dep = crate::resolve::extract_package_name(imp);
            result.referenced_dependencies.push(dep);
        }

        let modules = config_parser::extract_config_string_array(source, config_path, &["modules"]);
        let mut content_module_registered = false;
        for module in &modules {
            let dep = crate::resolve::extract_package_name(module);
            if dep == CONTENT_MODULE {
                content_module_registered = true;
            }
            result.referenced_dependencies.push(dep);
        }

        if content_module_registered
            && let Some(pattern) = content_config_entry_pattern(config_path, root)
        {
            add_default_used_export(&mut result, &pattern);
            result.push_entry_pattern(pattern);
        }

        let css = config_parser::extract_config_string_array(source, config_path, &["css"]);
        for entry in &css {
            if is_local_css_path(entry) {
                if let Some(normalized) = normalize_nuxt_path(entry, config_path, root, &src_dir) {
                    result.always_used_files.push(normalized);
                }
            } else {
                let dep = crate::resolve::extract_package_name(entry);
                result.referenced_dependencies.push(dep);
            }
        }

        let postcss_plugins =
            config_parser::extract_config_object_keys(source, config_path, &["postcss", "plugins"]);
        for plugin in &postcss_plugins {
            result
                .referenced_dependencies
                .push(crate::resolve::extract_package_name(plugin));
        }

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

        for (find, replacement) in
            config_parser::extract_config_path_aliases(source, config_path, &["alias"])
        {
            let replacement = config_parser::path_to_config_string(&replacement);
            if let Some(normalized) = normalize_nuxt_path(&replacement, config_path, root, &src_dir)
            {
                result.path_aliases.push((find, normalized));
            }
        }

        for dir in
            config_parser::extract_config_string_array(source, config_path, &["imports", "dirs"])
        {
            if let Some(pattern) = normalize_imports_dir_pattern(&dir, config_path, root, &src_dir)
            {
                result.push_entry_pattern(pattern);
            }
        }

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

    let components = format!("{runtime_dir}/components/**/*.{{{COMPONENT_ENTRY_GLOB}}}");
    add_default_used_export(result, &components);
    result.push_entry_pattern(components);

    let composables = format!("{runtime_dir}/composables/*.{{{SCRIPT_ENTRY_GLOB}}}");
    result.push_entry_pattern(composables);

    let utils = format!("{runtime_dir}/utils/*.{{{SCRIPT_ENTRY_GLOB}}}");
    result.push_entry_pattern(utils);

    let plugins = format!("{runtime_dir}/plugins/*.{{{SCRIPT_ENTRY_GLOB}}}");
    add_default_used_export(result, &plugins);
    result.push_entry_pattern(plugins);

    let locale_dir = root.join(runtime_dir).join("locale");
    if locale_dir.is_dir() {
        let locale = format!("{runtime_dir}/locale/*.{{{SCRIPT_ENTRY_GLOB}}}");
        result.push_entry_pattern(locale);
    }

    let types_dir = root.join(runtime_dir).join("types");
    if types_dir.is_dir() {
        let types = format!("{runtime_dir}/types/*.{{{SCRIPT_ENTRY_GLOB}}}");
        result.push_entry_pattern(types);
    }

    let vue_dir = root.join(runtime_dir).join("vue");
    if vue_dir.is_dir() {
        let vue_components = format!("{runtime_dir}/vue/**/*.{{{COMPONENT_ENTRY_GLOB}}}");
        add_default_used_export(result, &vue_components);
        result.push_entry_pattern(vue_components);
    }
}

fn default_nuxt_src_dir(root: &Path) -> PathBuf {
    if root.join("app").is_dir() {
        PathBuf::from("app")
    } else {
        PathBuf::new()
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

fn extract_nuxt_src_dir(source: &str, config_path: &Path, root: &Path) -> Option<PathBuf> {
    let raw = config_parser::extract_config_path(source, config_path, &["srcDir"])?;
    normalize_nuxt_src_dir(&raw, config_path, root)
}

fn normalize_nuxt_src_dir(raw: &Path, config_path: &Path, root: &Path) -> Option<PathBuf> {
    let raw_string = config_parser::path_to_config_string(raw);
    let trimmed = raw_string.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return Some(PathBuf::new());
    }
    config_parser::normalize_config_path_buf(
        config_parser::path_from_config_string(trimmed),
        config_path,
        root,
    )
}

fn add_src_dir_support(result: &mut PluginResult, src_dir: &Path) {
    let src_dir_string = config_parser::path_to_config_string(src_dir);
    result
        .path_aliases
        .push(("~/".to_string(), src_dir_string.clone()));
    result.path_aliases.push(("@/".to_string(), src_dir_string));

    if src_dir.as_os_str().is_empty() {
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

fn add_prefixed_default_used_exports(result: &mut PluginResult, prefix: &Path, patterns: &[&str]) {
    for pattern in patterns {
        add_default_used_export(result, prefix_with_src_dir(prefix, pattern));
    }
}

fn extend_prefixed_patterns(target: &mut Vec<String>, prefix: &Path, patterns: &[&str]) {
    target.extend(
        patterns
            .iter()
            .map(|pattern| prefix_with_src_dir(prefix, pattern)),
    );
}

fn component_dir_pattern(dir: &str) -> String {
    format!("{dir}/**/*.{{{COMPONENT_ENTRY_GLOB}}}")
}

/// Build the `content.config.{ts,js,mts,cts,mjs,cjs}` entry pattern for the
/// directory holding the nuxt.config (`config_path`'s parent), workspace-root
/// relative. Returns `None` when the path falls outside `root` (e.g. a relative
/// `config_path` in a unit test); production passes an absolute config path.
fn content_config_entry_pattern(config_path: &Path, root: &Path) -> Option<String> {
    let base = config_parser::normalize_config_path("content.config", config_path, root)?;
    Some(format!("{base}.{{{SCRIPT_ENTRY_GLOB}}}"))
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
    src_dir: &Path,
) -> Option<String> {
    if let Some(stripped) = raw.strip_prefix("~/").or_else(|| raw.strip_prefix("@/")) {
        return Some(prefix_with_src_dir(src_dir, stripped));
    }

    if let Some(stripped) = raw.strip_prefix("~~/").or_else(|| raw.strip_prefix("@@/")) {
        return Some(config_parser::path_to_config_string(
            &config_parser::path_from_config_string(stripped),
        ));
    }

    config_parser::normalize_config_path(raw, config_path, root)
}

fn normalize_imports_dir_pattern(
    raw: &str,
    config_path: &Path,
    root: &Path,
    src_dir: &Path,
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

fn prefix_with_src_dir(src_dir: &Path, path: &str) -> String {
    let path = config_parser::path_from_config_string(path);
    if src_dir.as_os_str().is_empty() {
        config_parser::path_to_config_string(&path)
    } else {
        let normalized = config_parser::lexical_normalize(&src_dir.join(path));
        config_parser::path_to_config_string(&normalized)
    }
}

/// Recursively walk a components directory, emitting an [`AutoImportRule`] (plus
/// its implicit `Lazy`-prefixed variant) for every component file found.
///
/// `base` is the components root (used to compute the relative path that drives
/// the name); `dir` is the directory currently being scanned.
fn collect_component_auto_imports(base: &Path, dir: &Path, rules: &mut Vec<AutoImportRule>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_component_auto_imports(base, &path, rules);
            continue;
        }
        if !has_component_extension(&path) {
            continue;
        }
        let Ok(rel) = path.strip_prefix(base) else {
            continue;
        };
        let Some(name) = derive_component_name(rel) else {
            continue;
        };
        push_component_rule(rules, name, path);
    }
}

/// Push the canonical rule and its `Lazy`-prefixed dynamic-import variant.
fn push_component_rule(rules: &mut Vec<AutoImportRule>, name: String, source: PathBuf) {
    let lazy = format!("Lazy{name}");
    rules.push(AutoImportRule {
        name,
        source: source.clone(),
        kind: AutoImportKind::DefaultComponent,
    });
    rules.push(AutoImportRule {
        name: lazy,
        source,
        kind: AutoImportKind::DefaultComponent,
    });
}

/// Whether the path has a component file extension (case-insensitive).
fn has_component_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| COMPONENT_EXTS.iter().any(|c| ext.eq_ignore_ascii_case(c)))
}

/// Derive the Nuxt component name from a path relative to the components root.
///
/// Mirrors Nuxt's directory-prefixed PascalCase convention with the
/// prefix-overlap dedup: `base/foo/Button.vue` becomes `BaseFooButton`,
/// `foo/Foo.vue` becomes `Foo` (not `FooFoo`), and `base/BaseButton.vue` becomes
/// `BaseButton` (not `BaseBaseButton`). `.client` / `.server` / `.global`
/// suffixes are stripped before deriving so paired files map to one name.
fn derive_component_name(rel: &Path) -> Option<String> {
    let stem = rel.file_stem().and_then(|s| s.to_str())?;
    let stem = strip_component_suffix(stem);
    if stem.is_empty() {
        return None;
    }
    let dir_segments: Vec<&str> = rel
        .parent()
        .map(|parent| {
            parent
                .components()
                .filter_map(|c| c.as_os_str().to_str())
                .collect()
        })
        .unwrap_or_default();
    Some(resolve_component_name(&dir_segments, stem))
}

/// Strip a single trailing `.client` / `.server` / `.global` segment.
fn strip_component_suffix(stem: &str) -> &str {
    if let Some((head, tail)) = stem.rsplit_once('.')
        && COMPONENT_NAME_SUFFIXES
            .iter()
            .any(|s| tail.eq_ignore_ascii_case(s))
    {
        return head;
    }
    stem
}

/// Combine PascalCase directory segments with the filename, removing the overlap
/// where the filename already restates the trailing directory segments. This is
/// a port of Nuxt's `resolveComponentName` dedup.
fn resolve_component_name(dir_segments: &[&str], file_stem: &str) -> String {
    let prefix_parts: Vec<String> = dir_segments
        .iter()
        .map(|seg| pascal_segment(seg))
        .filter(|p| !p.is_empty())
        .collect();
    let file_words = split_words(file_stem);
    let file_lower = file_words.join("/").to_lowercase();

    let mut kept = prefix_parts.len();
    let mut matched_suffix: Vec<String> = Vec::new();
    let mut index = prefix_parts.len();
    while index > 0 {
        index -= 1;
        let mut words: Vec<String> = split_words(&prefix_parts[index])
            .into_iter()
            .map(|w| w.to_lowercase())
            .collect();
        words.extend(matched_suffix.iter().cloned());
        matched_suffix = words;
        let matched_content = matched_suffix.join("/");
        if file_lower == matched_content || file_lower.starts_with(&format!("{matched_content}/")) {
            kept = index;
        }
    }

    let mut name = String::new();
    for part in &prefix_parts[..kept] {
        name.push_str(part);
    }
    name.push_str(&pascal_segment(file_stem));
    name
}

/// Split a string into lowercase word parts on separators (`-`, `_`, ` `, `.`)
/// and lower-to-upper case transitions. `BaseButton` -> `["base", "button"]`,
/// `my-widget` -> `["my", "widget"]`, `Card001` -> `["card001"]`.
fn split_words(input: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for ch in input.chars() {
        if ch == '-' || ch == '_' || ch == ' ' || ch == '.' {
            if !current.is_empty() {
                words.push(std::mem::take(&mut current));
            }
            prev_lower = false;
            continue;
        }
        if ch.is_ascii_uppercase() && prev_lower && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        current.push(ch.to_ascii_lowercase());
        prev_lower = ch.is_ascii_lowercase() || ch.is_ascii_digit();
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// PascalCase a path segment by capitalizing the first letter of each
/// separator-delimited part while PRESERVING the existing internal casing, so
/// acronyms survive: `UICard` -> `UICard`, `card001` -> `Card001`, `my-widget`
/// -> `MyWidget`, `base` -> `Base`. Routing through the lowercasing `split_words`
/// instead would collapse `UICard` to `Uicard` and break the table-key match
/// against the `<UICard />` tag the scanner captures verbatim. See issue #704.
fn pascal_segment(seg: &str) -> String {
    let mut out = String::new();
    for part in seg.split(['-', '_', ' ', '.']) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}

/// Whether an aggregated entry-pattern glob is one of the Nuxt CONSUMER component
/// directories (`components/**`, `app/components/**`, including workspace-prefixed
/// forms). These are the patterns `auto_imports` resolves, so the `autoImports`
/// flag drops them. Module-authoring `src/runtime/components` patterns are
/// intentionally NOT matched: `auto_imports` does not scan them, so they keep
/// their entry-pattern protection. See issue #704.
pub fn is_component_entry_pattern(pattern: &str) -> bool {
    const SUFFIX: &str = "components/**/*.{vue,ts,tsx,js,jsx}";
    let Some(prefix) = pattern.strip_suffix(SUFFIX) else {
        return false;
    };
    let valid_prefix = prefix.is_empty() || prefix.ends_with('/');
    valid_prefix && !prefix.contains("runtime/")
}

/// Conservative guard for the `autoImports` flag: whether the root `nuxt.config`
/// declares a `components:` key. When it does, custom `prefix` / `pathPrefix` /
/// `dirs` settings (which `auto_imports` does not model) may be in play, so the
/// component entry patterns are kept rather than dropped, avoiding false
/// `unused-file` reports. Conservative on purpose: any `components:` property key
/// keeps the patterns. See issue #704.
pub fn config_declares_components(root: &Path) -> bool {
    for name in ["nuxt.config.ts", "nuxt.config.js"] {
        let path = root.join(name);
        let Ok(source) = std::fs::read_to_string(&path) else {
            continue;
        };
        if source_has_components_key(&source) {
            return true;
        }
    }
    false
}

/// Whether the source declares a `components` property key in any position.
///
/// Tolerant on purpose: matches `components:`, `"components":`, `'components':`,
/// and inline shapes like `defineNuxtConfig({ components: [...] })` regardless of
/// line position or quoting. It can also match `components:` inside a comment or
/// string literal, but that only keeps the entry patterns (the safe direction:
/// no false `unused-file` reports), so over-matching is acceptable. See issue #704.
fn source_has_components_key(source: &str) -> bool {
    COMPONENTS_KEY_RE.is_match(source)
}

#[expect(
    clippy::expect_used,
    reason = "static Nuxt regex pattern is hard-coded and covered by plugin tests"
)]
static COMPONENTS_KEY_RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r#"["']?\bcomponents\b["']?\s*:"#).expect("valid regex")
});

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
    fn resolve_config_credits_content_config_when_module_registered() {
        let source = r#"
            export default defineNuxtConfig({
                modules: ["@nuxt/content"]
            });
        "#;
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/nuxt.config.ts"),
            source,
            Path::new("/project"),
        );

        let pattern = "content.config.{ts,js,mts,cts,mjs,cjs}";
        assert!(
            has_entry_pattern(&result, pattern),
            "@nuxt/content should credit content.config as an entry: {:?}",
            result.entry_patterns
        );
        assert!(
            has_used_export_rule(&result, pattern, &["default"]),
            "content.config should keep its default export alive"
        );
    }

    #[test]
    fn resolve_config_no_content_config_credit_without_module() {
        let source = r#"
            export default defineNuxtConfig({
                modules: ["@nuxtjs/tailwindcss"]
            });
        "#;
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/nuxt.config.ts"),
            source,
            Path::new("/project"),
        );

        assert!(
            !has_entry_pattern(&result, "content.config.{ts,js,mts,cts,mjs,cjs}"),
            "content.config must not be credited without @nuxt/content in modules: {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn resolve_config_content_config_resolves_relative_to_nested_config_dir() {
        let source = r#"
            export default defineNuxtConfig({
                modules: ["@nuxt/content"]
            });
        "#;
        let plugin = NuxtPlugin;
        let result = plugin.resolve_config(
            Path::new("/repo/docs/nuxt.config.ts"),
            source,
            Path::new("/repo"),
        );

        let pattern = "docs/content.config.{ts,js,mts,cts,mjs,cjs}";
        assert!(
            has_entry_pattern(&result, pattern),
            "nested nuxt.config should credit docs/content.config: {:?}",
            result.entry_patterns
        );
        assert!(
            has_used_export_rule(&result, pattern, &["default"]),
            "nested content.config should keep its default export alive"
        );
    }

    #[test]
    fn resolve_config_css_tilde_resolves_to_root() {
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
    fn resolve_config_src_dir_leading_slash_is_project_relative() {
        let source = r#"
            export default defineNuxtConfig({
                srcDir: "/src",
                imports: {
                    dirs: ["~/custom/composables"]
                }
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
            "leading-slash srcDir should remap ~/ under the project root"
        );
        assert!(has_entry_pattern(
            &result,
            "src/custom/composables/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}"
        ));
    }

    #[test]
    fn resolve_config_src_dir_normalizes_backslash_config_values() {
        let source = r#"
            export default defineNuxtConfig({
                srcDir: path.resolve(__dirname, "src\\\\app/"),
                imports: {
                    dirs: ["~/custom\\\\composables"]
                },
                components: [
                    { path: "@/feature\\\\components" }
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
                .contains(&("~/".to_string(), "src/app".to_string())),
            "srcDir should be normalized in aliases"
        );
        assert!(has_entry_pattern(
            &result,
            "src/app/custom/composables/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}"
        ));
        assert!(has_entry_pattern(
            &result,
            "src/app/feature/components/**/*.{vue,ts,tsx,js,jsx}"
        ));
        assert!(
            result
                .entry_patterns
                .iter()
                .all(|entry| !entry.contains('\\')),
            "entry patterns should use forward slashes: {:?}",
            result.entry_patterns
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

        assert!(
            !result.entry_patterns.iter().any(|p| p.contains("runtime")),
            "nuxt.config.ts should not add runtime patterns: {:?}",
            result.entry_patterns
        );
    }

    fn name_of(rel: &str) -> String {
        derive_component_name(Path::new(rel)).expect("component name")
    }

    #[test]
    fn component_name_flat() {
        assert_eq!(name_of("Card001.vue"), "Card001");
    }

    #[test]
    fn component_name_directory_prefix_concat() {
        assert_eq!(name_of("base/foo/Button.vue"), "BaseFooButton");
    }

    #[test]
    fn component_name_dedups_repeated_segment() {
        assert_eq!(name_of("foo/Foo.vue"), "Foo");
    }

    #[test]
    fn component_name_dedups_filename_prefix_overlap() {
        assert_eq!(name_of("base/BaseButton.vue"), "BaseButton");
    }

    #[test]
    fn component_name_kebab_directory_segments() {
        assert_eq!(name_of("my-widget/Header.vue"), "MyWidgetHeader");
    }

    #[test]
    fn component_name_preserves_consecutive_uppercase_acronyms() {
        assert_eq!(name_of("UICard.vue"), "UICard");
        assert_eq!(name_of("APIClient.vue"), "APIClient");
        assert_eq!(name_of("base/HTTPForm.vue"), "BaseHTTPForm");
    }

    #[test]
    fn component_name_strips_client_server_global_suffixes() {
        assert_eq!(name_of("Comments.client.vue"), "Comments");
        assert_eq!(name_of("Comments.server.vue"), "Comments");
        assert_eq!(name_of("Banner.global.vue"), "Banner");
    }

    #[test]
    fn auto_imports_emits_canonical_and_lazy_variants() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("components/base")).unwrap();
        std::fs::write(root.join("components/Card001.vue"), "<template></template>").unwrap();
        std::fs::write(
            root.join("components/base/Button.vue"),
            "<template></template>",
        )
        .unwrap();

        let rules = NuxtPlugin.auto_imports(root);
        let names: std::collections::BTreeSet<&str> =
            rules.iter().map(|r| r.name.as_str()).collect();

        assert!(names.contains("Card001"));
        assert!(names.contains("LazyCard001"));
        assert!(names.contains("BaseButton"));
        assert!(names.contains("LazyBaseButton"));
        assert!(
            rules
                .iter()
                .all(|r| matches!(r.kind, AutoImportKind::DefaultComponent)),
            "component rules are DefaultComponent kind"
        );
        let card = rules.iter().find(|r| r.name == "Card001").unwrap();
        assert_eq!(card.source, root.join("components/Card001.vue"));
    }

    #[test]
    fn auto_imports_paired_client_server_share_a_name_with_distinct_sources() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("app/components")).unwrap();
        std::fs::write(
            root.join("app/components/Comments.client.vue"),
            "<template></template>",
        )
        .unwrap();
        std::fs::write(
            root.join("app/components/Comments.server.vue"),
            "<template></template>",
        )
        .unwrap();

        let rules = NuxtPlugin.auto_imports(root);
        let comments_sources: std::collections::BTreeSet<_> = rules
            .iter()
            .filter(|r| r.name == "Comments")
            .map(|r| r.source.clone())
            .collect();
        assert_eq!(comments_sources.len(), 2);
    }

    #[test]
    fn auto_imports_empty_without_component_dirs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(NuxtPlugin.auto_imports(tmp.path()).is_empty());
    }

    #[test]
    fn component_entry_pattern_matches_consumer_dirs_only() {
        assert!(is_component_entry_pattern(
            "components/**/*.{vue,ts,tsx,js,jsx}"
        ));
        assert!(is_component_entry_pattern(
            "app/components/**/*.{vue,ts,tsx,js,jsx}"
        ));
        assert!(is_component_entry_pattern(
            "packages/web/components/**/*.{vue,ts,tsx,js,jsx}"
        ));
        assert!(!is_component_entry_pattern(
            "src/runtime/components/**/*.{vue,ts,tsx,js,jsx}"
        ));
        assert!(!is_component_entry_pattern(
            "pages/**/*.{vue,ts,tsx,js,jsx}"
        ));
    }

    #[test]
    fn config_declares_components_detects_key() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::write(
            root.join("nuxt.config.ts"),
            "export default defineNuxtConfig({\n  components: [{ path: '~/ui', prefix: 'U' }],\n})\n",
        )
        .unwrap();
        assert!(config_declares_components(root));
    }

    #[test]
    fn config_declares_components_detects_inline_and_quoted_keys() {
        assert!(source_has_components_key(
            "export default defineNuxtConfig({ components: [{ path: '~/ui' }] })"
        ));
        assert!(source_has_components_key(
            r#"export default { "components": [{ path: "~/ui" }] }"#
        ));
        assert!(source_has_components_key("  components : [\n  ]"));
        assert!(!source_has_components_key(
            "export default defineNuxtConfig({ modules: ['@nuxt/image'] })"
        ));
    }

    #[test]
    fn config_declares_components_false_without_key() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::write(
            root.join("nuxt.config.ts"),
            "export default defineNuxtConfig({\n  modules: ['@nuxt/image'],\n})\n",
        )
        .unwrap();
        assert!(!config_declares_components(root));
    }
}
