//! Declaration extraction helpers for `ModuleInfoExtractor`.
//!
//! These are inherent methods that extract export information from
//! declaration AST nodes, binding patterns, and require/import patterns.

use oxc_ast::ast::{
    BindingPattern, CallExpression, Declaration, ImportExpression, TSEnumMemberName,
    TSModuleDeclarationName, VariableDeclarator,
};

use crate::{
    DynamicImportInfo, ExportInfo, ExportName, MemberInfo, MemberKind, RequireCallInfo,
    VisibilityTag,
};
use fallow_types::extract::ClassHeritageInfo;

use super::helpers::{
    extract_class_instance_bindings, extract_class_members, extract_implemented_interface_names,
    extract_super_class_name, has_angular_class_decorator,
};
use super::{MemberAccess, ModuleInfoExtractor, extract_destructured_names};

impl ModuleInfoExtractor {
    pub(crate) fn extract_declaration_exports(
        &mut self,
        decl: &Declaration<'_>,
        is_type_only: bool,
    ) {
        match decl {
            Declaration::VariableDeclaration(var) => {
                for declarator in &var.declarations {
                    self.extract_binding_pattern_names(&declarator.id, is_type_only);
                }
            }
            Declaration::FunctionDeclaration(func) => {
                if let Some(id) = func.id.as_ref() {
                    let name = ExportName::Named(id.name.to_string());
                    // Check if this is an overload — same-named export already exists
                    if let Some(existing) = self.exports.iter_mut().find(|e| e.name == name) {
                        // Update to the implementation (last declaration wins)
                        existing.span = id.span;
                        existing.is_type_only = is_type_only;
                    } else {
                        self.exports.push(ExportInfo {
                            name,
                            local_name: Some(id.name.to_string()),
                            is_type_only,
                            visibility: VisibilityTag::None,
                            span: id.span,
                            members: vec![],
                            is_side_effect_used: false,
                            super_class: None,
                        });
                    }
                }
            }
            Declaration::ClassDeclaration(class) => {
                if let Some(id) = class.id.as_ref() {
                    let is_angular = has_angular_class_decorator(class);
                    let members = extract_class_members(class, is_angular);
                    let super_class = extract_super_class_name(class);
                    let implemented_interfaces = extract_implemented_interface_names(class);
                    let instance_bindings = if is_angular {
                        extract_class_instance_bindings(class)
                    } else {
                        Vec::new()
                    };
                    if super_class.is_some()
                        || !implemented_interfaces.is_empty()
                        || !instance_bindings.is_empty()
                    {
                        self.class_heritage.push(ClassHeritageInfo {
                            export_name: id.name.to_string(),
                            super_class: super_class.clone(),
                            implements: implemented_interfaces,
                            instance_bindings,
                        });
                    }
                    self.exports.push(ExportInfo {
                        name: ExportName::Named(id.name.to_string()),
                        local_name: Some(id.name.to_string()),
                        is_type_only,
                        is_side_effect_used: false,
                        visibility: VisibilityTag::None,
                        span: id.span,
                        members,
                        super_class,
                    });
                }
            }
            Declaration::TSTypeAliasDeclaration(alias) => {
                self.push_type_export(&alias.id.name, alias.id.span);
            }
            Declaration::TSInterfaceDeclaration(iface) => {
                self.push_type_export(&iface.id.name, iface.id.span);
            }
            Declaration::TSEnumDeclaration(enumd) => {
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
                        })
                    })
                    .collect();
                self.exports.push(ExportInfo {
                    name: ExportName::Named(enumd.id.name.to_string()),
                    local_name: Some(enumd.id.name.to_string()),
                    is_type_only,
                    visibility: VisibilityTag::None,
                    span: enumd.id.span,
                    members,
                    is_side_effect_used: false,
                    super_class: None,
                });
            }
            Declaration::TSModuleDeclaration(module) => {
                // `declare namespace` / `declare module` are type-only (ambient).
                // Runtime namespaces (`export namespace Foo { ... }`) compile to
                // real JavaScript objects and are NOT type-only.
                let ns_type_only = module.declare || is_type_only;
                match &module.id {
                    TSModuleDeclarationName::Identifier(id) => {
                        self.exports.push(ExportInfo {
                            name: ExportName::Named(id.name.to_string()),
                            local_name: Some(id.name.to_string()),
                            is_type_only: ns_type_only,
                            visibility: VisibilityTag::None,
                            span: id.span,
                            members: vec![],
                            is_side_effect_used: false,
                            super_class: None,
                        });
                    }
                    TSModuleDeclarationName::StringLiteral(lit) => {
                        self.exports.push(ExportInfo {
                            name: ExportName::Named(lit.value.to_string()),
                            local_name: Some(lit.value.to_string()),
                            is_type_only: ns_type_only,
                            visibility: VisibilityTag::None,
                            span: lit.span,
                            members: vec![],
                            is_side_effect_used: false,
                            super_class: None,
                        });
                    }
                }
            }
            _ => {}
        }
    }

    pub(crate) fn extract_binding_pattern_names(
        &mut self,
        pattern: &BindingPattern<'_>,
        is_type_only: bool,
    ) {
        for id in pattern.get_binding_identifiers() {
            self.exports.push(ExportInfo {
                name: ExportName::Named(id.name.to_string()),
                local_name: Some(id.name.to_string()),
                is_type_only,
                visibility: VisibilityTag::None,
                span: id.span,
                members: vec![],
                is_side_effect_used: false,
                super_class: None,
            });
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
                    self.pending_namespace_members.push(MemberInfo {
                        name: id.name.to_string(),
                        kind: MemberKind::NamespaceMember,
                        span: id.span,
                        has_decorator: false,
                    });
                }
            }
            Declaration::VariableDeclaration(var) => {
                for declarator in &var.declarations {
                    for id in declarator.id.get_binding_identifiers() {
                        self.pending_namespace_members.push(MemberInfo {
                            name: id.name.to_string(),
                            kind: MemberKind::NamespaceMember,
                            span: id.span,
                            has_decorator: false,
                        });
                    }
                }
            }
            Declaration::ClassDeclaration(class) => {
                if let Some(id) = class.id.as_ref() {
                    self.pending_namespace_members.push(MemberInfo {
                        name: id.name.to_string(),
                        kind: MemberKind::NamespaceMember,
                        span: id.span,
                        has_decorator: false,
                    });
                }
            }
            Declaration::TSEnumDeclaration(enumd) => {
                self.pending_namespace_members.push(MemberInfo {
                    name: enumd.id.name.to_string(),
                    kind: MemberKind::NamespaceMember,
                    span: enumd.id.span,
                    has_decorator: false,
                });
            }
            Declaration::TSInterfaceDeclaration(iface) => {
                self.pending_namespace_members.push(MemberInfo {
                    name: iface.id.name.to_string(),
                    kind: MemberKind::NamespaceMember,
                    span: iface.id.span,
                    has_decorator: false,
                });
            }
            Declaration::TSTypeAliasDeclaration(alias) => {
                self.pending_namespace_members.push(MemberInfo {
                    name: alias.id.name.to_string(),
                    kind: MemberKind::NamespaceMember,
                    span: alias.id.span,
                    has_decorator: false,
                });
            }
            Declaration::TSModuleDeclaration(module) => match &module.id {
                TSModuleDeclarationName::Identifier(id) => {
                    self.pending_namespace_members.push(MemberInfo {
                        name: id.name.to_string(),
                        kind: MemberKind::NamespaceMember,
                        span: id.span,
                        has_decorator: false,
                    });
                }
                TSModuleDeclarationName::StringLiteral(lit) => {
                    self.pending_namespace_members.push(MemberInfo {
                        name: lit.value.to_string(),
                        kind: MemberKind::NamespaceMember,
                        span: lit.span,
                        has_decorator: false,
                    });
                }
            },
            _ => {}
        }
    }

    /// Handle `const x = require('./y')` patterns, recording the require call
    /// and tracking namespace bindings for later member access narrowing.
    pub(super) fn handle_require_declaration(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        call: &CallExpression<'_>,
        source: &str,
    ) {
        match &declarator.id {
            BindingPattern::ObjectPattern(obj_pat) => {
                let names = extract_destructured_names(obj_pat);
                self.require_calls.push(RequireCallInfo {
                    source: source.to_string(),
                    span: call.span,
                    destructured_names: names,
                    local_name: None,
                });
                self.handled_require_spans.insert(call.span);
            }
            BindingPattern::BindingIdentifier(id) => {
                let local = id.name.to_string();
                self.namespace_binding_names.push(local.clone());
                self.require_calls.push(RequireCallInfo {
                    source: source.to_string(),
                    span: call.span,
                    destructured_names: Vec::new(),
                    local_name: Some(local),
                });
                self.handled_require_spans.insert(call.span);
            }
            _ => {}
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
        if let BindingPattern::ObjectPattern(obj_pat) = &declarator.id {
            if obj_pat.rest.is_some() {
                // Rest element captures remaining properties — mark as whole-object use
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

    /// Handle `const x = await import('./y')` and `const x = import('./y')` patterns,
    /// recording the dynamic import and tracking namespace bindings.
    pub(super) fn handle_dynamic_import_declaration(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        import_expr: &ImportExpression<'_>,
        source: &str,
    ) {
        match &declarator.id {
            BindingPattern::ObjectPattern(obj_pat) => {
                let names = extract_destructured_names(obj_pat);
                self.dynamic_imports.push(DynamicImportInfo {
                    source: source.to_string(),
                    span: import_expr.span,
                    destructured_names: names,
                    local_name: None,
                });
                self.handled_import_spans.insert(import_expr.span);
            }
            BindingPattern::BindingIdentifier(id) => {
                let local = id.name.to_string();
                self.namespace_binding_names.push(local.clone());
                self.dynamic_imports.push(DynamicImportInfo {
                    source: source.to_string(),
                    span: import_expr.span,
                    destructured_names: Vec::new(),
                    local_name: Some(local),
                });
                self.handled_import_spans.insert(import_expr.span);
            }
            _ => {}
        }
    }
}
