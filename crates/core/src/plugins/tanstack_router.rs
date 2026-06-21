//! `TanStack` Router plugin.
//!
//! Detects `TanStack` Router projects and marks route files as entry points.
//! Parses `tsr.config.json` to support custom route directories, generated
//! route-tree locations, and virtual route config.

use std::fs;
use std::path::{Path, PathBuf};

use super::{PathRule, Plugin, PluginResult, UsedExportRule, config_parser};
use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Expression, ImportDeclaration, ObjectExpression,
    Program, Statement, VariableDeclaration,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::SourceType;

const ENABLERS: &[&str] = &[
    "@tanstack/react-router",
    "@tanstack/solid-router",
    "@tanstack/start",
    "@tanstack/react-start",
    "@tanstack/solid-start",
    "@tanstack/virtual-file-routes",
];

const DEFAULT_ROUTE_DIRS: &[&str] = &["src/routes", "app/routes"];
const SUPPORTING_ENTRY_PATTERNS: &[&str] = &[
    "src/server.{ts,tsx,js,jsx}",
    "src/client.{ts,tsx,js,jsx}",
    "src/router.{ts,tsx,js,jsx}",
];
const DEFAULT_GENERATED_ROUTE_TREE_PATTERNS: &[&str] =
    &["src/routeTree.gen.ts", "src/routeTree.gen.js"];
const GENERATED_IMPORT_PATTERNS: &[&str] = &["/routeTree.gen"];
const VIRTUAL_MODULE_PREFIXES: &[&str] = &[
    "tanstack-start-manifest:",
    "tanstack-start-injected-head-scripts:",
];
const ENTRY_PATTERNS: &[&str] = &[
    "src/routes/**/*.{ts,tsx,js,jsx}",
    "app/routes/**/*.{ts,tsx,js,jsx}",
    "src/server.{ts,tsx,js,jsx}",
    "src/client.{ts,tsx,js,jsx}",
    "src/router.{ts,tsx,js,jsx}",
    "src/routeTree.gen.ts",
    "src/routeTree.gen.js",
];

const CONFIG_PATTERNS: &[&str] = &[
    "tsr.config.json",
    "vite.config.{ts,js,mts,mjs}",
    "rsbuild.config.{ts,js,mts,mjs}",
    "rspack.config.{ts,js,mts,mjs}",
    "webpack.config.{ts,js,mts,mjs,cjs}",
];
const ROUTER_PLUGIN_IMPORTS: &[&str] = &[
    "@tanstack/router-plugin/vite",
    "@tanstack/router-plugin/rspack",
    "@tanstack/router-plugin/webpack",
];

const ALWAYS_USED: &[&str] = &["tsr.config.json", "app.config.{ts,js}"];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "@tanstack/react-router",
    "@tanstack/react-router-devtools",
    "@tanstack/solid-router",
    "@tanstack/solid-router-devtools",
    "@tanstack/start",
    "@tanstack/react-start",
    "@tanstack/solid-start",
    "@tanstack/router-cli",
    "@tanstack/router-plugin",
    "@tanstack/router-vite-plugin",
    "@tanstack/virtual-file-routes",
];

const ROUTE_EXPORTS: &[&str] = &[
    "default",
    "Route",
    "loader",
    "action",
    "component",
    "errorComponent",
    "pendingComponent",
    "notFoundComponent",
    "beforeLoad",
    "ServerRoute",
];
const LAZY_ROUTE_EXPORTS: &[&str] = &[
    "Route",
    "component",
    "errorComponent",
    "pendingComponent",
    "notFoundComponent",
];
const DEFAULT_ROUTE_FILE_IGNORE_PREFIX: &str = "-";
const ROUTE_FILE_EXTENSIONS: &str = "{ts,tsx,js,jsx}";

pub struct TanstackRouterPlugin;

impl Plugin for TanstackRouterPlugin {
    fn name(&self) -> &'static str {
        "tanstack-router"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn entry_patterns(&self) -> &'static [&'static str] {
        ENTRY_PATTERNS
    }

    fn entry_pattern_rules(&self) -> Vec<PathRule> {
        let mut rules = DEFAULT_ROUTE_DIRS
            .iter()
            .flat_map(|route_dir| {
                [
                    route_dir_rule(
                        route_dir,
                        "",
                        DEFAULT_ROUTE_FILE_IGNORE_PREFIX,
                        None,
                        RouteFileKind::Standard,
                    ),
                    route_dir_rule(
                        route_dir,
                        "",
                        DEFAULT_ROUTE_FILE_IGNORE_PREFIX,
                        None,
                        RouteFileKind::Lazy,
                    ),
                ]
            })
            .collect::<Vec<_>>();
        rules.extend(
            DEFAULT_GENERATED_ROUTE_TREE_PATTERNS
                .iter()
                .chain(SUPPORTING_ENTRY_PATTERNS.iter())
                .map(|pattern| PathRule::from_static(pattern)),
        );
        rules
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

    fn generated_import_patterns(&self) -> &'static [&'static str] {
        GENERATED_IMPORT_PATTERNS
    }

    fn virtual_module_prefixes(&self) -> &'static [&'static str] {
        VIRTUAL_MODULE_PREFIXES
    }

    fn used_exports(&self) -> Vec<(&'static str, &'static [&'static str])> {
        vec![
            ("src/routes/**/*.{ts,tsx,js,jsx}", ROUTE_EXPORTS),
            ("app/routes/**/*.{ts,tsx,js,jsx}", ROUTE_EXPORTS),
            ("src/routes/**/*.lazy.{ts,tsx,js,jsx}", LAZY_ROUTE_EXPORTS),
            ("app/routes/**/*.lazy.{ts,tsx,js,jsx}", LAZY_ROUTE_EXPORTS),
        ]
    }

    fn used_export_rules(&self) -> Vec<UsedExportRule> {
        DEFAULT_ROUTE_DIRS
            .iter()
            .flat_map(|route_dir| {
                [
                    route_dir_used_export_rule(
                        route_dir,
                        "",
                        DEFAULT_ROUTE_FILE_IGNORE_PREFIX,
                        None,
                    ),
                    lazy_route_rule(route_dir, "", DEFAULT_ROUTE_FILE_IGNORE_PREFIX, None),
                ]
            })
            .collect()
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        if !is_tsr_config(config_path) {
            return resolve_bundler_config(config_path, source, root).unwrap_or_default();
        }

        resolve_tsr_config(config_path, source, root)
    }
}

fn resolve_tsr_config(config_path: &Path, source: &str, root: &Path) -> PluginResult {
    let route_dir = config_parser::extract_config_string(source, config_path, &["routesDirectory"])
        .as_deref()
        .and_then(|raw| config_parser::normalize_config_path(raw, config_path, root))
        .unwrap_or_else(|| "src/routes".to_string());

    resolve_route_options(RouteOptions {
        route_dir: route_dir.clone(),
        route_file_prefix: config_parser::extract_config_string(
            source,
            config_path,
            &["routeFilePrefix"],
        )
        .unwrap_or_default(),
        route_file_ignore_prefix: config_parser::extract_config_string(
            source,
            config_path,
            &["routeFileIgnorePrefix"],
        )
        .unwrap_or_else(|| DEFAULT_ROUTE_FILE_IGNORE_PREFIX.to_string()),
        route_file_ignore_pattern: config_parser::extract_config_string(
            source,
            config_path,
            &["routeFileIgnorePattern"],
        ),
        generated_route_tree: config_parser::extract_config_string(
            source,
            config_path,
            &["generatedRouteTree"],
        )
        .as_deref()
        .and_then(|raw| config_parser::normalize_config_path(raw, config_path, root)),
        virtual_route_config: resolve_virtual_route_config(config_path, source, root, &route_dir),
    })
}

