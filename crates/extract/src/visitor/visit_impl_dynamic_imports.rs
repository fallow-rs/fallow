//! Dynamic import capture helpers for the visitor implementation.

use super::*;

impl<'a> ModuleInfoExtractor {
    fn push_relative_dynamic_import_pattern(&mut self, prefix: String, span: Span) {
        if prefix.starts_with("./") || prefix.starts_with("../") {
            self.dynamic_import_patterns.push(DynamicImportPattern {
                prefix,
                suffix: None,
                span,
            });
        }
    }

    pub(super) fn record_import_meta_glob_patterns(&mut self, expr: &CallExpression<'_>) {
        if let Expression::StaticMemberExpression(member) = &expr.callee
            && member.property.name == "glob"
            && matches!(member.object, Expression::MetaProperty(_))
            && let Some(first_arg) = expr.arguments.first()
        {
            match first_arg {
                Argument::StringLiteral(lit) => {
                    self.push_relative_dynamic_import_pattern(lit.value.to_string(), expr.span);
                }
                Argument::ArrayExpression(arr) => {
                    for elem in &arr.elements {
                        if let ArrayExpressionElement::StringLiteral(lit) = elem {
                            self.push_relative_dynamic_import_pattern(
                                lit.value.to_string(),
                                expr.span,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn record_require_context_pattern(&mut self, expr: &CallExpression<'_>) {
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
    }

    pub(super) fn record_import_callback_dynamic_imports(&mut self, expr: &CallExpression<'_>) {
        if let Some(then_cb) = try_extract_import_then_callback(expr) {
            if let Some(local) = &then_cb.local_name {
                self.namespace_binding_names.push(local.clone());
            }
            self.handled_import_spans.insert(then_cb.import_span);
            self.dynamic_imports.push(DynamicImportInfo {
                source: then_cb.source,
                span: then_cb.import_span,
                destructured_names: then_cb.destructured_names,
                local_name: then_cb.local_name,
                is_speculative: false,
            });
        }
    }

    pub(super) fn record_arrow_wrapped_dynamic_import(&mut self, expr: &CallExpression<'_>) {
        if let Some((import_expr, source)) = try_extract_arrow_wrapped_import(&expr.arguments) {
            self.dynamic_imports.push(DynamicImportInfo {
                source: source.to_string(),
                span: import_expr.span,
                destructured_names: vec!["default".to_string()],
                local_name: None,
                is_speculative: false,
            });
            self.handled_import_spans.insert(import_expr.span);

            // Record the `import()` span when this is a
            // `next/dynamic(() => import('./X'), { ssr: false })` call. ssr:false
            // is Next.js's sanctioned client-only escape hatch, so the security
            // `client-server-leak` BFS must not treat a server-only module reached
            // only through it as a leak.
            if self.is_next_dynamic_ssr_false_call(expr) {
                self.client_only_dynamic_import_spans
                    .push(import_expr.span.start);
            }
        }
    }

    /// Whether `expr` is a `next/dynamic(callback, { ssr: false })` call: the
    /// callee is the local binding of the `next/dynamic` default import, and the
    /// second argument is an object literal with a literal `ssr: false` property.
    fn is_next_dynamic_ssr_false_call(&self, expr: &CallExpression<'_>) -> bool {
        let Expression::Identifier(callee) = &expr.callee else {
            return false;
        };
        if !self.is_default_import_from(&callee.name, "next/dynamic") {
            return false;
        }
        let Some(Argument::ObjectExpression(options)) = expr.arguments.get(1) else {
            return false;
        };
        options.properties.iter().any(|prop| {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                return false;
            };
            prop.key.static_name().as_deref() == Some("ssr")
                && matches!(&prop.value, Expression::BooleanLiteral(lit) if !lit.value)
        })
    }

    /// Whether `local_name` is bound to the default import of `source`
    /// (`import dynamic from "next/dynamic"`, or an aliased default).
    pub(super) fn is_default_import_from(&self, local_name: &str, source: &str) -> bool {
        self.imports.iter().any(|import| {
            import.source == source
                && import.local_name == local_name
                && matches!(import.imported_name, ImportedName::Default)
        })
    }

    /// Record a dynamic-import glob pattern from an interpolated template
    /// literal (`import(\`./views/${name}.js\`)`): a relative-prefixed quasi
    /// becomes a `DynamicImportPattern`; multiple interpolations widen the
    /// prefix with a recursive `**/` segment.
    pub(super) fn record_dynamic_import_template_pattern(
        &mut self,
        tpl: &TemplateLiteral<'a>,
        span: Span,
    ) {
        let first_quasi = tpl.quasis[0].value.raw.to_string();
        if !(first_quasi.starts_with("./") || first_quasi.starts_with("../")) {
            return;
        }
        let prefix = if tpl.expressions.len() > 1 {
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
            span,
        });
    }
}
