//! Oxc AST visitor for extracting imports, exports, re-exports, and member accesses.

mod declarations;
mod helpers;
mod visit_impl;

use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Expression, ImportExpression, ObjectPattern,
    ObjectProperty, ObjectPropertyKind, Statement,
};
use oxc_span::Span;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::suppress::Suppression;
use crate::{
    DynamicImportInfo, DynamicImportPattern, ExportInfo, ExportName, ImportInfo, ImportedName,
    MemberAccess, MemberInfo, ModuleInfo, ReExportInfo, RequireCallInfo, VisibilityTag,
};
use fallow_types::extract::{
    ClassHeritageInfo, LocalTypeDeclaration, PublicSignatureTypeReference,
};
use helpers::LitCustomElementDecorator;

#[derive(Debug, Clone)]
struct LocalClassExportInfo {
    members: Vec<MemberInfo>,
    super_class: Option<String>,
    implemented_interfaces: Vec<String>,
    instance_bindings: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
struct LocalSignatureTypeReference {
    owner_name: String,
    type_name: String,
    span: Span,
}

#[derive(Debug, Clone)]
struct ObjectBindingCandidate {
    binding_path: String,
    source_name: String,
}

#[derive(Debug, Clone)]
enum SideEffectRegistrationTarget {
    LocalClass(String),
    AnonymousDefaultExport(usize),
}

#[derive(Debug, Clone)]
struct LitCustomElementCandidate {
    decorator: LitCustomElementDecorator,
    target: SideEffectRegistrationTarget,
}

/// One Angular `@Component({ template: \`...\` })` decorator captured during
/// the visit pass, awaiting synthetic template-block complexity computation
/// in `parse.rs` (where `line_offsets` is available).
#[derive(Debug, Clone)]
pub(crate) struct InlineTemplateFinding {
    /// Template source content (already escape-interpreted when possible).
    pub(crate) template_source: String,
    /// Byte offset of the matched `@Component`/`@Directive` decorator's `@`
    /// in the host file. Used to remap the synthetic finding's line/col so
    /// jump-to-source lands on the decorator and `// fallow-ignore-next-line`
    /// comments above the decorator suppress the finding via the existing
    /// health-side check.
    pub(crate) decorator_start: u32,
}

/// AST visitor that extracts all import/export information in a single pass.
#[derive(Default)]
pub(crate) struct ModuleInfoExtractor {
    pub(crate) exports: Vec<ExportInfo>,
    pub(crate) imports: Vec<ImportInfo>,
    pub(crate) re_exports: Vec<ReExportInfo>,
    pub(crate) dynamic_imports: Vec<DynamicImportInfo>,
    pub(crate) dynamic_import_patterns: Vec<DynamicImportPattern>,
    pub(crate) require_calls: Vec<RequireCallInfo>,
    pub(crate) member_accesses: Vec<MemberAccess>,
    pub(crate) whole_object_uses: Vec<String>,
    pub(crate) has_cjs_exports: bool,
    /// Spans of `require()` calls already handled via destructured require detection.
    handled_require_spans: FxHashSet<Span>,
    /// Spans of `import()` expressions already handled via variable declarator detection.
    handled_import_spans: FxHashSet<Span>,
    /// Local names of namespace imports and namespace-like bindings
    /// (e.g., `import * as ns`, `const mod = require(...)`, `const mod = await import(...)`).
    /// Used to detect destructuring patterns like `const { a, b } = ns`.
    namespace_binding_names: Vec<String>,
    /// Local bindings and `this.<field>` aliases resolved to a target symbol name.
    /// Used so `x.method()` or `this.service.method()` can be mapped back to the
    /// imported/exported class or interface that owns the member.
    binding_target_names: FxHashMap<String, String>,
    /// Iterable receivers whose element type is known (Angular plural queries:
    /// `viewChildren<T>()` / `contentChildren<T>()` initializers and
    /// `@ViewChildren`/`@ContentChildren` decorated `QueryList<T>` properties).
    /// When the visitor sees `<receiver>.forEach(c => c.method())` (or the
    /// optional-chained `?.forEach`), the arrow's first parameter is bound to
    /// `T` so the inner `c.method` access can be resolved. Keys are the same
    /// dotted/parenthesized form produced by `static_member_object_name`.
    iterable_element_types: FxHashMap<String, String>,
    /// Object literal aliases resolved after the full AST walk so later import
    /// declarations can still seed namespace bindings.
    object_binding_candidates: Vec<ObjectBindingCandidate>,
    /// Nesting depth inside `TSModuleDeclaration` (namespace) bodies.
    /// When > 0, inner `export` declarations are collected as namespace members
    /// instead of being extracted as top-level module exports.
    namespace_depth: u32,
    /// Members collected while walking a namespace body.
    /// Moved to the namespace's `ExportInfo.members` after the walk completes.
    pending_namespace_members: Vec<MemberInfo>,
    /// Heritage metadata for exported classes.
    pub(crate) class_heritage: Vec<ClassHeritageInfo>,
    /// Module-scope type-capable declarations.
    pub(crate) local_type_declarations: Vec<LocalTypeDeclaration>,
    /// Public signature type references already mapped to exported names.
    pub(crate) public_signature_type_references: Vec<PublicSignatureTypeReference>,
    /// Public signature type references keyed by local declaration name.
    local_signature_type_references: Vec<LocalSignatureTypeReference>,
    /// Module-scope local class declarations keyed by local binding name.
    local_class_exports: FxHashMap<String, LocalClassExportInfo>,
    /// Module-scope Playwright fixture type aliases keyed by alias name.
    playwright_fixture_types: FxHashMap<String, Vec<(String, String)>>,
    /// Block nesting depth used to distinguish module-scope declarations.
    block_depth: u32,
    /// Function / arrow-function nesting depth used to distinguish module scope.
    function_depth: u32,
    /// Stack of super-class names for classes currently being walked.
    /// Each frame holds the local identifier from the `extends` clause, or `None`
    /// when the class has no super class (or an unanalyzable one like `extends mixin()`).
    /// Read when a `super.member` access is encountered, so it can be recorded as
    /// `MemberAccess { object: <super_local>, member }`. Dropped when the entry is `None`.
    pub(crate) class_super_stack: Vec<Option<String>>,
    /// Inline `@Component({ template: ... })` decorator entries captured during
    /// the visit pass. `parse.rs` reads these after the visit completes (so
    /// `line_offsets` is available) and synthesises `<template>` complexity
    /// findings on the host `.ts` file's `complexity` vec.
    pub(crate) inline_template_findings: Vec<InlineTemplateFinding>,
    /// Local class names registered as Web Components via either a Lit
    /// `customElements.define('tag', X)` call. Used in `into_module_info` /
    /// `merge_into` to flip `is_side_effect_used` on matching exports so they
    /// survive unused-export detection.
    pub(crate) side_effect_registered_class_names: FxHashSet<String>,
    /// Classes with a syntactic `@customElement(...)` decorator. These are
    /// resolved after the full walk so the decorator binding can be checked
    /// against imports regardless of source order.
    lit_custom_element_candidates: Vec<LitCustomElementCandidate>,
}

impl ModuleInfoExtractor {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn record_local_class_export(
        &mut self,
        name: String,
        members: Vec<MemberInfo>,
        super_class: Option<String>,
        implemented_interfaces: Vec<String>,
        instance_bindings: Vec<(String, String)>,
    ) {
        self.local_class_exports.insert(
            name,
            LocalClassExportInfo {
                members,
                super_class,
                implemented_interfaces,
                instance_bindings,
            },
        );
    }