fn resolve_bundler_config(config_path: &Path, source: &str, root: &Path) -> Option<PluginResult> {
    let source_type = SourceType::from_path(config_path).unwrap_or_default();
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let options = collect_router_plugin_route_options(&parsed.program, config_path, root)
        .into_iter()
        .next()?;

    Some(resolve_route_options(options))
}

#[derive(Debug, Default)]
struct RouteOptions {
    route_dir: String,
    route_file_prefix: String,
    route_file_ignore_prefix: String,
    route_file_ignore_pattern: Option<String>,
    generated_route_tree: Option<String>,
    virtual_route_config: VirtualRouteConfig,
}

fn resolve_route_options(options: RouteOptions) -> PluginResult {
    let mut result = PluginResult {
        replace_entry_patterns: true,
        replace_used_export_rules: true,
        ..PluginResult::default()
    };

    if options.virtual_route_config.is_empty() {
        add_route_dir_patterns(
            &mut result,
            &options.route_dir,
            &options.route_file_prefix,
            &options.route_file_ignore_prefix,
            options.route_file_ignore_pattern.as_deref(),
        );
    } else {
        apply_virtual_route_config(
            &mut result,
            options.virtual_route_config,
            &options.route_file_prefix,
            &options.route_file_ignore_prefix,
            options.route_file_ignore_pattern.as_deref(),
        );
    }

    if let Some(route_tree) = options.generated_route_tree {
        result.push_entry_pattern(route_tree);
    } else {
        result.extend_entry_patterns(DEFAULT_GENERATED_ROUTE_TREE_PATTERNS.iter().copied());
    }
    result.extend_entry_patterns(SUPPORTING_ENTRY_PATTERNS.iter().copied());

    result
}

fn is_tsr_config(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|file_name| file_name == "tsr.config.json")
}

#[derive(Debug, Default)]
struct VirtualRouteConfig {
    config_files: Vec<String>,
    route_files: Vec<String>,
    physical_dirs: Vec<String>,
}

impl VirtualRouteConfig {
    fn is_empty(&self) -> bool {
        self.config_files.is_empty() && self.route_files.is_empty() && self.physical_dirs.is_empty()
    }
}

fn resolve_virtual_route_config(
    config_path: &Path,
    source: &str,
    root: &Path,
    route_dir: &str,
) -> VirtualRouteConfig {
    let mut config = VirtualRouteConfig::default();

    if let Some(config_file) =
        config_parser::extract_config_string(source, config_path, &["virtualRouteConfig"])
            .as_deref()
            .and_then(|raw| config_parser::normalize_config_path(raw, config_path, root))
    {
        add_virtual_route_config_file(&mut config, root, &config_file);
    }

    for file in collect_inline_virtual_route_files(source) {
        if let Some(path) = normalize_project_relative(route_dir, &file) {
            push_unique(&mut config.route_files, path);
        }
    }

    config
}

fn resolve_bundler_route_options(
    program: &Program,
    options: &ObjectExpression,
    config_path: &Path,
    root: &Path,
) -> RouteOptions {
    let route_dir = extract_option_string(options, "routesDirectory")
        .as_deref()
        .and_then(|raw| config_parser::normalize_config_path(raw, config_path, root))
        .unwrap_or_else(|| "src/routes".to_string());

    RouteOptions {
        route_dir: route_dir.clone(),
        route_file_prefix: extract_option_string(options, "routeFilePrefix").unwrap_or_default(),
        route_file_ignore_prefix: extract_option_string(options, "routeFileIgnorePrefix")
            .unwrap_or_else(|| DEFAULT_ROUTE_FILE_IGNORE_PREFIX.to_string()),
        route_file_ignore_pattern: extract_option_string(options, "routeFileIgnorePattern"),
        generated_route_tree: extract_option_string(options, "generatedRouteTree")
            .as_deref()
            .and_then(|raw| config_parser::normalize_config_path(raw, config_path, root)),
        virtual_route_config: resolve_bundler_virtual_route_config(
            program,
            options,
            &route_dir,
            config_path,
            root,
        ),
    }
}

fn resolve_bundler_virtual_route_config(
    program: &Program,
    options: &ObjectExpression,
    route_dir: &str,
    config_path: &Path,
    root: &Path,
) -> VirtualRouteConfig {
    let mut config = VirtualRouteConfig::default();
    let Some(prop) = config_parser::find_property(options, "virtualRouteConfig") else {
        return config;
    };

    if let Some(config_file) = config_parser::expression_to_string(&prop.value)
        .as_deref()
        .and_then(|raw| config_parser::normalize_config_path(raw, config_path, root))
    {
        add_virtual_route_config_file(&mut config, root, &config_file);
        return config;
    }

    let refs = if let Expression::Identifier(identifier) = &prop.value {
        find_variable_init_expression(program, identifier.name.as_str())
            .map(|expr| collect_virtual_route_expression_refs(program, expr))
            .unwrap_or_default()
    } else {
        collect_virtual_route_expression_refs(program, &prop.value)
    };
    add_virtual_route_refs(&mut config, refs, route_dir);
    config
}

fn add_virtual_route_config_file(config: &mut VirtualRouteConfig, root: &Path, config_file: &str) {
    push_unique(&mut config.config_files, config_file.to_string());

    let file_path = root.join(config_file);
    let Ok(source) = fs::read_to_string(&file_path) else {
        return;
    };
    let base_dir = Path::new(config_file)
        .parent()
        .map_or_else(String::new, |parent| {
            parent.to_string_lossy().replace('\\', "/")
        });
    let refs = collect_virtual_route_call_refs(&source, &file_path);
    add_virtual_route_refs(config, refs, &base_dir);
}

fn add_virtual_route_refs(config: &mut VirtualRouteConfig, refs: VirtualRouteRefs, base_dir: &str) {
    for file in refs.route_files {
        if let Some(path) = normalize_project_relative(base_dir, &file) {
            push_unique(&mut config.route_files, path);
        }
    }
    for dir in refs.physical_dirs {
        if let Some(path) = normalize_project_relative(base_dir, &dir) {
            push_unique(&mut config.physical_dirs, path);
        }
    }
}

fn apply_virtual_route_config(
    result: &mut PluginResult,
    config: VirtualRouteConfig,
    route_file_prefix: &str,
    route_file_ignore_prefix: &str,
    route_file_ignore_pattern: Option<&str>,
) {
    for config_file in config.config_files {
        result.push_entry_pattern(config_file);
    }
    for route_file in config.route_files {
        result.push_entry_pattern(route_file.clone());
        result
            .used_exports
            .push(virtual_route_used_export_rule(&route_file));
    }
    for dir in config.physical_dirs {
        result.entry_patterns.push(route_dir_rule(
            &dir,
            route_file_prefix,
            route_file_ignore_prefix,
            route_file_ignore_pattern,
            RouteFileKind::Standard,
        ));
        result.entry_patterns.push(route_dir_rule(
            &dir,
            route_file_prefix,
            route_file_ignore_prefix,
            route_file_ignore_pattern,
            RouteFileKind::Lazy,
        ));
        result.used_exports.push(route_dir_used_export_rule(
            &dir,
            route_file_prefix,
            route_file_ignore_prefix,
            route_file_ignore_pattern,
        ));
        result.used_exports.push(lazy_route_rule(
            &dir,
            route_file_prefix,
            route_file_ignore_prefix,
            route_file_ignore_pattern,
        ));
    }
}

