//! Docusaurus documentation framework plugin.
//!
//! Covers content roots, swizzled theme components, localized/versioned docs,
//! and client assets discovered from `docusaurus.config.*`.

use std::path::{Path, PathBuf};

use oxc_allocator::Allocator;
use oxc_ast::ast::{Expression, ObjectExpression};
use oxc_parser::Parser;
use oxc_span::SourceType;
use rustc_hash::FxHashSet;

use super::{Plugin, PluginResult, UsedExportRule, config_parser};

const ENABLERS: &[&str] = &["@docusaurus/core"];

const ENTRY_PATTERNS: &[&str] = &[
    "docs/**/*.{md,mdx}",
    "blog/**/*.{md,mdx}",
    "src/pages/**/*.{ts,tsx,js,jsx,md,mdx}",
    "sidebars.{js,ts}",
    "src/theme/*.{ts,tsx,js,jsx}",
    "src/theme/**/index.{ts,tsx,js,jsx}",
    "versioned_docs/**/*.{md,mdx}",
    "i18n/*/docusaurus-plugin-content-docs/**/*.{md,mdx}",
    "i18n/*/docusaurus-plugin-content-blog/**/*.{md,mdx}",
    "i18n/*/docusaurus-plugin-content-pages/**/*.{md,mdx}",
];

const CONFIG_PATTERNS: &[&str] = &["docusaurus.config.{js,ts,mjs}"];

const ALWAYS_USED: &[&str] = &["docusaurus.config.{js,ts,mjs}"];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "@docusaurus/core",
    "@docusaurus/preset-classic",
    "@docusaurus/plugin-content-docs",
    "@docusaurus/plugin-content-blog",
    "@docusaurus/plugin-content-pages",
    "@docusaurus/theme-classic",
];

/// Virtual module prefixes provided by Docusaurus at build time.
/// These are resolved by the Docusaurus bundler and should not be
/// flagged as unlisted dependencies.
const VIRTUAL_MODULE_PREFIXES: &[&str] = &[
    "@theme/",
    "@theme-original/",
    "@docusaurus/",
    "@site/",
    "@generated/",
];

const DEFAULT_EXPORTS: &[&str] = &["default"];
const DEFAULT_DOCS_PATH: &str = "docs";
const DEFAULT_BLOG_PATH: &str = "blog";
const DEFAULT_PAGES_PATH: &str = "src/pages";
const DEFAULT_I18N_PATH: &str = "i18n";
const DEFAULT_STATIC_DIRECTORIES: &[&str] = &["static"];

pub struct DocusaurusPlugin;

impl Plugin for DocusaurusPlugin {
    fn name(&self) -> &'static str {
        "docusaurus"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
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

