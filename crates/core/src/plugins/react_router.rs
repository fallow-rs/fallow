//! React Router (v7+) framework plugin.
//!
//! Detects React Router projects and marks route files, root layout, and entry points.
//! Recognizes conventional route exports (loader, action, meta, etc.).

use std::{
    fs,
    path::{Path, PathBuf},
};

use fallow_extract::JsText;
use oxc_ast::ast::{
    Argument, ArrayExpressionElement, BindingPattern, CallExpression, Declaration, Expression,
    ImportDeclarationSpecifier, ObjectExpression, Program, Statement, VariableDeclaration,
};
use oxc_span::SourceType;
use rustc_hash::FxHashSet;

use super::{Plugin, PluginResult, config_parser};

const ENABLERS: &[&str] = &["@react-router/dev"];
const CONFIG_PATTERNS: &[&str] = &[
    "react-router.config.{ts,js,mjs,cjs}",
    "app/routes.{ts,js,mts,mjs}",
    "src/routes.{ts,js,mts,mjs}",
];

const ENTRY_PATTERNS: &[&str] = &[
    "app/routes/**/*.{ts,tsx,js,jsx}",
    "app/root.{ts,tsx,js,jsx}",
    "app/entry.client.{ts,tsx,js,jsx}",
    "app/entry.server.{ts,tsx,js,jsx}",
];

const ALWAYS_USED: &[&str] = &[
    "react-router.config.{ts,js,mjs,cjs}",
    "app/routes.{ts,js,mts,mjs}",
    "src/routes.{ts,js,mts,mjs}",
];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "@react-router/dev",
    "@react-router/serve",
    "@react-router/node",
];

/// Packages that the framework's built-in server entry imports. React Router
/// uses this built-in entry when the app directory has no `entry.server` file.
const DEFAULT_ENTRY_SERVER_DEPENDENCIES: &[&str] = &["isbot"];
const ENTRY_SERVER_EXTENSIONS: &[&str] = &["ts", "tsx", "js", "jsx"];

const BUNDLE_BOUNDARY_DIRS: &[&str] = &[".client", ".server"];
const GENERATED_TYPE_IMPORT_PREFIXES: &[&str] = &["./+types/"];

macro_rules! route_module_exports {
    ($($export:literal),+ $(,)?) => {
        const ROUTE_EXPORTS: &[&str] = &[$($export),+];
        const ROOT_EXPORTS: &[&str] = &[$($export,)+ "Layout"];
    };
}

route_module_exports!(
    "default",
    "loader",
    "clientLoader",
    "action",
    "clientAction",
    "meta",
    "links",
    "headers",
    "handle",
    "ErrorBoundary",
    "HydrateFallback",
    "shouldRevalidate",
    "middleware",
    "clientMiddleware",
);

const ROUTE_CONFIG_EXPORTS: &[&str] = &["default"];

/// Exports that the server runtime reads from `entry.server` (the
/// `ServerEntryModule` type). React Router 7 names the instrumentation hook
/// `unstable_instrumentations`; React Router 8 names it `instrumentations`.
const ENTRY_SERVER_EXPORTS: &[&str] = &[
    "default",
    "handleDataRequest",
    "handleError",
    "streamTimeout",
    "instrumentations",
    "unstable_instrumentations",
];

