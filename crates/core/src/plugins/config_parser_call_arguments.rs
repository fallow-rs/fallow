//! Find the call arguments that a called function can change.
//!
//! A config can change a value in a helper, as in `addPlugins(config)` or
//! `_.merge(config, extra)`. The inline passes do not copy a value that such a
//! call gets. A function that only reads its arguments, such as `path.resolve`
//! or `defineConfig`, changes nothing, and the exported config call, such as
//! `module.exports = withPlugins(config)`, gives the config to the tool.

use fallow_extract::JsText;
#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_syntax::scope::ScopeFlags;
use rustc_hash::{FxHashMap, FxHashSet};

use super::super::{MAX_CONFIG_WRAPPER_DEPTH, find_variable_init, is_module_exports_target};
use super::{changes_first_argument, member_root};

/// Functions that only read their arguments.
const READ_ONLY_FUNCTIONS: &[&str] = &[
    "require",
    "defineConfig",
    "defineProject",
    "defineWorkspace",
    "mergeConfig",
    "String",
    "Number",
    "Boolean",
    "fileURLToPath",
];

/// Methods of a global object that only read their arguments. An empty
/// method name stands for each method of that object.
const READ_ONLY_METHODS: &[(&str, &str)] = &[
    ("path", ""),
    ("console", ""),
    ("JSON", "stringify"),
    ("require", "resolve"),
    ("Object", "keys"),
    ("Object", "values"),
    ("Object", "entries"),
    ("Object", "hasOwn"),
    ("Object", "getOwnPropertyNames"),
    ("Array", "isArray"),
];

/// The module names of the Node.js `path` module. Each export only reads its
/// arguments.
const PATH_MODULES: &[&str] = &["path", "node:path"];

/// The `webpack-merge` package. Its merge functions return a new config and
/// do not change their arguments. The lodash `merge` changes its first
/// argument, so only an import from this package counts.
const WEBPACK_MERGE: &str = "webpack-merge";

/// The `webpack-merge` exports that only read their arguments.
const WEBPACK_MERGE_FUNCTIONS: &[&str] = &["merge", "mergeWithRules", "mergeWithCustomize"];

/// What one program can do with a call argument.
pub(super) struct CallArguments {
    /// Local names of imported functions that only read their arguments.
    functions: FxHashSet<String>,
    /// Local names of imported modules whose methods only read their
    /// arguments.
    modules: FxHashSet<String>,
    /// The spans of the exported config calls.
    exported_calls: FxHashSet<(u32, u32)>,
    /// The parameter names of each top-level function of the file. A call to
    /// such a function makes each parameter an alias of each argument, so a
    /// change through a parameter changes the argument.
    local_functions: FxHashMap<String, Vec<String>>,
}

impl CallArguments {
    pub(super) fn of(program: &Program<'_>) -> Self {
        let mut calls = Self {
            functions: FxHashSet::default(),
            modules: FxHashSet::default(),
            exported_calls: FxHashSet::default(),
            local_functions: FxHashMap::default(),
        };
        calls.collect_imports(program);
        collect_exported_calls(program, &mut calls.exported_calls);
        calls
    }

