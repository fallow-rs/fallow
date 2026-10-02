#[allow(
    clippy::wildcard_imports,
    reason = "package-resolution helpers use many AST node shapes"
)]
use oxc_ast::ast::*;
use oxc_ast_visit::{Visit, walk};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::DynamicImportInfo;
use fallow_types::extract::ImportLoadKind;

use super::super::ModuleInfoExtractor;
use super::{
    StaticPackageLoopBindings, for_of_binding_name, object_values_or_entries_argument_name,
};

/// The name of the CommonJS `require` function.
const REQUIRE: &str = "require";

/// Whether `expr` is `require.resolve`, by syntax only. The caller checks
/// that no local binding shadows `require`.
fn is_require_resolve_callee(expr: &Expression<'_>) -> bool {
    let Expression::StaticMemberExpression(member) = expr else {
        return false;
    };
    let Expression::Identifier(object) = &member.object else {
        return false;
    };
    object.name == REQUIRE && member.property.name == "resolve"
}

/// Whether `expr` is `import.meta.resolve`. Code cannot rebind `import.meta`,
/// so no shadow check applies.
fn is_import_meta_resolve_callee(expr: &Expression<'_>) -> bool {
    let Expression::StaticMemberExpression(member) = expr else {
        return false;
    };
    matches!(member.object, Expression::ImportMeta(_)) && member.property.name == "resolve"
}

/// The value of a string literal or of a template literal without expressions.
fn static_string_argument<'a>(argument: &'a Argument<'_>) -> Option<&'a str> {
    match argument {
        Argument::StringLiteral(lit) => Some(lit.value.as_str()),
        Argument::TemplateLiteral(tpl) if tpl.expressions.is_empty() => tpl
            .quasis
            .first()
            .and_then(|quasi| quasi.value.cooked.as_ref())
            .map(|cooked| cooked.as_str()),
        _ => None,
    }
}

/// Whether the program has top-level import or export syntax, the rule that
/// TypeScript uses to treat a file as a module and not as a script.
/// `export as namespace` alone does not make a module file.
pub(super) fn program_has_module_syntax(program: &Program<'_>) -> bool {
    program.body.iter().any(|statement| match statement {
        Statement::TSNamespaceExportDeclaration(_) => false,
        Statement::TSImportEqualsDeclaration(decl) => matches!(
            decl.module_reference,
            TSModuleReference::ExternalModuleReference(_)
        ),
        _ => statement.is_module_declaration(),
    })
}

fn package_from_resolution_specifier(specifier: &str) -> Option<String> {
    if !is_package_resolution_specifier(specifier) {
        return None;
    }
    let package_name = package_name_from_specifier(specifier)?;
    let suffix = specifier
        .strip_prefix(package_name.as_str())
        .unwrap_or_default();
    (suffix.is_empty() || suffix == "/package.json").then_some(package_name)
}

fn is_package_resolution_specifier(specifier: &str) -> bool {
    if specifier.is_empty()
        || specifier.starts_with('.')
        || specifier.starts_with('/')
        || specifier.starts_with('#')
        || specifier.starts_with('$')
        || specifier.contains('\\')
        || specifier.contains(' ')
        || specifier.contains('?')
        || specifier.contains('!')
        || specifier.contains(':')
    {
        return false;
    }
    specifier
        .bytes()
        .any(|b| b.is_ascii_alphabetic() || b == b'@')
}

fn package_name_from_specifier(specifier: &str) -> Option<String> {
    if specifier.starts_with('@') {
        let mut parts = specifier.split('/');
        let scope = parts.next()?;
        let package = parts.next()?;
        if package.is_empty() {
            return None;
        }
        return Some(format!("{scope}/{package}"));
    }

    specifier
        .split('/')
        .next()
        .filter(|name| !name.is_empty())
        .map(str::to_string)
}

fn package_values_from_raw_values(values: &[String]) -> Vec<String> {
    values
        .iter()
        .filter_map(|value| package_from_resolution_specifier(value))
        .collect()
}

fn static_object_string_property_values(
    obj: &ObjectExpression<'_>,
) -> FxHashMap<String, Vec<String>> {
    let mut values = FxHashMap::default();
    collect_static_object_string_property_values(obj, &mut values);
    values
}

