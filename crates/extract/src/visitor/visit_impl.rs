//! `Visit` trait implementation for `ModuleInfoExtractor`.
//!
//! Handles all AST node types: imports, exports, expressions, statements.

#[allow(clippy::wildcard_imports, reason = "many AST types used")]
use oxc_ast::ast::*;
use oxc_ast_visit::Visit;
use oxc_ast_visit::walk;

use crate::{
    DynamicImportInfo, DynamicImportPattern, ExportInfo, ExportName, ImportInfo, ImportedName,
    MemberAccess, ReExportInfo, RequireCallInfo,
};

use crate::asset_url::normalize_asset_url;

use super::helpers::{
    extract_angular_component_metadata, extract_class_members, extract_concat_parts,
    extract_super_class_name, has_angular_class_decorator, is_meta_url_arg,
    regex_pattern_to_suffix,
};
use super::{ModuleInfoExtractor, try_extract_dynamic_import, try_extract_require};

impl<'a> Visit<'a> for ModuleInfoExtractor {
    fn visit_import_declaration(&mut self, decl: &ImportDeclaration<'a>) {
        let source = decl.source.value.to_string();
        let is_type_only = decl.import_kind.is_type();

        let source_span = decl.source.span;

        if let Some(specifiers) = &decl.specifiers {
            for spec in specifiers {
                match spec {
                    ImportDeclarationSpecifier::ImportSpecifier(s) => {
                        self.imports.push(ImportInfo {
                            source: source.clone(),
                            imported_name: ImportedName::Named(s.imported.name().to_string()),
                            local_name: s.local.name.to_string(),
                            is_type_only: is_type_only || s.import_kind.is_type(),
                            span: s.span,
                            source_span,
                        });
                    }
                    ImportDeclarationSpecifier::ImportDefaultSpecifier(s) => {
                        self.imports.push(ImportInfo {
                            source: source.clone(),
                            imported_name: ImportedName::Default,
                            local_name: s.local.name.to_string(),
                            is_type_only,
                            span: s.span,
                            source_span,
                        });
                    }
                    ImportDeclarationSpecifier::ImportNamespaceSpecifier(s) => {
                        let local = s.local.name.to_string();
                        self.namespace_binding_names.push(local.clone());
                        self.imports.push(ImportInfo {
                            source: source.clone(),
                            imported_name: ImportedName::Namespace,
                            local_name: local,
                            is_type_only,
                            span: s.span,
                            source_span,
                        });
                    }
                }
            }
        } else {
            // Side-effect import: import './styles.css'
            self.imports.push(ImportInfo {
                source,
                imported_name: ImportedName::SideEffect,
                local_name: String::new(),
                is_type_only: false,
                span: decl.span,
                source_span,
            });
        }
    }

