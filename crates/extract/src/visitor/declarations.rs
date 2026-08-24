//! Declaration extraction helpers for `ModuleInfoExtractor`.
//!
//! These are inherent methods that extract export information from
//! declaration AST nodes, binding patterns, and require/import patterns.

use oxc_ast::ast::{
    Argument, BindingPattern, CallExpression, Declaration, Expression, ImportExpression,
    TSEnumMemberName, TSImportEqualsDeclaration, TSModuleDeclarationName, TSModuleReference,
    VariableDeclarator,
};

use crate::{
    ExportInfo, ExportName, MemberInfo, MemberKind, RequireCallInfo, SemanticFact,
    StringEnumMemberValueFact, VisibilityTag,
};
use fallow_types::extract::ClassHeritageInfo;

use super::helpers::{
    extract_class_generic_instance_bindings, extract_class_instance_bindings,
    extract_class_members, extract_class_type_parameter_names, extract_implemented_interface_names,
    extract_super_class_name, extract_super_class_type_args, has_angular_class_decorator,
};
use super::{MemberAccess, ModuleInfoExtractor, extract_destructured_names};

impl ModuleInfoExtractor {
    pub(crate) fn record_string_enum_member_values(
        &mut self,
        enumd: &oxc_ast::ast::TSEnumDeclaration<'_>,
    ) {
        for member in &enumd.body.members {
            let Some(Expression::StringLiteral(value)) = member.initializer.as_ref() else {
                continue;
            };
            let member_name = match &member.id {
                TSEnumMemberName::Identifier(id) => id.name.to_string(),
                TSEnumMemberName::String(name) | TSEnumMemberName::ComputedString(name) => {
                    name.value.to_string()
                }
                TSEnumMemberName::ComputedTemplateString(_) => continue,
            };
            self.semantic_facts
                .push(SemanticFact::StringEnumMemberValue(
                    StringEnumMemberValueFact {
                        enum_name: enumd.id.name.to_string(),
                        member_name,
                        value: value.value.to_string(),
                    },
                ));
        }
    }

    pub(crate) fn extract_declaration_exports(
        &mut self,
        decl: &Declaration<'_>,
        is_type_only: bool,
    ) {
        match decl {
            Declaration::VariableDeclaration(var) => {
                for declarator in &var.declarations {
                    self.extract_binding_pattern_names(&declarator.id, is_type_only);
                    if !is_type_only {
                        self.record_inline_server_action_const(declarator);
                    }
                }
            }
            Declaration::FunctionDeclaration(func) => {
                if let Some(id) = func.id.as_ref() {
                    self.upsert_function_declaration_export(
                        id.name.as_str(),
                        id.span,
                        is_type_only,
                    );
                    // An exported `async function f() { "use server" }` in a
                    // non-`"use server"` file is an inline Server Action; record
                    // its export name so the `unused-server-action` reclassifier
                    // can move a dead one out of `unused-export`. Only exported
                    // declarations reach this path, so capture is exported-only.
                    if !is_type_only
                        && super::visit_impl::function_body_has_use_server(func.body.as_deref())
                    {
                        self.inline_server_action_exports.push(id.name.to_string());
                    }
                }
            }
            Declaration::ClassDeclaration(class) => {
                self.extract_class_declaration_export(class, is_type_only);
            }
            Declaration::TSTypeAliasDeclaration(alias) => {
                self.push_type_export(&alias.id.name, alias.id.span);
            }
            Declaration::TSInterfaceDeclaration(iface) => {
                self.push_type_export(&iface.id.name, iface.id.span);
            }
            Declaration::TSEnumDeclaration(enumd) => {
                self.extract_enum_declaration_export(enumd, is_type_only);
            }
            Declaration::TSModuleDeclaration(module) => {
                self.extract_module_declaration_export(module, is_type_only);
            }
            Declaration::TSImportEqualsDeclaration(import_equals) => {
                self.record_exported_import_equals(import_equals);
            }
            // `export declare global { ... }` augments the global scope; it
            // declares no export of this file.
            Declaration::TSGlobalDeclaration(_) => {}
        }
    }