define_plugin! {
    struct ReactRouterPlugin => "react-router",
    enablers: ENABLERS,
    entry_patterns: ENTRY_PATTERNS,
    config_patterns: CONFIG_PATTERNS,
    always_used: ALWAYS_USED,
    tooling_dependencies: TOOLING_DEPENDENCIES,
    discovery_hidden_dirs: BUNDLE_BOUNDARY_DIRS,
    generated_type_import_prefixes: GENERATED_TYPE_IMPORT_PREFIXES,
    used_exports: [
        ("app/routes/**/*.{ts,tsx,js,jsx}", ROUTE_EXPORTS),
        ("app/root.{ts,tsx,js,jsx}", ROOT_EXPORTS),
        ("app/routes.{ts,js,mts,mjs}", ROUTE_CONFIG_EXPORTS),
        ("app/entry.server.{ts,tsx,js,jsx}", ENTRY_SERVER_EXPORTS),
    ],
    resolve_config(config_path, source, root) {
        let mut result = PluginResult::default();
        add_referenced_dependencies(&mut result, source, config_path);

        if is_react_router_project_config(config_path) {
            let app_dir = extract_app_directory(source, config_path, root);
            add_app_dir_patterns(&mut result, &app_dir);
            add_default_entry_server_dependencies(&mut result, root, &app_dir);
            if !matches!(app_dir.as_str(), "app" | "src") {
                collect_route_config_from_disk(&mut result, root, &app_dir);
            }
            return result;
        }

        let Some(app_dir) = route_config_app_dir(config_path, root) else {
            return result;
        };
        add_app_dir_patterns(&mut result, &app_dir);
        add_default_entry_server_dependencies(&mut result, root, &app_dir);
        collect_route_config_from_source(&mut result, source, config_path, root);
        result
    },
}

fn add_referenced_dependencies(result: &mut PluginResult, source: &str, config_path: &Path) {
    for import in config_parser::extract_imports(source, config_path) {
        result
            .referenced_dependencies
            .push(crate::resolve::extract_package_name(&import));
    }
}

fn is_react_router_project_config(config_path: &Path) -> bool {
    config_path.file_name().is_some_and(|name| {
        let file_name = name.to_string_lossy();
        file_name.starts_with("react-router.config.")
    })
}

fn extract_app_directory(source: &str, config_path: &Path, root: &Path) -> String {
    config_parser::extract_config_string(source, config_path, &["appDirectory"])
        .as_deref()
        .and_then(|raw| config_parser::normalize_config_path(raw, config_path, root))
        .unwrap_or_else(|| "app".to_string())
}

fn route_config_app_dir(config_path: &Path, root: &Path) -> Option<String> {
    let file_name = config_path.file_name()?.to_string_lossy();
    if !file_name.starts_with("routes.") {
        return None;
    }

    let parent = config_path.parent().unwrap_or(root);
    let relative = parent.strip_prefix(root).ok()?;
    let normalized = relative.to_string_lossy().replace('\\', "/");
    (!normalized.is_empty()).then_some(normalized)
}

fn add_app_dir_patterns(result: &mut PluginResult, app_dir: &str) {
    let route_dir_pattern = route_dir_pattern(app_dir);
    let route_config_pattern = route_config_pattern(app_dir);
    let root_pattern = format!("{app_dir}/root.{{ts,tsx,js,jsx}}");
    let entry_server_pattern = format!("{app_dir}/entry.server.{{ts,tsx,js,jsx}}");

    result.push_entry_pattern(route_dir_pattern.clone());
    result.push_entry_pattern(root_pattern.clone());
    result.push_entry_pattern(format!("{app_dir}/entry.client.{{ts,tsx,js,jsx}}"));
    result.push_entry_pattern(entry_server_pattern.clone());
    result.push_entry_pattern(route_config_pattern.clone());

    result.push_used_export_rule(route_dir_pattern, ROUTE_EXPORTS.iter().copied());
    result.push_used_export_rule(root_pattern, ROOT_EXPORTS.iter().copied());
    result.push_used_export_rule(route_config_pattern, ROUTE_CONFIG_EXPORTS.iter().copied());
    result.push_used_export_rule(entry_server_pattern, ENTRY_SERVER_EXPORTS.iter().copied());
}

/// Credits the imports of the built-in server entry when the app has no
/// authored `entry.server` file. An authored file is an entry point, so its
/// own imports decide which packages are used.
fn add_default_entry_server_dependencies(result: &mut PluginResult, root: &Path, app_dir: &str) {
    let app_path = root.join(app_dir);
    let has_authored_entry = ENTRY_SERVER_EXTENSIONS
        .iter()
        .any(|ext| app_path.join(format!("entry.server.{ext}")).is_file());
    if has_authored_entry {
        return;
    }
    result.referenced_dependencies.extend(
        DEFAULT_ENTRY_SERVER_DEPENDENCIES
            .iter()
            .map(|dep| (*dep).to_string()),
    );
}