    /// The root names that a call or a `new` expression passes to a function
    /// that can change them. Only a direct argument counts, such as `common`
    /// in `configure(common)` or `common.resolve` in `configure(common.resolve)`.
    /// A name in an object or array literal argument does not count. An alias
    /// such as `const all = { web }; configure(all)` covers that case.
    ///
    /// The first argument of a mutator such as `Object.assign` is a change
    /// that `mutated_receiver` counts, and its other arguments are only read.
    pub(super) fn changeable<'e>(
        &self,
        callee: &Expression<'_>,
        span: Span,
        arguments: &'e [Argument<'_>],
    ) -> Vec<&'e str> {
        let callee = callee.without_parentheses();
        if self.exported_calls.contains(&(span.start, span.end))
            || self.local_function_parameters(callee).is_some()
            || self.reads_arguments_only(callee)
            || matches!(callee, Expression::StaticMemberExpression(member) if changes_first_argument(member))
        {
            return Vec::new();
        }
        let mut roots = Vec::new();
        for argument in arguments {
            match argument {
                Argument::SpreadElement(spread) => push_direct_roots(&spread.argument, &mut roots),
                argument => {
                    if let Some(expr) = argument.as_expression() {
                        push_direct_roots(expr, &mut roots);
                    }
                }
            }
        }
        roots
    }

    /// The parameter names of the top-level function that `callee` names.
    pub(super) fn local_function_parameters(&self, callee: &Expression<'_>) -> Option<&[String]> {
        let Expression::Identifier(id) = callee.without_parentheses() else {
            return None;
        };
        self.local_functions
            .get(id.name.as_str())
            .map(Vec::as_slice)
    }

    fn reads_arguments_only(&self, callee: &Expression<'_>) -> bool {
        match callee {
            Expression::Identifier(id) => {
                READ_ONLY_FUNCTIONS.contains(&id.name.as_str())
                    || self.functions.contains(id.name.as_str())
            }
            Expression::StaticMemberExpression(member) => {
                let Expression::Identifier(owner) = member.object.without_parentheses() else {
                    return false;
                };
                self.modules.contains(owner.name.as_str())
                    || READ_ONLY_METHODS.iter().any(|(name, method)| {
                        owner.name == *name
                            && (method.is_empty() || member.property.name == *method)
                    })
            }
            _ => false,
        }
    }

    /// Record the top-level imports and `require` bindings of the `path`
    /// module and of the `webpack-merge` functions.
    fn collect_imports(&mut self, program: &Program<'_>) {
        for statement in &program.body {
            match statement {
                Statement::ImportDeclaration(decl) => {
                    let Some(source) = decl.source.value.as_str() else {
                        continue;
                    };
                    for specifier in decl.specifiers.iter().flatten() {
                        match specifier {
                            ImportDeclarationSpecifier::ImportSpecifier(named) => self
                                .named_import(
                                    source,
                                    &named.imported.name(),
                                    named.local.name.as_str(),
                                ),
                            ImportDeclarationSpecifier::ImportDefaultSpecifier(default) => {
                                self.whole_import(source, default.local.name.as_str());
                            }
                            ImportDeclarationSpecifier::ImportNamespaceSpecifier(namespace) => {
                                self.whole_import(source, namespace.local.name.as_str());
                            }
                        }
                    }
                }
                Statement::VariableDeclaration(decl) => {
                    for declarator in &decl.declarations {
                        self.collect_require(declarator);
                        self.collect_function_binding(declarator);
                    }
                }
                Statement::FunctionDeclaration(func) => {
                    if let Some(id) = &func.id {
                        self.local_functions
                            .insert(id.name.to_string(), parameter_names(&func.params));
                    }
                }
                _ => {}
            }
        }
    }

    /// Record a binding such as `const path = require("path")` or
    /// `const { merge } = require("webpack-merge")`.
    fn collect_require(&mut self, declarator: &VariableDeclarator<'_>) {
        let Some(source) = declarator.init.as_ref().and_then(required_source) else {
            return;
        };
        match &declarator.id {
            BindingPattern::BindingIdentifier(id) => self.whole_import(source, id.name.as_str()),
            BindingPattern::ObjectPattern(pattern) => {
                for property in &pattern.properties {
                    if let (Some(key), BindingPattern::BindingIdentifier(local)) =
                        (property.key.static_name(), &property.value)
                    {
                        self.named_import(source, &key.text(), local.name.as_str());
                    }
                }
            }
            _ => {}
        }
    }

    /// Record a binding such as `const configure = (cfg) => {...}`.
    fn collect_function_binding(&mut self, declarator: &VariableDeclarator<'_>) {
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        let params = match declarator
            .init
            .as_ref()
            .map(Expression::get_inner_expression)
        {
            Some(Expression::ArrowFunctionExpression(arrow)) => &arrow.params,
            Some(Expression::FunctionExpression(func)) => &func.params,
            _ => return,
        };
        self.local_functions
            .insert(id.name.to_string(), parameter_names(params));
    }

    fn named_import(&mut self, source: &str, imported: &str, local: &str) {
        if PATH_MODULES.contains(&source)
            || (source == WEBPACK_MERGE && WEBPACK_MERGE_FUNCTIONS.contains(&imported))
        {
            self.functions.insert(local.to_owned());
        }
    }

    /// A default import, a namespace import or a whole `require`. The
    /// `webpack-merge` default export of version 4 is the merge function.
    fn whole_import(&mut self, source: &str, local: &str) {
        if PATH_MODULES.contains(&source) {
            self.modules.insert(local.to_owned());
        } else if source == WEBPACK_MERGE {
            self.functions.insert(local.to_owned());
        }
    }
}

/// The names that the parameters of a function bind.
fn parameter_names(params: &FormalParameters<'_>) -> Vec<String> {
    params
        .items
        .iter()
        .map(|item| &item.pattern)
        .chain(params.rest.iter().map(|rest| &rest.rest.argument))
        .flat_map(BindingPattern::get_binding_identifiers)
        .map(|identifier| identifier.name.to_string())
        .collect()
}

