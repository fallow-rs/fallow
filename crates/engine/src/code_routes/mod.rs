//! Entry routes that source code defines, for the coverage inventory upload.
//!
//! Two route sources need the source text, not only the file path:
//!
//! - An Elysia app defines its routes with calls such as
//!   `app.get("/repos/:repo", handler)`. [`elysia_routes`] reads every file that
//!   imports `elysia`, joins the instance, group and mount prefixes, and gives
//!   each route the file of its handler.
//! - A TanStack Router route file gives its route id in the argument of
//!   `createFileRoute("/repo/$repo")`. [`file_route_id`] reads that argument.
//!
//! The extraction is syntactic. A route with a path or a prefix that is not a
//! string literal stays out of the result.

use std::path::Path;

use fallow_extract::JsText as _;
use fallow_graph::graph::{EffectiveExportResolution, ExportNamespace, ModuleGraph};
use fallow_types::discover::FileId;
use fallow_types::extract::ImportedName;
use oxc_ast::ast::{
    Argument, ArrowFunctionBody, ArrowFunctionExpression, BindingPattern, CallExpression,
    Declaration, ExportDefaultDeclaration, Expression, Function, ImportDeclaration,
    ImportDeclarationSpecifier, ModuleExportName, NewExpression, ObjectPropertyKind, Program,
    Statement, VariableDeclarator,
};
use oxc_ast_visit::{Visit, walk};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::module_graph::RetainedModuleGraph;

/// Package name of the Elysia web framework.
const ELYSIA_PACKAGE: &str = "elysia";

/// Elysia route methods and the HTTP method each one serves. `all` serves any
/// method.
const ELYSIA_METHODS: &[(&str, Option<&str>)] = &[
    ("get", Some("GET")),
    ("head", Some("HEAD")),
    ("post", Some("POST")),
    ("put", Some("PUT")),
    ("patch", Some("PATCH")),
    ("delete", Some("DELETE")),
    ("options", Some("OPTIONS")),
    ("all", None),
];

/// HTTP methods that the method argument of `app.route(method, path, handler)`
/// can name.
const ROUTE_CALL_METHODS: &[&str] = &["GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"];

/// TanStack Router functions whose first argument is the route id.
const FILE_ROUTE_FUNCTIONS: &[&str] = &[
    "createFileRoute",
    "createLazyFileRoute",
    "createServerFileRoute",
];

/// Cap on the mount levels between a route and its root app, so a mount cycle
/// cannot loop.
const MAX_MOUNT_DEPTH: usize = 16;

/// Cap on the mount prefixes of one Elysia instance, so a plugin that many
/// apps mount cannot multiply the routes without bound.
const MAX_PREFIXES_PER_INSTANCE: usize = 32;

/// One route that source code defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeRoute {
    /// The file of the route handler: the route entry.
    pub file_id: FileId,
    /// The full route pattern, with the parameter spelling of the framework.
    pub pattern: String,
    /// The HTTP method, or `None` when the route serves any method.
    pub method: Option<&'static str>,
}

/// The route id in the first `createFileRoute`, `createLazyFileRoute` or
/// `createServerFileRoute` call of `source`, when that argument is a string
/// literal.
#[must_use]
pub fn file_route_id(path: &Path, source: &str) -> Option<String> {
    if !source.contains("FileRoute") {
        return None;
    }
    let source_type = oxc_span::SourceType::from_path(path).unwrap_or_default();
    fallow_extract::parse_bounded_or_default(source, source_type, |parsed| {
        let mut finder = FileRouteFinder::default();
        finder.visit_program(&parsed.program);
        finder.route_id
    })
}

#[derive(Default)]
struct FileRouteFinder {
    route_id: Option<String>,
}

impl<'a> Visit<'a> for FileRouteFinder {
    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if self.route_id.is_some() {
            return;
        }
        if let Expression::Identifier(callee) = &call.callee
            && FILE_ROUTE_FUNCTIONS.contains(&callee.name.as_str())
        {
            self.route_id = call.arguments.first().and_then(literal_argument);
            return;
        }
        walk::walk_call_expression(self, call);
    }
}