fn route_dir_pattern(app_dir: &str) -> String {
    format!("{app_dir}/routes/**/*.{{ts,tsx,js,jsx}}")
}

fn route_config_pattern(app_dir: &str) -> String {
    format!("{app_dir}/routes.{{ts,js,mts,mjs}}")
}

fn collect_route_config_from_disk(result: &mut PluginResult, root: &Path, app_dir: &str) {
    for candidate in route_config_candidates(root, app_dir) {
        if !candidate.is_file() {
            continue;
        }
        let Ok(source) = fs::read_to_string(&candidate) else {
            continue;
        };
        add_referenced_dependencies(result, &source, &candidate);
        collect_route_config_from_source(result, &source, &candidate, root);
        break;
    }
}

fn route_config_candidates(root: &Path, app_dir: &str) -> [PathBuf; 4] {
    [
        root.join(app_dir).join("routes.ts"),
        root.join(app_dir).join("routes.js"),
        root.join(app_dir).join("routes.mts"),
        root.join(app_dir).join("routes.mjs"),
    ]
}

/// Source extensions that a route config module can have. The lookup order
/// follows the TypeScript resolution order.
const ROUTE_CONFIG_MODULE_EXTENSIONS: &[&str] =
    &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

/// The state of one walk over the route config and the local modules that it
/// imports. Route file paths stay relative to the directory of the route
/// config, also when an imported module declares them.
struct RouteWalk<'w> {
    root: &'w Path,
    result: &'w mut PluginResult,
    /// The bindings that the walk followed. The set stops import cycles and
    /// cycles between local variables.
    followed: FxHashSet<FollowedBinding>,
}

