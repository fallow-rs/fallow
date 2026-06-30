#[allow(clippy::wildcard_imports, reason = "many sink helper AST types used")]
use oxc_ast::ast::*;
use oxc_span::{GetSpan, Span};

use fallow_types::extract::{
    SecurityControlSite, SinkArgKind, SinkLiteralValue, SinkShape, SinkSite,
    SkippedSecurityCalleeExpressionKind, SkippedSecurityCalleeReason, SkippedSecurityCalleeSite,
};

use super::super::ModuleInfoExtractor;
use super::visit_security_controls::security_control_kind_for_callee;
use super::{
    classify_arg_kind, classify_url_shape, flatten_callee_path, flatten_member_path,
    is_non_literal_arg, is_token_like_security_name, object_key_metadata,
    object_literal_properties, should_capture_hardcoded_secret_literal, sink_literal_value,
    static_string_literal_value, unwrap_parens, unwrap_static_expr,
};

struct ArgSinkSiteInput<'site, 'ast> {
    callee_path: &'site str,
    sink_shape: SinkShape,
    arg_index: u32,
    arg_expr: &'site Expression<'ast>,
    arg_literal: Option<SinkLiteralValue>,
    arg_is_non_literal: bool,
    url_arg_literal: Option<String>,
    span: Span,
}

/// Per-argument inputs to [`ModuleInfoExtractor::push_security_sink_arg`].
///
/// Bundles the cohesive call-/new-expression argument context (the callee path,
/// sink shape, argument index and expression, the call-level arg-0 URL literal,
/// and the owning span) that `capture_call_sink_args` and
/// `capture_new_expression_sink` both assemble per argument.
struct PushSinkArgInput<'site, 'ast> {
    callee_path: &'site str,
    sink_shape: SinkShape,
    arg_index: u32,
    arg_expr: &'site Expression<'ast>,
    url_arg_literal: Option<String>,
    span: Span,
}

fn contains_computed_member(expr: &Expression<'_>) -> bool {
    match unwrap_parens(expr) {
        Expression::ComputedMemberExpression(_) => true,
        Expression::StaticMemberExpression(member) => contains_computed_member(&member.object),
        _ => false,
    }
}

fn skipped_callee_expression_kind(expr: &Expression<'_>) -> SkippedSecurityCalleeExpressionKind {
    match unwrap_parens(expr) {
        Expression::StaticMemberExpression(_) => {
            SkippedSecurityCalleeExpressionKind::StaticMemberExpression
        }
        Expression::ComputedMemberExpression(_) => {
            SkippedSecurityCalleeExpressionKind::ComputedMemberExpression
        }
        Expression::Identifier(_) => SkippedSecurityCalleeExpressionKind::Identifier,
        _ => SkippedSecurityCalleeExpressionKind::Other,
    }
}

fn should_capture_member_assign_sink(
    callee_path: &str,
    arg_literal: Option<&SinkLiteralValue>,
    arg_is_non_literal: bool,
) -> bool {
    arg_is_non_literal
        || arg_literal.is_some_and(|literal| {
            should_capture_literal_sink_value(callee_path, SinkShape::MemberAssign, 0, literal)
        })
}

fn should_capture_literal_sink_value(
    callee_path: &str,
    sink_shape: SinkShape,
    arg_index: u32,
    literal: &SinkLiteralValue,
) -> bool {
    match sink_shape {
        SinkShape::Call | SinkShape::MemberCall => match literal {
            SinkLiteralValue::String(value) => {
                (arg_index == 1 && is_post_message_callee(callee_path) && value == "*")
                    || (arg_index == 0 && is_weak_crypto_literal_callee(callee_path))
                    || (arg_index == 0 && is_string_code_callee(callee_path))
                    || (arg_index == 0 && is_temp_file_literal_callee(callee_path))
                    || (arg_index == 0
                        && is_cleartext_transport_literal_callee(callee_path)
                        && is_cleartext_transport_literal(value))
                    || (arg_index == 0
                        && is_literal_metadata_url_callee(callee_path)
                        && is_metadata_service_literal(value))
            }
            SinkLiteralValue::Integer(_) => arg_index == 1 && is_chmod_literal_callee(callee_path),
            SinkLiteralValue::Boolean(_) | SinkLiteralValue::Null => false,
        },
        SinkShape::NewExpression => match literal {
            SinkLiteralValue::String(value) => {
                arg_index == 0
                    && (callee_path == "Function"
                        || (callee_path == "WebSocket" && is_cleartext_websocket_literal(value)))
            }
            SinkLiteralValue::Integer(_)
            | SinkLiteralValue::Boolean(_)
            | SinkLiteralValue::Null => false,
        },
        SinkShape::MemberAssign => {
            arg_index == 0
                && callee_path == "process.env.NODE_TLS_REJECT_UNAUTHORIZED"
                && matches!(literal, SinkLiteralValue::String(value) if value == "0")
        }
        SinkShape::TaggedTemplate | SinkShape::JsxAttr | SinkShape::SecretLiteral => false,
    }
}