/// The Elysia routes of every file in `graph` that imports `elysia`.
///
/// An instance that another instance mounts with `.use()` gets the prefixes of
/// each mount. An instance that no instance mounts is a root app. The routes
/// are sorted by pattern, method and file.
#[must_use]
pub fn elysia_routes(graph: &RetainedModuleGraph) -> Vec<CodeRoute> {
    let graph = graph.as_graph();
    let mut file_ids: Vec<FileId> = graph
        .package_usage
        .get(ELYSIA_PACKAGE)
        .into_iter()
        .chain(graph.type_only_package_usage.get(ELYSIA_PACKAGE))
        .flatten()
        .copied()
        .collect();
    file_ids.sort_unstable_by_key(|id| id.0);
    file_ids.dedup();

    let mut files: FxHashMap<FileId, ElysiaFile> = FxHashMap::default();
    for file_id in file_ids {
        let Some(module) = graph.modules.get(file_id.0 as usize) else {
            continue;
        };
        let Ok(source) = std::fs::read_to_string(&module.path) else {
            continue;
        };
        let parsed = elysia_file(&module.path, &source);
        if !parsed.is_empty() {
            files.insert(file_id, parsed);
        }
    }
    resolve_elysia_routes(graph, &files)
}

/// The Elysia facts of one file. Instance ids are local to the file.
#[derive(Debug, Default, PartialEq, Eq)]
struct ElysiaFile {
    instance_count: usize,
    /// Export name and instance of each exported Elysia instance.
    exports: Vec<(String, usize)>,
    routes: Vec<LocalRoute>,
    mounts: Vec<Mount>,
}

impl ElysiaFile {
    const fn is_empty(&self) -> bool {
        self.instance_count == 0
    }
}

/// A route, with the path relative to the root of its instance.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LocalRoute {
    instance: usize,
    method: Option<&'static str>,
    path: String,
    handler: Reference,
}

/// A `.use(child)` call. `base` is the path of the call site relative to the
/// root of the parent instance, `None` when a prefix on the way is not a
/// string literal.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Mount {
    parent: usize,
    base: Option<String>,
    child: Child,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Child {
    /// An instance of this file.
    Local(usize),
    /// A binding that may come from an import of this file.
    Binding(Reference),
}

/// A value that a route or a mount names.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Reference {
    /// Code of this file, such as an inline arrow function.
    Here,
    /// A binding, optionally with a member (`handlers.getRepo`).
    Name {
        local: String,
        member: Option<String>,
    },
}

/// An Elysia app expression: the instance, plus the path of the expression
/// relative to the root of that instance.
#[derive(Debug, Clone)]
struct AppRef {
    instance: usize,
    base: Option<String>,
}

fn elysia_file(path: &Path, source: &str) -> ElysiaFile {
    let source_type = oxc_span::SourceType::from_path(path).unwrap_or_default();
    fallow_extract::parse_bounded_or_default(source, source_type, |parsed| {
        let mut collector = ElysiaCollector::default();
        collector.visit_program(&parsed.program);
        collector.collect_exports(&parsed.program);
        collector.file
    })
}

#[derive(Default)]
struct ElysiaCollector {
    file: ElysiaFile,
    /// Local names of the `Elysia` class.
    class_names: FxHashSet<String>,
    /// Bindings to app expressions; the last binding of a name wins.
    bindings: Vec<(String, AppRef)>,
}

impl ElysiaCollector {
    fn binding(&self, name: &str) -> Option<&AppRef> {
        self.bindings
            .iter()
            .rev()
            .find_map(|(bound, app)| (bound == name).then_some(app))
    }