    fn visit_export_named_declaration(&mut self, decl: &ExportNamedDeclaration<'a>) {
        let is_namespace = matches!(&decl.declaration, Some(Declaration::TSModuleDeclaration(_)));

        // Inside a namespace body: collect as member, not top-level export
        if self.namespace_depth > 0 {
            if let Some(declaration) = &decl.declaration {
                self.extract_namespace_members(declaration);
            }
            if is_namespace {
                self.namespace_depth += 1;
            }
            walk::walk_export_named_declaration(self, decl);
            if is_namespace {
                self.namespace_depth -= 1;
            }
            return;
        }

        let is_type_only = decl.export_kind.is_type();

        if let Some(source) = &decl.source {
            // Re-export: export { foo } from './bar'
            for spec in &decl.specifiers {
                self.re_exports.push(ReExportInfo {
                    source: source.value.to_string(),
                    imported_name: spec.local.name().to_string(),
                    exported_name: spec.exported.name().to_string(),
                    is_type_only: is_type_only || spec.export_kind.is_type(),
                    span: spec.span,
                });
            }
        } else {
            // Local export
            if let Some(declaration) = &decl.declaration {
                self.extract_declaration_exports(declaration, is_type_only);
            }
            for spec in &decl.specifiers {
                let local_name_str = spec.local.name().as_str();
                let spec_type_only = is_type_only || spec.export_kind.is_type();

                // "Import then re-export" pattern: `import { X } from './a'; export { X };`
                // is semantically equivalent to `export { X } from './a';`. Without this
                // detection, we would emit an ExportInfo that (1) collides with the
                // original export in duplicate-export detection and (2) is never reached
                // by re-export chain propagation, causing false unused-export reports.
                //
                // Order-sensitive: relies on imports being visited before exports in
                // source order. The reverse (`export { X }; import { X } from './a';`)
                // is valid JS but vanishingly rare and falls back to local export.
                let matching_import = self.imports.iter().find(|imp| {
                    imp.local_name == local_name_str
                        && matches!(
                            imp.imported_name,
                            ImportedName::Named(_) | ImportedName::Default
                        )
                });

                if let Some(import) = matching_import {
                    let imported_name_str = match &import.imported_name {
                        ImportedName::Named(name) => name.clone(),
                        ImportedName::Default => "default".to_string(),
                        // The matches! guard above filters Namespace/SideEffect, so
                        // this arm is unreachable. Crash loudly if the guard is ever
                        // widened without updating this match.
                        ImportedName::Namespace | ImportedName::SideEffect => {
                            unreachable!("filtered by matches! guard above")
                        }
                    };
                    self.re_exports.push(ReExportInfo {
                        source: import.source.clone(),
                        imported_name: imported_name_str,
                        exported_name: spec.exported.name().to_string(),
                        is_type_only: spec_type_only || import.is_type_only,
                        span: spec.span,
                    });
                } else {
                    self.exports.push(ExportInfo {
                        name: ExportName::Named(spec.exported.name().to_string()),
                        local_name: Some(spec.local.name().to_string()),
                        is_type_only: spec_type_only,
                        is_public: false,
                        span: spec.span,
                        members: vec![],
                        super_class: None,
                    });
                }
            }
        }

        // For namespace declarations: walk the body while tracking depth,
        // then attach collected members to the namespace export.
        if is_namespace {
            self.namespace_depth += 1;
            self.pending_namespace_members.clear();
        }
        walk::walk_export_named_declaration(self, decl);
        if is_namespace {
            self.namespace_depth -= 1;
            if let Some(ns_export) = self.exports.last_mut() {
                ns_export.members = std::mem::take(&mut self.pending_namespace_members);
            }
        }
    }

    fn visit_export_default_declaration(&mut self, decl: &ExportDefaultDeclaration<'a>) {
        // Extract members and super_class for default-exported classes
        let (members, super_class) =
            if let ExportDefaultDeclarationKind::ClassDeclaration(class) = &decl.declaration {
                (
                    extract_class_members(class, has_angular_class_decorator(class)),
                    extract_super_class_name(class),
                )
            } else {
                (vec![], None)
            };

        self.exports.push(ExportInfo {
            name: ExportName::Default,
            local_name: None,
            is_type_only: false,
            is_public: false,
            span: decl.span,
            members,
            super_class,
        });

        walk::walk_export_default_declaration(self, decl);
    }

    fn visit_export_all_declaration(&mut self, decl: &ExportAllDeclaration<'a>) {
        let exported_name = decl
            .exported
            .as_ref()
            .map_or_else(|| "*".to_string(), |e| e.name().to_string());

        self.re_exports.push(ReExportInfo {
            source: decl.source.value.to_string(),
            imported_name: "*".to_string(),
            exported_name,
            is_type_only: decl.export_kind.is_type(),
            span: decl.span,
        });

        walk::walk_export_all_declaration(self, decl);
    }