fn collect_static_object_string_property_values(
    obj: &ObjectExpression<'_>,
    values: &mut FxHashMap<String, Vec<String>>,
) {
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        let Some(key_name) = prop.key.static_name() else {
            continue;
        };
        match &prop.value {
            Expression::StringLiteral(lit) => {
                values
                    .entry(key_name.to_string())
                    .or_default()
                    .push(lit.value.to_string());
            }
            Expression::ObjectExpression(child) => {
                collect_static_object_string_property_values(child, values);
            }
            _ => {}
        }
    }
}

/// The path segment that leads to the installed binaries of a package manager.
const NODE_MODULES_BIN: &str = "node_modules/.bin/";

/// The binary names in each `node_modules/.bin/<name>` path segment of `text`.
///
/// The segment must start the text or follow a character that cannot be part
/// of a directory name, such as `/`, a space or a quote. The name ends at the
/// first character that a binary name does not use, such as `/`, a space or a
/// quote. A segment without a name gives nothing.
fn bin_names_in_text(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices(NODE_MODULES_BIN)
        .filter(|(start, _)| {
            text[..*start]
                .chars()
                .next_back()
                .is_none_or(|prev| !is_bin_name_char(prev))
        })
        .filter_map(|(start, _)| {
            let rest = &text[start + NODE_MODULES_BIN.len()..];
            let end = rest
                .find(|ch: char| !is_bin_name_char(ch))
                .unwrap_or(rest.len());
            let name = &rest[..end];
            (!name.is_empty()).then_some(name)
        })
}

fn is_bin_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '+' | '@')
}

impl ModuleInfoExtractor {
    /// Whether `callee` is `require.resolve` on the `require` of the module.
    ///
    /// A parameter or a declaration named `require` in a nested scope, such
    /// as `function f(require) { require.resolve('./x') }`, is some other
    /// function, so its calls reference nothing. A module-level
    /// `const require = createRequire(import.meta.url)` stays the module
    /// `require`: it resolves relative to the same file.
    fn is_module_require_resolve(&self, callee: &Expression<'_>) -> bool {
        is_require_resolve_callee(callee) && !self.nested_scope_shadows(REQUIRE)
    }

    pub(super) fn record_static_package_values(&mut self, name: &str, init: &Expression<'_>) {
        match init {
            Expression::StringLiteral(lit) => {
                self.static_string_bindings
                    .insert(name.to_string(), lit.value.to_string());
            }
            Expression::ArrayExpression(array) => {
                let values: Vec<String> = array
                    .elements
                    .iter()
                    .filter_map(|element| match element {
                        ArrayExpressionElement::StringLiteral(lit) => Some(lit.value.to_string()),
                        _ => None,
                    })
                    .collect();
                if !values.is_empty() {
                    self.static_string_arrays.insert(name.to_string(), values);
                }
            }
            Expression::ObjectExpression(obj) => {
                let values = static_object_string_property_values(obj);
                if !values.is_empty() {
                    self.static_object_property_values
                        .insert(name.to_string(), values);
                }
            }
            _ => {}
        }
    }

    pub(super) fn try_record_package_path_reference(&mut self, call: &CallExpression<'_>) {
        if (self.is_module_require_resolve(&call.callee)
            || is_import_meta_resolve_callee(&call.callee))
            && let Some(arg) = call.arguments.first()
        {
            let references = self.package_references_from_argument(arg);
            self.push_package_path_references(references);
            self.try_record_package_resolve_site(call);
        }

        if let Expression::Identifier(callee) = &call.callee
            && let Some(arg_index) = self
                .package_resolution_function_args
                .get(callee.name.as_str())
                .copied()
            && let Some(arg) = call.arguments.get(arg_index)
        {
            let references = self.package_references_from_argument(arg);
            self.push_package_path_references(references);
        }
    }