    fn collect_exports(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            match statement {
                Statement::ExportDeclaration(export) => {
                    let Declaration::VariableDeclaration(decl) = &export.declaration else {
                        continue;
                    };
                    for declarator in &decl.declarations {
                        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                            self.export_binding(id.name.as_str(), id.name.as_str());
                        }
                    }
                }
                Statement::ExportNamedDeclaration(export) => {
                    for specifier in &export.specifiers {
                        let local = module_export_name(&specifier.local);
                        let exported = module_export_name(&specifier.exported);
                        self.export_binding(&local, &exported);
                    }
                }
                Statement::ExportDefaultDeclaration(export) => {
                    if let Some(Expression::Identifier(id)) = export.declaration.as_expression() {
                        self.export_binding(id.name.as_str(), "default");
                    }
                }
                _ => {}
            }
        }
    }

    fn export_binding(&mut self, local: &str, exported: &str) {
        if let Some(app) = self.binding(local) {
            let instance = app.instance;
            self.push_export(exported, instance);
        }
    }

    fn push_export(&mut self, exported: &str, instance: usize) {
        if !self.file.exports.iter().any(|(name, _)| name == exported) {
            self.file.exports.push((exported.to_owned(), instance));
        }
    }

    /// Evaluate `expr` as an app expression and record the routes and mounts
    /// of its call chain. `None` when `expr` is not an app expression.
    fn app(&mut self, expr: &Expression<'_>) -> Option<AppRef> {
        match expr.get_inner_expression() {
            Expression::NewExpression(new) => self.app_new(new),
            Expression::Identifier(id) => self.binding(id.name.as_str()).cloned(),
            Expression::CallExpression(call) => self.app_call(call),
            _ => None,
        }
    }

    fn app_new(&mut self, new: &NewExpression<'_>) -> Option<AppRef> {
        let Expression::Identifier(callee) = &new.callee else {
            return None;
        };
        if !self.class_names.contains(callee.name.as_str()) {
            return None;
        }
        let instance = self.file.instance_count;
        self.file.instance_count += 1;
        Some(AppRef {
            instance,
            base: constructor_prefix(new.arguments.first()),
        })
    }

    fn app_call(&mut self, call: &CallExpression<'_>) -> Option<AppRef> {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        let app = self.app(&member.object)?;
        let method = member.property.name.as_str();
        if let Some(&(_, http)) = ELYSIA_METHODS.iter().find(|(name, _)| *name == method) {
            self.push_route(&app, http, call.arguments.first(), call.arguments.get(1));
        } else {
            match method {
                "route" => self.route_call(&app, call),
                "group" => self.group_call(&app, call),
                "guard" => {
                    if call.arguments.len() >= 2
                        && let Some(callback) = call.arguments.last()
                    {
                        self.callback_body(callback, app.clone());
                    }
                }
                "use" => {
                    if let Some(argument) = call.arguments.first() {
                        self.use_call(&app, argument);
                    }
                }
                _ => {}
            }
        }
        Some(app)
    }

    fn route_call(&mut self, app: &AppRef, call: &CallExpression<'_>) {
        let Some(method) = call.arguments.first().and_then(literal_argument) else {
            return;
        };
        let upper = method.to_ascii_uppercase();
        let Some(http) = ROUTE_CALL_METHODS.iter().find(|m| **m == upper) else {
            return;
        };
        self.push_route(
            app,
            Some(http),
            call.arguments.get(1),
            call.arguments.get(2),
        );
    }

    fn push_route(
        &mut self,
        app: &AppRef,
        method: Option<&'static str>,
        path: Option<&Argument<'_>>,
        handler: Option<&Argument<'_>>,
    ) {
        let (Some(base), Some(path)) = (&app.base, path.and_then(literal_argument)) else {
            return;
        };
        self.file.routes.push(LocalRoute {
            instance: app.instance,
            method,
            path: join_path(base, &path),
            handler: handler.map_or(Reference::Here, reference),
        });
    }

    fn group_call(&mut self, app: &AppRef, call: &CallExpression<'_>) {
        let Some(callback) = call.arguments.last() else {
            return;
        };
        if call.arguments.len() < 2 {
            return;
        }
        let prefix = call.arguments.first().and_then(literal_argument);
        let base = match (&app.base, prefix) {
            (Some(base), Some(prefix)) => Some(join_path(base, &prefix)),
            _ => None,
        };
        self.callback_body(
            callback,
            AppRef {
                instance: app.instance,
                base,
            },
        );
    }

    fn use_call(&mut self, app: &AppRef, argument: &Argument<'_>) {
        let child = match argument {
            Argument::Identifier(id) => match self.binding(id.name.as_str()) {
                Some(child) => Child::Local(child.instance),
                None => Child::Binding(reference(argument)),
            },
            Argument::StaticMemberExpression(_) => Child::Binding(reference(argument)),
            Argument::ArrowFunctionExpression(_) | Argument::FunctionExpression(_) => {
                // A functional plugin receives the parent app.
                self.callback_body(argument, app.clone());
                return;
            }
            _ => {
                let Some(expr) = argument.as_expression() else {
                    return;
                };
                let Some(child) = self.app(expr) else {
                    return;
                };
                Child::Local(child.instance)
            }
        };
        self.file.mounts.push(Mount {
            parent: app.instance,
            base: app.base.clone(),
            child,
        });
    }

    /// Evaluate the body of a callback whose first parameter is `app`.
    fn callback_body(&mut self, callback: &Argument<'_>, app: AppRef) {
        let depth = self.bindings.len();
        match callback {
            Argument::ArrowFunctionExpression(arrow) => {
                if let Some(name) = first_param_name_arrow(arrow) {
                    self.bindings.push((name, app));
                }
                match &arrow.body {
                    ArrowFunctionBody::FunctionBody(body) => self.visit_function_body(body),
                    body => {
                        if let Some(expr) = body.as_expression() {
                            self.visit_expression(expr);
                        }
                    }
                }
            }
            Argument::FunctionExpression(function) => {
                if let Some(name) = first_param_name_function(function) {
                    self.bindings.push((name, app));
                }
                if let Some(body) = &function.body {
                    self.visit_function_body(body);
                }
            }
            _ => {}
        }
        self.bindings.truncate(depth);
    }
}