fn is_direct_numeric_clamp_expr(expr: &Expression<'_>) -> bool {
    let Expression::CallExpression(call) = unwrap_static_expr(expr) else {
        return false;
    };
    let Some(callee_path) = flatten_callee_path(&call.callee) else {
        return false;
    };
    if callee_path == "Math.min" {
        return call
            .arguments
            .iter()
            .filter_map(Argument::as_expression)
            .any(|arg| matches!(sink_literal_value(arg), Some(SinkLiteralValue::Integer(_))));
    }
    callee_path == "Math.max"
        && call
            .arguments
            .iter()
            .filter_map(Argument::as_expression)
            .any(is_direct_numeric_clamp_expr)
}

fn is_resource_amplification_callee(
    callee_path: &str,
    sink_shape: SinkShape,
    arg_index: u32,
) -> bool {
    if arg_index != 0 {
        return false;
    }
    match sink_shape {
        SinkShape::Call | SinkShape::NewExpression => callee_path == "Array",
        SinkShape::MemberCall => {
            matches!(
                callee_path,
                "Buffer.alloc" | "Buffer.allocUnsafe" | "Buffer.allocUnsafeSlow"
            ) || matches!(
                callee_path.rsplit('.').next(),
                Some("repeat" | "padStart" | "padEnd")
            )
        }
        SinkShape::MemberAssign
        | SinkShape::TaggedTemplate
        | SinkShape::JsxAttr
        | SinkShape::SecretLiteral => false,
    }
}

fn should_skip_clamped_resource_amplification_arg(
    callee_path: &str,
    sink_shape: SinkShape,
    arg_index: u32,
    expr: &Expression<'_>,
) -> bool {
    is_resource_amplification_callee(callee_path, sink_shape, arg_index)
        && is_direct_numeric_clamp_expr(expr)
}

fn is_post_message_callee(callee_path: &str) -> bool {
    callee_path == "postMessage" || callee_path.ends_with(".postMessage")
}

fn is_weak_crypto_literal_callee(callee_path: &str) -> bool {
    matches!(
        callee_path,
        "createHash"
            | "createCipher"
            | "createDecipher"
            | "createCipheriv"
            | "createDecipheriv"
            | "crypto.createHash"
            | "crypto.createCipher"
            | "crypto.createDecipher"
            | "crypto.createCipheriv"
            | "crypto.createDecipheriv"
    )
}

fn is_string_code_callee(callee_path: &str) -> bool {
    matches!(callee_path, "setTimeout" | "setInterval")
}

fn is_chmod_literal_callee(callee_path: &str) -> bool {
    matches!(
        callee_path,
        "fs.chmod" | "fs.chmodSync" | "fs.promises.chmod" | "chmod" | "chmodSync"
    )
}

fn is_temp_file_literal_callee(callee_path: &str) -> bool {
    matches!(
        callee_path,
        "fs.writeFile"
            | "fs.writeFileSync"
            | "fs.appendFile"
            | "fs.appendFileSync"
            | "fs.createWriteStream"
            | "fs.promises.writeFile"
            | "fs.promises.appendFile"
            | "writeFile"
            | "writeFileSync"
            | "appendFile"
            | "appendFileSync"
            | "createWriteStream"
    )
}

fn is_literal_metadata_url_callee(callee_path: &str) -> bool {
    matches!(
        callee_path,
        "fetch"
            | "axios.get"
            | "axios.post"
            | "got"
            | "ky"
            | "needle"
            | "request"
            | "http.request"
            | "https.request"
            | "undici.request"
    )
}

