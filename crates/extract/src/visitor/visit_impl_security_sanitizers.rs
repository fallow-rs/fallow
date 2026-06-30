#[allow(
    clippy::wildcard_imports,
    reason = "many sanitizer helper AST types used"
)]
use oxc_ast::ast::*;

use fallow_types::extract::{
    SanitizedSinkArg, SanitizerScope, SecurityControlKind, SecurityControlSite,
};

use super::super::{ModuleInfoExtractor, SecurityPathSinkBinding};
use super::{
    extract_arrow_return_expr, extract_function_body_final_return_expr, flatten_callee_path,
    is_dompurify_source, static_string_literal_value, unwrap_parens, unwrap_static_expr,
};

fn statement_exits_current_flow(stmt: &Statement<'_>) -> bool {
    match stmt {
        Statement::ReturnStatement(_) | Statement::ThrowStatement(_) => true,
        Statement::BlockStatement(block) => {
            block.body.first().is_some_and(statement_exits_current_flow)
        }
        _ => false,
    }
}

fn path_relative_starts_with_parent<'b>(expr: &'b Expression<'_>) -> Option<&'b str> {
    let Expression::CallExpression(call) = unwrap_parens(expr) else {
        return None;
    };
    let Expression::StaticMemberExpression(member) = &call.callee else {
        return None;
    };
    if member.property.name != "startsWith" {
        return None;
    }
    let Expression::Identifier(relative) = &member.object else {
        return None;
    };
    let Some(Argument::StringLiteral(prefix)) = call.arguments.first() else {
        return None;
    };
    if prefix.value.as_str() != ".." {
        return None;
    }
    Some(relative.name.as_str())
}

#[derive(Debug, Clone)]
struct ReplaceStep {
    pattern: String,
    replacement: String,
}

fn sanitizer_scope_for_local_helper(
    extractor: &ModuleInfoExtractor,
    params: &FormalParameters<'_>,
    expr: &Expression<'_>,
) -> Option<SanitizerScope> {
    if local_html_escape_helper_expr(params, expr)
        || matches!(unwrap_static_expr(expr), Expression::TemplateLiteral(template) if extractor.html_template_is_sanitized(template))
    {
        return Some(SanitizerScope::Html);
    }
    if local_sql_identifier_quote_helper_expr(params, expr) {
        return Some(SanitizerScope::SqlIdentifier);
    }
    None
}

fn simple_param_names<'a>(params: &'a FormalParameters<'a>) -> Vec<&'a str> {
    params
        .items
        .iter()
        .filter_map(|param| match &param.pattern {
            BindingPattern::BindingIdentifier(id) => Some(id.name.as_str()),
            _ => None,
        })
        .collect()
}

fn local_html_escape_helper_expr(params: &FormalParameters<'_>, expr: &Expression<'_>) -> bool {
    let names = simple_param_names(params);
    let [param] = names.as_slice() else {
        return false;
    };
    let Some(steps) = replace_chain_for_param(expr, param) else {
        return false;
    };
    has_replace_step(&steps, "&", &["&amp;"])
        && has_replace_step(&steps, "<", &["&lt;"])
        && has_replace_step(&steps, ">", &["&gt;"])
}

fn local_sql_identifier_quote_helper_expr(
    params: &FormalParameters<'_>,
    expr: &Expression<'_>,
) -> bool {
    let names = simple_param_names(params);
    let [param] = names.as_slice() else {
        return false;
    };
    let Expression::TemplateLiteral(template) = unwrap_static_expr(expr) else {
        return false;
    };
    if template.expressions.len() != 1 || template.quasis.len() != 2 {
        return false;
    }
    if template
        .quasis
        .iter()
        .any(|quasi| template_raw(quasi) != "\"")
    {
        return false;
    }
    let Some(steps) = replace_chain_for_param(&template.expressions[0], param) else {
        return false;
    };
    has_replace_step(&steps, "\"", &["\"\""])
}

fn replace_chain_for_param(expr: &Expression<'_>, param: &str) -> Option<Vec<ReplaceStep>> {
    let mut steps = Vec::new();
    let mut current = unwrap_static_expr(expr);
    loop {
        match current {
            Expression::CallExpression(call) => {
                let step = replace_step_from_call(call)?;
                let Expression::StaticMemberExpression(member) = unwrap_parens(&call.callee) else {
                    return None;
                };
                steps.push(step);
                current = unwrap_static_expr(&member.object);
            }
            Expression::Identifier(ident) if ident.name == param => return Some(steps),
            _ => return None,
        }
    }
}