impl<'a> Visit<'a> for ElysiaCollector {
    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        if decl.source.value.text() != ELYSIA_PACKAGE {
            return;
        }
        for specifier in decl.specifiers.iter().flatten() {
            match specifier {
                ImportDeclarationSpecifier::ImportSpecifier(specifier)
                    if specifier.imported.name().as_str() == "Elysia" =>
                {
                    self.class_names.insert(specifier.local.name.to_string());
                }
                ImportDeclarationSpecifier::ImportDefaultSpecifier(specifier) => {
                    self.class_names.insert(specifier.local.name.to_string());
                }
                _ => {}
            }
        }
    }

    fn visit_variable_declarator(&mut self, declarator: &VariableDeclarator<'a>) {
        let Some(init) = &declarator.init else {
            return;
        };
        match self.app(init) {
            Some(app) => {
                if let BindingPattern::BindingIdentifier(id) = &declarator.id {
                    self.bindings.push((id.name.to_string(), app));
                }
            }
            None => walk::walk_variable_declarator(self, declarator),
        }
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if self.app_call(call).is_none() {
            walk::walk_call_expression(self, call);
        }
    }

    fn visit_new_expression(&mut self, new: &NewExpression<'a>) {
        if self.app_new(new).is_none() {
            walk::walk_new_expression(self, new);
        }
    }

    fn visit_export_default_declaration(&mut self, decl: &ExportDefaultDeclaration<'a>) {
        let Some(expr) = decl.declaration.as_expression() else {
            walk::walk_export_default_declaration(self, decl);
            return;
        };
        if matches!(expr, Expression::Identifier(_)) {
            return;
        }
        match self.app(expr) {
            Some(app) => self.push_export("default", app.instance),
            None => walk::walk_export_default_declaration(self, decl),
        }
    }
}

/// The prefix of `new Elysia(options)`: `Some("")` without a `prefix` option,
/// `None` when the options or the prefix are not literal.
fn constructor_prefix(options: Option<&Argument<'_>>) -> Option<String> {
    let Some(options) = options else {
        return Some(String::new());
    };
    let Argument::ObjectExpression(object) = options else {
        return None;
    };
    for property in &object.properties {
        match property {
            ObjectPropertyKind::ObjectProperty(property) => {
                if property
                    .key
                    .static_name()
                    .is_some_and(|name| name.text() == "prefix")
                {
                    return literal_expression(&property.value)
                        .map(|prefix| join_path("", &prefix));
                }
            }
            ObjectPropertyKind::SpreadProperty(_) => return None,
        }
    }
    Some(String::new())
}