fn is_cleartext_transport_literal_callee(callee_path: &str) -> bool {
    matches!(
        callee_path,
        "fetch"
            | "axios.get"
            | "axios.post"
            | "got"
            | "ky"
            | "needle"
            | "request"
            | "http.request"
            | "http.get"
            | "superagent.get"
            | "undici.request"
    )
}

fn is_cleartext_transport_literal(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("ftp://")
}

fn is_cleartext_websocket_literal(value: &str) -> bool {
    value.to_ascii_lowercase().starts_with("ws://")
}

fn is_metadata_service_literal(value: &str) -> bool {
    value.contains("169.254.169.254") || value.contains("metadata.google.internal")
}

fn should_capture_missing_jwt_verify_options(
    callee_path: &str,
    sink_shape: SinkShape,
    arg_len: usize,
) -> bool {
    arg_len == 2
        && matches!(sink_shape, SinkShape::Call | SinkShape::MemberCall)
        && (callee_path == "verify" || callee_path.ends_with(".verify"))
}

/// Collect the bare identifier names referenced anywhere inside a sink argument
/// expression, deduped in source order. Used by the analyze layer to back-trace
/// the argument to a source-tainted local binding. This is a bounded, shallow
/// structural walk over the common taint-carrying shapes (member roots, binary /
/// template / call / paren / conditional / sequence / await / unary), NOT a full
/// expression evaluator: an identifier that never surfaces in these shapes is
/// simply not collected (a conservative miss, never a false source link).
fn collect_arg_idents(expr: &Expression<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_idents_into(expr, &mut out);
    out
}

fn collect_arg_source_paths(expr: &Expression<'_>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    collect_source_paths_into(expr, &mut out);
    out
}

/// The arg-0 URL of a call as a static string literal, for the `secret-to-network`
/// destination signal (#890). `Some(literal)` when the destination is a plain
/// string literal or a no-substitution template (almost always intended auth);
/// `None` when it is dynamic (an interpolated URL, an env-configured base, a
/// variable) or absent. A dynamic destination is the higher-signal exfil case.
fn call_url_arg_literal(expr: &CallExpression<'_>) -> Option<String> {
    match expr.arguments.first()?.as_expression()? {
        Expression::StringLiteral(lit) => Some(lit.value.to_string()),
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => {
            tpl.quasis.first().map(|q| q.value.raw.to_string())
        }
        _ => None,
    }
}

fn push_ident(name: &str, out: &mut Vec<String>) {
    if !out.iter().any(|n| n == name) {
        out.push(name.to_string());
    }
}

fn push_source_path(path: String, out: &mut Vec<String>) {
    if !out.iter().any(|existing| existing == &path) {
        out.push(path);
    }
}

fn push_member_source_paths(path: &str, out: &mut Vec<String>) {
    // A public env var is build-inlined, not a secret source; record neither the
    // full path nor the `process.env` / `import.meta.env` object prefix (#890).
    if fallow_types::extract::is_public_env_path(path) {
        return;
    }
    push_source_path(path.to_string(), out);
    if let Some((object, _)) = path.rsplit_once('.') {
        push_source_path(object.to_string(), out);
    }
}

/// Collect secret source paths from a `StaticMemberExpression`: flatten the
/// member path and push it (and its source prefixes), then recurse into the
/// object. #890: a public env read (`process.env.NEXT_PUBLIC_*`) contributes no
/// secret source, and returns before recursing so the bare `process.env` /
/// `import.meta.env` object is not re-pushed as a source and defeat the
/// exclusion.
fn collect_static_member_source_path(
    expr: &Expression<'_>,
    member: &StaticMemberExpression<'_>,
    out: &mut Vec<String>,
) {
    if let Some(path) = flatten_member_path(expr) {
        if fallow_types::extract::is_public_env_path(&path) {
            return;
        }
        push_member_source_paths(&path, out);
    }
    collect_source_paths_into(&member.object, out);
}