    pub(crate) fn binding_target_names(&self) -> &FxHashMap<String, String> {
        &self.binding_target_names
    }

    fn is_lit_custom_element_decorator(&self, decorator: &LitCustomElementDecorator) -> bool {
        const LIT_DECORATOR_SOURCES: &[&str] =
            &["lit/decorators.js", "lit/decorators/custom-element.js"];

        self.imports.iter().any(|import| {
            LIT_DECORATOR_SOURCES.contains(&import.source.as_str())
                && match decorator {
                    LitCustomElementDecorator::Named { local_name } => {
                        import.local_name == *local_name
                            && matches!(
                                &import.imported_name,
                                ImportedName::Named(name) if name == "customElement"
                            )
                    }
                    LitCustomElementDecorator::Namespace { local_name } => {
                        import.local_name == *local_name
                            && matches!(import.imported_name, ImportedName::Namespace)
                    }
                }
        })
    }

    /// Whether `local_name` is bound to the `register` export of `node:module`
    /// (or a namespace import of `node:module` when `via_namespace` is true).
    ///
    /// Loader registrations like
    /// `import { register } from 'node:module'; register('@swc-node/register/esm', ...)`
    /// load the loader module by specifier; without this lookup the loader
    /// dependency is reported as unused (issue #293).
    pub(crate) fn is_node_module_register(&self, local_name: &str, via_namespace: bool) -> bool {
        const NODE_MODULE_SOURCES: &[&str] = &["node:module", "module"];

        self.imports.iter().any(|import| {
            NODE_MODULE_SOURCES.contains(&import.source.as_str())
                && import.local_name == local_name
                && if via_namespace {
                    matches!(import.imported_name, ImportedName::Namespace)
                } else {
                    matches!(
                        &import.imported_name,
                        ImportedName::Named(name) if name == "register"
                    )
                }
        })
    }