fn virtual_route_used_export_rule(path: &str) -> UsedExportRule {
    let exports = if path.contains(".lazy.") {
        LAZY_ROUTE_EXPORTS
    } else {
        ROUTE_EXPORTS
    };
    UsedExportRule::new(path.to_string(), exports.iter().copied())
}

fn collect_inline_virtual_route_files(source: &str) -> Vec<String> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(source) else {
        return Vec::new();
    };
    let Some(virtual_config) = json.get("virtualRouteConfig") else {
        return Vec::new();
    };

    let mut files = Vec::new();
    collect_json_file_properties(virtual_config, &mut files);
    files
}

fn collect_json_file_properties(value: &serde_json::Value, files: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(object) => {
            if let Some(serde_json::Value::String(file)) = object.get("file") {
                push_unique(files, file.clone());
            }
            for child in object.values() {
                collect_json_file_properties(child, files);
            }
        }
        serde_json::Value::Array(array) => {
            for child in array {
                collect_json_file_properties(child, files);
            }
        }
        _ => {}
    }
}

#[derive(Debug, Default)]
struct VirtualRouteRefs {
    route_files: Vec<String>,
    physical_dirs: Vec<String>,
}

fn collect_virtual_route_call_refs(source: &str, path: &Path) -> VirtualRouteRefs {
    let source_type = SourceType::from_path(path).unwrap_or_default();
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, source, source_type).parse();
    let mut collector = VirtualRouteCallCollector::default();
    collector.visit_program(&parsed.program);
    collector.refs
}

fn collect_virtual_route_expression_refs(program: &Program, expr: &Expression) -> VirtualRouteRefs {
    let mut collector = VirtualRouteCallCollector::from_imports(program);
    collector.visit_expression(expr);
    collector.refs
}

fn collect_router_plugin_route_options(
    program: &Program,
    config_path: &Path,
    root: &Path,
) -> Vec<RouteOptions> {
    let mut collector = RouterPluginCallCollector {
        route_options: Vec::new(),
        local_names: Vec::new(),
        namespaces: Vec::new(),
        program,
        config_path,
        root,
    };
    collector.visit_program(program);
    collector.route_options
}

fn extract_option_string(options: &ObjectExpression, key: &str) -> Option<String> {
    config_parser::find_property(options, key)
        .and_then(|prop| config_parser::expression_to_string(&prop.value))
}

fn find_variable_init_expression<'a>(
    program: &'a Program<'a>,
    name: &str,
) -> Option<&'a Expression<'a>> {
    for stmt in &program.body {
        let Statement::VariableDeclaration(decl) = stmt else {
            continue;
        };
        for declarator in &decl.declarations {
            if let BindingPattern::BindingIdentifier(identifier) = &declarator.id
                && identifier.name == name
                && let Some(init) = &declarator.init
            {
                return Some(init);
            }
        }
    }
    None
}

struct RouterPluginCallCollector<'a> {
    route_options: Vec<RouteOptions>,
    local_names: Vec<String>,
    namespaces: Vec<String>,
    program: &'a Program<'a>,
    config_path: &'a Path,
    root: &'a Path,
}

impl<'a> Visit<'a> for RouterPluginCallCollector<'a> {
    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        if !ROUTER_PLUGIN_IMPORTS
            .iter()
            .any(|source| decl.source.value == *source)
        {
            return;
        }

        if let Some(specifiers) = &decl.specifiers {
            for specifier in specifiers {
                match specifier {
                    oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(specifier)
                        if specifier.imported.name() == "tanstackRouter" =>
                    {
                        push_unique(&mut self.local_names, specifier.local.name.to_string());
                    }
                    oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(
                        specifier,
                    ) => {
                        push_unique(&mut self.namespaces, specifier.local.name.to_string());
                    }
                    _ => {}
                }
            }
        }
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if self.is_router_plugin_call(call)
            && let Some(Expression::ObjectExpression(options)) =
                call.arguments.first().and_then(Argument::as_expression)
        {
            self.route_options.push(resolve_bundler_route_options(
                self.program,
                options,
                self.config_path,
                self.root,
            ));
        }

        walk::walk_call_expression(self, call);
    }

    fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'a>) {
        for declarator in &decl.declarations {
            let Some(init) = &declarator.init else {
                continue;
            };
            let Some(source) = require_source(init) else {
                continue;
            };
            if !ROUTER_PLUGIN_IMPORTS.iter().any(|import| source == *import) {
                continue;
            }

            match &declarator.id {
                BindingPattern::BindingIdentifier(identifier) => {
                    push_unique(&mut self.namespaces, identifier.name.to_string());
                }
                BindingPattern::ObjectPattern(object) => {
                    for prop in &object.properties {
                        if prop
                            .key
                            .static_name()
                            .is_some_and(|name| name == "tanstackRouter")
                            && let BindingPattern::BindingIdentifier(identifier) = &prop.value
                        {
                            push_unique(&mut self.local_names, identifier.name.to_string());
                        }
                    }
                }
                _ => {}
            }
        }

        walk::walk_variable_declaration(self, decl);
    }
}

impl RouterPluginCallCollector<'_> {
    fn is_router_plugin_call(&self, call: &CallExpression<'_>) -> bool {
        match &call.callee {
            Expression::Identifier(identifier) => self
                .local_names
                .iter()
                .any(|name| name == identifier.name.as_str()),
            Expression::StaticMemberExpression(member) if matches!(&member.object, Expression::Identifier(object) if self.namespaces.iter().any(|name| name == object.name.as_str())) => {
                member.property.name == "tanstackRouter"
            }
            _ => false,
        }
    }
}

#[derive(Default)]
struct VirtualRouteCallCollector {
    refs: VirtualRouteRefs,
    helper_bindings: Vec<(String, String)>,
    namespaces: Vec<String>,
}

impl VirtualRouteCallCollector {
    fn from_imports(program: &Program) -> Self {
        let mut collector = Self::default();
        for stmt in &program.body {
            if let Statement::ImportDeclaration(decl) = stmt {
                collector.visit_import_declaration(decl);
            }
        }
        collector
    }
}

impl<'a> Visit<'a> for VirtualRouteCallCollector {
    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        if decl.source.value != "@tanstack/virtual-file-routes" {
            return;
        }

        if let Some(specifiers) = &decl.specifiers {
            for specifier in specifiers {
                match specifier {
                    oxc_ast::ast::ImportDeclarationSpecifier::ImportSpecifier(specifier) => {
                        let helper = specifier.imported.name().to_string();
                        if is_virtual_route_helper(&helper) {
                            push_unique_pair(
                                &mut self.helper_bindings,
                                specifier.local.name.to_string(),
                                helper,
                            );
                        }
                    }
                    oxc_ast::ast::ImportDeclarationSpecifier::ImportNamespaceSpecifier(
                        specifier,
                    ) => {
                        push_unique(&mut self.namespaces, specifier.local.name.to_string());
                    }
                    oxc_ast::ast::ImportDeclarationSpecifier::ImportDefaultSpecifier(_) => {}
                }
            }
        }
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if let Some(callee) = self.virtual_route_helper(call) {
            match callee {
                "rootRoute" | "index" => {
                    if let Some(file) = string_arg(call, 0) {
                        push_unique(&mut self.refs.route_files, file);
                    }
                }
                "route" => {
                    if let Some(file) = string_arg(call, 1) {
                        push_unique(&mut self.refs.route_files, file);
                    }
                }
                "layout" => {
                    if let Some(file) = string_arg(call, 1).or_else(|| string_arg(call, 0)) {
                        push_unique(&mut self.refs.route_files, file);
                    }
                }
                "physical" => {
                    if let Some(dir) = string_arg(call, 1).or_else(|| string_arg(call, 0)) {
                        push_unique(&mut self.refs.physical_dirs, dir);
                    }
                }
                _ => {}
            }
        }