impl RouteWalk<'_> {
    /// Records the binding and tells whether the walk sees it for the first time.
    fn first_visit(
        &mut self,
        module: &Path,
        name: &str,
        kind: BindingKind,
        base_dir: &Path,
    ) -> bool {
        self.followed.insert(FollowedBinding {
            module: module.to_path_buf(),
            name: name.to_string(),
            kind,
            base_dir: base_dir.to_path_buf(),
        })
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum BindingKind {
    Export,
    Local,
}

/// A binding of one module, walked with one route base directory. The
/// `relative()` helper can walk the same binding with another directory.
#[derive(PartialEq, Eq, Hash)]
struct FollowedBinding {
    module: PathBuf,
    name: String,
    kind: BindingKind,
    base_dir: PathBuf,
}

/// One parsed module of the route config.
#[derive(Clone, Copy)]
struct RouteModule<'s, 'a> {
    program: &'s Program<'a>,
    path: &'s Path,
}

/// The value of a module export, before the walk resolves it.
enum ExportTarget<'s, 'a> {
    Expression(&'s Expression<'a>),
    Local(&'s str),
    ReExport { source: &'s str, name: &'s str },
}

fn collect_route_config_from_source(
    result: &mut PluginResult,
    source: &str,
    route_config_path: &Path,
    root: &Path,
) {
    let base_dir = route_config_path.parent().unwrap_or(root);
    let mut walk = RouteWalk {
        root,
        result,
        followed: FxHashSet::default(),
    };
    walk.first_visit(route_config_path, "default", BindingKind::Export, base_dir);
    parse_route_module(source, route_config_path, |module| {
        collect_module_export(module, "default", base_dir, &mut walk);
    });
}

fn parse_route_module(source: &str, path: &Path, walk: impl FnOnce(RouteModule<'_, '_>) + Send) {
    let source_type = SourceType::from_path(path).unwrap_or_default();
    fallow_extract::parse_bounded_or_default(source, source_type, |parsed| {
        walk(RouteModule {
            program: &parsed.program,
            path,
        });
    });
}

fn collect_module_export(
    module: RouteModule<'_, '_>,
    export_name: &str,
    base_dir: &Path,
    walk: &mut RouteWalk<'_>,
) {
    for target in find_export_targets(module.program, export_name) {
        match target {
            ExportTarget::Expression(expr) => collect_route_entries(expr, module, base_dir, walk),
            ExportTarget::Local(name) => collect_binding(name, module, base_dir, walk),
            ExportTarget::ReExport { source, name } => {
                follow_module_export(module.path, source, name, base_dir, walk);
            }
        }
    }
}

/// Finds the values that `program` exports as `name`. A function or class
/// default export holds no route entries, so the result is empty for it.
fn find_export_targets<'s, 'a>(
    program: &'s Program<'a>,
    name: &'s str,
) -> Vec<ExportTarget<'s, 'a>> {
    let mut targets = Vec::new();
    let mut star_sources = Vec::new();
    for stmt in &program.body {
        match stmt {
            Statement::ExportDefaultDeclaration(decl) if name == "default" => {
                let Some(expr) = decl.declaration.as_expression() else {
                    continue;
                };
                let expr = strip_wrappers(expr);
                targets.push(match unwrap_identifier_name(expr) {
                    Some(local) => ExportTarget::Local(local),
                    None => ExportTarget::Expression(expr),
                });
            }
            Statement::ExportDeclaration(decl) => {
                if let Declaration::VariableDeclaration(var) = &decl.declaration
                    && let Some(init) = find_declarator_init(var, name)
                {
                    targets.push(ExportTarget::Expression(init));
                }
            }
            Statement::ExportNamedDeclaration(decl) => {
                targets.extend(
                    decl.specifiers
                        .iter()
                        .filter(|specifier| specifier.exported.name() == name)
                        .map(|specifier| ExportTarget::Local(specifier.local.name().as_str())),
                );
            }
            Statement::ExportFromDeclaration(decl) => {
                let Some(source) = decl.source.value.as_str() else {
                    continue;
                };
                targets.extend(
                    decl.specifiers
                        .iter()
                        .filter(|specifier| specifier.exported.name() == name)
                        .map(|specifier| ExportTarget::ReExport {
                            source,
                            name: specifier.local.name().as_str(),
                        }),
                );
            }
            Statement::ExportAllDeclaration(decl)
                if decl.exported.is_none() && name != "default" =>
            {
                star_sources.extend(decl.source.value.as_str());
            }
            _ => {}
        }
    }
    if targets.is_empty() {
        targets.extend(
            star_sources
                .into_iter()
                .map(|source| ExportTarget::ReExport { source, name }),
        );
    }
    targets
}

/// Follows a local binding to its declaration in the module, or to the
/// export of the local module that the binding imports.
fn collect_binding(
    name: &str,
    module: RouteModule<'_, '_>,
    base_dir: &Path,
    walk: &mut RouteWalk<'_>,
) {
    if !walk.first_visit(module.path, name, BindingKind::Local, base_dir) {
        return;
    }
    if let Some(init) = find_variable_init_expression(module.program, name) {
        collect_route_entries(init, module, base_dir, walk);
        return;
    }
    if let Some((source, imported)) = find_import_binding(module.program, name) {
        follow_module_export(module.path, source, imported, base_dir, walk);
    }
}

fn find_import_binding<'s>(program: &'s Program<'_>, local: &str) -> Option<(&'s str, &'s str)> {
    program.body.iter().find_map(|stmt| {
        let Statement::ImportDeclaration(decl) = stmt else {
            return None;
        };
        let imported = decl
            .specifiers
            .as_ref()?
            .iter()
            .find_map(|specifier| match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(spec) if spec.local.name == local => {
                    Some(spec.imported.name().as_str())
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(spec)
                    if spec.local.name == local =>
                {
                    Some("default")
                }
                _ => None,
            })?;
        Some((decl.source.value.as_str()?, imported))
    })
}

/// Parses the local module that `specifier` names and walks its export. The
/// walk follows only relative specifiers to files inside the project root.
fn follow_module_export(
    importer: &Path,
    specifier: &str,
    export_name: &str,
    base_dir: &Path,
    walk: &mut RouteWalk<'_>,
) {
    if !specifier.starts_with('.') {
        return;
    }
    let Some(path) = importer
        .parent()
        .and_then(|dir| resolve_local_module(dir, specifier))
    else {
        return;
    };
    if !path.starts_with(walk.root)
        || !walk.first_visit(&path, export_name, BindingKind::Export, base_dir)
    {
        return;
    }
    let Ok(source) = fs::read_to_string(&path) else {
        return;
    };
    parse_route_module(&source, &path, |module| {
        collect_module_export(module, export_name, base_dir, walk);
    });
}

/// Resolves a relative specifier the way a bundler does: the file itself, the
/// file with a source extension, then the index file of a directory. A `.js`
/// specifier also matches a TypeScript source with the same stem.
fn resolve_local_module(dir: &Path, specifier: &str) -> Option<PathBuf> {
    let base = config_parser::lexical_normalize(&dir.join(specifier));
    if base.is_file() {
        return Some(base);
    }
    let with_extension = ROUTE_CONFIG_MODULE_EXTENSIONS.iter().map(|ext| {
        let mut path = base.clone().into_os_string();
        path.push(".");
        path.push(ext);
        PathBuf::from(path)
    });
    let typescript_source = ["js", "mjs", "cjs"]
        .iter()
        .zip(["ts", "mts", "cts"])
        .filter(|(js, _)| base.extension().is_some_and(|ext| ext == **js))
        .map(|(_, ts)| base.with_extension(ts));
    let index = ROUTE_CONFIG_MODULE_EXTENSIONS
        .iter()
        .map(|ext| base.join(format!("index.{ext}")));
    with_extension
        .chain(typescript_source)
        .chain(index)
        .find(|path| path.is_file())
}

fn find_declarator_init<'s, 'a>(
    decl: &'s VariableDeclaration<'a>,
    name: &str,
) -> Option<&'s Expression<'a>> {
    decl.declarations.iter().find_map(|declarator| {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return None;
        };
        if id.name != name {
            return None;
        }
        declarator.init.as_ref().map(strip_wrappers)
    })
}