    fn apply_lit_custom_element_candidates(&mut self) {
        if self.lit_custom_element_candidates.is_empty() {
            return;
        }

        let mut class_names = Vec::new();
        let mut anonymous_default_indices = Vec::new();
        for candidate in &self.lit_custom_element_candidates {
            if !self.is_lit_custom_element_decorator(&candidate.decorator) {
                continue;
            }
            match &candidate.target {
                SideEffectRegistrationTarget::LocalClass(class_name) => {
                    class_names.push(class_name.clone());
                }
                SideEffectRegistrationTarget::AnonymousDefaultExport(index) => {
                    anonymous_default_indices.push(*index);
                }
            }
        }

        self.side_effect_registered_class_names.extend(class_names);
        for index in anonymous_default_indices {
            if let Some(export) = self.exports.get_mut(index) {
                export.is_side_effect_used = true;
            }
        }
    }

    fn record_lit_custom_element_candidate(
        &mut self,
        decorator: LitCustomElementDecorator,
        target: SideEffectRegistrationTarget,
    ) {
        self.lit_custom_element_candidates
            .push(LitCustomElementCandidate { decorator, target });
    }

    /// Set `is_side_effect_used = true` on each export whose local binding name
    /// was recorded as side-effect-registered. Runs as a post-walk pass so it
    /// covers both `export class X {}` (export pushed during the class
    /// declaration) and `class X {}; export { X }` / `export default X` patterns
    /// where the export and the registration site are visited at different
    /// points in the traversal.
    fn apply_side_effect_registrations(&mut self) {
        self.apply_lit_custom_element_candidates();
        if self.side_effect_registered_class_names.is_empty() {
            return;
        }
        for export in &mut self.exports {
            let Some(local_name) = export.local_name.as_deref() else {
                continue;
            };
            if self.side_effect_registered_class_names.contains(local_name) {
                export.is_side_effect_used = true;
            }
        }
    }

    fn enrich_local_class_exports(&mut self) {
        if self.local_class_exports.is_empty() {
            return;
        }

        for export in &mut self.exports {
            let Some(local_name) = export.local_name.as_deref() else {
                continue;
            };
            let Some(local_class) = self.local_class_exports.get(local_name) else {
                continue;
            };

            if export.members.is_empty() {
                export.members = local_class.members.clone();
            }
            if export.super_class.is_none() {
                export.super_class = local_class.super_class.clone();
            }

            let export_name = export.name.to_string();
            let already_has_heritage = self
                .class_heritage
                .iter()
                .any(|heritage| heritage.export_name == export_name);
            if !already_has_heritage
                && (local_class.super_class.is_some()
                    || !local_class.implemented_interfaces.is_empty()
                    || !local_class.instance_bindings.is_empty())
            {
                self.class_heritage.push(ClassHeritageInfo {
                    export_name,
                    super_class: local_class.super_class.clone(),
                    implements: local_class.implemented_interfaces.clone(),
                    instance_bindings: local_class.instance_bindings.clone(),
                });
            }
        }
    }