        walk::walk_call_expression(self, call);
    }

    fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'a>) {
        for declarator in &decl.declarations {
            let Some(init) = &declarator.init else {
                continue;
            };
            let Some(source) = require_source(init) else {
                continue;
            };
            if source != "@tanstack/virtual-file-routes" {
                continue;
            }

            match &declarator.id {
                BindingPattern::BindingIdentifier(identifier) => {
                    push_unique(&mut self.namespaces, identifier.name.to_string());
                }
                BindingPattern::ObjectPattern(object) => {
                    for prop in &object.properties {
                        let Some(helper) = prop.key.static_name() else {
                            continue;
                        };
                        if is_virtual_route_helper(&helper)
                            && let BindingPattern::BindingIdentifier(identifier) = &prop.value
                        {
                            push_unique_pair(
                                &mut self.helper_bindings,
                                identifier.name.to_string(),
                                helper.to_string(),
                            );
                        }
                    }
                }
                _ => {}
            }
        }

        walk::walk_variable_declaration(self, decl);
    }
}

impl VirtualRouteCallCollector {
    fn virtual_route_helper<'a>(&'a self, call: &'a CallExpression<'a>) -> Option<&'a str> {
        match &call.callee {
            Expression::Identifier(identifier) => {
                self.helper_bindings.iter().find_map(|(local, helper)| {
                    (local == identifier.name.as_str()).then_some(helper.as_str())
                })
            }
            Expression::StaticMemberExpression(member) if matches!(&member.object, Expression::Identifier(object) if self.namespaces.iter().any(|name| name == object.name.as_str())) =>
            {
                let helper = member.property.name.as_str();
                is_virtual_route_helper(helper).then_some(helper)
            }
            _ => None,
        }
    }
}

fn is_virtual_route_helper(name: &str) -> bool {
    matches!(
        name,
        "rootRoute" | "index" | "route" | "layout" | "physical"
    )
}

fn push_unique_pair(values: &mut Vec<(String, String)>, local: String, helper: String) {
    if !values.iter().any(|(existing, _)| existing == &local) {
        values.push((local, helper));
    }
}

fn string_arg(call: &CallExpression<'_>, index: usize) -> Option<String> {
    call.arguments
        .get(index)
        .and_then(|argument| match argument {
            Argument::StringLiteral(value) => Some(value.value.to_string()),
            Argument::TemplateLiteral(value) if value.expressions.is_empty() => value
                .quasis
                .first()
                .map(|quasi| quasi.value.raw.to_string()),
            _ => None,
        })
}

fn require_source(expr: &Expression<'_>) -> Option<String> {
    let Expression::CallExpression(call) = expr else {
        return None;
    };
    if !matches!(&call.callee, Expression::Identifier(identifier) if identifier.name == "require") {
        return None;
    }
    string_arg(call, 0)
}

fn normalize_project_relative(base_dir: &str, raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.starts_with('/') || raw.contains("://") {
        return None;
    }

    let path = Path::new(raw);
    let joined = if path.is_absolute() {
        PathBuf::from(path)
    } else if base_dir.is_empty() {
        path.to_path_buf()
    } else {
        Path::new(base_dir).join(path)
    };

    let normalized = lexical_normalize(&joined)
        .to_string_lossy()
        .replace('\\', "/");
    (!normalized.is_empty()).then_some(normalized)
}