    fn upsert_function_declaration_export(
        &mut self,
        name: &str,
        span: oxc_span::Span,
        is_type_only: bool,
    ) {
        let export_name = ExportName::Named(name.to_string());
        if let Some(existing) = self.exports.iter_mut().find(|e| e.name == export_name) {
            existing.span = span;
            existing.is_type_only = is_type_only;
        } else {
            self.exports.push(ExportInfo {
                name: export_name,
                local_name: Some(name.to_string()),
                is_type_only,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span,
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            });
        }
    }

    fn extract_class_declaration_export(
        &mut self,
        class: &oxc_ast::ast::Class<'_>,
        is_type_only: bool,
    ) {
        let Some(id) = class.id.as_ref() else {
            return;
        };
        let is_angular = has_angular_class_decorator(class);
        self.record_angular_inputs_outputs(class, is_angular);
        let members = extract_class_members(class, is_angular);
        let super_class = extract_super_class_name(class);
        let implemented_interfaces = extract_implemented_interface_names(class);
        let instance_bindings =
            extract_class_instance_bindings(class, |local_name, source, imported_name| {
                self.is_named_import_from(local_name, source, imported_name)
            });
        if super_class.is_some()
            || !implemented_interfaces.is_empty()
            || !instance_bindings.is_empty()
        {
            self.class_heritage.push(ClassHeritageInfo {
                export_name: id.name.to_string(),
                super_class: super_class.clone(),
                implements: implemented_interfaces,
                type_parameters: extract_class_type_parameter_names(class),
                instance_bindings,
                super_class_type_args: extract_super_class_type_args(class),
                generic_instance_bindings: extract_class_generic_instance_bindings(class),
            });
        }
        self.exports.push(ExportInfo {
            name: ExportName::Named(id.name.to_string()),
            local_name: Some(id.name.to_string()),
            is_type_only,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: id.span,
            members,
            super_class,
        });
    }

    fn extract_enum_declaration_export(
        &mut self,
        enumd: &oxc_ast::ast::TSEnumDeclaration<'_>,
        is_type_only: bool,
    ) {
        let members: Vec<MemberInfo> = enumd
            .body
            .members
            .iter()
            .filter_map(|member| {
                let name = match &member.id {
                    TSEnumMemberName::Identifier(id) => id.name.to_string(),
                    TSEnumMemberName::String(s) | TSEnumMemberName::ComputedString(s) => {
                        s.value.to_string()
                    }
                    TSEnumMemberName::ComputedTemplateString(_) => return None,
                };
                Some(MemberInfo {
                    name,
                    kind: MemberKind::EnumMember,
                    span: member.span,
                    has_decorator: false,
                    decorator_names: Vec::new(),
                    is_instance_returning_static: false,
                    is_self_returning: false,
                })
            })
            .collect();
        self.exports.push(ExportInfo {
            name: ExportName::Named(enumd.id.name.to_string()),
            local_name: Some(enumd.id.name.to_string()),
            is_type_only,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: enumd.id.span,
            members,
            is_side_effect_used: false,
            super_class: None,
        });
    }

    fn extract_module_declaration_export(
        &mut self,
        module: &oxc_ast::ast::TSModuleDeclaration<'_>,
        is_type_only: bool,
    ) {
        let ns_type_only = module.declare || is_type_only;
        let (name, span) = match &module.id {
            TSModuleDeclarationName::Identifier(id) => (id.name.to_string(), id.span),
            TSModuleDeclarationName::StringLiteral(lit) => (lit.value.to_string(), lit.span),
        };
        self.exports.push(ExportInfo {
            name: ExportName::Named(name.clone()),
            local_name: Some(name),
            is_type_only: ns_type_only,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span,
            members: vec![],
            is_side_effect_used: false,
            super_class: None,
        });
    }