    fn used_exports(&self) -> Vec<(&'static str, &'static [&'static str])> {
        vec![
            ("src/pages/**/*.{ts,tsx,js,jsx}", DEFAULT_EXPORTS),
            ("sidebars.{js,ts}", DEFAULT_EXPORTS),
            ("src/theme/*.{ts,tsx,js,jsx}", DEFAULT_EXPORTS),
            ("src/theme/**/index.{ts,tsx,js,jsx}", DEFAULT_EXPORTS),
        ]
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let mut referenced_dependencies = FxHashSet::default();

        for import in config_parser::extract_imports(source, config_path) {
            referenced_dependencies.insert(crate::resolve::extract_package_name(&import));
        }

        let Some(parsed) = parse_docusaurus_config(source, config_path, root) else {
            result.referenced_dependencies = sort_strings(referenced_dependencies);
            return result;
        };

        referenced_dependencies.extend(parsed.referenced_dependencies);

        result.replace_entry_patterns = true;
        result.replace_used_export_rules = true;

        let mut entry_patterns = FxHashSet::default();
        let mut used_export_rules = FxHashSet::default();
        let mut always_used_files = parsed.always_used_files;

        add_theme_swizzle_rules(&mut entry_patterns, &mut used_export_rules);

        for docs in parsed.docs_instances {
            add_docs_rules(&docs, &parsed.i18n_path, &mut entry_patterns);
            if let Some(sidebar_path) = docs.sidebar_path {
                used_export_rules.insert(sidebar_path.clone());
                always_used_files.insert(sidebar_path);
            }
        }

        for blog in parsed.blog_instances {
            add_blog_rules(&blog, &parsed.i18n_path, &mut entry_patterns);
        }

        for pages in parsed.pages_instances {
            add_pages_rules(
                &pages,
                &parsed.i18n_path,
                &mut entry_patterns,
                &mut used_export_rules,
            );
        }

        result.entry_patterns = sort_strings(entry_patterns)
            .into_iter()
            .map(super::PathRule::new)
            .collect();
        result.used_exports = sort_strings(used_export_rules)
            .into_iter()
            .map(|pattern| UsedExportRule::new(pattern, DEFAULT_EXPORTS.iter().copied()))
            .collect();
        result.always_used_files = sort_strings(always_used_files);
        result.referenced_dependencies = sort_strings(referenced_dependencies);
        result
    }
}

#[derive(Debug)]
struct ParsedDocusaurusConfig {
    docs_instances: Vec<DocsInstance>,
    blog_instances: Vec<BlogInstance>,
    pages_instances: Vec<PagesInstance>,
    i18n_path: String,
    static_directories: Vec<String>,
    always_used_files: FxHashSet<String>,
    referenced_dependencies: FxHashSet<String>,
}

impl Default for ParsedDocusaurusConfig {
    fn default() -> Self {
        Self {
            docs_instances: Vec::new(),
            blog_instances: Vec::new(),
            pages_instances: Vec::new(),
            i18n_path: DEFAULT_I18N_PATH.to_string(),
            static_directories: DEFAULT_STATIC_DIRECTORIES
                .iter()
                .map(|dir| (*dir).to_string())
                .collect(),
            always_used_files: FxHashSet::default(),
            referenced_dependencies: FxHashSet::default(),
        }
    }
}

#[derive(Debug, Clone)]
struct DocsInstance {
    path: String,
    id: Option<String>,
    sidebar_path: Option<String>,
}

impl Default for DocsInstance {
    fn default() -> Self {
        Self {
            path: DEFAULT_DOCS_PATH.to_string(),
            id: None,
            sidebar_path: None,
        }
    }
}

#[derive(Debug, Clone)]
struct BlogInstance {
    path: String,
    id: Option<String>,
}

impl Default for BlogInstance {
    fn default() -> Self {
        Self {
            path: DEFAULT_BLOG_PATH.to_string(),
            id: None,
        }
    }
}

#[derive(Debug, Clone)]
struct PagesInstance {
    path: String,
    id: Option<String>,
}

impl Default for PagesInstance {
    fn default() -> Self {
        Self {
            path: DEFAULT_PAGES_PATH.to_string(),
            id: None,
        }
    }
}

fn parse_docusaurus_config(
    source: &str,
    config_path: &Path,
    root: &Path,
) -> Option<ParsedDocusaurusConfig> {
    let source_type = SourceType::from_path(config_path).unwrap_or_default();
    let alloc = Allocator::default();
    let parsed = Parser::new(&alloc, source, source_type).parse();
    let config = config_parser::find_config_object_pub(&parsed.program)?;

    let mut result = ParsedDocusaurusConfig {
        i18n_path: parse_i18n_path(config, config_path, root),
        static_directories: parse_static_directories(config, config_path, root),
        ..ParsedDocusaurusConfig::default()
    };
    let static_directories = result.static_directories.clone();

    collect_top_level_assets(config, config_path, root, &static_directories, &mut result);

    if let Some(plugins_expr) = config_parser::property_expr(config, "plugins") {
        parse_plugin_entries(plugins_expr, config_path, root, &mut result);
    }
    if let Some(themes_expr) = config_parser::property_expr(config, "themes") {
        parse_theme_entries(themes_expr, config_path, root, &mut result);
    }
    if let Some(presets_expr) = config_parser::property_expr(config, "presets") {
        parse_preset_entries(presets_expr, config_path, root, &mut result);
    }

    Some(result)
}

fn collect_top_level_assets(
    config: &ObjectExpression<'_>,
    config_path: &Path,
    root: &Path,
    static_directories: &[String],
    parsed: &mut ParsedDocusaurusConfig,
) {
    for raw_path in property_array_object_paths(config, "scripts", "src") {
        parsed.always_used_files.extend(resolve_site_asset_paths(
            &raw_path,
            config_path,
            root,
            static_directories,
        ));
    }

    for raw_path in property_array_object_paths(config, "stylesheets", "href") {
        parsed.always_used_files.extend(resolve_site_asset_paths(
            &raw_path,
            config_path,
            root,
            static_directories,
        ));
    }

    for raw_path in property_path_values(config, "clientModules") {
        if let Some(path) = normalize_project_path(&raw_path, config_path, root) {
            parsed.always_used_files.insert(path);
        }
    }
}

fn parse_plugin_entries(
    expr: &Expression<'_>,
    config_path: &Path,
    root: &Path,
    parsed: &mut ParsedDocusaurusConfig,
) {
    let Some(array) = config_parser::array_expression(expr) else {
        return;
    };

    for element in &array.elements {
        let Some(entry_expr) = element.as_expression() else {
            continue;
        };
        let Some((module_name, options)) = tuple_name_and_options(entry_expr) else {
            continue;
        };

        track_module_reference("plugins", &module_name, config_path, root, parsed);

        match normalize_plugin_name(&module_name) {
            Some(DocusaurusPluginKind::Docs) => {
                parsed
                    .docs_instances
                    .push(parse_docs_instance(options, config_path, root));
            }
            Some(DocusaurusPluginKind::Blog) => {
                parsed
                    .blog_instances
                    .push(parse_blog_instance(options, config_path, root));
            }
            Some(DocusaurusPluginKind::Pages) => {
                parsed
                    .pages_instances
                    .push(parse_pages_instance(options, config_path, root));
            }
            None => {}
        }
    }
}

fn parse_theme_entries(
    expr: &Expression<'_>,
    config_path: &Path,
    root: &Path,
    parsed: &mut ParsedDocusaurusConfig,
) {
    let Some(array) = config_parser::array_expression(expr) else {
        return;
    };

    for element in &array.elements {
        let Some(entry_expr) = element.as_expression() else {
            continue;
        };
        let Some((module_name, options)) = tuple_name_and_options(entry_expr) else {
            continue;
        };

        track_module_reference("themes", &module_name, config_path, root, parsed);

        if normalize_theme_name(&module_name) == Some(DocusaurusThemeKind::Classic)
            && let Some(options) = options
        {
            parsed
                .always_used_files
                .extend(parse_theme_custom_css(options, config_path, root));
        }
    }
}

fn parse_preset_entries(
    expr: &Expression<'_>,
    config_path: &Path,
    root: &Path,
    parsed: &mut ParsedDocusaurusConfig,
) {
    let Some(array) = config_parser::array_expression(expr) else {
        return;
    };

    for element in &array.elements {
        let Some(entry_expr) = element.as_expression() else {
            continue;
        };
        let Some((module_name, options)) = tuple_name_and_options(entry_expr) else {
            continue;
        };

        track_module_reference("presets", &module_name, config_path, root, parsed);

        if normalize_preset_name(&module_name) == Some(DocusaurusPresetKind::Classic) {
            parse_classic_preset(options, config_path, root, parsed);
        }
    }
}

fn track_module_reference(
    kind: &str,
    module_name: &str,
    config_path: &Path,
    root: &Path,
    parsed: &mut ParsedDocusaurusConfig,
) {
    if let Some(dep) = normalize_module_dependency(kind, module_name) {
        parsed.referenced_dependencies.insert(dep);
    } else if let Some(local_path) = normalize_local_module_path(module_name, config_path, root) {
        parsed.always_used_files.insert(local_path);
    }
}

fn parse_classic_preset(
    options: Option<&ObjectExpression<'_>>,
    config_path: &Path,
    root: &Path,
    parsed: &mut ParsedDocusaurusConfig,
) {
    parsed
        .docs_instances
        .extend(parse_optional_docs_preset_section(
            options,
            config_path,
            root,
        ));
    parsed
        .blog_instances
        .extend(parse_optional_blog_preset_section(
            options,
            config_path,
            root,
        ));
    parsed
        .pages_instances
        .extend(parse_optional_pages_preset_section(
            options,
            config_path,
            root,
        ));

    if let Some(theme_options) =
        options.and_then(|obj| config_parser::property_object(obj, "theme"))
    {
        parsed
            .always_used_files
            .extend(parse_theme_custom_css(theme_options, config_path, root));
    }
}

fn parse_optional_docs_preset_section(
    options: Option<&ObjectExpression<'_>>,
    config_path: &Path,
    root: &Path,
) -> Vec<DocsInstance> {
    match options.and_then(|obj| config_parser::property_expr(obj, "docs")) {
        Some(expr) if config_parser::is_disabled_expression(expr) => Vec::new(),
        Some(expr) => vec![parse_docs_instance(
            config_parser::object_expression(expr),
            config_path,
            root,
        )],
        None => vec![DocsInstance::default()],
    }
}

fn parse_optional_blog_preset_section(
    options: Option<&ObjectExpression<'_>>,
    config_path: &Path,
    root: &Path,
) -> Vec<BlogInstance> {
    match options.and_then(|obj| config_parser::property_expr(obj, "blog")) {
        Some(expr) if config_parser::is_disabled_expression(expr) => Vec::new(),
        Some(expr) => vec![parse_blog_instance(
            config_parser::object_expression(expr),
            config_path,
            root,
        )],
        None => vec![BlogInstance::default()],
    }
}

fn parse_optional_pages_preset_section(
    options: Option<&ObjectExpression<'_>>,
    config_path: &Path,
    root: &Path,
) -> Vec<PagesInstance> {
    match options.and_then(|obj| config_parser::property_expr(obj, "pages")) {
        Some(expr) if config_parser::is_disabled_expression(expr) => Vec::new(),
        Some(expr) => vec![parse_pages_instance(
            config_parser::object_expression(expr),
            config_path,
            root,
        )],
        None => vec![PagesInstance::default()],
    }
}

fn parse_docs_instance(
    options: Option<&ObjectExpression<'_>>,
    config_path: &Path,
    root: &Path,
) -> DocsInstance {
    let mut instance = DocsInstance::default();

    if let Some(options) = options {
        if let Some(path) = property_project_path(options, "path", config_path, root) {
            instance.path = path;
        }
        instance.id = normalize_instance_id(config_parser::property_string(options, "id"));
        instance.sidebar_path = property_project_path(options, "sidebarPath", config_path, root);
    }

    instance
}

fn parse_blog_instance(
    options: Option<&ObjectExpression<'_>>,
    config_path: &Path,
    root: &Path,
) -> BlogInstance {
    let mut instance = BlogInstance::default();

    if let Some(options) = options {
        if let Some(path) = property_project_path(options, "path", config_path, root) {
            instance.path = path;
        }
        instance.id = normalize_instance_id(config_parser::property_string(options, "id"));
    }

    instance
}

fn parse_pages_instance(
    options: Option<&ObjectExpression<'_>>,
    config_path: &Path,
    root: &Path,
) -> PagesInstance {
    let mut instance = PagesInstance::default();

    if let Some(options) = options {
        if let Some(path) = property_project_path(options, "path", config_path, root) {
            instance.path = path;
        }
        instance.id = normalize_instance_id(config_parser::property_string(options, "id"));
    }

    instance
}

fn parse_theme_custom_css(
    options: &ObjectExpression<'_>,
    config_path: &Path,
    root: &Path,
) -> Vec<String> {
    property_path_values(options, "customCss")
        .into_iter()
        .filter_map(|raw| normalize_project_path(&raw, config_path, root))
        .collect()
}

fn parse_i18n_path(config: &ObjectExpression<'_>, config_path: &Path, root: &Path) -> String {
    config_parser::property_object(config, "i18n")
        .and_then(|i18n| property_project_path(i18n, "path", config_path, root))
        .unwrap_or_else(|| DEFAULT_I18N_PATH.to_string())
}

fn parse_static_directories(
    config: &ObjectExpression<'_>,
    config_path: &Path,
    root: &Path,
) -> Vec<String> {
    let directories = config_parser::property_expr(config, "staticDirectories")
        .map(config_parser::expression_to_path_values)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|raw| normalize_project_path(&raw, config_path, root))
        .collect::<Vec<_>>();