fn lexical_normalize(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

fn add_route_dir_patterns(
    result: &mut PluginResult,
    route_dir: &str,
    route_file_prefix: &str,
    route_file_ignore_prefix: &str,
    route_file_ignore_pattern: Option<&str>,
) {
    result.entry_patterns.push(route_dir_rule(
        route_dir,
        route_file_prefix,
        route_file_ignore_prefix,
        route_file_ignore_pattern,
        RouteFileKind::Standard,
    ));
    result.entry_patterns.push(route_dir_rule(
        route_dir,
        route_file_prefix,
        route_file_ignore_prefix,
        route_file_ignore_pattern,
        RouteFileKind::Lazy,
    ));
    result.used_exports.push(route_dir_used_export_rule(
        route_dir,
        route_file_prefix,
        route_file_ignore_prefix,
        route_file_ignore_pattern,
    ));
    result.used_exports.push(lazy_route_rule(
        route_dir,
        route_file_prefix,
        route_file_ignore_prefix,
        route_file_ignore_pattern,
    ));
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RouteFileKind {
    Standard,
    Lazy,
}

#[derive(Default)]
struct RouteDirExclusions {
    globs: Vec<String>,
    segment_regexes: Vec<String>,
}

fn route_dir_rule(
    route_dir: &str,
    route_file_prefix: &str,
    route_file_ignore_prefix: &str,
    route_file_ignore_pattern: Option<&str>,
    file_kind: RouteFileKind,
) -> PathRule {
    let mut exclusions = route_dir_exclusions(
        route_dir,
        route_file_ignore_prefix,
        route_file_ignore_pattern,
    );
    if file_kind == RouteFileKind::Standard {
        exclusions.globs.push(route_file_pattern(
            route_dir,
            route_file_prefix,
            RouteFileKind::Lazy,
        ));
    }

    PathRule::new(route_file_pattern(route_dir, route_file_prefix, file_kind))
        .with_excluded_globs(exclusions.globs)
        .with_excluded_segment_regexes(exclusions.segment_regexes)
}

fn route_dir_used_export_rule(
    route_dir: &str,
    route_file_prefix: &str,
    route_file_ignore_prefix: &str,
    route_file_ignore_pattern: Option<&str>,
) -> UsedExportRule {
    used_export_rule_from_path_rule(
        route_dir_rule(
            route_dir,
            route_file_prefix,
            route_file_ignore_prefix,
            route_file_ignore_pattern,
            RouteFileKind::Standard,
        ),
        ROUTE_EXPORTS,
    )
}

fn lazy_route_rule(
    route_dir: &str,
    route_file_prefix: &str,
    route_file_ignore_prefix: &str,
    route_file_ignore_pattern: Option<&str>,
) -> UsedExportRule {
    used_export_rule_from_path_rule(
        route_dir_rule(
            route_dir,
            route_file_prefix,
            route_file_ignore_prefix,
            route_file_ignore_pattern,
            RouteFileKind::Lazy,
        ),
        LAZY_ROUTE_EXPORTS,
    )
}

fn route_dir_exclusions(
    route_dir: &str,
    route_file_ignore_prefix: &str,
    route_file_ignore_pattern: Option<&str>,
) -> RouteDirExclusions {
    let mut exclusions = RouteDirExclusions::default();

    if !route_file_ignore_prefix.is_empty() {
        exclusions
            .globs
            .push(format!("{route_dir}/**/{route_file_ignore_prefix}*"));
        exclusions
            .globs
            .push(format!("{route_dir}/**/{route_file_ignore_prefix}*/**/*"));
    }

    if let Some(pattern) = route_file_ignore_pattern {
        exclusions.segment_regexes.push(pattern.to_string());
    }

    exclusions
}

fn route_file_pattern(
    route_dir: &str,
    route_file_prefix: &str,
    file_kind: RouteFileKind,
) -> String {
    match file_kind {
        RouteFileKind::Standard => {
            format!("{route_dir}/**/{route_file_prefix}*.{ROUTE_FILE_EXTENSIONS}")
        }
        RouteFileKind::Lazy => {
            format!("{route_dir}/**/{route_file_prefix}*.lazy.{ROUTE_FILE_EXTENSIONS}")
        }
    }
}

fn used_export_rule_from_path_rule(
    rule: PathRule,
    exports: &'static [&'static str],
) -> UsedExportRule {
    UsedExportRule::new(rule.pattern, exports.iter().copied())
        .with_excluded_globs(rule.exclude_globs)
        .with_excluded_regexes(rule.exclude_regexes)
        .with_excluded_segment_regexes(rule.exclude_segment_regexes)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    // ---------------------------------------------------------------------------
    // Plugin trait accessors (lines 169-178, 178-193)
    // ---------------------------------------------------------------------------

    #[test]
    fn used_exports_returns_both_route_dirs_and_lazy_variants() {
        let plugin = TanstackRouterPlugin;
        let rules = plugin.used_exports();
        assert!(
            rules
                .iter()
                .any(|(pat, _)| *pat == "src/routes/**/*.{ts,tsx,js,jsx}"),
            "expected src/routes standard pattern"
        );
        assert!(
            rules
                .iter()
                .any(|(pat, _)| *pat == "app/routes/**/*.{ts,tsx,js,jsx}"),
            "expected app/routes standard pattern"
        );
        assert!(
            rules
                .iter()
                .any(|(pat, _)| *pat == "src/routes/**/*.lazy.{ts,tsx,js,jsx}"),
            "expected lazy pattern"
        );
        // lazy variants carry only the subset of exports
        let lazy_exports = rules
            .iter()
            .find(|(pat, _)| *pat == "src/routes/**/*.lazy.{ts,tsx,js,jsx}")
            .map_or(&[][..], |(_, exports)| *exports);
        assert!(
            lazy_exports.contains(&"component"),
            "lazy exports missing component"
        );
        assert!(
            !lazy_exports.contains(&"loader"),
            "lazy exports must not include loader"
        );
    }

    #[test]
    fn used_export_rules_covers_both_default_route_dirs() {
        let plugin = TanstackRouterPlugin;
        let rules = plugin.used_export_rules();
        assert!(
            rules
                .iter()
                .any(|r| r.path.pattern == "src/routes/**/*.{ts,tsx,js,jsx}"),
            "expected src/routes rule"
        );
        assert!(
            rules
                .iter()
                .any(|r| r.path.pattern == "app/routes/**/*.{ts,tsx,js,jsx}"),
            "expected app/routes rule"
        );
    }

    // ---------------------------------------------------------------------------
    // resolve_config: non-tsr-config path falls back to bundler config (197-201)
    // ---------------------------------------------------------------------------

    #[test]
    fn resolve_config_on_unknown_bundler_config_returns_default() {
        let plugin = TanstackRouterPlugin;
        // vite.config.ts without a tanstackRouter() call -> empty PluginResult
        let result = plugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            "export default {};",
            Path::new("/project"),
        );
        assert!(result.entry_patterns.is_empty());
    }

    #[test]
    fn resolve_config_on_bundler_config_with_tanstack_router_call_extracts_routes_dir() {
        let plugin = TanstackRouterPlugin;
        let source = r#"
import { tanstackRouter } from "@tanstack/router-plugin/vite";
export default {
  plugins: [tanstackRouter({ routesDirectory: "./custom/routes" })],
};
"#;
        let result = plugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(
            result.replace_entry_patterns,
            "should replace entry patterns"
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|r| r.pattern == "custom/routes/**/*.{ts,tsx,js,jsx}"),
            "custom routes dir pattern missing: {:?}",
            result.entry_patterns,
        );
    }

    // ---------------------------------------------------------------------------
    // tsr.config.json: virtualRouteConfig as inline JSON object (lines 330-336,
    // 477-503)
    // ---------------------------------------------------------------------------

    #[test]
    fn tsr_config_with_inline_virtual_route_config_extracts_file_entries() {
        let plugin = TanstackRouterPlugin;
        let source = r#"{
            "routesDirectory": "./src/routes",
            "virtualRouteConfig": {
                "type": "rootRoute",
                "file": "root.tsx",
                "children": [
                    { "type": "route", "path": "/about", "file": "about.tsx" }
                ]
            }
        }"#;
        let result = plugin.resolve_config(
            Path::new("/project/tsr.config.json"),
            source,
            Path::new("/project"),
        );
        assert!(
            result.replace_entry_patterns,
            "should replace entry patterns"
        );
        let patterns: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|r| r.pattern.as_str())
            .collect();
        assert!(
            patterns.iter().any(|p| p.contains("root.tsx")),
            "root.tsx missing from entry patterns: {patterns:?}"
        );
        assert!(
            patterns.iter().any(|p| p.contains("about.tsx")),
            "about.tsx missing from entry patterns: {patterns:?}"
        );
    }

    #[test]
    fn collect_json_file_properties_traverses_array_children() {
        let source = r#"{
            "routesDirectory": "./src/routes",
            "virtualRouteConfig": [
                { "file": "a.tsx" },
                { "file": "b.tsx" }
            ]
        }"#;
        let files = collect_inline_virtual_route_files(source);
        assert!(
            files.contains(&"a.tsx".to_string()),
            "a.tsx missing: {files:?}"
        );
        assert!(
            files.contains(&"b.tsx".to_string()),
            "b.tsx missing: {files:?}"
        );
    }

    #[test]
    fn collect_json_file_properties_ignores_non_file_leaves() {
        // A scalar at the top level (not object/array) should yield nothing.
        let source = r#"{ "virtualRouteConfig": "not-an-object" }"#;
        let files = collect_inline_virtual_route_files(source);
        assert!(files.is_empty(), "expected no files: {files:?}");
    }

    #[test]
    fn collect_inline_virtual_route_files_returns_empty_on_invalid_json() {
        let files = collect_inline_virtual_route_files("not json at all");
        assert!(files.is_empty());
    }

    #[test]
    fn collect_inline_virtual_route_files_returns_empty_when_key_absent() {
        let files = collect_inline_virtual_route_files(r#"{ "routesDirectory": "./src/routes" }"#);
        assert!(files.is_empty());
    }

    // ---------------------------------------------------------------------------
    // resolve_bundler_virtual_route_config: inline object expression (line 394)
    // ---------------------------------------------------------------------------

    #[test]
    fn bundler_config_virtual_route_config_as_inline_object_with_route_calls() {
        let plugin = TanstackRouterPlugin;
        let source = r#"
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import { rootRoute, route } from "@tanstack/virtual-file-routes";
export default {
  plugins: [tanstackRouter({
    virtualRouteConfig: rootRoute("root.tsx", [
      route("/about", "about.tsx"),
    ]),
  })],
};
"#;
        let result = plugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        let patterns: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|r| r.pattern.as_str())
            .collect();
        assert!(
            patterns.iter().any(|p| p.contains("root.tsx")),
            "root.tsx not found: {patterns:?}"
        );
        assert!(
            patterns.iter().any(|p| p.contains("about.tsx")),
            "about.tsx not found: {patterns:?}"
        );
    }

    #[test]
    fn bundler_config_virtual_route_config_as_variable_reference() {
        let plugin = TanstackRouterPlugin;
        let source = r#"
import { tanstackRouter } from "@tanstack/router-plugin/vite";
import { rootRoute, index } from "@tanstack/virtual-file-routes";
const routes = rootRoute("root.tsx", [index("index.tsx")]);
export default {
  plugins: [tanstackRouter({ virtualRouteConfig: routes })],
};
"#;
        let result = plugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        let patterns: Vec<&str> = result
            .entry_patterns
            .iter()
            .map(|r| r.pattern.as_str())
            .collect();
        assert!(
            patterns.iter().any(|p| p.contains("root.tsx")),
            "root.tsx missing: {patterns:?}"
        );
        assert!(
            patterns.iter().any(|p| p.contains("index.tsx")),
            "index.tsx missing: {patterns:?}"
        );
    }

    // ---------------------------------------------------------------------------
    // find_variable_init_expression: identifier not found (line 573-577)
    // ---------------------------------------------------------------------------

    #[test]
    fn bundler_config_virtual_route_config_unknown_variable_yields_no_routes() {
        let plugin = TanstackRouterPlugin;
        // virtualRouteConfig references an identifier that is never declared.
        let source = r#"
import { tanstackRouter } from "@tanstack/router-plugin/vite";
export default {
  plugins: [tanstackRouter({ virtualRouteConfig: undeclaredVariable })],
};
"#;
        let result = plugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        // Should not panic; virtual route config should be empty so standard
        // route patterns are produced instead.
        assert!(result.replace_entry_patterns);
    }

    // ---------------------------------------------------------------------------
    // RouterPluginCallCollector: namespace import + require forms (606-680)
    // ---------------------------------------------------------------------------

    #[test]
    fn bundler_config_router_plugin_via_namespace_import() {
        let plugin = TanstackRouterPlugin;
        let source = r#"
import * as tsr from "@tanstack/router-plugin/vite";
export default {
  plugins: [tsr.tanstackRouter({ routesDirectory: "./pages" })],
};
"#;
        let result = plugin.resolve_config(
            Path::new("/project/vite.config.ts"),
            source,
            Path::new("/project"),
        );
        assert!(result.replace_entry_patterns);
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|r| r.pattern == "pages/**/*.{ts,tsx,js,jsx}"),
            "namespace import route dir missing: {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn bundler_config_router_plugin_via_destructured_require() {
        let plugin = TanstackRouterPlugin;
        let source = r#"