/// The module that `require("...")` loads.
fn required_source<'e>(init: &'e Expression<'_>) -> Option<&'e str> {
    let Expression::CallExpression(call) = init.get_inner_expression() else {
        return None;
    };
    let Expression::Identifier(callee) = &call.callee else {
        return None;
    };
    if callee.name != "require" {
        return None;
    }
    match call.arguments.first()? {
        Argument::StringLiteral(literal) => literal.value.as_str(),
        _ => None,
    }
}

/// The root names of a direct argument: an identifier, a member chain, or
/// each branch of `a || b`, `a ?? b` or `cond ? a : b`.
fn push_direct_roots<'e>(expr: &'e Expression<'_>, roots: &mut Vec<&'e str>) {
    match expr.without_parentheses().get_inner_expression() {
        Expression::LogicalExpression(logical) => {
            push_direct_roots(&logical.left, roots);
            push_direct_roots(&logical.right, roots);
        }
        Expression::ConditionalExpression(conditional) => {
            push_direct_roots(&conditional.consequent, roots);
            push_direct_roots(&conditional.alternate, roots);
        }
        other => roots.extend(member_root(other)),
    }
}

/// The spans of the calls that give the config to the tool: the exported
/// call, such as `module.exports = merge(common, {...})`, a call in its
/// arguments, such as `withA(withB(config))`, and a call that an exported
/// config callback returns, such as `return merge(common, {...})`.
fn collect_exported_calls(program: &Program<'_>, spans: &mut FxHashSet<(u32, u32)>) {
    for statement in &program.body {
        match statement {
            Statement::ExportDefaultDeclaration(decl) => match &decl.declaration {
                ExportDefaultDeclarationKind::FunctionDeclaration(func) => {
                    mark_returned_calls(program, func.body.as_deref(), spans);
                }
                kind => {
                    if let Some(expr) = kind.as_expression() {
                        mark_exported(program, expr, MAX_CONFIG_WRAPPER_DEPTH, true, spans);
                    }
                }
            },
            Statement::ExpressionStatement(stmt) => {
                if let Expression::AssignmentExpression(assign) = &stmt.expression
                    && is_module_exports_target(&assign.left)
                {
                    mark_exported(
                        program,
                        &assign.right,
                        MAX_CONFIG_WRAPPER_DEPTH,
                        true,
                        spans,
                    );
                }
            }
            _ => {}
        }
    }
}

fn mark_exported(
    program: &Program<'_>,
    expr: &Expression<'_>,
    depth: u8,
    follow_binding: bool,
    spans: &mut FxHashSet<(u32, u32)>,
) {
    match expr.get_inner_expression() {
        Expression::CallExpression(call) => {
            spans.insert((call.span.start, call.span.end));
            if depth == 0 {
                return;
            }
            for argument in &call.arguments {
                if let Some(argument) = argument.as_expression() {
                    mark_exported(program, argument, depth - 1, follow_binding, spans);
                }
            }
        }
        Expression::ArrowFunctionExpression(arrow) => match &arrow.body {
            ArrowFunctionBody::FunctionBody(body) => {
                mark_returned_calls(program, Some(body), spans);
            }
            body => {
                if let Some(expr) = body.as_expression() {
                    mark_exported(program, expr, depth, false, spans);
                }
            }
        },
        Expression::FunctionExpression(func) => {
            mark_returned_calls(program, func.body.as_deref(), spans);
        }
        Expression::Identifier(id) if follow_binding => {
            if let Some(init) = find_variable_init(program, &id.name) {
                mark_exported(program, init, depth, false, spans);
            }
        }
        _ => {}
    }
}

/// Mark each call that a `return` of `body` gives, outside nested functions.
/// A returned call can hold a wrapper call, as in `return withA(merge(a, b))`.
fn mark_returned_calls(
    program: &Program<'_>,
    body: Option<&FunctionBody<'_>>,
    spans: &mut FxHashSet<(u32, u32)>,
) {
    struct Returns<'p, 'b, 's> {
        program: &'p Program<'b>,
        spans: &'s mut FxHashSet<(u32, u32)>,
    }
    impl<'a> Visit<'a> for Returns<'_, '_, '_> {
        fn visit_return_statement(&mut self, ret: &ReturnStatement<'a>) {
            if let Some(argument) = &ret.argument
                && matches!(
                    argument.get_inner_expression(),
                    Expression::CallExpression(_)
                )
            {
                mark_exported(
                    self.program,
                    argument,
                    MAX_CONFIG_WRAPPER_DEPTH,
                    false,
                    self.spans,
                );
            }
        }
        fn visit_function(&mut self, _func: &Function<'a>, _flags: ScopeFlags) {}
        fn visit_arrow_function_expression(&mut self, _arrow: &ArrowFunctionExpression<'a>) {}
    }
    let Some(body) = body else {
        return;
    };
    Returns { program, spans }.visit_function_body(body);
}