fn find_variable_init_expression<'s, 'a>(
    program: &'s Program<'a>,
    name: &str,
) -> Option<&'s Expression<'a>> {
    program.body.iter().find_map(|stmt| match stmt {
        Statement::VariableDeclaration(decl) => find_declarator_init(decl, name),
        Statement::ExportDeclaration(export) => match &export.declaration {
            Declaration::VariableDeclaration(decl) => find_declarator_init(decl, name),
            _ => None,
        },
        _ => None,
    })
}

fn strip_wrappers<'s, 'a>(expr: &'s Expression<'a>) -> &'s Expression<'a> {
    match expr {
        Expression::ParenthesizedExpression(paren) => strip_wrappers(&paren.expression),
        Expression::TSSatisfiesExpression(ts_sat) => strip_wrappers(&ts_sat.expression),
        Expression::TSAsExpression(ts_as) => strip_wrappers(&ts_as.expression),
        _ => expr,
    }
}

fn unwrap_identifier_name<'s>(expr: &'s Expression<'_>) -> Option<&'s str> {
    match strip_wrappers(expr) {
        Expression::Identifier(id) => Some(id.name.as_str()),
        _ => None,
    }
}

fn collect_route_entries(
    expr: &Expression<'_>,
    module: RouteModule<'_, '_>,
    base_dir: &Path,
    walk: &mut RouteWalk<'_>,
) {
    match strip_wrappers(expr) {
        Expression::ArrayExpression(array) => {
            for element in &array.elements {
                let element_expr = match element {
                    ArrayExpressionElement::SpreadElement(spread) => &spread.argument,
                    _ => match element.as_expression() {
                        Some(element_expr) => element_expr,
                        None => continue,
                    },
                };
                collect_route_entries(element_expr, module, base_dir, walk);
            }
        }
        Expression::ObjectExpression(obj) => {
            collect_route_object(obj, module, base_dir, walk);
        }
        Expression::CallExpression(call) => {
            collect_route_call(call, module, base_dir, walk);
        }
        Expression::Identifier(id) => {
            collect_binding(id.name.as_str(), module, base_dir, walk);
        }
        _ => {}
    }
}