    if directories.is_empty() {
        DEFAULT_STATIC_DIRECTORIES
            .iter()
            .map(|dir| (*dir).to_string())
            .collect()
    } else {
        sort_strings(directories)
    }
}

fn add_theme_swizzle_rules(
    entry_patterns: &mut FxHashSet<String>,
    used_exports: &mut FxHashSet<String>,
) {
    entry_patterns.insert("src/theme/*.{ts,tsx,js,jsx}".to_string());
    entry_patterns.insert("src/theme/**/index.{ts,tsx,js,jsx}".to_string());
    used_exports.insert("src/theme/*.{ts,tsx,js,jsx}".to_string());
    used_exports.insert("src/theme/**/index.{ts,tsx,js,jsx}".to_string());
}

fn add_docs_rules(
    instance: &DocsInstance,
    i18n_path: &str,
    entry_patterns: &mut FxHashSet<String>,
) {
    entry_patterns.insert(format!("{}/**/*.{{md,mdx}}", instance.path));
    entry_patterns.insert(format!(
        "{i18n_path}/*/{}/**/*.{{md,mdx}}",
        docs_i18n_dir(instance.id.as_deref())
    ));
    entry_patterns.insert(format!(
        "{}/**/*.{{md,mdx}}",
        docs_versioned_dir(instance.id.as_deref())
    ));
}