const { tanstackRouter } = require("@tanstack/router-plugin/vite");
module.exports = {
  plugins: [tanstackRouter({ routesDirectory: "./pages" })],
};
"#;
        let result = plugin.resolve_config(
            Path::new("/project/webpack.config.js"),
            source,
            Path::new("/project"),
        );
        assert!(result.replace_entry_patterns);
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|r| r.pattern == "pages/**/*.{ts,tsx,js,jsx}"),
            "require destructure route dir missing: {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn bundler_config_router_plugin_via_namespace_require() {
        let plugin = TanstackRouterPlugin;
        let source = r#"
const tsr = require("@tanstack/router-plugin/vite");
module.exports = {
  plugins: [tsr.tanstackRouter({ routesDirectory: "./pages" })],
};
"#;
        let result = plugin.resolve_config(
            Path::new("/project/webpack.config.cjs"),
            source,
            Path::new("/project"),
        );
        assert!(result.replace_entry_patterns);
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|r| r.pattern == "pages/**/*.{ts,tsx,js,jsx}"),
            "namespace require route dir missing: {:?}",
            result.entry_patterns
        );
    }

    // ---------------------------------------------------------------------------
    // VirtualRouteCallCollector: require-based virtual file routes (719-798)
    // ---------------------------------------------------------------------------

    #[test]
    fn virtual_route_collector_via_destructured_require() {
        let source = r#"
const { rootRoute, route } = require("@tanstack/virtual-file-routes");
module.exports = rootRoute("root.tsx", [route("/about", "about.tsx")]);
"#;
        let path = Path::new("virtual.routes.js");
        let refs = collect_virtual_route_call_refs(source, path);
        assert!(
            refs.route_files.contains(&"root.tsx".to_string()),
            "root.tsx missing: {:?}",
            refs.route_files
        );
        assert!(
            refs.route_files.contains(&"about.tsx".to_string()),
            "about.tsx missing: {:?}",
            refs.route_files
        );
    }

    #[test]
    fn virtual_route_collector_via_namespace_require() {
        let source = r#"
const vfr = require("@tanstack/virtual-file-routes");
module.exports = vfr.rootRoute("root.tsx", [vfr.physical("routes/admin", "admin")]);
"#;
        let path = Path::new("virtual.routes.js");
        let refs = collect_virtual_route_call_refs(source, path);
        assert!(
            refs.route_files.contains(&"root.tsx".to_string()),
            "root.tsx missing: {:?}",
            refs.route_files
        );
        assert!(
            refs.physical_dirs.contains(&"admin".to_string()),
            "admin dir missing: {:?}",
            refs.physical_dirs
        );
    }

    #[test]
    fn virtual_route_collector_ignores_non_virtual_file_routes_require() {
        let source = r#"
const { rootRoute } = require("@tanstack/some-other-package");
module.exports = rootRoute("root.tsx");
"#;
        let path = Path::new("virtual.routes.js");
        let refs = collect_virtual_route_call_refs(source, path);
        // The helper binding should not have been registered, so no files.
        assert!(refs.route_files.is_empty());
    }

    // ---------------------------------------------------------------------------
    // VirtualRouteCallCollector: namespace member call (lines 804-817)
    // ---------------------------------------------------------------------------

    #[test]
    fn virtual_route_helper_via_namespace_member_call() {
        let source = r#"
import * as vfr from "@tanstack/virtual-file-routes";
export default vfr.rootRoute("root.tsx", [
  vfr.index("index.tsx"),
  vfr.layout("layout.tsx", [vfr.route("/about", "about.tsx")]),
  vfr.physical("./physical-routes", "phys"),
]);
"#;
        let path = Path::new("routes.ts");
        let refs = collect_virtual_route_call_refs(source, path);
        assert!(
            refs.route_files.contains(&"root.tsx".to_string()),
            "root.tsx missing: {:?}",
            refs.route_files
        );
        assert!(
            refs.route_files.contains(&"index.tsx".to_string()),
            "index.tsx missing: {:?}",
            refs.route_files
        );
        assert!(
            refs.route_files.contains(&"layout.tsx".to_string()),
            "layout.tsx missing: {:?}",
            refs.route_files
        );
        assert!(
            refs.route_files.contains(&"about.tsx".to_string()),
            "about.tsx missing: {:?}",
            refs.route_files
        );
        assert!(
            refs.physical_dirs.contains(&"phys".to_string()),
            "phys dir missing: {:?}",
            refs.physical_dirs
        );
    }

    // ---------------------------------------------------------------------------
    // string_arg: template literal form (lines 839-841)
    // ---------------------------------------------------------------------------

    #[test]
    fn virtual_route_call_with_template_literal_string_argument() {
        // rootRoute accepts a template literal with no expressions as a file arg.
        let source = r#"
import { rootRoute } from "@tanstack/virtual-file-routes";
export default rootRoute(`root.tsx`);
"#;
        let path = Path::new("routes.ts");
        let refs = collect_virtual_route_call_refs(source, path);
        assert!(
            refs.route_files.contains(&"root.tsx".to_string()),
            "template literal arg not captured: {:?}",
            refs.route_files
        );
    }

    // ---------------------------------------------------------------------------
    // require_source: non-call-expression fast path (line 847-848)
    // ---------------------------------------------------------------------------

    #[test]
    fn virtual_route_collector_skips_non_call_expression_init() {
        // Declarator initialised with a literal, not a call expression.
        let source = r#"
const { rootRoute } = "@tanstack/virtual-file-routes";
"#;
        let path = Path::new("routes.ts");
        // Should not panic; no bindings recorded so no files.
        let refs = collect_virtual_route_call_refs(source, path);
        assert!(refs.route_files.is_empty());
    }

    // ---------------------------------------------------------------------------
    // push_unique: deduplication (lines 891-895)
    // ---------------------------------------------------------------------------

    #[test]
    fn push_unique_does_not_insert_duplicate_values() {
        let mut values: Vec<String> = Vec::new();
        push_unique(&mut values, "a".to_string());
        push_unique(&mut values, "b".to_string());
        push_unique(&mut values, "a".to_string());
        assert_eq!(values, vec!["a".to_string(), "b".to_string()]);
    }

    // ---------------------------------------------------------------------------
    // route_dir_exclusions: ignore pattern branch (lines 1019-1021)
    // ---------------------------------------------------------------------------

    #[test]
    fn route_rules_with_empty_ignore_prefix_produce_no_prefix_exclusion_glob() {
        let rule = route_dir_rule("src/routes", "", "", None, RouteFileKind::Standard);
        // Empty ignore prefix: no exclusion globs for the prefix pattern.
        assert!(
            !rule.exclude_globs.iter().any(|g| g.ends_with("/**/*")),
            "unexpected exclusion glob: {:?}",
            rule.exclude_globs
        );
    }

    #[test]
    fn route_dir_exclusions_appends_segment_regex_when_ignore_pattern_set() {
        let exclusions =
            route_dir_exclusions("src/routes", DEFAULT_ROUTE_FILE_IGNORE_PREFIX, Some("^__"));
        assert!(
            exclusions.segment_regexes.contains(&"^__".to_string()),
            "segment regex not added: {:?}",
            exclusions.segment_regexes
        );
    }

    #[test]
    fn route_dir_exclusions_empty_ignore_pattern_skips_segment_regex() {
        let exclusions = route_dir_exclusions("src/routes", DEFAULT_ROUTE_FILE_IGNORE_PREFIX, None);
        assert!(exclusions.segment_regexes.is_empty());
    }

    // ---------------------------------------------------------------------------
    // apply_virtual_route_config: lazy route used-export rule (lines 466-472)
    // ---------------------------------------------------------------------------

    #[test]
    fn virtual_route_file_ending_in_lazy_gets_lazy_exports() {
        let rule = virtual_route_used_export_rule("src/routes/about.lazy.tsx");
        assert!(
            rule.exports.contains(&"component".to_string()),
            "lazy exports missing component: {:?}",
            rule.exports
        );
        assert!(
            !rule.exports.contains(&"loader".to_string()),
            "lazy export rule should not include loader: {:?}",
            rule.exports
        );
    }

    #[test]
    fn virtual_route_file_not_lazy_gets_full_route_exports() {
        let rule = virtual_route_used_export_rule("src/routes/about.tsx");
        assert!(
            rule.exports.contains(&"loader".to_string()),
            "standard route missing loader: {:?}",
            rule.exports
        );
        assert!(
            rule.exports.contains(&"default".to_string()),
            "standard route missing default: {:?}",
            rule.exports
        );
    }

    // ---------------------------------------------------------------------------
    // add_virtual_route_refs: normalizes paths relative to base dir (416-427)
    // ---------------------------------------------------------------------------

    #[test]
    fn add_virtual_route_refs_normalizes_route_file_relative_to_base_dir() {
        let mut config = VirtualRouteConfig::default();
        let refs = VirtualRouteRefs {
            route_files: vec!["./about.tsx".to_string()],
            physical_dirs: vec!["./admin".to_string()],
        };
        add_virtual_route_refs(&mut config, refs, "src/routes");
        assert_eq!(config.route_files, vec!["src/routes/about.tsx".to_string()]);
        assert_eq!(config.physical_dirs, vec!["src/routes/admin".to_string()]);
    }

    #[test]
    fn add_virtual_route_refs_skips_absolute_paths() {
        let mut config = VirtualRouteConfig::default();
        let refs = VirtualRouteRefs {
            route_files: vec!["/absolute/root.tsx".to_string()],
            physical_dirs: vec![],
        };
        add_virtual_route_refs(&mut config, refs, "src/routes");
        assert!(
            config.route_files.is_empty(),
            "absolute paths should be skipped: {:?}",
            config.route_files
        );
    }

    // ---------------------------------------------------------------------------
    // add_virtual_route_config_file: reads config file from disk (400-414)
    // ---------------------------------------------------------------------------

    #[cfg_attr(miri, ignore)]
    #[test]
    fn add_virtual_route_config_file_reads_and_parses_virtual_route_file() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let config_file = "src/routes.ts";
        let routes_path = root.join("src");
        std::fs::create_dir_all(&routes_path).expect("create dir");
        let full_path = routes_path.join("routes.ts");
        let mut f = std::fs::File::create(&full_path).expect("create file");
        writeln!(
            f,
            r#"import {{ rootRoute, index }} from "@tanstack/virtual-file-routes";
export default rootRoute("root.tsx", [index("index.tsx")]);"#
        )
        .expect("write");

        let mut config = VirtualRouteConfig::default();
        add_virtual_route_config_file(&mut config, root, config_file);

        assert!(
            config.config_files.contains(&config_file.to_string()),
            "config file not registered: {:?}",
            config.config_files
        );
        let files: Vec<String> = config
            .route_files
            .iter()
            .map(|s| s.replace('\\', "/"))
            .collect();
        assert!(
            files.iter().any(|f| f.contains("root.tsx")),
            "root.tsx missing: {files:?}"
        );
        assert!(
            files.iter().any(|f| f.contains("index.tsx")),
            "index.tsx missing: {files:?}"
        );
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn add_virtual_route_config_file_returns_early_when_file_missing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let mut config = VirtualRouteConfig::default();
        // File does not exist on disk; should record config_file but not panic.
        add_virtual_route_config_file(&mut config, root, "nonexistent/routes.ts");
        assert!(
            config
                .config_files
                .contains(&"nonexistent/routes.ts".to_string()),
            "config file not registered even when missing from disk"
        );
        assert!(config.route_files.is_empty());
    }

    // ---------------------------------------------------------------------------
    // tsr.config.json: virtualRouteConfig as file path string (lines 320-328)
    // ---------------------------------------------------------------------------

    #[cfg_attr(miri, ignore)]
    #[test]
    fn tsr_config_virtual_route_config_as_file_path_string() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        let routes_dir = root.join("src");
        std::fs::create_dir_all(&routes_dir).expect("create dir");
        let virtual_path = routes_dir.join("routes.ts");
        let mut f = std::fs::File::create(&virtual_path).expect("create file");
        writeln!(
            f,
            r#"import {{ rootRoute }} from "@tanstack/virtual-file-routes";
export default rootRoute("root.tsx");"#
        )
        .expect("write");

        let plugin = TanstackRouterPlugin;
        let source = r#"{ "virtualRouteConfig": "./src/routes.ts" }"#;
        let result = plugin.resolve_config(&root.join("tsr.config.json"), source, root);
        let patterns: Vec<String> = result
            .entry_patterns
            .iter()
            .map(|r| r.pattern.replace('\\', "/"))
            .collect();
        assert!(
            patterns.iter().any(|p| p.contains("routes.ts")),
            "virtual config file not in entry patterns: {patterns:?}"
        );
        assert!(
            patterns.iter().any(|p| p.contains("root.tsx")),
            "root.tsx from virtual config file not in entry patterns: {patterns:?}"
        );
    }

    // ---------------------------------------------------------------------------
    // layout helper: file as first arg when no path segment provided (lines 746-749)
    // ---------------------------------------------------------------------------

    #[test]
    fn virtual_route_layout_with_file_as_first_arg() {
        let source = r#"
import { rootRoute, layout } from "@tanstack/virtual-file-routes";
export default rootRoute("root.tsx", [layout("layout.tsx")]);
"#;
        let path = Path::new("routes.ts");
        let refs = collect_virtual_route_call_refs(source, path);
        assert!(
            refs.route_files.contains(&"layout.tsx".to_string()),
            "layout.tsx missing from first-arg form: {:?}",
            refs.route_files
        );
    }

    #[test]
    fn virtual_route_layout_with_path_segment_and_file_as_second_arg() {
        let source = r#"
import { rootRoute, layout } from "@tanstack/virtual-file-routes";
export default rootRoute("root.tsx", [layout("/dashboard", "dashboard-layout.tsx")]);
"#;
        let path = Path::new("routes.ts");
        let refs = collect_virtual_route_call_refs(source, path);
        assert!(
            refs.route_files
                .contains(&"dashboard-layout.tsx".to_string()),
            "dashboard-layout.tsx missing from second-arg form: {:?}",
            refs.route_files
        );
    }

    // ---------------------------------------------------------------------------
    // physical helper: dir as first arg when no prefix is provided (lines 750-753)
    // ---------------------------------------------------------------------------

    #[test]
    fn virtual_route_physical_with_dir_as_first_arg() {
        let source = r#"
import { rootRoute, physical } from "@tanstack/virtual-file-routes";
export default rootRoute("root.tsx", [physical("admin")]);
"#;
        let path = Path::new("routes.ts");
        let refs = collect_virtual_route_call_refs(source, path);
        assert!(
            refs.physical_dirs.contains(&"admin".to_string()),
            "admin dir missing from first-arg form: {:?}",
            refs.physical_dirs
        );
    }

    // ---------------------------------------------------------------------------
    // normalize_project_relative edge cases (856-875)
    // ---------------------------------------------------------------------------

    #[test]
    fn normalize_project_relative_rejects_empty_string() {
        assert!(normalize_project_relative("src/routes", "").is_none());
    }

    #[test]
    fn normalize_project_relative_rejects_url_with_scheme() {
        assert!(normalize_project_relative("src/routes", "https://example.com/foo").is_none());
    }

    #[test]
    fn normalize_project_relative_resolves_dotdot_components() {
        let result = normalize_project_relative("src/routes/sub", "../sibling.tsx");
        assert_eq!(
            result.as_deref().map(|s| s.replace('\\', "/")),
            Some("src/routes/sibling.tsx".to_string())
        );
    }

    #[test]
    fn normalize_project_relative_with_empty_base_dir() {
        let result = normalize_project_relative("", "pages/home.tsx");
        assert_eq!(
            result.as_deref().map(|s| s.replace('\\', "/")),
            Some("pages/home.tsx".to_string())
        );
    }

    // ---------------------------------------------------------------------------
    // Existing tests (unchanged)
    // ---------------------------------------------------------------------------

    #[test]
    fn used_exports_cover_lazy_routes_without_inheriting_non_lazy_exports() {
        let lazy_rule = lazy_route_rule("src/routes", "", DEFAULT_ROUTE_FILE_IGNORE_PREFIX, None);
        let broad_rule =
            route_dir_used_export_rule("src/routes", "", DEFAULT_ROUTE_FILE_IGNORE_PREFIX, None);

        assert_eq!(
            lazy_rule.path.pattern,
            "src/routes/**/*.lazy.{ts,tsx,js,jsx}"
        );
        assert!(lazy_rule.exports.contains(&"Route".to_string()));
        assert!(lazy_rule.exports.contains(&"component".to_string()));
        assert!(
            broad_rule
                .path
                .exclude_globs
                .contains(&"src/routes/**/*.lazy.{ts,tsx,js,jsx}".to_string())
        );
    }

    #[test]
    fn resolve_config_uses_custom_routes_directory() {
        let plugin = TanstackRouterPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/tsr.config.json"),
            r#"{
                "routesDirectory": "./app/pages",
                "generatedRouteTree": "./app/routeTree.gen.ts",
                "routeFileIgnorePrefix": "-"
            }"#,
            Path::new("/project"),
        );

        assert!(result.replace_entry_patterns);
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|rule| rule.pattern == "app/pages/**/*.{ts,tsx,js,jsx}"),
            "entry patterns: {:?}",
            result.entry_patterns
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|rule| rule.pattern == "app/routeTree.gen.ts")
        );
        assert!(
            result
                .entry_patterns
                .iter()
                .any(|rule| rule.pattern == "src/router.{ts,tsx,js,jsx}")
        );
    }

    #[test]
    fn resolve_config_keeps_default_supporting_entries_with_custom_route_dir() {
        let plugin = TanstackRouterPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/tsr.config.json"),
            r#"{
                "routesDirectory": "./app/pages"
            }"#,
            Path::new("/project"),
        );

        for expected in [
            "app/pages/**/*.{ts,tsx,js,jsx}",
            "src/routeTree.gen.ts",
            "src/routeTree.gen.js",
            "src/server.{ts,tsx,js,jsx}",
            "src/client.{ts,tsx,js,jsx}",
            "src/router.{ts,tsx,js,jsx}",
        ] {
            assert!(
                result
                    .entry_patterns
                    .iter()
                    .any(|rule| rule.pattern == expected),
                "missing supporting entry pattern {expected}: {:?}",
                result.entry_patterns
            );
        }
    }

    #[test]
    fn route_rules_honor_route_file_prefix() {
        let route_rule = route_dir_used_export_rule(
            "app/pages",
            "route-",
            DEFAULT_ROUTE_FILE_IGNORE_PREFIX,
            None,
        );

        assert_eq!(
            route_rule.path.pattern,
            "app/pages/**/route-*.{ts,tsx,js,jsx}"
        );
        assert!(
            route_rule
                .path
                .exclude_globs
                .contains(&"app/pages/**/route-*.lazy.{ts,tsx,js,jsx}".to_string())
        );
    }

    #[test]
    fn route_rules_preserve_segment_ignore_regexes() {
        let route_rule = route_dir_used_export_rule(
            "app/pages",
            "",
            DEFAULT_ROUTE_FILE_IGNORE_PREFIX,
            Some("^ignored\\."),
        );

        assert!(
            route_rule
                .path
                .exclude_globs
                .contains(&"app/pages/**/-*".to_string())
        );
        assert_eq!(
            route_rule.path.exclude_segment_regexes,
            vec!["^ignored\\.".to_string()]
        );
        assert!(route_rule.path.exclude_regexes.is_empty());
    }
}