    fn record_exported_instance_bindings(&mut self) {
        if self.binding_target_names.is_empty() {
            return;
        }

        let additional_accesses: Vec<MemberAccess> = self
            .exports
            .iter()
            .filter_map(|export| {
                let local_name = export.local_name.as_deref()?;
                let target_name = self.binding_target_names.get(local_name)?;
                Some(MemberAccess {
                    object: format!("{}{}", crate::INSTANCE_EXPORT_SENTINEL, export.name),
                    member: target_name.clone(),
                })
            })
            .collect();

        self.member_accesses.extend(additional_accesses);
    }

    fn map_local_signature_refs_to_exports(&mut self) {
        if self.local_signature_type_references.is_empty() {
            return;
        }

        for export in &self.exports {
            let export_name = export.name.to_string();
            let Some(local_name) = export.local_name.as_deref().or(Some(export_name.as_str()))
            else {
                continue;
            };
            self.public_signature_type_references.extend(
                self.local_signature_type_references
                    .iter()
                    .filter(|reference| reference.owner_name == local_name)
                    .map(|reference| PublicSignatureTypeReference {
                        export_name: export_name.clone(),
                        type_name: reference.type_name.clone(),
                        span: reference.span,
                    }),
            );
        }
    }

    /// Map bound member accesses to their target symbol member accesses.
    ///
    /// When `const x = new Foo()` and later `x.bar()`, or `const x: Service`
    /// and later `x.bar()`, emit an additional `MemberAccess` against the
    /// resolved symbol name so the analysis layer can track the member usage.
    fn resolve_bound_member_accesses(&mut self) {
        if self.binding_target_names.is_empty() {
            return;
        }
        let additional_accesses: Vec<MemberAccess> = self
            .member_accesses
            .iter()
            .filter_map(|access| {
                self.binding_target_names
                    .get(&access.object)
                    .map(|target_name| MemberAccess {
                        object: target_name.clone(),
                        member: access.member.clone(),
                    })
            })
            .collect();
        let additional_whole: Vec<String> = self
            .whole_object_uses
            .iter()
            .filter_map(|name| self.binding_target_names.get(name).cloned())
            .collect();
        self.member_accesses.extend(additional_accesses);
        self.whole_object_uses.extend(additional_whole);
    }

    fn resolve_object_binding_candidates(&mut self) {
        if self.object_binding_candidates.is_empty() {
            return;
        }

        let candidates = self.object_binding_candidates.clone();
        let max_iterations = candidates.len().saturating_add(1);
        for _ in 0..max_iterations {
            let mut changed = false;
            for candidate in &candidates {
                changed |= self.resolve_object_binding_candidate(candidate);
            }
            if !changed {
                break;
            }
        }
    }

    /// Push a type-only export (type alias or interface).
    fn push_type_export(&mut self, name: &str, span: Span) {
        self.exports.push(ExportInfo {
            name: ExportName::Named(name.to_string()),
            local_name: Some(name.to_string()),
            is_type_only: true,
            visibility: VisibilityTag::None,
            span,
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        });
    }

    /// Convert this extractor into a `ModuleInfo`, consuming its fields.
    pub(crate) fn into_module_info(
        mut self,
        file_id: fallow_types::discover::FileId,
        content_hash: u64,
        suppressions: Vec<Suppression>,
    ) -> ModuleInfo {
        self.enrich_local_class_exports();
        self.record_exported_instance_bindings();
        self.resolve_object_binding_candidates();
        self.resolve_bound_member_accesses();
        self.map_local_signature_refs_to_exports();
        self.apply_side_effect_registrations();
        ModuleInfo {
            file_id,
            exports: self.exports,
            imports: self.imports,
            re_exports: self.re_exports,
            dynamic_imports: self.dynamic_imports,
            dynamic_import_patterns: self.dynamic_import_patterns,
            require_calls: self.require_calls,
            member_accesses: self.member_accesses,
            whole_object_uses: self.whole_object_uses,
            has_cjs_exports: self.has_cjs_exports,
            content_hash,
            suppressions,
            unused_import_bindings: Vec::new(),
            type_referenced_import_bindings: Vec::new(),
            value_referenced_import_bindings: Vec::new(),
            line_offsets: Vec::new(),
            complexity: Vec::new(),
            flag_uses: Vec::new(),
            class_heritage: self.class_heritage,
            local_type_declarations: self.local_type_declarations,
            public_signature_type_references: self.public_signature_type_references,
        }
    }