fn replace_step_from_call(call: &CallExpression<'_>) -> Option<ReplaceStep> {
    let Expression::StaticMemberExpression(member) = unwrap_parens(&call.callee) else {
        return None;
    };
    if !matches!(member.property.name.as_str(), "replace" | "replaceAll") {
        return None;
    }
    Some(ReplaceStep {
        pattern: replace_pattern(call.arguments.first()?)?,
        replacement: argument_static_string(call.arguments.get(1)?)?,
    })
}

fn replace_pattern(arg: &Argument<'_>) -> Option<String> {
    match arg {
        Argument::RegExpLiteral(lit) => Some(lit.regex.pattern.text.to_string()),
        _ => argument_static_string(arg),
    }
}

fn argument_static_string(arg: &Argument<'_>) -> Option<String> {
    match arg {
        Argument::StringLiteral(lit) => Some(lit.value.to_string()),
        _ => arg.as_expression().and_then(static_string_literal_value),
    }
}

fn has_replace_step(steps: &[ReplaceStep], pattern: &str, replacements: &[&str]) -> bool {
    steps.iter().any(|step| {
        pattern_matches(&step.pattern, pattern)
            && replacements
                .iter()
                .any(|replacement| step.replacement == *replacement)
    })
}

fn pattern_matches(actual: &str, expected: &str) -> bool {
    actual == expected || actual.strip_prefix('\\') == Some(expected)
}

fn template_raw<'a>(quasi: &'a TemplateElement<'_>) -> &'a str {
    quasi.value.raw.as_str()
}

fn html_template_interpolation_is_text(template: &TemplateLiteral<'_>, index: usize) -> bool {
    template
        .quasis
        .get(index)
        .map(template_raw)
        .is_some_and(html_raw_ends_in_text_context)
}

fn html_raw_ends_in_text_context(raw: &str) -> bool {
    let Some(last_lt) = raw.rfind('<') else {
        return true;
    };
    raw.rfind('>').is_some_and(|last_gt| last_gt > last_lt)
}

fn sql_identifier_template_context(raw: &str) -> bool {
    let lower = raw.trim_end().to_ascii_lowercase();
    [
        "select",
        " from",
        " join",
        " into",
        " update",
        " table",
        " by",
        "delete from",
        "alter table",
        "truncate table",
    ]
    .iter()
    .any(|suffix| lower.ends_with(suffix))
}