    /// Record the site of a direct `require.resolve('pkg')` call.
    ///
    /// The argument is a string literal or a template literal without
    /// expressions, so the call names the package at a known location. A call
    /// with a second argument (the `paths` option) resolves from other
    /// directories and gets no site. Names from resolver functions, loop
    /// bindings and static tables also get no site: they only credit the
    /// dependency.
    fn try_record_package_resolve_site(&mut self, call: &CallExpression<'_>) {
        if call.arguments.len() != 1 {
            return;
        }
        let Some(package_name) = call
            .arguments
            .first()
            .and_then(static_string_argument)
            .and_then(package_from_resolution_specifier)
        else {
            return;
        };
        self.package_resolve_sites.push((package_name, call.span));
    }

    /// Record `require.resolve('./file')` as a reference to a project file.
    ///
    /// The call returns a path, and code hands that path to a consumer that
    /// static analysis cannot follow, such as a webpack module replacement or
    /// a worker. That consumer uses the whole module, so the edge credits every
    /// export. The call does not load the module, so the edge is a path
    /// reference and never closes a cycle.
    ///
    /// The argument is a string literal or a template literal without
    /// expressions. A call with a second argument (the `paths` option)
    /// resolves from other directories, so it is not recorded. The reference
    /// is speculative: a target that is not on disk, such as build output or a
    /// native addon, is dropped and does not become an unresolved import.
    pub(super) fn try_record_relative_require_resolve(&mut self, call: &CallExpression<'_>) {
        if !self.is_module_require_resolve(&call.callee) || call.arguments.len() != 1 {
            return;
        }
        let Some(source) = call.arguments.first().and_then(static_string_argument) else {
            return;
        };
        if !(source.starts_with("./") || source.starts_with("../")) {
            return;
        }
        self.dynamic_imports.push(DynamicImportInfo {
            source: source.to_string(),
            span: call.span,
            destructured_names: Vec::new(),
            local_name: Some(String::new()),
            is_speculative: true,
        });
        self.mark_import_load_kind(call.span, ImportLoadKind::PathReference);
    }

    /// Record the package that a module augmentation names.
    ///
    /// TypeScript treats `declare module 'pkg' { ... }` as an augmentation
    /// only in a module file, and there `pkg` must resolve. So the declaration
    /// is a type-only use of the package. In a script file the same syntax
    /// declares an ambient module, which uses nothing. A wildcard pattern such
    /// as `'*.svg'` and a relative path name no package.
    pub(super) fn record_module_augmentation(&mut self, decl: &TSExternalModuleDeclaration<'_>) {
        if !self.is_module_file || self.ambient_module_depth > 0 || decl.body.is_none() {
            return;
        }
        let specifier = decl.id.value.as_str();
        if specifier.contains('*') || !is_package_resolution_specifier(specifier) {
            return;
        }
        if let Some(package_name) = package_name_from_specifier(specifier)
            && !self.type_package_references.contains(&package_name)
        {
            self.type_package_references.push(package_name);
        }
    }

    /// Record the binary name of each `node_modules/.bin/<name>` path in the
    /// text of a string literal or a template quasi.
    ///
    /// Code hands such a path to a consumer that static analysis cannot
    /// follow, such as a child process. The analysis maps the name to the
    /// package that declares the binary and credits that package.
    pub(super) fn record_bin_path_references(&mut self, text: &str) {
        for name in bin_names_in_text(text) {
            if !self.bin_path_references.iter().any(|known| known == name) {
                self.bin_path_references.push(name.to_string());
            }
        }
    }

    fn push_package_path_references(&mut self, references: Vec<String>) {
        for package_name in references {
            if !self.package_path_references.contains(&package_name) {
                self.package_path_references.push(package_name);
            }
        }
    }

    fn package_references_from_argument(&self, arg: &Argument<'_>) -> Vec<String> {
        match arg {
            Argument::StringLiteral(lit) => package_from_resolution_specifier(lit.value.as_str())
                .into_iter()
                .collect(),
            Argument::TemplateLiteral(tpl) => self.package_references_from_template(tpl),
            Argument::Identifier(ident) => self.package_values_for_identifier(&ident.name),
            Argument::StaticMemberExpression(member) => {
                self.package_values_for_static_member(member)
            }
            _ => arg.as_expression().map_or_else(Vec::new, |expr| {
                self.package_references_from_expression(expr)
            }),
        }
    }