    fn extract_binding_pattern_names(&mut self, pattern: &BindingPattern<'_>, is_type_only: bool) {
        for id in pattern.get_binding_identifiers() {
            self.exports.push(ExportInfo {
                name: ExportName::Named(id.name.to_string()),
                local_name: Some(id.name.to_string()),
                is_type_only,
                visibility: VisibilityTag::None,
                expected_unused_reason: None,
                span: id.span,
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            });
        }
    }

    /// Record an exported `const f = () => { "use server" }` /
    /// `const f = async () => {...}` / `const f = function() {...}` inline Server
    /// Action by its binding name, so the `unused-server-action` reclassifier can
    /// move a dead one out of `unused-export`. Only fires for a plain identifier
    /// binding whose initializer is an arrow / function expression with a body
    /// carrying an inline `"use server"` directive.
    fn record_inline_server_action_const(&mut self, declarator: &VariableDeclarator<'_>) {
        let Some(init) = declarator.init.as_ref() else {
            return;
        };
        let body_has_use_server = match init {
            Expression::ArrowFunctionExpression(arrow) => {
                super::visit_impl::function_body_has_use_server(Some(&arrow.body))
            }
            Expression::FunctionExpression(func) => {
                super::visit_impl::function_body_has_use_server(func.body.as_deref())
            }
            _ => false,
        };
        if !body_has_use_server {
            return;
        }
        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            self.inline_server_action_exports.push(id.name.to_string());
        }
    }

    /// Extract namespace member names from a declaration inside a namespace body.
    ///
    /// Called when `namespace_depth > 0` to collect inner exported declarations
    /// as `MemberInfo` entries instead of top-level module exports.
    pub(crate) fn extract_namespace_members(&mut self, decl: &Declaration<'_>) {
        match decl {
            Declaration::FunctionDeclaration(func) => {
                if let Some(id) = func.id.as_ref() {
                    self.push_namespace_member(id.name.to_string(), id.span);
                }
            }
            Declaration::VariableDeclaration(var) => {
                for declarator in &var.declarations {
                    for id in declarator.id.get_binding_identifiers() {
                        self.push_namespace_member(id.name.to_string(), id.span);
                    }
                }
            }
            Declaration::ClassDeclaration(class) => {
                if let Some(id) = class.id.as_ref() {
                    self.push_namespace_member(id.name.to_string(), id.span);
                }
            }
            Declaration::TSEnumDeclaration(enumd) => {
                self.push_namespace_member(enumd.id.name.to_string(), enumd.id.span);
            }
            Declaration::TSInterfaceDeclaration(iface) => {
                self.push_namespace_member(iface.id.name.to_string(), iface.id.span);
            }
            Declaration::TSTypeAliasDeclaration(alias) => {
                self.push_namespace_member(alias.id.name.to_string(), alias.id.span);
            }
            Declaration::TSModuleDeclaration(module) => match &module.id {
                TSModuleDeclarationName::Identifier(id) => {
                    self.push_namespace_member(id.name.to_string(), id.span);
                }
                TSModuleDeclarationName::StringLiteral(lit) => {
                    self.push_namespace_member(lit.value.to_string(), lit.span);
                }
            },
            Declaration::TSImportEqualsDeclaration(import_equals) => {
                self.record_exported_import_equals(import_equals);
            }
            // `declare global { ... }` inside a namespace body augments the
            // global scope; it contributes no namespace member.
            Declaration::TSGlobalDeclaration(_) => {}
        }
    }

    /// Push a single namespace-member entry with the shared `NamespaceMember`
    /// defaults (no decorator / static / self-return signals).
    fn push_namespace_member(&mut self, name: String, span: oxc_span::Span) {
        self.pending_namespace_members.push(MemberInfo {
            name,
            kind: MemberKind::NamespaceMember,
            span,
            has_decorator: false,
            decorator_names: Vec::new(),
            is_instance_returning_static: false,
            is_self_returning: false,
        });
    }

    /// Handle `const x = require('./y')` patterns, recording the require call
    /// and tracking namespace bindings for later member access narrowing.
    pub(super) fn handle_require_declaration(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        call: &CallExpression<'_>,
        source: &str,
    ) {
        // Span of the specifier string literal (with quotes) for the diagnostic
        // squiggly; falls back to the whole call when the argument is not a
        // plain string literal.
        let source_span = match call.arguments.first() {
            Some(Argument::StringLiteral(lit)) => lit.span,
            _ => call.span,
        };
        match &declarator.id {
            BindingPattern::ObjectPattern(obj_pat) => {
                let names = extract_destructured_names(obj_pat);
                self.require_calls.push(RequireCallInfo {
                    source: source.to_string(),
                    span: call.span,
                    source_span,
                    destructured_names: names,
                    local_name: None,
                    is_type_only: false,
                });
                self.handled_require_spans.insert(call.span);
            }
            BindingPattern::BindingIdentifier(id) => {
                let local = id.name.to_string();
                self.record_namespace_binding_name(local.clone());
                self.require_calls.push(RequireCallInfo {
                    source: source.to_string(),
                    span: call.span,
                    source_span,
                    destructured_names: Vec::new(),
                    local_name: Some(local),
                    is_type_only: false,
                });
                self.handled_require_spans.insert(call.span);
            }
            _ => {}
        }
    }

    /// Handle `import X = require('./y')`, the TypeScript spelling of a
    /// CommonJS require binding. It records the same shape the
    /// `BindingIdentifier` arm of [`Self::handle_require_declaration`] records
    /// for `const X = require('./y')`: one non-destructured require call plus
    /// the namespace binding name, so the target keeps its edge and `X.member`
    /// narrows the target's exports the way a namespace import does (issue
    /// #2365). The two paths run in parallel rather than one calling the other,
    /// because a `TSExternalModuleReference` carries a `StringLiteral` where the
    /// variable form carries a `CallExpression`; the `handled_require_spans`
    /// insert is therefore deliberately absent, since `record_bare_require_call`
    /// only ever sees call expressions.
    ///
    /// The binding is also recorded in `import_equals_bindings` so the semantic
    /// pass classifies its type and value usage: without that, `X.SomeType` in
    /// type position would leave the target's type exports uncredited.
    ///
    /// `import type X = require('./y')` is the one spelling TypeScript erases
    /// completely: the emitted JavaScript holds no `require` call at all. It
    /// keeps the edge, because the target is still a type-space reference, but
    /// carries `is_type_only`, so dependency classification treats it the way
    /// it treats `import type * as X from './y'` instead of claiming the
    /// package is imported at runtime.
    ///
    /// `import X = Some.Namespace` names an entity declared in this file rather
    /// than a module, so it records nothing.
    pub(super) fn handle_import_equals_declaration(
        &mut self,
        decl: &TSImportEqualsDeclaration<'_>,
    ) {
        let TSModuleReference::ExternalModuleReference(reference) = &decl.module_reference else {
            return;
        };
        let local = decl.id.name.to_string();
        self.record_namespace_binding_name(local.clone());
        self.import_equals_bindings.push(local.clone());
        self.require_calls.push(RequireCallInfo {
            source: reference.expression.value.to_string(),
            // The `require('./y')` reference, matching the call span a
            // `const X = require('./y')` declaration records.
            span: reference.span,
            source_span: reference.expression.span,
            destructured_names: Vec::new(),
            local_name: Some(local),
            is_type_only: decl.import_kind.is_type(),
        });
    }

    /// Credit the exported form, `export import X = require('./y')`, with a
    /// whole-object use of its binding.
    ///
    /// The declaration hands the required module object to consumers the graph
    /// cannot enumerate, exactly as `import * as X from './y'; export { X }`
    /// does, so every export of the target is credited. Without it an entry
    /// point that only re-exports the binding has no member access to narrow
    /// with, `is_entry_with_no_access` fires, and every export of the target
    /// turns into a false `unused-export` row (issues #2365, #2373).
    ///
    /// The credit is unconditional, matching the twin, which has no condition
    /// either: `narrow_namespace_references` matches `whole_object_uses`
    /// against the local name of an import edge of this same file, and a
    /// file-level `import X = require(...)` owns that name outright, because a
    /// second root binding of it is a TypeScript duplicate-identifier error.
    /// The one shape that can still collide is a same-named require inside a
    /// nested scope (`const X = require('./other')` in a function body), which
    /// over-credits `./other`; that direction loses a finding and never invents
    /// one, whereas withholding the credit reported every export of the real
    /// target as unused.
    ///
    /// [`Self::push_whole_object_use`] deduplicates, so a whole-object use the
    /// walk records for the same name (a genuine `Object.values(X)`) stays one
    /// entry.
    ///
    /// The entity-name form stays out of scope: `export import X = Some.Ns`
    /// aliases a local declaration, not a module.
    ///
    /// The namespace-body call site (`namespace N { export import X =
    /// require('./x') }`) is defensive leniency only: TypeScript rejects that
    /// spelling with TS1147, so no compiling project reaches it. The file-level
    /// form is the one real code writes.
    fn record_exported_import_equals(&mut self, decl: &TSImportEqualsDeclaration<'_>) {
        if matches!(
            &decl.module_reference,
            TSModuleReference::ExternalModuleReference(_)
        ) {
            self.exported_import_equals_names
                .push(decl.id.name.to_string());
            self.push_whole_object_use(decl.id.name.to_string());
        }
    }

    /// Handle namespace destructuring: `const { a, b } = ns` where `ns` is a namespace
    /// import, dynamic import namespace, or require namespace.
    /// Records member accesses so the graph can narrow which exports are used.
    pub(super) fn handle_namespace_destructuring(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        ident_name: &str,
    ) {
        if self.namespace_like_binding_is_shadowed(ident_name) {
            return;
        }
        if let BindingPattern::ObjectPattern(obj_pat) = &declarator.id {
            if obj_pat.rest.is_some() {
                self.whole_object_uses.push(ident_name.to_string());
            } else {
                for prop in &obj_pat.properties {
                    if let Some(name) = prop.key.static_name() {
                        self.member_accesses.push(MemberAccess {
                            object: ident_name.to_string(),
                            member: name.to_string(),
                        });
                    }
                }
            }
        }
    }

    /// Record dynamic-import edges for a `const {..}/x = await import(...)`
    /// declaration. `sources` carries one specifier per statically-resolvable
    /// branch (a conditional/logical `import()` yields several), so each branch
    /// gets an edge crediting the same destructured names / namespace binding.
    ///
    /// Every branch is credited with every binding, so branch-correlated use
    /// (`cond ? m.xOnly() : m.yOnly()`) marks both members on both targets. That
    /// over-credits in the false-negative direction only (it can hide a dead
    /// export, never invent an `unused-export`), matching fallow's conservative
    /// posture; a grouped conditional edge would be needed for branch precision.
    pub(super) fn handle_dynamic_import_declaration(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        import_expr: &ImportExpression<'_>,
        sources: &[String],
    ) {
        match &declarator.id {
            BindingPattern::ObjectPattern(obj_pat) => {
                let names = extract_destructured_names(obj_pat);
                self.push_dynamic_import_branches(sources, import_expr.span, &names, None);
                self.handled_import_spans.insert(import_expr.span);
            }
            BindingPattern::BindingIdentifier(id) => {
                let local = id.name.to_string();
                self.record_namespace_binding_name(local.clone());
                self.push_dynamic_import_branches(sources, import_expr.span, &[], Some(&local));
                self.handled_import_spans.insert(import_expr.span);
            }
            _ => {}
        }
    }
}