fn collect_route_object(
    obj: &ObjectExpression<'_>,
    module: RouteModule<'_, '_>,
    base_dir: &Path,
    walk: &mut RouteWalk<'_>,
) {
    if let Some(file) = config_parser::property_string(obj, "file") {
        add_route_module_file(walk.result, &file, base_dir, walk.root);
    }

    if let Some(children) = config_parser::property_expr(obj, "children") {
        collect_route_entries(children, module, base_dir, walk);
    }
}

fn collect_route_call(
    call: &CallExpression<'_>,
    module: RouteModule<'_, '_>,
    base_dir: &Path,
    walk: &mut RouteWalk<'_>,
) {
    let Some(name) = unwrap_identifier_name(&call.callee) else {
        return;
    };
    let root = walk.root;

    match name {
        "route" => {
            add_argument_module_file(call, 1, base_dir, walk);
            collect_argument_children(call, 2, module, base_dir, walk);
        }
        "index" => add_argument_module_file(call, 0, base_dir, walk),
        "layout" => {
            add_argument_module_file(call, 0, base_dir, walk);
            collect_argument_children(call, 1, module, base_dir, walk);
        }
        "prefix" => collect_argument_children(call, 1, module, base_dir, walk),
        "relative" => {
            let next_base = nth_argument_string(call, 0)
                .and_then(|raw| normalize_route_base_dir(&raw, base_dir, root))
                .unwrap_or_else(|| base_dir.to_path_buf());
            collect_argument_children(call, 1, module, &next_base, walk);
        }
        "flatRoutes" => {
            if let Some(route_dir) = extract_flat_routes_root_dir(call, base_dir, root) {
                add_route_dir_override(walk.result, &route_dir);
            }
        }
        _ => collect_unknown_call_arguments(call, module, base_dir, walk),
    }
}

/// Register the route module file named by the string argument at `index`.
fn add_argument_module_file(
    call: &CallExpression<'_>,
    index: usize,
    base_dir: &Path,
    walk: &mut RouteWalk<'_>,
) {
    if let Some(file) = nth_argument_string(call, index) {
        add_route_module_file(walk.result, &file, base_dir, walk.root);
    }
}

/// Walk the child route entries in the argument at `index`.
fn collect_argument_children(
    call: &CallExpression<'_>,
    index: usize,
    module: RouteModule<'_, '_>,
    base_dir: &Path,
    walk: &mut RouteWalk<'_>,
) {
    if let Some(children) = nth_argument_expression(call, index) {
        collect_route_entries(children, module, base_dir, walk);
    }
}

/// A call with an unknown name can wrap route entries, so walk every
/// argument, including spread arguments.
fn collect_unknown_call_arguments(
    call: &CallExpression<'_>,
    module: RouteModule<'_, '_>,
    base_dir: &Path,
    walk: &mut RouteWalk<'_>,
) {
    for argument in &call.arguments {
        let argument_expr = match argument {
            Argument::SpreadElement(spread) => Some(&spread.argument),
            _ => argument.as_expression(),
        };
        if let Some(argument_expr) = argument_expr {
            collect_route_entries(argument_expr, module, base_dir, walk);
        }
    }
}

fn nth_argument_expression<'a>(
    call: &'a CallExpression<'a>,
    index: usize,
) -> Option<&'a Expression<'a>> {
    call.arguments
        .get(index)?
        .as_expression()
        .map(strip_wrappers)
}

fn nth_argument_string(call: &CallExpression<'_>, index: usize) -> Option<String> {
    expression_string(nth_argument_expression(call, index)?)
}

fn expression_string(expr: &Expression<'_>) -> Option<String> {
    match strip_wrappers(expr) {
        Expression::StringLiteral(lit) => Some(lit.value.text().into_owned()),
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
            tpl.quasis.first().map(|quasi| quasi.value.raw.to_string())
        }
        _ => None,
    }
}

fn extract_flat_routes_root_dir(
    call: &CallExpression<'_>,
    base_dir: &Path,
    root: &Path,
) -> Option<String> {
    let options_expr = nth_argument_expression(call, 0)?;
    let options = config_parser::object_expression(options_expr)?;
    let raw = config_parser::property_string(options, "rootDirectory")?;
    normalize_route_path(&raw, base_dir, root)
}