    fn package_references_from_expression(&self, expr: &Expression<'_>) -> Vec<String> {
        match expr {
            Expression::StringLiteral(lit) => package_from_resolution_specifier(lit.value.as_str())
                .into_iter()
                .collect(),
            Expression::TemplateLiteral(tpl) => self.package_references_from_template(tpl),
            Expression::Identifier(ident) => self.package_values_for_identifier(&ident.name),
            Expression::StaticMemberExpression(member) => {
                self.package_values_for_static_member(member)
            }
            _ => Vec::new(),
        }
    }

    fn package_references_from_template(&self, tpl: &TemplateLiteral<'_>) -> Vec<String> {
        if tpl.expressions.is_empty() {
            return tpl
                .quasis
                .first()
                .and_then(|quasi| package_from_resolution_specifier(quasi.value.raw.as_str()))
                .into_iter()
                .collect();
        }

        if tpl.expressions.len() != 1 || tpl.quasis.len() != 2 {
            return Vec::new();
        }

        let Some(first) = tpl.quasis.first() else {
            return Vec::new();
        };
        let Some(last) = tpl.quasis.last() else {
            return Vec::new();
        };
        if !first.value.raw.is_empty() || last.value.raw.as_str() != "/package.json" {
            return Vec::new();
        }

        self.package_references_from_expression(&tpl.expressions[0])
    }

    fn package_values_for_identifier(&self, name: &str) -> Vec<String> {
        for scope in self.loop_string_bindings.iter().rev() {
            if let Some(values) = scope.get(name) {
                return package_values_from_raw_values(values);
            }
        }

        self.static_string_bindings
            .get(name)
            .map_or_else(Vec::new, |value| {
                package_from_resolution_specifier(value)
                    .into_iter()
                    .collect()
            })
    }

    fn package_values_for_static_member(&self, member: &StaticMemberExpression<'_>) -> Vec<String> {
        let Expression::Identifier(object) = &member.object else {
            return Vec::new();
        };
        let property = member.property.name.as_str();

        for scope in self.loop_object_property_values.iter().rev() {
            if let Some(properties) = scope.get(object.name.as_str())
                && let Some(values) = properties.get(property)
            {
                return package_values_from_raw_values(values);
            }
        }

        self.static_object_property_values
            .get(object.name.as_str())
            .and_then(|properties| properties.get(property))
            .map_or_else(Vec::new, |values| package_values_from_raw_values(values))
    }

    pub(super) fn static_package_loop_bindings(
        &self,
        stmt: &ForOfStatement<'_>,
    ) -> Option<StaticPackageLoopBindings> {
        let loop_name = for_of_binding_name(&stmt.left)?;
        let mut strings = FxHashMap::default();
        let mut objects = FxHashMap::default();

        if let Expression::Identifier(iterable) = &stmt.right
            && let Some(values) = self.static_string_arrays.get(iterable.name.as_str())
        {
            strings.insert(loop_name.clone(), values.clone());
        }

        if let Some(object_name) = object_values_or_entries_argument_name(&stmt.right)
            && let Some(properties) = self.static_object_property_values.get(&object_name)
        {
            objects.insert(loop_name, properties.clone());
        }

        (!strings.is_empty() || !objects.is_empty()).then_some((strings, objects))
    }
}

pub(super) fn package_resolution_arg_index(
    params: &FormalParameters<'_>,
    body: &FunctionBody<'_>,
    known_helpers: &FxHashMap<String, usize>,
) -> Option<usize> {
    let param_names: Vec<String> = params
        .items
        .iter()
        .filter_map(|param| match &param.pattern {
            BindingPattern::BindingIdentifier(id) => Some(id.name.to_string()),
            _ => None,
        })
        .collect();
    let param_set: FxHashSet<String> = param_names.iter().cloned().collect();
    if params_bind_require(params) {
        return None;
    }
    let mut collector = PackageResolutionParamCollector {
        params: &param_set,
        known_helpers,
        matched: FxHashSet::default(),
        require_shadow_depth: 0,
    };
    collector.visit_function_body(body);

    param_names
        .iter()
        .position(|name| collector.matched.contains(name))
}