/// Join a path to a base the way Elysia does: a path without a leading `/`
/// gets one.
fn join_path(base: &str, path: &str) -> String {
    if path.is_empty() || path.starts_with('/') {
        format!("{base}{path}")
    } else {
        format!("{base}/{path}")
    }
}

/// The final form of a joined pattern: `/` for an empty path, and no trailing
/// `/` after a segment.
fn finish_pattern(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        "/".to_owned()
    } else if trimmed.starts_with('/') {
        trimmed.to_owned()
    } else {
        format!("/{trimmed}")
    }
}

fn literal_argument(argument: &Argument<'_>) -> Option<String> {
    argument.as_expression().and_then(literal_expression)
}

/// The text of a string literal or of a template literal without
/// substitutions.
fn literal_expression(expr: &Expression<'_>) -> Option<String> {
    match expr.get_inner_expression() {
        Expression::StringLiteral(literal) => Some(literal.value.text().into_owned()),
        Expression::TemplateLiteral(template) if template.expressions.is_empty() => template
            .quasis
            .first()
            .map(|quasi| quasi.value.raw.to_string()),
        _ => None,
    }
}

fn reference(argument: &Argument<'_>) -> Reference {
    let Some(expr) = argument.as_expression() else {
        return Reference::Here;
    };
    match expr.get_inner_expression() {
        Expression::Identifier(id) => Reference::Name {
            local: id.name.to_string(),
            member: None,
        },
        Expression::StaticMemberExpression(member) => match &member.object {
            Expression::Identifier(object) => Reference::Name {
                local: object.name.to_string(),
                member: Some(member.property.name.to_string()),
            },
            _ => Reference::Here,
        },
        _ => Reference::Here,
    }
}

fn first_param_name_arrow(arrow: &ArrowFunctionExpression<'_>) -> Option<String> {
    binding_name(&arrow.params.items.first()?.pattern)
}

fn first_param_name_function(function: &Function<'_>) -> Option<String> {
    binding_name(&function.params.items.first()?.pattern)
}

fn binding_name(pattern: &BindingPattern<'_>) -> Option<String> {
    match pattern {
        BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
        _ => None,
    }
}

fn module_export_name(name: &ModuleExportName<'_>) -> String {
    match name {
        ModuleExportName::IdentifierName(id) => id.name.to_string(),
        ModuleExportName::IdentifierReference(id) => id.name.to_string(),
        ModuleExportName::StringLiteral(literal) => literal.value.text().into_owned(),
    }
}

/// A global Elysia instance: the file and the local instance id.
type InstanceKey = (FileId, usize);