    fn visit_import_expression(&mut self, expr: &ImportExpression<'a>) {
        // Skip imports already handled via visit_variable_declaration (with local_name capture)
        if self.handled_import_spans.contains(&expr.span) {
            walk::walk_import_expression(self, expr);
            return;
        }

        match &expr.source {
            Expression::StringLiteral(lit) => {
                self.dynamic_imports.push(DynamicImportInfo {
                    source: lit.value.to_string(),
                    span: expr.span,
                    destructured_names: Vec::new(),
                    local_name: None,
                });
            }
            Expression::TemplateLiteral(tpl)
                if !tpl.quasis.is_empty() && !tpl.expressions.is_empty() =>
            {
                // Template literal with expressions: extract prefix/suffix.
                // For multi-expression templates like `./a/${x}/${y}.js` (3 quasis),
                // use `**/` in the prefix so the glob can match nested directories.
                let first_quasi = tpl.quasis[0].value.raw.to_string();
                if first_quasi.starts_with("./") || first_quasi.starts_with("../") {
                    let prefix = if tpl.expressions.len() > 1 {
                        // Multiple dynamic segments: use ** to match any nesting depth
                        format!("{first_quasi}**/")
                    } else {
                        first_quasi
                    };
                    let suffix = if tpl.quasis.len() > 1 {
                        let last = &tpl.quasis[tpl.quasis.len() - 1];
                        let s = last.value.raw.to_string();
                        if s.is_empty() { None } else { Some(s) }
                    } else {
                        None
                    };
                    self.dynamic_import_patterns.push(DynamicImportPattern {
                        prefix,
                        suffix,
                        span: expr.span,
                    });
                }
            }
            Expression::TemplateLiteral(tpl)
                if !tpl.quasis.is_empty() && tpl.expressions.is_empty() =>
            {
                // No-substitution template literal: treat as exact string
                let value = tpl.quasis[0].value.raw.to_string();
                if !value.is_empty() {
                    self.dynamic_imports.push(DynamicImportInfo {
                        source: value,
                        span: expr.span,
                        destructured_names: Vec::new(),
                        local_name: None,
                    });
                }
            }
            Expression::BinaryExpression(bin)
                if bin.operator == oxc_ast::ast::BinaryOperator::Addition =>
            {
                if let Some((prefix, suffix)) = extract_concat_parts(bin)
                    && (prefix.starts_with("./") || prefix.starts_with("../"))
                {
                    self.dynamic_import_patterns.push(DynamicImportPattern {
                        prefix,
                        suffix,
                        span: expr.span,
                    });
                }
            }
            _ => {}
        }

        walk::walk_import_expression(self, expr);
    }

    fn visit_variable_declaration(&mut self, decl: &VariableDeclaration<'a>) {
        for declarator in &decl.declarations {
            let Some(init) = &declarator.init else {
                continue;
            };

            // `const x = require('./y')` — static require
            if let Some((call, source)) = try_extract_require(init) {
                self.handle_require_declaration(declarator, call, source);
                continue;
            }

            // `const x = new ClassName(...)` — instance creation for member tracking.
            // Scope-unaware: shadowing causes false negatives, not false positives.
            // Built-in constructors are skipped to avoid spurious mappings.
            if let Expression::NewExpression(new_expr) = init
                && let Expression::Identifier(callee) = &new_expr.callee
                && let BindingPattern::BindingIdentifier(id) = &declarator.id
                && !super::helpers::is_builtin_constructor(callee.name.as_str())
            {
                self.instance_binding_names
                    .insert(id.name.to_string(), callee.name.to_string());
                // No `continue` — falls through to dynamic import detection (which
                // won't match NewExpression) and then the loop continues.
            }

            // `const [x] = wrapper(() => new ClassName(...))` — instance creation
            // through a wrapper function with a factory initializer (e.g., React's
            // `useState`, `useMemo`). The first array-destructured element is bound
            // to the class returned by the factory.
            if let Expression::CallExpression(call) = init
                && let BindingPattern::ArrayPattern(arr_pat) = &declarator.id
                && let Some(Some(BindingPattern::BindingIdentifier(id))) = arr_pat.elements.first()
                && let Some(class_name) =
                    super::helpers::try_extract_factory_new_class(&call.arguments)
            {
                self.instance_binding_names
                    .insert(id.name.to_string(), class_name);
            }

            // `const { a, b } = ns` — namespace destructuring for member narrowing.
            // Scope-unaware: consistent with flat member_accesses approach.
            if let Expression::Identifier(ident) = init
                && self
                    .namespace_binding_names
                    .iter()
                    .any(|n| n == ident.name.as_str())
            {
                self.handle_namespace_destructuring(declarator, &ident.name);
                continue;
            }

            // `const x = await import('./y')` or `const x = import('./y')`
            let Some((import_expr, source)) = try_extract_dynamic_import(init) else {
                continue;
            };
            self.handle_dynamic_import_declaration(declarator, import_expr, source);
        }
        walk::walk_variable_declaration(self, decl);
    }

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        // Detect require()
        if let Expression::Identifier(ident) = &expr.callee
            && ident.name == "require"
            && let Some(Argument::StringLiteral(lit)) = expr.arguments.first()
            && !self.handled_require_spans.contains(&expr.span)
        {
            self.require_calls.push(RequireCallInfo {
                source: lit.value.to_string(),
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: None,
            });
        }