/// Whether a parameter list binds the name `require`, also through a
/// destructuring pattern.
fn params_bind_require(params: &FormalParameters<'_>) -> bool {
    params.items.iter().any(|param| {
        param
            .pattern
            .get_binding_identifiers()
            .iter()
            .any(|id| id.name == REQUIRE)
    })
}

struct PackageResolutionParamCollector<'p> {
    params: &'p FxHashSet<String>,
    known_helpers: &'p FxHashMap<String, usize>,
    matched: FxHashSet<String>,
    /// How many enclosing nested functions bind a `require` parameter. Inside
    /// such a function, `require.resolve` is not the module `require`.
    require_shadow_depth: usize,
}

impl<'a> Visit<'a> for PackageResolutionParamCollector<'_> {
    fn visit_function(&mut self, func: &Function<'a>, flags: oxc_semantic::ScopeFlags) {
        let shadows = params_bind_require(&func.params);
        self.require_shadow_depth += usize::from(shadows);
        walk::walk_function(self, func, flags);
        self.require_shadow_depth -= usize::from(shadows);
    }

    fn visit_arrow_function_expression(&mut self, expr: &ArrowFunctionExpression<'a>) {
        let shadows = params_bind_require(&expr.params);
        self.require_shadow_depth += usize::from(shadows);
        walk::walk_arrow_function_expression(self, expr);
        self.require_shadow_depth -= usize::from(shadows);
    }

    fn visit_call_expression(&mut self, call: &CallExpression<'a>) {
        if self.require_shadow_depth == 0
            && is_require_resolve_callee(&call.callee)
            && let Some(arg) = call.arguments.first()
            && let Some(param) = package_resolution_param_from_argument(arg, self.params)
        {
            self.matched.insert(param);
        }

        if call_joins_node_modules_with_param(call, self.params)
            && let Some(param) = call
                .arguments
                .iter()
                .find_map(|arg| package_param_argument_identifier_name(arg, self.params))
        {
            self.matched.insert(param);
        }

        if let Expression::Identifier(callee) = &call.callee
            && let Some(arg_index) = self.known_helpers.get(callee.name.as_str()).copied()
            && let Some(arg) = call.arguments.get(arg_index)
            && let Some(param) = package_param_argument_identifier_name(arg, self.params)
        {
            self.matched.insert(param);
        }

        walk::walk_call_expression(self, call);
    }
}

fn package_resolution_param_from_argument(
    arg: &Argument<'_>,
    params: &FxHashSet<String>,
) -> Option<String> {
    match arg {
        Argument::Identifier(ident) if params.contains(ident.name.as_str()) => {
            Some(ident.name.to_string())
        }
        Argument::TemplateLiteral(tpl)
            if tpl.expressions.len() == 1
                && tpl.quasis.len() == 2
                && tpl.quasis.first()?.value.raw.is_empty()
                && tpl.quasis.last()?.value.raw.as_str() == "/package.json" =>
        {
            package_param_expression_identifier_name(&tpl.expressions[0], params)
        }
        _ => arg
            .as_expression()
            .and_then(|expr| package_param_expression_identifier_name(expr, params)),
    }
}

fn call_joins_node_modules_with_param(
    call: &CallExpression<'_>,
    params: &FxHashSet<String>,
) -> bool {
    let has_node_modules = call.arguments.iter().any(
        |arg| matches!(arg, Argument::StringLiteral(lit) if lit.value.as_str() == "node_modules"),
    );
    has_node_modules
        && call
            .arguments
            .iter()
            .any(|arg| package_param_argument_identifier_name(arg, params).is_some())
}

fn package_param_argument_identifier_name(
    arg: &Argument<'_>,
    params: &FxHashSet<String>,
) -> Option<String> {
    match arg {
        Argument::Identifier(ident) if params.contains(ident.name.as_str()) => {
            Some(ident.name.to_string())
        }
        _ => arg
            .as_expression()
            .and_then(|expr| package_param_expression_identifier_name(expr, params)),
    }
}

fn package_param_expression_identifier_name(
    expr: &Expression<'_>,
    params: &FxHashSet<String>,
) -> Option<String> {
    match expr {
        Expression::Identifier(ident) if params.contains(ident.name.as_str()) => {
            Some(ident.name.to_string())
        }
        _ => None,
    }
}