    /// Merge this extractor's fields into an existing `ModuleInfo`.
    ///
    /// Used by SFC scripts where multiple `<script>` blocks contribute to a
    /// single `ModuleInfo`. `inline_template_findings` is intentionally not
    /// merged here: synthetic `<template>` complexity findings live on
    /// `ModuleInfo.complexity` (populated at `parse.rs` time by
    /// `append_inline_template_complexity`), not on this extractor's transient
    /// holding vec. SFC scripts cannot host Angular `@Component` decorators
    /// anyway, so the omission is observable only if a future caller starts
    /// running this visitor on `.ts` content via `merge_into`. Add the
    /// `inline_template_findings` plumbing at that point.
    pub(crate) fn merge_into(mut self, info: &mut ModuleInfo) {
        debug_assert!(
            self.inline_template_findings.is_empty(),
            "merge_into is the SFC-script path and SFC scripts cannot host \
             Angular @Component decorators; if a future caller routes \
             Angular content here, plumb inline_template_findings into the \
             merge step before relying on this assertion"
        );
        self.enrich_local_class_exports();
        self.record_exported_instance_bindings();
        self.resolve_object_binding_candidates();
        self.resolve_bound_member_accesses();
        self.map_local_signature_refs_to_exports();
        self.apply_side_effect_registrations();
        info.imports.extend(self.imports);
        info.exports.extend(self.exports);
        info.re_exports.extend(self.re_exports);
        info.dynamic_imports.extend(self.dynamic_imports);
        info.dynamic_import_patterns
            .extend(self.dynamic_import_patterns);
        info.require_calls.extend(self.require_calls);
        info.member_accesses.extend(self.member_accesses);
        info.whole_object_uses.extend(self.whole_object_uses);
        info.has_cjs_exports |= self.has_cjs_exports;
        info.class_heritage.extend(self.class_heritage);
        info.local_type_declarations
            .extend(self.local_type_declarations);
        info.public_signature_type_references
            .extend(self.public_signature_type_references);
    }
}

/// Extract destructured property names from an object pattern.
///
/// Returns an empty `Vec` when a rest element is present (conservative:
/// the caller cannot know which names are captured).
fn extract_destructured_names(obj_pat: &ObjectPattern<'_>) -> Vec<String> {
    if obj_pat.rest.is_some() {
        return Vec::new();
    }
    obj_pat
        .properties
        .iter()
        .filter_map(|prop| prop.key.static_name().map(|n| n.to_string()))
        .collect()
}

/// Try to match `require('...')` from a call expression initializer.
///
/// Returns `(call_expr, source_string)` on success.
fn try_extract_require<'a, 'b>(
    init: &'b Expression<'a>,
) -> Option<(&'b CallExpression<'a>, &'b str)> {
    let Expression::CallExpression(call) = init else {
        return None;
    };
    let Expression::Identifier(callee) = &call.callee else {
        return None;
    };
    if callee.name != "require" {
        return None;
    }
    let Some(Argument::StringLiteral(lit)) = call.arguments.first() else {
        return None;
    };
    Some((call, &lit.value))
}

/// Try to extract a dynamic `import()` expression (possibly wrapped in `await`)
/// with a static string source.
///
/// Returns `(import_expr, source_string)` on success.
fn try_extract_dynamic_import<'a, 'b>(
    init: &'b Expression<'a>,
) -> Option<(&'b ImportExpression<'a>, &'b str)> {
    let import_expr = extract_import_expression(init)?;
    let Expression::StringLiteral(lit) = &import_expr.source else {
        return None;
    };
    Some((import_expr, &lit.value))
}

/// Try to extract a dynamic import returned by a known route/component callback.
///
/// This covers framework route declarations such as
/// `loadChildren: () => import('./feature.routes')` and Vue-style
/// `component: () => import('./View.vue')`, where the framework consumes the
/// module default export even though user code does not spell `.default`.
fn try_extract_property_callback_import<'a, 'b>(
    prop: &'b ObjectProperty<'a>,
) -> Option<(&'b ImportExpression<'a>, &'b str)> {
    let property_name = prop.key.static_name()?;
    if !matches!(
        property_name.as_ref(),
        "component" | "loadChildren" | "loadComponent"
    ) {
        return None;
    }

    let import_expr = match &prop.value {
        Expression::ArrowFunctionExpression(arrow) => {
            if arrow.expression {
                let Some(Statement::ExpressionStatement(expr_stmt)) = arrow.body.statements.first()
                else {
                    return None;
                };
                extract_import_expression(&expr_stmt.expression)?
            } else {
                extract_import_from_return_body(&arrow.body.statements)?
            }
        }
        Expression::FunctionExpression(func) => {
            let body = func.body.as_ref()?;
            extract_import_from_return_body(&body.statements)?
        }
        _ => return None,
    };
    let Expression::StringLiteral(lit) = &import_expr.source else {
        return None;
    };
    Some((import_expr, &lit.value))
}

fn extract_import_expression<'a, 'b>(expr: &'b Expression<'a>) -> Option<&'b ImportExpression<'a>> {
    match expr {
        Expression::AwaitExpression(await_expr) => extract_import_expression(&await_expr.argument),
        Expression::ImportExpression(imp) => Some(imp),
        Expression::ParenthesizedExpression(paren) => extract_import_expression(&paren.expression),
        _ => None,
    }
}

/// Try to extract a dynamic `import()` expression wrapped in an arrow function
/// that appears as an argument to a call expression. This covers patterns like:
///
/// - `React.lazy(() => import('./Foo'))`
/// - `loadable(() => import('./Component'))`
/// - `defineAsyncComponent(() => import('./View'))`
///
/// Returns `(import_expr, source_string)` on success.
fn try_extract_arrow_wrapped_import<'a, 'b>(
    arguments: &'b [Argument<'a>],
) -> Option<(&'b ImportExpression<'a>, &'b str)> {
    for arg in arguments {
        let import_expr = match arg {
            Argument::ArrowFunctionExpression(arrow) => {
                if arrow.expression {
                    // Expression body: `() => import('./x')`
                    let Some(Statement::ExpressionStatement(expr_stmt)) =
                        arrow.body.statements.first()
                    else {
                        continue;
                    };
                    let Expression::ImportExpression(imp) = &expr_stmt.expression else {
                        continue;
                    };
                    imp
                } else {
                    // Block body: `() => { return import('./x'); }`
                    let Some(imp) = extract_import_from_return_body(&arrow.body.statements) else {
                        continue;
                    };
                    imp
                }
            }
            Argument::FunctionExpression(func) => {
                // `function() { return import('./x'); }`
                let Some(body) = &func.body else {
                    continue;
                };
                let Some(imp) = extract_import_from_return_body(&body.statements) else {
                    continue;
                };
                imp
            }
            _ => continue,
        };
        let Expression::StringLiteral(lit) = &import_expr.source else {
            continue;
        };
        return Some((import_expr, &lit.value));
    }
    None
}