fn add_blog_rules(
    instance: &BlogInstance,
    i18n_path: &str,
    entry_patterns: &mut FxHashSet<String>,
) {
    entry_patterns.insert(format!("{}/**/*.{{md,mdx}}", instance.path));
    entry_patterns.insert(format!(
        "{i18n_path}/*/{}/**/*.{{md,mdx}}",
        plugin_i18n_dir("docusaurus-plugin-content-blog", instance.id.as_deref())
    ));
}

fn add_pages_rules(
    instance: &PagesInstance,
    i18n_path: &str,
    entry_patterns: &mut FxHashSet<String>,
    used_exports: &mut FxHashSet<String>,
) {
    entry_patterns.insert(format!("{}/**/*.{{ts,tsx,js,jsx,md,mdx}}", instance.path));
    entry_patterns.insert(format!(
        "{i18n_path}/*/{}/**/*.{{md,mdx}}",
        plugin_i18n_dir("docusaurus-plugin-content-pages", instance.id.as_deref())
    ));
    used_exports.insert(format!("{}/**/*.{{ts,tsx,js,jsx}}", instance.path));
}

fn docs_i18n_dir(id: Option<&str>) -> String {
    plugin_i18n_dir("docusaurus-plugin-content-docs", id)
}

fn docs_versioned_dir(id: Option<&str>) -> String {
    match normalize_instance_id_ref(id) {
        Some(id) => format!("{id}_versioned_docs"),
        None => "versioned_docs".to_string(),
    }
}