/// Resolve the mounts across files and build the full route patterns.
fn resolve_elysia_routes(
    graph: &ModuleGraph,
    files: &FxHashMap<FileId, ElysiaFile>,
) -> Vec<CodeRoute> {
    let imports: FxHashMap<FileId, FxHashMap<String, (FileId, ImportedName)>> = files
        .keys()
        .map(|&file_id| (file_id, import_bindings(graph, file_id)))
        .collect();
    let resolve = |file_id: FileId, reference: &Reference| -> Option<(FileId, Option<String>)> {
        let Reference::Name { local, member } = reference else {
            return None;
        };
        let (target, imported) = imports.get(&file_id)?.get(local)?;
        let name = match imported {
            ImportedName::Named(name) => name.clone(),
            ImportedName::Default => "default".to_owned(),
            ImportedName::Namespace => member.clone()?,
            ImportedName::SideEffect => return None,
        };
        let origin = match graph.resolve_export(*target, &name, ExportNamespace::Value) {
            EffectiveExportResolution::Unique(binding) => binding.origin_file(),
            EffectiveExportResolution::Missing | EffectiveExportResolution::Ambiguous => *target,
        };
        Some((origin, Some(name)))
    };

    let mut parents: FxHashMap<InstanceKey, Vec<(InstanceKey, Option<String>)>> =
        FxHashMap::default();
    for (&file_id, file) in files {
        for mount in &file.mounts {
            let child = match &mount.child {
                Child::Local(instance) => Some((file_id, *instance)),
                Child::Binding(reference) => {
                    resolve(file_id, reference).and_then(|(origin, name)| {
                        let name = name?;
                        files
                            .get(&origin)?
                            .exports
                            .iter()
                            .find_map(|(export, instance)| {
                                (*export == name).then_some((origin, *instance))
                            })
                    })
                }
            };
            if let Some(child) = child {
                parents
                    .entry(child)
                    .or_default()
                    .push(((file_id, mount.parent), mount.base.clone()));
            }
        }
    }

    let mut prefixes: FxHashMap<InstanceKey, Vec<String>> = FxHashMap::default();
    let mut visiting: FxHashSet<InstanceKey> = FxHashSet::default();
    let mut routes = Vec::new();
    let mut file_ids: Vec<&FileId> = files.keys().collect();
    file_ids.sort_unstable_by_key(|id| id.0);
    for &file_id in file_ids {
        let file = &files[&file_id];
        for route in &file.routes {
            let key = (file_id, route.instance);
            let instance_prefixes = mount_prefixes(key, &parents, &mut prefixes, &mut visiting);
            let handler_file =
                resolve(file_id, &route.handler).map_or(file_id, |(origin, _)| origin);
            for prefix in instance_prefixes {
                routes.push(CodeRoute {
                    file_id: handler_file,
                    pattern: finish_pattern(&format!("{prefix}{}", route.path)),
                    method: route.method,
                });
            }
        }
    }
    routes.sort_by(|a, b| {
        a.pattern
            .cmp(&b.pattern)
            .then_with(|| a.method.cmp(&b.method))
            .then_with(|| a.file_id.0.cmp(&b.file_id.0))
    });
    routes.dedup();
    routes
}

/// The path prefixes under which an instance serves its routes: `[""]` for a
/// root app, one prefix per mount path otherwise. A mount under a prefix that
/// is not literal adds no prefix.
///
/// `memo` keeps the prefixes of each instance, so each instance is walked once.
/// Without it, a file with many mounts on each level makes a number of walks
/// that grows exponentially with the depth. `visiting` holds the instances on
/// the current mount chain: a mount cycle and a chain longer than
/// `MAX_MOUNT_DEPTH` add no prefix.
fn mount_prefixes(
    key: InstanceKey,
    parents: &FxHashMap<InstanceKey, Vec<(InstanceKey, Option<String>)>>,
    memo: &mut FxHashMap<InstanceKey, Vec<String>>,
    visiting: &mut FxHashSet<InstanceKey>,
) -> Vec<String> {
    if let Some(known) = memo.get(&key) {
        return known.clone();
    }
    let Some(mounts) = parents.get(&key) else {
        return vec![String::new()];
    };
    if visiting.len() >= MAX_MOUNT_DEPTH || !visiting.insert(key) {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    'mounts: for (parent, base) in mounts {
        let Some(base) = base else {
            continue;
        };
        for prefix in mount_prefixes(*parent, parents, memo, visiting) {
            let joined = format!("{prefix}{base}");
            if !out.contains(&joined) {
                out.push(joined);
                if out.len() == MAX_PREFIXES_PER_INSTANCE {
                    break 'mounts;
                }
            }
        }
    }
    visiting.remove(&key);
    memo.insert(key, out.clone());
    out
}

/// Local binding name to the imported file and name, for the value imports of
/// `file_id`.
fn import_bindings(
    graph: &ModuleGraph,
    file_id: FileId,
) -> FxHashMap<String, (FileId, ImportedName)> {
    let mut out = FxHashMap::default();
    for (target, symbols) in graph.outgoing_symbol_edges(file_id) {
        for symbol in symbols.iter().filter(|symbol| !symbol.is_type_only) {
            out.entry(symbol.local_name.clone())
                .or_insert_with(|| (target, symbol.imported_name.clone()));
        }
    }
    out
}

#[cfg(test)]
mod tests;