impl ModuleInfoExtractor {
    fn record_sanitizer_function_helper(
        &mut self,
        name: &str,
        params: &FormalParameters<'_>,
        expr: &Expression<'_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        if let Some(scope) = sanitizer_scope_for_local_helper(self, params, expr) {
            self.module_sanitizer_helpers
                .insert(name.to_string(), scope);
        } else {
            self.module_sanitizer_helpers.remove(name);
        }
    }

    pub(super) fn record_sanitizer_function_declaration(&mut self, function: &Function<'_>) {
        let (Some(id), Some(body)) = (function.id.as_ref(), function.body.as_deref()) else {
            return;
        };
        let Some(expr) = extract_function_body_final_return_expr(body) else {
            self.module_sanitizer_helpers.remove(id.name.as_str());
            return;
        };
        self.record_sanitizer_function_helper(id.name.as_str(), &function.params, expr);
    }

    pub(super) fn record_sanitizer_helper_from_variable_declarator(
        &mut self,
        decl: &VariableDeclaration<'_>,
        declarator: &VariableDeclarator<'_>,
        init: &Expression<'_>,
    ) {
        if !self.is_module_scope() {
            return;
        }
        let BindingPattern::BindingIdentifier(id) = &declarator.id else {
            return;
        };
        if decl.kind != VariableDeclarationKind::Const {
            self.module_sanitizer_helpers.remove(id.name.as_str());
            return;
        }
        let helper = match init {
            Expression::ArrowFunctionExpression(arrow) => {
                extract_arrow_return_expr(arrow).map(|expr| (&arrow.params, expr))
            }
            Expression::FunctionExpression(function) => function
                .body
                .as_deref()
                .and_then(extract_function_body_final_return_expr)
                .map(|expr| (&function.params, expr)),
            _ => None,
        };
        if let Some((params, expr)) = helper {
            self.record_sanitizer_function_helper(id.name.as_str(), params, expr);
        } else {
            self.module_sanitizer_helpers.remove(id.name.as_str());
        }
    }

    pub(super) fn record_dompurify_import_binding(
        &mut self,
        source: &str,
        local: &str,
        is_type_only: bool,
    ) {
        if !is_type_only && self.is_module_scope() && is_dompurify_source(source) {
            self.dompurify_bindings.insert(local.to_string());
        }
    }

    pub(super) fn record_dompurify_require_binding(
        &mut self,
        declarator: &VariableDeclarator<'_>,
        source: &str,
    ) {
        if !self.is_module_scope() || !is_dompurify_source(source) {
            return;
        }
        if let BindingPattern::BindingIdentifier(id) = &declarator.id {
            self.dompurify_bindings.insert(id.name.to_string());
        }
    }

    pub(super) fn sanitizer_scope_for_expr(&self, expr: &Expression<'_>) -> Option<SanitizerScope> {
        match unwrap_parens(expr) {
            Expression::Identifier(ident) => self.sanitizer_scope_for_identifier(&ident.name),
            Expression::AwaitExpression(await_expr) => {
                self.sanitizer_scope_for_expr(&await_expr.argument)
            }
            Expression::CallExpression(call) if self.is_dompurify_sanitize_call(call) => {
                Some(SanitizerScope::Html)
            }
            Expression::CallExpression(call) => self.sanitizer_helper_scope_for_call(call),
            Expression::ObjectExpression(obj) => self.sanitizer_scope_for_object(obj),
            Expression::TemplateLiteral(template) if self.html_template_is_sanitized(template) => {
                Some(SanitizerScope::Html)
            }
            Expression::TemplateLiteral(template)
                if self.sql_identifier_template_is_sanitized(template) =>
            {
                Some(SanitizerScope::SqlIdentifier)
            }
            _ => None,
        }
    }

    fn sanitizer_scope_for_object(&self, obj: &ObjectExpression<'_>) -> Option<SanitizerScope> {
        obj.properties.iter().find_map(|prop| {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                return None;
            };
            if prop.key.static_name().is_none_or(|name| name != "__html") {
                return None;
            }
            self.sanitizer_scope_for_expr(&prop.value)
        })
    }

    fn is_dompurify_sanitize_call(&self, call: &CallExpression<'_>) -> bool {
        let Some(callee_path) = flatten_callee_path(&call.callee) else {
            return false;
        };
        let Some((object, method)) = callee_path.rsplit_once('.') else {
            return false;
        };
        method == "sanitize"
            && self.dompurify_bindings.contains(object)
            && !self.nested_scope_shadows(object)
    }

    fn sanitizer_helper_scope_for_call(&self, call: &CallExpression<'_>) -> Option<SanitizerScope> {
        let Expression::Identifier(callee) = unwrap_parens(&call.callee) else {
            return None;
        };
        if self.nested_scope_shadows(&callee.name) || call.arguments.is_empty() {
            return None;
        }
        self.module_sanitizer_helpers
            .get(callee.name.as_str())
            .copied()
    }

    pub(super) fn html_template_is_sanitized(&self, template: &TemplateLiteral<'_>) -> bool {
        !template.expressions.is_empty()
            && template
                .expressions
                .iter()
                .enumerate()
                .all(|(index, expr)| {
                    html_template_interpolation_is_text(template, index)
                        && self.sanitizer_scope_for_expr(expr) == Some(SanitizerScope::Html)
                })
    }

    fn sql_identifier_template_is_sanitized(&self, template: &TemplateLiteral<'_>) -> bool {
        !template.expressions.is_empty()
            && template
                .expressions
                .iter()
                .enumerate()
                .all(|(index, expr)| {
                    let Some(raw) = template.quasis.get(index).map(template_raw) else {
                        return false;
                    };
                    sql_identifier_template_context(raw)
                        && self.sanitizer_scope_for_expr(expr)
                            == Some(SanitizerScope::SqlIdentifier)
                })
    }

    pub(super) fn record_sanitized_sink_arg(
        &mut self,
        span_start: u32,
        arg_index: u32,
        expr: &Expression<'_>,
    ) {
        let Some(scope) = self.sanitizer_scope_for_expr(expr) else {
            return;
        };
        self.sanitized_sink_args.push(SanitizedSinkArg {
            span_start,
            arg_index,
            scope,
        });
        self.security_control_sites.push(SecurityControlSite {
            kind: SecurityControlKind::Sanitization,
            callee_path: "sanitized-sink-argument".to_string(),
            span_start,
            span_end: span_start,
        });
    }

    fn record_guarded_path_sink_arg(&mut self, local: &str) {
        let Some(binding) = self.path_sink_binding(local) else {
            return;
        };
        self.sanitized_sink_args.push(SanitizedSinkArg {
            span_start: binding.span_start,
            arg_index: binding.arg_index,
            scope: SanitizerScope::Path,
        });
    }

    pub(super) fn record_fail_closed_guard_after_statement(&mut self, stmt: &Statement<'_>) {
        let Statement::IfStatement(if_stmt) = stmt else {
            return;
        };
        if if_stmt.alternate.is_some() || !statement_exits_current_flow(&if_stmt.consequent) {
            return;
        }
        if let Some(target) = self.url_allowlist_guard_target(&if_stmt.test) {
            self.record_sanitizer_binding(&target, Some(SanitizerScope::Url));
        }
        if let Some(target) = self.path_containment_guard_target(&if_stmt.test) {
            self.record_sanitizer_binding(&target, Some(SanitizerScope::Path));
            self.record_guarded_path_sink_arg(&target);
        }
    }

    fn url_allowlist_guard_target(&self, expr: &Expression<'_>) -> Option<String> {
        let Expression::UnaryExpression(unary) = unwrap_parens(expr) else {
            return None;
        };
        if unary.operator != UnaryOperator::LogicalNot {
            return None;
        }
        let Expression::CallExpression(call) = unwrap_parens(&unary.argument) else {
            return None;
        };
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return None;
        };
        if !matches!(member.property.name.as_str(), "has" | "includes") {
            return None;
        }
        let Expression::Identifier(allowlist) = &member.object else {
            return None;
        };
        if !self.literal_allowlist_binding(&allowlist.name) {
            return None;
        }
        let Some(Argument::Identifier(target)) = call.arguments.first() else {
            return None;
        };
        Some(target.name.to_string())
    }

    fn path_containment_guard_target(&self, expr: &Expression<'_>) -> Option<String> {
        let Expression::LogicalExpression(logical) = unwrap_parens(expr) else {
            return None;
        };
        if logical.operator != LogicalOperator::Or {
            return None;
        }
        let left = path_relative_starts_with_parent(&logical.left)
            .or_else(|| self.path_is_absolute_relative_arg(&logical.left));
        let right = path_relative_starts_with_parent(&logical.right)
            .or_else(|| self.path_is_absolute_relative_arg(&logical.right));
        let (Some(left), Some(right)) = (left, right) else {
            return None;
        };
        if left != right {
            return None;
        }
        self.path_relative_binding(left).map(str::to_string)
    }

    fn path_is_absolute_relative_arg<'b>(&self, expr: &'b Expression<'_>) -> Option<&'b str> {
        let Expression::CallExpression(call) = unwrap_parens(expr) else {
            return None;
        };
        if !self.is_node_path_method_call(call, "isAbsolute") {
            return None;
        }
        let Some(Argument::Identifier(rel)) = call.arguments.first() else {
            return None;
        };
        Some(rel.name.as_str())
    }

    fn is_node_path_method_call(&self, call: &CallExpression<'_>, method: &str) -> bool {
        let Expression::StaticMemberExpression(member) = &call.callee else {
            return false;
        };
        if member.property.name != method {
            return false;
        }
        let Expression::Identifier(object) = &member.object else {
            return false;
        };
        self.node_path_namespace_bindings
            .contains(object.name.as_str())
            && !self.nested_scope_shadows(object.name.as_str())
    }

    pub(super) fn path_sink_binding_for_expr(
        &self,
        expr: &Expression<'_>,
    ) -> Option<SecurityPathSinkBinding> {
        let Expression::CallExpression(call) = unwrap_parens(expr) else {
            return None;
        };
        if !["join", "normalize", "resolve"]
            .iter()
            .any(|method| self.is_node_path_method_call(call, method))
        {
            return None;
        }
        Some(SecurityPathSinkBinding {
            span_start: call.span.start,
            arg_index: 0,
        })
    }

    pub(super) fn path_relative_target_for_expr(&self, expr: &Expression<'_>) -> Option<String> {
        let Expression::CallExpression(call) = unwrap_parens(expr) else {
            return None;
        };
        if !self.is_node_path_method_call(call, "relative") {
            return None;
        }
        let Some(Argument::Identifier(target)) = call.arguments.get(1) else {
            return None;
        };
        Some(target.name.to_string())
    }
}
