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
    use super::*;

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