        // Detect Object.values(X), Object.keys(X), Object.entries(X) — whole-object use
        if let Expression::StaticMemberExpression(member) = &expr.callee
            && let Expression::Identifier(obj) = &member.object
            && obj.name == "Object"
            && matches!(
                member.property.name.as_str(),
                "values" | "keys" | "entries" | "getOwnPropertyNames"
            )
            && let Some(Argument::Identifier(arg_ident)) = expr.arguments.first()
        {
            self.whole_object_uses.push(arg_ident.name.to_string());
        }

        // Detect import.meta.glob() — Vite pattern
        if let Expression::StaticMemberExpression(member) = &expr.callee
            && member.property.name == "glob"
            && matches!(member.object, Expression::MetaProperty(_))
            && let Some(first_arg) = expr.arguments.first()
        {
            match first_arg {
                Argument::StringLiteral(lit) => {
                    let s = lit.value.to_string();
                    if s.starts_with("./") || s.starts_with("../") {
                        self.dynamic_import_patterns.push(DynamicImportPattern {
                            prefix: s,
                            suffix: None,
                            span: expr.span,
                        });
                    }
                }
                Argument::ArrayExpression(arr) => {
                    for elem in &arr.elements {
                        if let ArrayExpressionElement::StringLiteral(lit) = elem {
                            let s = lit.value.to_string();
                            if s.starts_with("./") || s.starts_with("../") {
                                self.dynamic_import_patterns.push(DynamicImportPattern {
                                    prefix: s,
                                    suffix: None,
                                    span: expr.span,
                                });
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Detect require.context() — Webpack pattern
        if let Expression::StaticMemberExpression(member) = &expr.callee
            && member.property.name == "context"
            && let Expression::Identifier(obj) = &member.object
            && obj.name == "require"
            && let Some(Argument::StringLiteral(dir_lit)) = expr.arguments.first()
        {
            let dir = dir_lit.value.to_string();
            if dir.starts_with("./") || dir.starts_with("../") {
                let recursive = expr
                    .arguments
                    .get(1)
                    .is_some_and(|arg| matches!(arg, Argument::BooleanLiteral(b) if b.value));
                let prefix = if recursive {
                    format!("{dir}/**/")
                } else {
                    format!("{dir}/")
                };
                // Parse the optional third argument (regex filter) and convert
                // simple extension patterns (e.g., /\.vue$/) to a glob suffix.
                let suffix = expr.arguments.get(2).and_then(|arg| match arg {
                    Argument::RegExpLiteral(re) => regex_pattern_to_suffix(&re.regex.pattern.text),
                    _ => None,
                });
                self.dynamic_import_patterns.push(DynamicImportPattern {
                    prefix,
                    suffix,
                    span: expr.span,
                });
            }
        }

        walk::walk_call_expression(self, expr);
    }

    fn visit_new_expression(&mut self, expr: &oxc_ast::ast::NewExpression<'a>) {
        // Detect `new URL('./path', import.meta.url)` pattern.
        // This is the standard Vite/bundler pattern for referencing worker files and assets.
        // Treat the path as a dynamic import so the target file is considered reachable.
        if let Expression::Identifier(callee) = &expr.callee
            && callee.name == "URL"
            && expr.arguments.len() == 2
            && let Some(Argument::StringLiteral(path_lit)) = expr.arguments.first()
            && is_meta_url_arg(&expr.arguments[1])
            && (path_lit.value.starts_with("./") || path_lit.value.starts_with("../"))
        {
            self.dynamic_imports.push(DynamicImportInfo {
                source: path_lit.value.to_string(),
                span: expr.span,
                destructured_names: Vec::new(),
                local_name: None,
            });
        }

        walk::walk_new_expression(self, expr);
    }

    #[expect(
        clippy::excessive_nesting,
        reason = "CJS export pattern matching requires deep nesting"
    )]
    fn visit_assignment_expression(&mut self, expr: &AssignmentExpression<'a>) {
        // Detect module.exports = ... and exports.foo = ...
        if let AssignmentTarget::StaticMemberExpression(member) = &expr.left {
            if let Expression::Identifier(obj) = &member.object {
                if obj.name == "module" && member.property.name == "exports" {
                    self.has_cjs_exports = true;
                    // Extract exports from `module.exports = { foo, bar }`
                    if let Expression::ObjectExpression(obj_expr) = &expr.right {
                        for prop in &obj_expr.properties {
                            if let oxc_ast::ast::ObjectPropertyKind::ObjectProperty(p) = prop
                                && let Some(name) = p.key.static_name()
                            {
                                self.exports.push(ExportInfo {
                                    name: ExportName::Named(name.to_string()),
                                    local_name: None,
                                    is_type_only: false,
                                    is_public: false,
                                    span: p.span,
                                    members: vec![],
                                    super_class: None,
                                });
                            }
                        }
                    }
                }
                if obj.name == "exports" {
                    self.has_cjs_exports = true;
                    self.exports.push(ExportInfo {
                        name: ExportName::Named(member.property.name.to_string()),
                        local_name: None,
                        is_type_only: false,
                        is_public: false,
                        span: expr.span,
                        members: vec![],
                        super_class: None,
                    });
                }
            } else if let Expression::StaticMemberExpression(inner) = &member.object
                && let Expression::Identifier(obj) = &inner.object
                && obj.name == "module"
                && inner.property.name == "exports"
            {
                // Extract `module.exports.foo = value` as named export
                self.has_cjs_exports = true;
                self.exports.push(ExportInfo {
                    name: ExportName::Named(member.property.name.to_string()),
                    local_name: None,
                    is_type_only: false,
                    is_public: false,
                    span: expr.span,
                    members: vec![],
                    super_class: None,
                });
            }
            // Capture `this.member = ...` assignment patterns within class bodies.
            // This indicates the class uses the member internally.
            if matches!(member.object, Expression::ThisExpression(_)) {
                self.member_accesses.push(MemberAccess {
                    object: "this".to_string(),
                    member: member.property.name.to_string(),
                });
                // Track `this.field = new ClassName(...)` for chained member access
                // resolution. Enables `this.field.method()` to count as usage of
                // `ClassName.method`. Uses the `instance_binding_names` map with a
                // synthetic `"this.field"` key (safe: dots are invalid in identifiers).
                if let Expression::NewExpression(new_expr) = &expr.right
                    && let Expression::Identifier(callee) = &new_expr.callee
                    && !super::helpers::is_builtin_constructor(callee.name.as_str())
                {
                    self.instance_binding_names.insert(
                        format!("this.{}", member.property.name),
                        callee.name.to_string(),
                    );
                }
            }
        }
        walk::walk_assignment_expression(self, expr);
    }

    fn visit_static_member_expression(&mut self, expr: &StaticMemberExpression<'a>) {
        // Capture `Identifier.member` patterns (e.g., `Status.Active`, `MyClass.create()`)
        if let Expression::Identifier(obj) = &expr.object {
            self.member_accesses.push(MemberAccess {
                object: obj.name.to_string(),
                member: expr.property.name.to_string(),
            });
        }
        // Capture `this.member` patterns within class bodies — these members are used internally
        if matches!(expr.object, Expression::ThisExpression(_)) {
            self.member_accesses.push(MemberAccess {
                object: "this".to_string(),
                member: expr.property.name.to_string(),
            });
        }
        // Capture `this.field.member` patterns — chained access through a class field.
        // Recorded as `MemberAccess { object: "this.field", member }` which is later
        // resolved via `instance_binding_names` when `this.field = new ClassName(...)`.
        if let Expression::StaticMemberExpression(inner) = &expr.object
            && matches!(inner.object, Expression::ThisExpression(_))
        {
            self.member_accesses.push(MemberAccess {
                object: format!("this.{}", inner.property.name),
                member: expr.property.name.to_string(),
            });
        }
        walk::walk_static_member_expression(self, expr);
    }

    fn visit_computed_member_expression(&mut self, expr: &ComputedMemberExpression<'a>) {
        if let Expression::Identifier(obj) = &expr.object {
            if let Expression::StringLiteral(lit) = &expr.expression {
                // Computed access with string literal resolves to a specific member
                self.member_accesses.push(MemberAccess {
                    object: obj.name.to_string(),
                    member: lit.value.to_string(),
                });
            } else {
                // Dynamic computed access — mark all members as used
                self.whole_object_uses.push(obj.name.to_string());
            }
        }
        walk::walk_computed_member_expression(self, expr);
    }

    fn visit_ts_qualified_name(&mut self, it: &TSQualifiedName<'a>) {
        // Capture `Enum.Member` in type positions (e.g., `type X = Status.Active`)
        if let TSTypeName::IdentifierReference(obj) = &it.left {
            self.member_accesses.push(MemberAccess {
                object: obj.name.to_string(),
                member: it.right.name.to_string(),
            });
        }
        walk::walk_ts_qualified_name(self, it);
    }

    fn visit_ts_mapped_type(&mut self, it: &TSMappedType<'a>) {
        // `{ [K in SomeEnum]: ... }` — all members of the constraint type are implicitly used
        if let TSType::TSTypeReference(type_ref) = &it.constraint
            && let TSTypeName::IdentifierReference(ident) = &type_ref.type_name
        {
            self.whole_object_uses.push(ident.name.to_string());
        }
        // `{ [K in keyof typeof SomeEnum]: ... }` — whole-object use via keyof typeof
        if let TSType::TSTypeOperatorType(op) = &it.constraint
            && op.operator == TSTypeOperatorOperator::Keyof
            && let TSType::TSTypeQuery(query) = &op.type_annotation
            && let TSTypeQueryExprName::IdentifierReference(ident) = &query.expr_name
        {
            self.whole_object_uses.push(ident.name.to_string());
        }
        walk::walk_ts_mapped_type(self, it);
    }

    fn visit_ts_type_reference(&mut self, it: &TSTypeReference<'a>) {
        // `Record<SomeEnum, T>` — the first type arg is iterated as mapped keys.
        // Syntactically approximate: also fires for non-enum identifiers (interfaces,
        // classes), consistent with the conservative approach in other whole-object heuristics.
        if let TSTypeName::IdentifierReference(name) = &it.type_name
            && name.name == "Record"
            && let Some(type_args) = &it.type_arguments
            && let Some(first_arg) = type_args.params.first()
            && let TSType::TSTypeReference(key_ref) = first_arg
            && let TSTypeName::IdentifierReference(key_ident) = &key_ref.type_name
        {
            self.whole_object_uses.push(key_ident.name.to_string());
        }
        walk::walk_ts_type_reference(self, it);
    }

    fn visit_for_in_statement(&mut self, stmt: &ForInStatement<'a>) {
        if let Expression::Identifier(ident) = &stmt.right {
            self.whole_object_uses.push(ident.name.to_string());
        }
        walk::walk_for_in_statement(self, stmt);
    }

    fn visit_spread_element(&mut self, elem: &SpreadElement<'a>) {
        if let Expression::Identifier(ident) = &elem.argument {
            self.whole_object_uses.push(ident.name.to_string());
        }
        walk::walk_spread_element(self, elem);
    }

    fn visit_class(&mut self, class: &Class<'a>) {
        // Detect Angular @Component decorator and extract all metadata:
        // templateUrl/styleUrl imports, inline template refs, host binding refs,
        // and inputs/outputs member names.
        if let Some(meta) = extract_angular_component_metadata(class) {
            // Emit SideEffect imports for templateUrl and styleUrl/styleUrls.
            // Angular resolves both `'app.html'` and `'./app.html'` relative to
            // the component file; normalize bare filenames so downstream
            // resolution doesn't misclassify them as npm packages.
            if let Some(ref template_url) = meta.template_url {
                self.imports.push(ImportInfo {
                    source: normalize_asset_url(template_url),
                    imported_name: ImportedName::SideEffect,
                    local_name: String::new(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                    source_span: oxc_span::Span::default(),
                });
            }
            for style_url in &meta.style_urls {
                self.imports.push(ImportInfo {
                    source: normalize_asset_url(style_url),
                    imported_name: ImportedName::SideEffect,
                    local_name: String::new(),
                    is_type_only: false,
                    span: oxc_span::Span::default(),
                    source_span: oxc_span::Span::default(),
                });
            }

            // Scan inline template for member references
            if let Some(ref template) = meta.inline_template {
                let refs = crate::sfc_template::angular::collect_angular_template_refs(template);
                for name in refs {
                    self.member_accesses.push(MemberAccess {
                        object: crate::sfc_template::angular::ANGULAR_TPL_SENTINEL.to_string(),
                        member: name,
                    });
                }
            }

            // Emit sentinel accesses for host binding member references
            for name in &meta.host_member_refs {
                self.member_accesses.push(MemberAccess {
                    object: crate::sfc_template::angular::ANGULAR_TPL_SENTINEL.to_string(),
                    member: name.clone(),
                });
            }

            // Emit sentinel accesses for inputs/outputs metadata members
            for name in &meta.input_output_members {
                self.member_accesses.push(MemberAccess {
                    object: crate::sfc_template::angular::ANGULAR_TPL_SENTINEL.to_string(),
                    member: name.clone(),
                });
            }
        }
        walk::walk_class(self, class);
    }
}