fn collect_source_paths_into(expr: &Expression<'_>, out: &mut Vec<String>) {
    match expr {
        Expression::ParenthesizedExpression(paren) => {
            collect_source_paths_into(&paren.expression, out);
        }
        Expression::TSAsExpression(ts_as) => {
            collect_source_paths_into(&ts_as.expression, out);
        }
        Expression::TSSatisfiesExpression(ts_sat) => {
            collect_source_paths_into(&ts_sat.expression, out);
        }
        Expression::TSNonNullExpression(ts_non_null) => {
            collect_source_paths_into(&ts_non_null.expression, out);
        }
        Expression::StaticMemberExpression(member) => {
            collect_static_member_source_path(expr, member, out);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_source_paths_into(&member.object, out);
            collect_source_paths_into(&member.expression, out);
        }
        Expression::BinaryExpression(bin) => {
            collect_source_paths_into(&bin.left, out);
            collect_source_paths_into(&bin.right, out);
        }
        Expression::LogicalExpression(logical) => {
            collect_source_paths_into(&logical.left, out);
            collect_source_paths_into(&logical.right, out);
        }
        Expression::ConditionalExpression(cond) => {
            collect_source_paths_into(&cond.test, out);
            collect_source_paths_into(&cond.consequent, out);
            collect_source_paths_into(&cond.alternate, out);
        }
        Expression::SequenceExpression(seq) => {
            for e in &seq.expressions {
                collect_source_paths_into(e, out);
            }
        }
        Expression::TemplateLiteral(tpl) => {
            for e in &tpl.expressions {
                collect_source_paths_into(e, out);
            }
        }
        Expression::AwaitExpression(await_expr) => {
            collect_source_paths_into(&await_expr.argument, out);
        }
        Expression::UnaryExpression(unary) => collect_source_paths_into(&unary.argument, out),
        Expression::CallExpression(call) => {
            collect_source_paths_into(&call.callee, out);
            for arg in &call.arguments {
                if let Some(arg_expr) = arg.as_expression() {
                    collect_source_paths_into(arg_expr, out);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            // A direct source read nested in an object literal value
            // (`{ content: req.body.text }`) still carries taint.
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(prop) = prop {
                    collect_source_paths_into(&prop.value, out);
                }
            }
        }
        Expression::ArrayExpression(array) => {
            // A direct source read nested in an array element, including an object
            // in an array (`messages: [{ content: req.body.text }]`).
            for element in &array.elements {
                if let Some(element_expr) = element.as_expression() {
                    collect_source_paths_into(element_expr, out);
                }
            }
        }
        _ => {}
    }
}

fn collect_idents_into(expr: &Expression<'_>, out: &mut Vec<String>) {
    match expr {
        Expression::Identifier(ident) => push_ident(&ident.name, out),
        Expression::ParenthesizedExpression(paren) => collect_idents_into(&paren.expression, out),
        Expression::TSAsExpression(ts_as) => collect_idents_into(&ts_as.expression, out),
        Expression::TSSatisfiesExpression(ts_sat) => collect_idents_into(&ts_sat.expression, out),
        Expression::TSNonNullExpression(ts_non_null) => {
            collect_idents_into(&ts_non_null.expression, out);
        }
        Expression::StaticMemberExpression(member) => {
            // The leading object root carries the taint (`id` in `id.value`,
            // `req` in `req.query.id`); the property name is a static key.
            collect_idents_into(&member.object, out);
        }
        Expression::ComputedMemberExpression(member) => {
            collect_idents_into(&member.object, out);
            collect_idents_into(&member.expression, out);
        }
        Expression::BinaryExpression(bin) => {
            collect_idents_into(&bin.left, out);
            collect_idents_into(&bin.right, out);
        }
        Expression::LogicalExpression(logical) => {
            collect_idents_into(&logical.left, out);
            collect_idents_into(&logical.right, out);
        }
        Expression::ConditionalExpression(cond) => {
            collect_idents_into(&cond.test, out);
            collect_idents_into(&cond.consequent, out);
            collect_idents_into(&cond.alternate, out);
        }
        Expression::SequenceExpression(seq) => {
            for e in &seq.expressions {
                collect_idents_into(e, out);
            }
        }
        Expression::TemplateLiteral(tpl) => {
            for e in &tpl.expressions {
                collect_idents_into(e, out);
            }
        }
        Expression::AwaitExpression(await_expr) => collect_idents_into(&await_expr.argument, out),
        Expression::UnaryExpression(unary) => collect_idents_into(&unary.argument, out),
        Expression::CallExpression(call) => {
            // The callee can carry the taint (`getId().trim()` -> getId), as can
            // each argument (`escape(id)` -> id). Bounded one level by recursion.
            collect_idents_into(&call.callee, out);
            for arg in &call.arguments {
                if let Some(arg_expr) = arg.as_expression() {
                    collect_idents_into(arg_expr, out);
                }
            }
        }
        Expression::ObjectExpression(obj) => {
            for prop in &obj.properties {
                if let ObjectPropertyKind::ObjectProperty(prop) = prop {
                    collect_idents_into(&prop.value, out);
                }
            }
        }
        Expression::ArrayExpression(array) => {
            // Taint can ride an array element, including an object nested in an
            // array (`messages: [{ content: userInput }]`, the canonical OpenAI /
            // Anthropic chat shape). Recurse into each element expression.
            for element in &array.elements {
                if let Some(element_expr) = element.as_expression() {
                    collect_idents_into(element_expr, out);
                }
            }
        }
        _ => {}
    }
}

impl ModuleInfoExtractor {
    fn capture_redos_regex_sink(&mut self, expr: &CallExpression<'_>) {
        let Some((input_expr, pattern)) = self.redos_regex_application(expr) else {
            return;
        };
        if !is_non_literal_arg(input_expr) {
            return;
        }
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::MemberCall,
            callee_path: "RegExp.redos".to_string(),
            arg_index: 0,
            arg_is_non_literal: true,
            arg_kind: classify_arg_kind(input_expr),
            arg_literal: None,
            regex_pattern: Some(pattern),
            object_properties: Vec::new(),
            object_property_keys: Vec::new(),
            object_property_keys_complete: false,
            arg_idents: collect_arg_idents(input_expr),
            arg_source_paths: collect_arg_source_paths(input_expr),
            span_start: expr.span.start,
            span_end: expr.span.end,
            url_arg_literal: None,
            url_shape: None,
        });
    }

    fn capture_security_control_call(&mut self, callee_path: &str, span: Span) {
        let Some(kind) = security_control_kind_for_callee(callee_path) else {
            return;
        };
        self.security_control_sites.push(SecurityControlSite {
            kind,
            callee_path: callee_path.to_string(),
            span_start: span.start,
            span_end: span.end,
        });
    }

    pub(super) fn capture_security_call_sites(&mut self, expr: &CallExpression<'_>) {
        self.capture_redos_regex_sink(expr);
        self.capture_declarative_validation_control(expr);
        self.capture_call_sink(expr);
    }

    fn record_skipped_security_callee(
        &mut self,
        callee: &Expression<'_>,
        reason: SkippedSecurityCalleeReason,
    ) {
        let callee = super::unwrap_parens(callee);
        self.security_sinks_skipped += 1;
        self.security_unresolved_callee_sites
            .push(SkippedSecurityCalleeSite {
                reason,
                expression_kind: skipped_callee_expression_kind(callee),
                span_start: callee.span().start,
                span_end: callee.span().end,
            });
    }

    /// Capture a call/member-call sink site (category-blind). Pushes one
    /// `SinkSite` per admitted positional argument; a callee that cannot be
    /// flattened to a static path increments the blind-spot counter instead.
    /// Capture one argument of a call / new-expression sink into
    /// `security_sinks`, applying the literal / non-literal capture gates.
    ///
    /// Shared by `capture_call_sink` and `capture_new_expression_sink` (oxc
    /// represents `new X(...)` as a distinct `NewExpression`), keeping the
    /// per-argument `SinkSite` construction byte-identical across both shapes.
    /// `input.url_arg_literal` is the call-level arg-0 URL signal (always `None`
    /// for new-expressions). `input.span` is the owning call / new expression's
    /// span.
    fn push_security_sink_arg(&mut self, input: PushSinkArgInput<'_, '_>) {
        let PushSinkArgInput {
            callee_path,
            sink_shape,
            arg_index,
            arg_expr,
            url_arg_literal,
            span,
        } = input;
        let arg_literal = self.static_sink_literal_value(arg_expr);
        let arg_is_non_literal = arg_literal.is_none() && is_non_literal_arg(arg_expr);
        if arg_is_non_literal
            && should_skip_clamped_resource_amplification_arg(
                callee_path,
                sink_shape,
                arg_index,
                arg_expr,
            )
        {
            return;
        }
        if !arg_is_non_literal
            && !arg_literal.as_ref().is_some_and(|literal| {
                should_capture_literal_sink_value(callee_path, sink_shape, arg_index, literal)
            })
        {
            return;
        }
        if arg_is_non_literal {
            self.record_sanitized_sink_arg(span.start, arg_index, arg_expr);
        }
        let site = self.build_arg_sink_site(ArgSinkSiteInput {
            callee_path,
            sink_shape,
            arg_index,
            arg_expr,
            arg_literal,
            arg_is_non_literal,
            url_arg_literal,
            span,
        });
        self.security_sinks.push(site);
    }

    /// Build the per-argument `SinkSite` for `push_security_sink_arg` once the
    /// capture gates have passed. Non-literal arguments carry the ident /
    /// source-path / arg-kind / url-shape metadata; literal arguments carry the
    /// literal value with empty metadata.
    fn build_arg_sink_site(&self, input: ArgSinkSiteInput<'_, '_>) -> SinkSite {
        let ArgSinkSiteInput {
            callee_path,
            sink_shape,
            arg_index,
            arg_expr,
            arg_literal,
            arg_is_non_literal,
            url_arg_literal,
            span,
        } = input;
        let object_keys = object_key_metadata(arg_expr);
        SinkSite {
            sink_shape,
            callee_path: callee_path.to_string(),
            arg_index,
            arg_is_non_literal,
            arg_kind: if arg_is_non_literal {
                classify_arg_kind(arg_expr)
            } else {
                SinkArgKind::Literal
            },
            arg_literal,
            object_properties: object_literal_properties(arg_expr),
            object_property_keys: object_keys.keys,
            object_property_keys_complete: object_keys.complete,
            arg_idents: if arg_is_non_literal {
                collect_arg_idents(arg_expr)
            } else {
                Vec::new()
            },
            arg_source_paths: if arg_is_non_literal {
                collect_arg_source_paths(arg_expr)
            } else {
                Vec::new()
            },
            regex_pattern: None,
            span_start: span.start,
            span_end: span.end,
            url_arg_literal,
            url_shape: if arg_is_non_literal {
                classify_url_shape(arg_expr, &self.static_string_bindings)
            } else {
                None
            },
        }
    }

    fn capture_call_sink(&mut self, expr: &CallExpression<'_>) {
        let Some(callee_path) = flatten_callee_path(&expr.callee) else {
            self.record_unresolved_call_sink(expr);
            return;
        };
        self.capture_security_control_call(&callee_path, expr.span);
        let sink_shape = if callee_path.contains('.') {
            SinkShape::MemberCall
        } else {
            SinkShape::Call
        };
        self.capture_call_sink_args(expr, &callee_path, sink_shape);
        if should_capture_missing_jwt_verify_options(&callee_path, sink_shape, expr.arguments.len())
        {
            self.security_sinks.push(SinkSite {
                sink_shape,
                callee_path,
                arg_index: 2,
                arg_is_non_literal: false,
                arg_kind: SinkArgKind::Object,
                arg_literal: None,
                object_properties: Vec::new(),
                object_property_keys: Vec::new(),
                object_property_keys_complete: true,
                arg_idents: Vec::new(),
                arg_source_paths: Vec::new(),
                regex_pattern: None,
                span_start: expr.span.start,
                span_end: expr.span.end,
                url_arg_literal: None,
                url_shape: None,
            });
        }
    }

    fn record_unresolved_call_sink(&mut self, expr: &CallExpression<'_>) {
        if self.redos_regex_application(expr).is_some() {
            return;
        }
        let reason = if contains_computed_member(&expr.callee) {
            SkippedSecurityCalleeReason::ComputedMember
        } else {
            SkippedSecurityCalleeReason::DynamicDispatch
        };
        self.record_skipped_security_callee(&expr.callee, reason);
    }

    fn capture_call_sink_args(
        &mut self,
        expr: &CallExpression<'_>,
        callee_path: &str,
        sink_shape: SinkShape,
    ) {
        // The arg-0 URL literal is captured once per call so secret-to-network
        // findings can carry a destination-host signal on the arg-1 sink.
        let url_arg_literal = call_url_arg_literal(expr);
        for (index, arg) in expr.arguments.iter().enumerate() {
            let Some(arg_expr) = arg.as_expression() else {
                continue;
            };
            let Ok(arg_index) = u32::try_from(index) else {
                continue;
            };
            self.push_security_sink_arg(PushSinkArgInput {
                callee_path,
                sink_shape,
                arg_index,
                arg_expr,
                url_arg_literal: url_arg_literal.clone(),
                span: expr.span,
            });
        }
    }

    /// Capture constructor-call sink sites. This is intentionally separate from
    /// call capture because oxc represents `new Function("...")` as a
    /// `NewExpression`, not a `CallExpression`.
    pub(super) fn capture_new_expression_sink(&mut self, expr: &NewExpression<'_>) {
        let Some(callee_path) = flatten_callee_path(&expr.callee) else {
            return;
        };
        for (index, arg) in expr.arguments.iter().enumerate() {
            let Some(arg_expr) = arg.as_expression() else {
                continue;
            };
            let Ok(arg_index) = u32::try_from(index) else {
                continue;
            };
            self.push_security_sink_arg(PushSinkArgInput {
                callee_path: &callee_path,
                sink_shape: SinkShape::NewExpression,
                arg_index,
                arg_expr,
                url_arg_literal: None,
                span: expr.span,
            });
        }
    }

    pub(super) fn capture_math_random_context_sink(
        &mut self,
        context_name: &str,
        expr: &Expression<'_>,
        span: Span,
    ) {
        if !is_token_like_security_name(context_name)
            || !super::expression_contains_math_random_call(expr)
        {
            return;
        }
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::MemberCall,
            callee_path: "Math.random".to_string(),
            arg_index: 0,
            arg_is_non_literal: false,
            arg_kind: SinkArgKind::NoArg,
            arg_literal: None,
            object_properties: Vec::new(),
            object_property_keys: Vec::new(),
            object_property_keys_complete: false,
            arg_idents: vec![context_name.to_string()],
            arg_source_paths: Vec::new(),
            regex_pattern: None,
            span_start: span.start,
            span_end: span.end,
            url_arg_literal: None,
            url_shape: None,
        });
    }

    pub(super) fn capture_hardcoded_secret_literal_sink(
        &mut self,
        context_name: &str,
        expr: &Expression<'_>,
        span: Span,
    ) {
        let Some(value) = static_string_literal_value(expr) else {
            return;
        };
        if !should_capture_hardcoded_secret_literal(context_name, &value) {
            return;
        }
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::SecretLiteral,
            callee_path: context_name.to_string(),
            arg_index: 0,
            arg_is_non_literal: false,
            arg_kind: SinkArgKind::Literal,
            arg_literal: Some(SinkLiteralValue::String(value)),
            regex_pattern: None,
            object_properties: Vec::new(),
            object_property_keys: Vec::new(),
            object_property_keys_complete: false,
            arg_idents: vec![context_name.to_string()],
            arg_source_paths: Vec::new(),
            span_start: span.start,
            span_end: span.end,
            url_arg_literal: None,
            url_shape: None,
        });
    }

    /// Capture a member-assignment sink site (e.g. `el.innerHTML = userInput`).
    /// Static-member targets with a non-literal RHS are captured; one exact
    /// literal TLS-env assignment is admitted because the literal value is the
    /// security signal. A target whose object cannot be flattened increments
    /// the blind-spot counter.
    pub(super) fn capture_member_assign_sink(&mut self, expr: &AssignmentExpression<'_>) {
        let AssignmentTarget::StaticMemberExpression(member) = &expr.left else {
            return;
        };
        let Some(callee_path) = self.member_assign_callee_path(member) else {
            return;
        };
        let arg_literal = self.static_sink_literal_value(&expr.right);
        let arg_is_non_literal = arg_literal.is_none() && is_non_literal_arg(&expr.right);
        if !should_capture_member_assign_sink(
            &callee_path,
            arg_literal.as_ref(),
            arg_is_non_literal,
        ) {
            return;
        }
        self.record_member_assign_sink(expr, callee_path, arg_literal, arg_is_non_literal);
    }

    fn member_assign_callee_path(&mut self, member: &StaticMemberExpression<'_>) -> Option<String> {
        let Some(object_path) = flatten_callee_path(&member.object) else {
            self.record_skipped_security_callee(
                &member.object,
                SkippedSecurityCalleeReason::UnsupportedAssignmentObject,
            );
            return None;
        };
        Some(format!("{}.{}", object_path, member.property.name))
    }

    fn record_member_assign_sink(
        &mut self,
        expr: &AssignmentExpression<'_>,
        callee_path: String,
        arg_literal: Option<SinkLiteralValue>,
        arg_is_non_literal: bool,
    ) {
        if arg_is_non_literal {
            self.record_sanitized_sink_arg(expr.span.start, 0, &expr.right);
        }
        let object_keys = object_key_metadata(&expr.right);
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::MemberAssign,
            callee_path,
            arg_index: 0,
            arg_is_non_literal,
            arg_kind: if arg_is_non_literal {
                classify_arg_kind(&expr.right)
            } else {
                SinkArgKind::Literal
            },
            arg_literal,
            object_properties: object_literal_properties(&expr.right),
            object_property_keys: object_keys.keys,
            object_property_keys_complete: object_keys.complete,
            arg_idents: if arg_is_non_literal {
                collect_arg_idents(&expr.right)
            } else {
                Vec::new()
            },
            arg_source_paths: if arg_is_non_literal {
                collect_arg_source_paths(&expr.right)
            } else {
                Vec::new()
            },
            regex_pattern: None,
            span_start: expr.span.start,
            span_end: expr.span.end,
            url_arg_literal: None,
            url_shape: if arg_is_non_literal {
                classify_url_shape(&expr.right, &self.static_string_bindings)
            } else {
                None
            },
        });
    }

    /// Capture a tagged-template sink site (e.g. ``sql`...${x}...` ``). Only
    /// templates with at least one substitution are captured.
    pub(super) fn capture_tagged_template_sink(&mut self, expr: &TaggedTemplateExpression<'_>) {
        if expr.quasi.expressions.is_empty() {
            return;
        }
        let Some(callee_path) = flatten_callee_path(&expr.tag) else {
            return;
        };
        let mut arg_idents: Vec<String> = Vec::new();
        let mut arg_source_paths: Vec<String> = Vec::new();
        for substitution in &expr.quasi.expressions {
            collect_idents_into(substitution, &mut arg_idents);
            collect_source_paths_into(substitution, &mut arg_source_paths);
        }
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::TaggedTemplate,
            callee_path,
            arg_index: 0,
            arg_is_non_literal: true,
            // A tagged template is captured only with substitutions, so the
            // argument is always a template-with-substitution.
            arg_kind: SinkArgKind::TemplateWithSubst,
            arg_literal: None,
            object_properties: Vec::new(),
            object_property_keys: Vec::new(),
            object_property_keys_complete: false,
            arg_idents,
            arg_source_paths,
            regex_pattern: None,
            span_start: expr.span.start,
            span_end: expr.span.end,
            url_arg_literal: None,
            url_shape: None,
        });
    }

    /// Capture a JSX-attribute sink site (e.g. `dangerouslySetInnerHTML={x}`).
    /// Only identifier-named attributes with a non-literal expression-container
    /// value are captured; the empty `{}` form yields no expression and is
    /// skipped without an explicit arm.
    pub(super) fn capture_jsx_attr_sink(&mut self, attr: &JSXAttribute<'_>) {
        let JSXAttributeName::Identifier(name) = &attr.name else {
            return;
        };
        let Some(JSXAttributeValue::ExpressionContainer(container)) = &attr.value else {
            return;
        };
        let Some(value_expr) = container.expression.as_expression() else {
            return;
        };
        if !is_non_literal_arg(value_expr) {
            return;
        }
        self.record_sanitized_sink_arg(attr.span.start, 0, value_expr);
        let object_keys = object_key_metadata(value_expr);
        self.security_sinks.push(SinkSite {
            sink_shape: SinkShape::JsxAttr,
            callee_path: name.name.to_string(),
            arg_index: 0,
            arg_is_non_literal: true,
            arg_kind: classify_arg_kind(value_expr),
            arg_literal: None,
            object_properties: object_literal_properties(value_expr),
            object_property_keys: object_keys.keys,
            object_property_keys_complete: object_keys.complete,
            arg_idents: collect_arg_idents(value_expr),
            arg_source_paths: collect_arg_source_paths(value_expr),
            regex_pattern: None,
            span_start: attr.span.start,
            span_end: attr.span.end,
            url_arg_literal: None,
            url_shape: None,
        });
    }
}