/// Extract an `import()` expression from a block body's return statement.
fn extract_import_from_return_body<'a, 'b>(
    stmts: &'b [Statement<'a>],
) -> Option<&'b ImportExpression<'a>> {
    for stmt in stmts.iter().rev() {
        if let Statement::ReturnStatement(ret) = stmt
            && let Some(argument) = &ret.argument
            && let Some(imp) = extract_import_expression(argument)
        {
            return Some(imp);
        }
    }
    None
}

/// Result from extracting a `.then()` callback on a dynamic import.
struct ImportThenCallback {
    /// The import specifier string (e.g., `"./lib"`).
    source: String,
    /// The span of the `import()` expression (for dedup).
    import_span: oxc_span::Span,
    /// Named exports accessed in the callback, if extractable.
    destructured_names: Vec<String>,
    /// The callback parameter name if it's a simple identifier binding,
    /// for namespace-style narrowing when specific member names cannot
    /// be statically extracted from the body.
    local_name: Option<String>,
}

/// Try to extract a `.then()` callback on a dynamic `import()` expression.
///
/// Handles patterns like:
/// - `import('./lib').then(m => m.foo)` — expression body member access
/// - `import('./lib').then(({ foo, bar }) => { ... })` — param destructuring
/// - `import('./lib').then(m => { ... m.foo ... })` — namespace binding
///
/// Returns extraction results on success.
fn try_extract_import_then_callback(expr: &CallExpression<'_>) -> Option<ImportThenCallback> {
    // Callee must be `<something>.then`
    let Expression::StaticMemberExpression(member) = &expr.callee else {
        return None;
    };
    if member.property.name != "then" {
        return None;
    }

    // The object must be an `import('...')` expression with a string literal source
    let Expression::ImportExpression(import_expr) = &member.object else {
        return None;
    };
    let Expression::StringLiteral(lit) = &import_expr.source else {
        return None;
    };
    let source = lit.value.to_string();
    let import_span = import_expr.span;

    // First argument must be a callback (arrow or function expression)
    let first_arg = expr.arguments.first()?;

    match first_arg {
        Argument::ArrowFunctionExpression(arrow) => {
            let param = arrow.params.items.first()?;
            match &param.pattern {
                // Destructured: `({ foo, bar }) => ...`
                BindingPattern::ObjectPattern(obj_pat) => Some(ImportThenCallback {
                    source,
                    import_span,
                    destructured_names: extract_destructured_names(obj_pat),
                    local_name: None,
                }),
                // Identifier: `m => m.foo` or `m => { ... }`
                BindingPattern::BindingIdentifier(id) => {
                    let param_name = id.name.to_string();

                    // For expression bodies, try to extract direct member access
                    if arrow.expression
                        && let Some(Statement::ExpressionStatement(expr_stmt)) =
                            arrow.body.statements.first()
                        && let Some(names) =
                            extract_member_names_from_expr(&expr_stmt.expression, &param_name)
                    {
                        return Some(ImportThenCallback {
                            source,
                            import_span,
                            destructured_names: names,
                            local_name: None,
                        });
                    }

                    // Fall back to namespace binding for narrowing
                    Some(ImportThenCallback {
                        source,
                        import_span,
                        destructured_names: Vec::new(),
                        local_name: Some(param_name),
                    })
                }
                _ => None,
            }
        }
        Argument::FunctionExpression(func) => {
            let param = func.params.items.first()?;
            match &param.pattern {
                BindingPattern::ObjectPattern(obj_pat) => Some(ImportThenCallback {
                    source,
                    import_span,
                    destructured_names: extract_destructured_names(obj_pat),
                    local_name: None,
                }),
                BindingPattern::BindingIdentifier(id) => Some(ImportThenCallback {
                    source,
                    import_span,
                    destructured_names: Vec::new(),
                    local_name: Some(id.name.to_string()),
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Extract member names from an expression that accesses the given parameter.
///
/// Handles:
/// - `m.foo` → `["foo"]`
/// - `({ default: m.Foo })` → `["Foo"]` (React.lazy `.then` pattern)
fn extract_member_names_from_expr(expr: &Expression<'_>, param_name: &str) -> Option<Vec<String>> {
    match expr {
        // `m.foo`
        Expression::StaticMemberExpression(member) => {
            if let Expression::Identifier(obj) = &member.object
                && obj.name == param_name
            {
                Some(vec![member.property.name.to_string()])
            } else {
                None
            }
        }
        // `({ default: m.Foo })` — wrapped in parens as object literal
        Expression::ObjectExpression(obj) => extract_member_names_from_object(obj, param_name),
        // Parenthesized: `(expr)` — unwrap and recurse
        Expression::ParenthesizedExpression(paren) => {
            extract_member_names_from_expr(&paren.expression, param_name)
        }
        _ => None,
    }
}

/// Extract member names from object literal properties that access the given parameter.
fn extract_member_names_from_object(
    obj: &oxc_ast::ast::ObjectExpression<'_>,
    param_name: &str,
) -> Option<Vec<String>> {
    let mut names = Vec::new();
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(p) = prop
            && let Expression::StaticMemberExpression(member) = &p.value
            && let Expression::Identifier(obj) = &member.object
            && obj.name == param_name
        {
            names.push(member.property.name.to_string());
        }
    }
    if names.is_empty() { None } else { Some(names) }
}

#[cfg(all(test, not(miri)))]
mod tests;