fn normalize_route_path(raw: &str, base_dir: &Path, root: &Path) -> Option<String> {
    let synthetic_path = base_dir.join("__fallow__.ts");
    config_parser::normalize_config_path(raw, &synthetic_path, root)
}

fn normalize_route_base_dir(raw: &str, base_dir: &Path, root: &Path) -> Option<PathBuf> {
    normalize_route_path(raw, base_dir, root).map(|relative| root.join(relative))
}

/// Route file paths are literal. A segment such as `[id]` must not act as a
/// glob character class, so the pattern escapes it.
fn add_route_module_file(result: &mut PluginResult, raw_path: &str, base_dir: &Path, root: &Path) {
    let Some(path) = normalize_route_path(raw_path, base_dir, root) else {
        return;
    };
    let pattern = globset::escape(&path);
    result.push_entry_pattern(pattern.clone());
    result.push_used_export_rule(pattern, ROUTE_EXPORTS.iter().copied());
}

fn add_route_dir_override(result: &mut PluginResult, route_dir: &str) {
    let pattern = format!("{}/**/*.{{ts,tsx,js,jsx}}", globset::escape(route_dir));
    result.push_entry_pattern(pattern.clone());
    result.push_used_export_rule(pattern, ROUTE_EXPORTS.iter().copied());
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn used_exports_cover_root_and_route_config() {
        let plugin = ReactRouterPlugin;
        let exports = plugin.used_exports();

        assert!(exports.iter().any(|(pattern, names)| {
            pattern == &"app/root.{ts,tsx,js,jsx}" && names.contains(&"Layout")
        }));
        assert!(exports.iter().any(|(pattern, names)| {
            pattern == &"app/routes/**/*.{ts,tsx,js,jsx}"
                && names.contains(&"clientMiddleware")
                && names.contains(&"middleware")
        }));
        assert!(exports.iter().any(|(pattern, names)| {
            pattern == &"app/routes.{ts,js,mts,mjs}" && names == &["default"]
        }));
    }

    #[test]
    fn discovery_hidden_dirs_include_bundle_boundaries() {
        let plugin = ReactRouterPlugin;
        assert_eq!(plugin.discovery_hidden_dirs(), [".client", ".server"]);
    }

    fn has_entry_pattern(result: &PluginResult, pattern: &str) -> bool {
        result
            .entry_patterns
            .iter()
            .any(|entry_pattern| entry_pattern.pattern == pattern)
    }

    #[test]
    fn resolve_config_honors_custom_app_directory() {
        let plugin = ReactRouterPlugin;
        let source = r#"export default { appDirectory: "src" };"#;

        let result = plugin.resolve_config(
            Path::new("/project/react-router.config.ts"),
            source,
            Path::new("/project"),
        );

        assert!(has_entry_pattern(&result, "src/root.{ts,tsx,js,jsx}"));
        assert!(has_entry_pattern(&result, "src/routes.{ts,js,mts,mjs}"));
        assert!(result.used_exports.iter().any(|rule| {
            rule.path.pattern == "src/routes/**/*.{ts,tsx,js,jsx}"
                && rule.exports.iter().any(|export| export == "loader")
        }));
        assert!(result.used_exports.iter().any(|rule| {
            rule.path.pattern == "src/entry.server.{ts,tsx,js,jsx}"
                && rule.exports.iter().any(|export| export == "streamTimeout")
        }));
    }

    #[test]
    fn resolve_config_discovers_route_modules_and_flat_routes() {
        let plugin = ReactRouterPlugin;
        let source = r#"
            import { flatRoutes } from "@react-router/fs-routes";
            import { index, layout, route } from "@react-router/dev/routes";

            export default [
                index("./marketing/home.tsx"),
                layout("./account/layout.tsx", [route("login", "./account/login.tsx")]),
                flatRoutes({ rootDirectory: "file-routes" }),
            ];
        "#;

        let result = plugin.resolve_config(
            Path::new("/project/app/routes.ts"),
            source,
            Path::new("/project"),
        );

        for pattern in [
            "app/marketing/home.tsx",
            "app/account/layout.tsx",
            "app/account/login.tsx",
            "app/file-routes/**/*.{ts,tsx,js,jsx}",
        ] {
            assert!(
                has_entry_pattern(&result, pattern),
                "missing {pattern}: {:?}",
                result.entry_patterns
            );
        }
        assert!(result.used_exports.iter().any(|rule| {
            rule.path.pattern == "app/account/login.tsx"
                && rule.exports.iter().any(|export| export == "clientAction")
        }));
    }

    #[test]
    fn resolve_config_reads_custom_route_file_from_disk() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("web")).unwrap();
        fs::write(
            temp.path().join("web/routes.ts"),
            r#"
                import { index } from "@react-router/dev/routes";
                export default [index("./marketing/home.tsx")];
            "#,
        )
        .unwrap();

        let plugin = ReactRouterPlugin;
        let result = plugin.resolve_config(
            temp.path().join("react-router.config.ts").as_path(),
            r#"export default { appDirectory: "web" };"#,
            temp.path(),
        );

        assert!(has_entry_pattern(&result, "web/marketing/home.tsx"));
    }

    #[test]
    fn resolve_config_follows_star_reexports_and_import_cycles() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("app/config")).unwrap();
        fs::write(
            temp.path().join("app/config/index.ts"),
            r#"export * from "./first";"#,
        )
        .unwrap();
        fs::write(
            temp.path().join("app/config/first.ts"),
            r#"
                import { route } from "@react-router/dev/routes";
                import { second } from "./second.js";
                export const first = [route("first", "./first.tsx"), ...second];
            "#,
        )
        .unwrap();
        fs::write(
            temp.path().join("app/config/second.ts"),
            r#"
                import { index } from "@react-router/dev/routes";
                import { first } from "./first";
                export const second = [index("./second.tsx"), ...first];
            "#,
        )
        .unwrap();

        let plugin = ReactRouterPlugin;
        let result = plugin.resolve_config(
            temp.path().join("app/routes.ts").as_path(),
            r#"
                import { first } from "./config";
                import external from "external-routes";
                export default [...first, ...external];
            "#,
            temp.path(),
        );

        assert!(has_entry_pattern(&result, "app/first.tsx"));
        assert!(has_entry_pattern(&result, "app/second.tsx"));
    }

    #[test]
    fn resolve_config_stops_at_local_variable_cycles() {
        let plugin = ReactRouterPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/app/routes.ts"),
            r#"
                import { route } from "@react-router/dev/routes";
                const first = [...second, route("first", "./first.tsx")];
                const second = [...first];
                export default first;
            "#,
            Path::new("/project"),
        );

        assert!(has_entry_pattern(&result, "app/first.tsx"));
    }

    #[test]
    fn resolve_config_escapes_literal_route_paths() {
        let plugin = ReactRouterPlugin;
        let result = plugin.resolve_config(
            Path::new("/project/app/routes.ts"),
            r#"
                import { route } from "@react-router/dev/routes";
                export default [route(":id", "./items/[id]/page.tsx")];
            "#,
            Path::new("/project"),
        );

        assert!(has_entry_pattern(&result, "app/items/[[]id[]]/page.tsx"));
    }

    #[test]
    fn resolve_config_credits_default_entry_server_dependencies() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("app")).unwrap();

        let plugin = ReactRouterPlugin;
        let result = plugin.resolve_config(
            temp.path().join("app/routes.ts").as_path(),
            "export default [];",
            temp.path(),
        );

        assert!(
            result
                .referenced_dependencies
                .iter()
                .any(|dep| dep == "isbot")
        );
    }

    #[test]
    fn resolve_config_skips_default_entry_server_dependencies_for_authored_entry() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("web")).unwrap();
        fs::write(temp.path().join("web/entry.server.tsx"), "").unwrap();

        let plugin = ReactRouterPlugin;
        let result = plugin.resolve_config(
            temp.path().join("react-router.config.ts").as_path(),
            r#"export default { appDirectory: "web" };"#,
            temp.path(),
        );

        assert!(
            !result
                .referenced_dependencies
                .iter()
                .any(|dep| dep == "isbot")
        );
    }
}