fn plugin_i18n_dir(base: &str, id: Option<&str>) -> String {
    match normalize_instance_id_ref(id) {
        Some(id) => format!("{base}-{id}"),
        None => base.to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocusaurusPluginKind {
    Docs,
    Blog,
    Pages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocusaurusThemeKind {
    Classic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DocusaurusPresetKind {
    Classic,
}

fn normalize_plugin_name(module_name: &str) -> Option<DocusaurusPluginKind> {
    match module_name {
        "@docusaurus/plugin-content-docs" | "content-docs" | "docusaurus-plugin-content-docs" => {
            Some(DocusaurusPluginKind::Docs)
        }
        "@docusaurus/plugin-content-blog" | "content-blog" | "docusaurus-plugin-content-blog" => {
            Some(DocusaurusPluginKind::Blog)
        }
        "@docusaurus/plugin-content-pages"
        | "content-pages"
        | "docusaurus-plugin-content-pages" => Some(DocusaurusPluginKind::Pages),
        _ => None,
    }
}

fn normalize_theme_name(module_name: &str) -> Option<DocusaurusThemeKind> {
    match module_name {
        "@docusaurus/theme-classic" | "classic" | "docusaurus-theme-classic" => {
            Some(DocusaurusThemeKind::Classic)
        }
        _ => None,
    }
}

fn normalize_preset_name(module_name: &str) -> Option<DocusaurusPresetKind> {
    match module_name {
        "@docusaurus/preset-classic" | "classic" | "docusaurus-preset-classic" => {
            Some(DocusaurusPresetKind::Classic)
        }
        _ => None,
    }
}

fn normalize_module_dependency(kind: &str, module_name: &str) -> Option<String> {
    if module_name.starts_with('.') || module_name.starts_with('/') {
        return None;
    }

    let normalized = match (kind, module_name) {
        ("plugins", "content-docs") => "@docusaurus/plugin-content-docs",
        ("plugins", "content-blog") => "@docusaurus/plugin-content-blog",
        ("plugins", "content-pages") => "@docusaurus/plugin-content-pages",
        ("themes", "classic") => "@docusaurus/theme-classic",
        ("presets", "classic") => "@docusaurus/preset-classic",
        _ => module_name,
    };

    Some(crate::resolve::extract_package_name(normalized))
}

fn normalize_local_module_path(raw: &str, config_path: &Path, root: &Path) -> Option<String> {
    (raw.starts_with('.') || raw.starts_with('/'))
        .then(|| config_parser::normalize_config_path(raw, config_path, root))
        .flatten()
}

fn normalize_project_path(
    raw: impl AsRef<Path>,
    config_path: &Path,
    root: &Path,
) -> Option<String> {
    config_parser::normalize_config_path(raw, config_path, root)
}

fn resolve_site_asset_paths(
    raw: &str,
    config_path: &Path,
    root: &Path,
    static_directories: &[String],
) -> Vec<String> {
    if is_external_resource(raw) {
        return Vec::new();
    }

    if let Some(site_relative) = raw.strip_prefix('/') {
        return static_directories
            .iter()
            .filter_map(|directory| {
                let candidate = join_project_relative_path(directory, site_relative);
                let normalized = config_parser::normalize_config_path(
                    format!("/{candidate}"),
                    config_path,
                    root,
                )?;
                root.join(&normalized).is_file().then_some(normalized)
            })
            .collect();
    }

    normalize_project_path(raw, config_path, root)
        .into_iter()
        .collect()
}

fn tuple_name_and_options<'a>(
    expr: &'a Expression<'a>,
) -> Option<(String, Option<&'a ObjectExpression<'a>>)> {
    match expr {
        Expression::ArrayExpression(tuple) => {
            let name = tuple
                .elements
                .first()
                .and_then(|item| item.as_expression())
                .and_then(config_parser::expression_to_path_string)?;
            let options = tuple
                .elements
                .get(1)
                .and_then(|item| item.as_expression())
                .and_then(config_parser::object_expression);
            Some((name, options))
        }
        _ => config_parser::expression_to_path_string(expr).map(|name| (name, None)),
    }
}

fn property_project_path(
    obj: &ObjectExpression<'_>,
    key: &str,
    config_path: &Path,
    root: &Path,
) -> Option<String> {
    config_parser::property_expr(obj, key)
        .and_then(config_parser::expression_to_path)
        .and_then(|raw| normalize_project_path(raw, config_path, root))
}

fn property_path_values(obj: &ObjectExpression<'_>, key: &str) -> Vec<PathBuf> {
    config_parser::property_expr(obj, key)
        .map(config_parser::expression_to_path_values)
        .unwrap_or_default()
}

fn property_array_object_paths(obj: &ObjectExpression<'_>, key: &str, field: &str) -> Vec<String> {
    let Some(expr) = config_parser::property_expr(obj, key) else {
        return Vec::new();
    };
    let Some(array) = config_parser::array_expression(expr) else {
        return Vec::new();
    };

    let mut paths = Vec::new();
    for element in &array.elements {
        let Some(item) = element.as_expression() else {
            continue;
        };
        match item {
            Expression::ObjectExpression(object) => {
                if let Some(raw) = config_parser::property_expr(object, field)
                    .and_then(config_parser::expression_to_path_string)
                {
                    paths.push(raw);
                }
            }
            _ => {
                if let Some(raw) = config_parser::expression_to_path_string(item) {
                    paths.push(raw);
                }
            }
        }
    }

    paths
}

fn normalize_instance_id(id: Option<String>) -> Option<String> {
    id.and_then(|id| normalize_instance_id_ref(Some(id.as_str())).map(str::to_string))
}

fn normalize_instance_id_ref(id: Option<&str>) -> Option<&str> {
    match id {
        Some(id) if !id.is_empty() && id != "default" => Some(id),
        _ => None,
    }
}

fn join_project_relative_path(base: &str, child: &str) -> String {
    PathBuf::from(base)
        .join(child.trim_start_matches('/'))
        .to_string_lossy()
        .replace('\\', "/")
}

fn is_external_resource(raw: &str) -> bool {
    raw.contains("://") || raw.starts_with("//") || raw.starts_with("data:")
}

fn sort_strings(values: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut values: Vec<String> = values.into_iter().collect();
    values.sort();
    values.dedup();
    values
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    fn has_entry_pattern(result: &PluginResult, pattern: &str) -> bool {
        result
            .entry_patterns
            .iter()
            .any(|entry_pattern| entry_pattern.pattern == pattern)
    }

    fn has_used_export_rule(result: &PluginResult, pattern: &str) -> bool {
        result
            .used_exports
            .iter()
            .any(|rule| rule.path.pattern == pattern && rule.exports == vec!["default".to_string()])
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "test fixture; linear setup/assert, length is not a maintainability concern"
    )]
    fn resolve_config_supports_async_config_creators_and_custom_roots() {
        let temp = tempdir().expect("temp dir");
        fs::create_dir_all(temp.path().join("static-assets/styles")).expect("styles dir");
        fs::create_dir_all(temp.path().join("public/scripts")).expect("scripts dir");
        fs::write(temp.path().join("static-assets/styles/local.css"), "").expect("style file");
        fs::write(temp.path().join("public/scripts/runtime.js"), "").expect("script file");

        let source = r#"
            export default async function createConfigAsync() {
                return {
                    staticDirectories: ["static-assets", "public"],
                    i18n: {
                        path: "translations",
                    },
                    scripts: [
                        "./src/client/custom-script.js",
                        "/scripts/runtime.js",
                        "https://cdn.example.com/script.js",
                    ],
                    stylesheets: [
                        { href: "./src/css/extra.css" },
                        "/styles/local.css",
                    ],
                    clientModules: ["./src/client/global-client.js"],
                    presets: [
                        [
                            "classic",
                            {
                                docs: {
                                    path: "knowledge",
                                    sidebarPath: "./knowledge-sidebars.ts",
                                },
                                blog: {
                                    path: "updates",
                                },
                                pages: {
                                    path: "site-pages",
                                },
                                theme: {
                                    customCss: ["./src/css/custom.css"],
                                },
                            },
                        ],
                    ],
                    plugins: [
                        [
                            "docusaurus-plugin-content-docs",
                            {
                                id: "community",
                                path: "community",
                                sidebarPath: "./community-sidebars.ts",
                            },
                        ],
                    ],
                    themes: ["docusaurus-theme-classic"],
                };
            }
        "#;

        let plugin = DocusaurusPlugin;
        let result = plugin.resolve_config(
            &temp.path().join("docusaurus.config.ts"),
            source,
            temp.path(),
        );

        assert!(result.replace_entry_patterns);
        assert!(result.replace_used_export_rules);
        assert!(has_entry_pattern(&result, "knowledge/**/*.{md,mdx}"));
        assert!(has_entry_pattern(&result, "community/**/*.{md,mdx}"));
        assert!(has_entry_pattern(
            &result,
            "site-pages/**/*.{ts,tsx,js,jsx,md,mdx}"
        ));
        assert!(has_entry_pattern(
            &result,
            "community_versioned_docs/**/*.{md,mdx}"
        ));
        assert!(has_entry_pattern(
            &result,
            "translations/*/docusaurus-plugin-content-docs-community/**/*.{md,mdx}"
        ));
        assert!(has_used_export_rule(
            &result,
            "site-pages/**/*.{ts,tsx,js,jsx}"
        ));
        assert!(
            result
                .always_used_files
                .contains(&"knowledge-sidebars.ts".to_string())
        );
        assert!(
            result
                .always_used_files
                .contains(&"community-sidebars.ts".to_string())
        );
        assert!(
            result
                .always_used_files
                .contains(&"src/css/custom.css".to_string())
        );
        assert!(
            result
                .always_used_files
                .contains(&"src/client/custom-script.js".to_string())
        );
        assert!(
            result
                .always_used_files
                .contains(&"src/css/extra.css".to_string())
        );
        assert!(
            result
                .always_used_files
                .contains(&"public/scripts/runtime.js".to_string())
        );
        assert!(
            result
                .always_used_files
                .contains(&"static-assets/styles/local.css".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"@docusaurus/preset-classic".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"docusaurus-plugin-content-docs".to_string())
        );
        assert!(
            result
                .referenced_dependencies
                .contains(&"docusaurus-theme-classic".to_string())
        );
    }

    #[test]
    fn resolve_config_respects_disabled_classic_sections() {
        let source = r#"
            export default {
                presets: [
                    [
                        "classic",
                        {
                            docs: false,
                            blog: false,
                            pages: { path: "site-pages" },
                        },
                    ],
                ],
            };
        "#;

        let plugin = DocusaurusPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/docusaurus.config.ts"),
            source,
            Path::new("/project"),
        );

        assert!(!has_entry_pattern(&result, "docs/**/*.{md,mdx}"));
        assert!(!has_entry_pattern(&result, "blog/**/*.{md,mdx}"));
        assert!(has_entry_pattern(
            &result,
            "site-pages/**/*.{ts,tsx,js,jsx,md,mdx}"
        ));
    }

    #[test]
    fn resolve_config_limits_dynamic_rules_to_enabled_content_plugins() {
        let source = r#"
            export default {
                themes: ["classic"],
            };
        "#;

        let plugin = DocusaurusPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/docusaurus.config.ts"),
            source,
            Path::new("/project"),
        );

        assert!(has_entry_pattern(&result, "src/theme/*.{ts,tsx,js,jsx}"));
        assert!(!has_entry_pattern(&result, "docs/**/*.{md,mdx}"));
        assert!(!has_entry_pattern(&result, "blog/**/*.{md,mdx}"));
        assert!(!has_entry_pattern(
            &result,
            "src/pages/**/*.{ts,tsx,js,jsx,md,mdx}"
        ));
        assert!(!has_used_export_rule(&result, "sidebars.{js,ts}"));
    }

    #[test]
    fn resolve_config_does_not_assume_sidebar_file_for_classic_docs() {
        let source = r#"
            export default {
                presets: ["classic"],
            };
        "#;

        let plugin = DocusaurusPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/docusaurus.config.ts"),
            source,
            Path::new("/project"),
        );

        assert!(has_entry_pattern(&result, "docs/**/*.{md,mdx}"));
        assert!(
            !result
                .always_used_files
                .iter()
                .any(|path| path == "sidebars.js" || path == "sidebars.ts")
        );
        assert!(!has_used_export_rule(&result, "sidebars.{js,ts}"));
    }

    #[test]
    fn resolve_config_normalizes_default_plugin_instance_ids() {
        let source = r#"
            export default {
                plugins: [
                    [
                        "@docusaurus/plugin-content-docs",
                        {
                            id: "default",
                        },
                    ],
                ],
            };
        "#;

        let plugin = DocusaurusPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/docusaurus.config.ts"),
            source,
            Path::new("/project"),
        );

        assert!(has_entry_pattern(
            &result,
            "i18n/*/docusaurus-plugin-content-docs/**/*.{md,mdx}"
        ));
        assert!(has_entry_pattern(&result, "versioned_docs/**/*.{md,mdx}"));
        assert!(!has_entry_pattern(
            &result,
            "i18n/*/docusaurus-plugin-content-docs-default/**/*.{md,mdx}"
        ));
        assert!(!has_entry_pattern(
            &result,
            "default_versioned_docs/**/*.{md,mdx}"
        ));
    }
}
