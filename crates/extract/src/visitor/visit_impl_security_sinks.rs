use oxc_ast::ast::*;
use oxc_span::{GetSpan, Span};
use rustc_hash::{FxHashMap, FxHashSet};
use std::borrow::Cow;

use fallow_types::extract::{
    SecurityControlSite, SinkArgKind, SinkLiteralValue, SinkShape, SinkSite,
    SkippedSecurityCalleeExpressionKind, SkippedSecurityCalleeReason, SkippedSecurityCalleeSite,
};
use fallow_types::security_sinks::{JSX_SPREAD_SINK_PROPS, node_binding_module};

use super::super::ModuleInfoExtractor;
use super::visit_security_controls::security_control_kind_for_callee;
use super::visit_source_paths::join_member_path;
use super::{
    classify_arg_kind, classify_url_shape, flatten_member_path, is_non_literal_arg,
    is_static_write_head_headers, is_token_like_security_name, object_key_metadata,
    object_literal_properties, should_capture_hardcoded_secret_literal, sink_literal_value,
    static_string_literal_value, unwrap_parens, unwrap_static_expr,
};
use crate::js_text::JsText;

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

/// What a local binding holds, when that value changes the meaning of a sink.
#[derive(Debug, Clone, PartialEq, Eq)]
enum SinkBinding {
    /// A code-executing global (`const e = eval`), valued by the global name.
    CodeExec(String),
    /// A Node.js built-in module (`const vm = require("vm")`), valued by the
    /// `node:` path that the catalogue matches (`node:vm`).
    NodeModule(String),
    /// A member of a Node.js built-in module (`import { runInNewContext }
    /// from "vm"`), valued by the `node:` member path
    /// (`node:vm.runInNewContext`).
    NodeModuleMember(String),
    /// A `<script>` element (`document.createElement("script")`).
    ScriptElement,
}

/// Properties of a `<script>` element whose value the browser loads or runs
/// as code.
const SCRIPT_ELEMENT_CODE_PROPERTIES: &[&str] = &["src", "text", "textContent", "innerText"];

/// Callee root that names a proven `<script>` element in a sink path.
const SCRIPT_ELEMENT_ROOT: &str = "HTMLScriptElement";

/// Methods that parse their argument as markup. A call of one of these
/// methods on an object that the static path cannot flatten
/// (`document.createRange().createContextualFragment(x)`) still resolves
/// through [`ModuleInfoExtractor::sink_object_path`].
const OBJECT_ROOTED_HTML_METHODS: &[&str] = &["createContextualFragment", "insertAdjacentHTML"];

/// Per-module local bindings that re-expose a sink under another name.
#[derive(Default)]
pub(in crate::visitor) struct SinkAliasState {
    /// Module-scope bindings that change the meaning of a sink, keyed by the
    /// local name.
    bindings: FxHashMap<String, SinkBinding>,
    /// Per `nested_declaration_stack` frame: the sink bindings that the frame
    /// declares. A name that a nearer frame declares without such a value
    /// hides an outer binding.
    scoped_bindings: Vec<FxHashMap<String, SinkBinding>>,
    /// Names that an enclosing `catch` clause binds. The visitor does not
    /// record a catch parameter in `nested_declaration_stack`, so these names
    /// hide every alias.
    catch_param_names: Vec<FxHashSet<String>>,
    /// JSX sink props of locals bound to an object literal, keyed by the local
    /// name. A JSX spread of the local (`<div {...props} />`) emits them.
    jsx_spread_object_sinks: FxHashMap<String, Vec<SinkSite>>,
}

/// Flatten an `Identifier` or `StaticMemberExpression` callee to a dotted path.
///
/// Deliberately narrower than `static_member_object_name`: it accepts ONLY bare
/// identifiers and static member chains (no call/new forms), so the catalogue
/// matcher sees a clean dotted callee path. Returns `None` for dynamic dispatch,
/// computed members, or aliased call forms (those count as blind spots).
pub(in crate::visitor) fn flatten_callee_path(mut expr: &Expression<'_>) -> Option<String> {
    let mut properties = Vec::new();
    let root = loop {
        crate::cost::walk_step();
        match unwrap_parens(expr) {
            Expression::Identifier(ident) => break ident.name.as_str(),
            Expression::StaticMemberExpression(member) => {
                properties.push(member.property.name.as_str());
                expr = &member.object;
            }
            _ => return None,
        }
    };
    Some(join_member_path(root, &properties))
}

/// Global objects whose member access reaches the global binding itself.
const GLOBAL_OBJECT_ROOTS: &[&str] = &["globalThis", "window", "self", "global"];

/// Code-executing globals that keep their meaning behind a global-object
/// prefix (`window.eval`) or a local alias (`const e = eval`).
const CODE_EXEC_GLOBALS: &[&str] = &["eval", "Function", "setTimeout", "setInterval"];

/// Map `globalThis.eval`, `window.Function` and `self.setTimeout` to the bare
/// global, so the catalogue `call` rows see the same callee as the direct form.
/// Only the code-executing globals are mapped: other globals keep their member
/// path, so existing `*.member` rows and fingerprints do not move.
fn normalize_global_code_exec_path(path: String) -> String {
    if let Some((root, rest)) = path.split_once('.')
        && GLOBAL_OBJECT_ROOTS.contains(&root)
        && CODE_EXEC_GLOBALS.contains(&rest)
    {
        return rest.to_string();
    }
    path
}

fn set_sink_binding(
    bindings: &mut FxHashMap<String, SinkBinding>,
    name: &str,
    binding: Option<SinkBinding>,
) {
    match binding {
        Some(binding) => {
            bindings.insert(name.to_string(), binding);
        }
        None => {
            bindings.remove(name);
        }
    }
}

/// Whether `expr` is `<object>.createElement("script")`.
fn is_create_script_element_call(expr: &Expression<'_>) -> bool {
    let Expression::CallExpression(call) = unwrap_static_expr(expr) else {
        return false;
    };
    let Expression::StaticMemberExpression(member) = unwrap_parens(&call.callee) else {
        return false;
    };
    member.property.name == "createElement"
        && call
            .arguments
            .first()
            .and_then(Argument::as_expression)
            .and_then(static_string_literal_value)
            .is_some_and(|tag| tag.eq_ignore_ascii_case("script"))
}

/// Whether an unresolved computed assignment key builds a property name from
/// string parts (`el["inner" + suffix]`, `` el[`${kind}HTML`] ``). These keys
/// are the evasion shape, so they become blind spots. A `+` chain must hold a
/// string operand: index arithmetic (`data[i + 1] = v`) and a bare identifier
/// key (`cache[key] = v`) are data writes and stay out of the blind-spot
/// report.
fn is_string_building_key(expr: &Expression<'_>) -> bool {
    match unwrap_static_expr(expr) {
        Expression::BinaryExpression(bin) => {
            bin.operator == BinaryOperator::Addition
                && (has_string_operand(&bin.left) || has_string_operand(&bin.right))
        }
        Expression::TemplateLiteral(tpl) => !tpl.expressions.is_empty(),
        _ => false,
    }
}

fn has_string_operand(expr: &Expression<'_>) -> bool {
    match unwrap_static_expr(expr) {
        Expression::StringLiteral(_) | Expression::TemplateLiteral(_) => true,
        Expression::BinaryExpression(bin) if bin.operator == BinaryOperator::Addition => {
            has_string_operand(&bin.left) || has_string_operand(&bin.right)
        }
        _ => false,
    }
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
        "fs.chmod" | "fs.chmodSync" | "fs.promises.chmod" | "node:fs.chmod" | "node:fs.chmodSync"
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
            | "node:fs.writeFile"
            | "node:fs.writeFileSync"
            | "node:fs.appendFile"
            | "node:fs.appendFileSync"
            | "node:fs.createWriteStream"
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
        Expression::StringLiteral(lit) => Some(lit.value.text().into_owned()),
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
        Expression::BinaryExpression(bin) => collect_binary_source_paths(bin, out),
        Expression::LogicalExpression(logical) => collect_logical_source_paths(logical, out),
        Expression::ConditionalExpression(cond) => collect_conditional_source_paths(cond, out),
        Expression::SequenceExpression(seq) => collect_sequence_source_paths(seq, out),
        Expression::TemplateLiteral(tpl) => collect_template_source_paths(tpl, out),
        Expression::AwaitExpression(await_expr) => {
            collect_source_paths_into(&await_expr.argument, out);
        }
        Expression::UnaryExpression(unary) => collect_source_paths_into(&unary.argument, out),
        Expression::CallExpression(call) => collect_call_source_paths(call, out),
        Expression::ObjectExpression(obj) => collect_object_source_paths(obj, out),
        Expression::ArrayExpression(array) => collect_array_source_paths(array, out),
        _ => {}
    }
}

fn collect_binary_source_paths(bin: &BinaryExpression<'_>, out: &mut Vec<String>) {
    collect_source_paths_into(&bin.left, out);
    collect_source_paths_into(&bin.right, out);
}

fn collect_logical_source_paths(logical: &LogicalExpression<'_>, out: &mut Vec<String>) {
    collect_source_paths_into(&logical.left, out);
    collect_source_paths_into(&logical.right, out);
}

fn collect_conditional_source_paths(cond: &ConditionalExpression<'_>, out: &mut Vec<String>) {
    collect_source_paths_into(&cond.test, out);
    collect_source_paths_into(&cond.consequent, out);
    collect_source_paths_into(&cond.alternate, out);
}

fn collect_sequence_source_paths(seq: &SequenceExpression<'_>, out: &mut Vec<String>) {
    for expr in &seq.expressions {
        collect_source_paths_into(expr, out);
    }
}

fn collect_template_source_paths(tpl: &TemplateLiteral<'_>, out: &mut Vec<String>) {
    for expr in &tpl.expressions {
        collect_source_paths_into(expr, out);
    }
}

fn collect_call_source_paths(call: &CallExpression<'_>, out: &mut Vec<String>) {
    collect_source_paths_into(&call.callee, out);
    for arg in &call.arguments {
        if let Some(arg_expr) = arg.as_expression() {
            collect_source_paths_into(arg_expr, out);
        }
    }
}

fn collect_object_source_paths(obj: &ObjectExpression<'_>, out: &mut Vec<String>) {
    for prop in &obj.properties {
        if let ObjectPropertyKind::ObjectProperty(prop) = prop {
            collect_source_paths_into(&prop.value, out);
        }
    }
}

fn collect_array_source_paths(array: &ArrayExpression<'_>, out: &mut Vec<String>) {
    for element in &array.elements {
        if let Some(element_expr) = element.as_expression() {
            collect_source_paths_into(element_expr, out);
        }
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
        Expression::BinaryExpression(bin) => collect_binary_idents(bin, out),
        Expression::LogicalExpression(logical) => collect_logical_idents(logical, out),
        Expression::ConditionalExpression(cond) => collect_conditional_idents(cond, out),
        Expression::SequenceExpression(seq) => collect_sequence_idents(seq, out),
        Expression::TemplateLiteral(tpl) => collect_template_idents(tpl, out),
        Expression::AwaitExpression(await_expr) => collect_idents_into(&await_expr.argument, out),
        Expression::UnaryExpression(unary) => collect_idents_into(&unary.argument, out),
        Expression::CallExpression(call) => collect_call_idents(call, out),
        Expression::ObjectExpression(obj) => collect_object_idents(obj, out),
        Expression::ArrayExpression(array) => collect_array_idents(array, out),
        _ => {}
    }
}

fn collect_binary_idents(bin: &BinaryExpression<'_>, out: &mut Vec<String>) {
    collect_idents_into(&bin.left, out);
    collect_idents_into(&bin.right, out);
}

fn collect_logical_idents(logical: &LogicalExpression<'_>, out: &mut Vec<String>) {
    collect_idents_into(&logical.left, out);
    collect_idents_into(&logical.right, out);
}

fn collect_conditional_idents(cond: &ConditionalExpression<'_>, out: &mut Vec<String>) {
    collect_idents_into(&cond.test, out);
    collect_idents_into(&cond.consequent, out);
    collect_idents_into(&cond.alternate, out);
}

fn collect_sequence_idents(seq: &SequenceExpression<'_>, out: &mut Vec<String>) {
    for expr in &seq.expressions {
        collect_idents_into(expr, out);
    }
}

fn collect_template_idents(tpl: &TemplateLiteral<'_>, out: &mut Vec<String>) {
    for expr in &tpl.expressions {
        collect_idents_into(expr, out);
    }
}

fn collect_call_idents(call: &CallExpression<'_>, out: &mut Vec<String>) {
    collect_idents_into(&call.callee, out);
    for arg in &call.arguments {
        if let Some(arg_expr) = arg.as_expression() {
            collect_idents_into(arg_expr, out);
        }
    }
}

fn collect_object_idents(obj: &ObjectExpression<'_>, out: &mut Vec<String>) {
    for prop in &obj.properties {
        match prop {
            ObjectPropertyKind::ObjectProperty(prop) => {
                // A computed key carries data too: `{ [name]: value }` writes a
                // header or field whose name comes from `name`.
                if prop.computed
                    && let Some(key) = prop.key.as_expression()
                {
                    collect_idents_into(key, out);
                }
                collect_idents_into(&prop.value, out);
            }
            ObjectPropertyKind::SpreadProperty(spread) => {
                collect_idents_into(&spread.argument, out);
            }
        }
    }
}

fn collect_array_idents(array: &ArrayExpression<'_>, out: &mut Vec<String>) {
    for element in &array.elements {
        if let Some(element_expr) = element.as_expression() {
            collect_idents_into(element_expr, out);
        }
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
        // A path built only from trusted module-relative bases and literal
        // segments is treated like a literal (issue #3711).
        let arg_is_non_literal = arg_literal.is_none()
            && is_non_literal_arg(arg_expr)
            && !self.is_trusted_static_path_expr(arg_expr);
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
                classify_url_shape(arg_expr, self)
            } else {
                None
            },
        }
    }

    fn capture_call_sink(&mut self, expr: &CallExpression<'_>) {
        let Some(callee_path) = self.call_sink_callee_path(&expr.callee) else {
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
        self.capture_function_body_after_spread(
            &callee_path,
            sink_shape,
            &expr.arguments,
            expr.span,
        );
        self.capture_set_attribute_sink(expr, &callee_path);
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
            if callee_path.ends_with(".writeHead")
                && matches!(arg_index, 1 | 2)
                && is_static_write_head_headers(arg_expr)
            {
                continue;
            }
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
        let Some(callee_path) = self.security_callee_path(&expr.callee) else {
            if contains_computed_member(&expr.callee) {
                self.record_skipped_security_callee(
                    &expr.callee,
                    SkippedSecurityCalleeReason::ComputedMember,
                );
            }
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
        self.capture_function_body_after_spread(
            &callee_path,
            SinkShape::NewExpression,
            &expr.arguments,
            expr.span,
        );
    }

    /// The Function constructor takes its code as the last argument. When a
    /// spread fills the parameter list (`new Function(...names, body)`), the
    /// body has no fixed index, so it is captured as argument 0. A spread body
    /// is captured through its argument.
    fn capture_function_body_after_spread(
        &mut self,
        callee_path: &str,
        sink_shape: SinkShape,
        arguments: &[Argument<'_>],
        span: Span,
    ) {
        if callee_path != "Function" {
            return;
        }
        let (Some(Argument::SpreadElement(_)), Some(body)) = (arguments.first(), arguments.last())
        else {
            return;
        };
        let arg_expr = match body {
            Argument::SpreadElement(spread) => &spread.argument,
            body => match body.as_expression() {
                Some(expr) => expr,
                None => return,
            },
        };
        self.push_security_sink_arg(PushSinkArgInput {
            callee_path,
            sink_shape,
            arg_index: 0,
            arg_expr,
            url_arg_literal: None,
            span,
        });
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
        let callee_path = match &expr.left {
            AssignmentTarget::StaticMemberExpression(member) => self
                .script_element_assign_path(&member.object, member.property.name.as_str())
                .or_else(|| {
                    self.member_assign_callee_path(&member.object, member.property.name.as_str())
                }),
            AssignmentTarget::ComputedMemberExpression(member) => {
                self.computed_member_assign_callee_path(member, &expr.right)
            }
            _ => None,
        };
        let Some(callee_path) = callee_path else {
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

    /// `HTMLScriptElement.<property>` when `object` is a local bound to a new
    /// `<script>` element and the property loads or holds its code. Other
    /// elements keep their member path, so `span.textContent = x` and
    /// `img.src = x` match no row.
    fn script_element_assign_path(
        &self,
        object: &Expression<'_>,
        property: &str,
    ) -> Option<String> {
        if !SCRIPT_ELEMENT_CODE_PROPERTIES.contains(&property) {
            return None;
        }
        let Expression::Identifier(ident) = unwrap_static_expr(object) else {
            return None;
        };
        matches!(
            self.sink_binding(ident.name.as_str()),
            Some(SinkBinding::ScriptElement)
        )
        .then(|| format!("{SCRIPT_ELEMENT_ROOT}.{property}"))
    }

    fn member_assign_callee_path(
        &mut self,
        object: &Expression<'_>,
        property: &str,
    ) -> Option<String> {
        let object_path = self
            .static_callee_path(object)
            .or_else(|| self.sink_object_path(object));
        let Some(object_path) = object_path else {
            self.record_skipped_security_callee(
                object,
                SkippedSecurityCalleeReason::UnsupportedAssignmentObject,
            );
            return None;
        };
        // `this.__proto__ = new.target.prototype` is the `setPrototypeOf`
        // fallback of an `Error` subclass. It sets the prototype of the new
        // instance and cannot reach a shared prototype.
        if object_path == "this" && property == "__proto__" {
            return None;
        }
        Some(format!("{object_path}.{property}"))
    }

    /// Resolve `el["innerHTML"] = x` and `el["inner" + "HTML"] = x` to the
    /// static member path. A key built from parts that do not fold to a
    /// constant (`el[prefix + "HTML"] = x`) is a blind spot when the value is
    /// non-literal, never a silent miss.
    fn computed_member_assign_callee_path(
        &mut self,
        member: &ComputedMemberExpression<'_>,
        value: &Expression<'_>,
    ) -> Option<String> {
        if let Some(key) = self.static_property_key(&member.expression) {
            return self.member_assign_callee_path(&member.object, &key);
        }
        if is_string_building_key(&member.expression) && is_non_literal_arg(value) {
            self.security_sinks_skipped += 1;
            self.security_unresolved_callee_sites
                .push(SkippedSecurityCalleeSite {
                    reason: SkippedSecurityCalleeReason::ComputedMember,
                    expression_kind: SkippedSecurityCalleeExpressionKind::ComputedMemberExpression,
                    span_start: member.span.start,
                    span_end: member.span.end,
                });
        }
        None
    }

    /// Resolve a callee to the dotted path the catalogue matches.
    ///
    /// On top of [`Self::static_callee_path`], this maps a global-object prefix
    /// on a code-executing global (`window.eval`) and a local alias of one
    /// (`const e = eval; e(code)`) to the bare global. A binding of a Node.js
    /// built-in module in
    /// [`NODE_BINDING_MODULES`](fallow_types::security_sinks::NODE_BINDING_MODULES)
    /// resolves to its `node:` path (`runInNewContext(code)` after
    /// `import { runInNewContext } from "vm"` becomes
    /// `node:vm.runInNewContext`). `fs.promises.<member>` on a module binding
    /// becomes `node:fs.<member>`, the path of an `fs/promises` import.
    fn security_callee_path(&self, expr: &Expression<'_>) -> Option<String> {
        let path = normalize_global_code_exec_path(self.static_callee_path(expr)?);
        let (root, rest) = match path.split_once('.') {
            Some((root, rest)) => (root, Some(rest)),
            None => (path.as_str(), None),
        };
        let resolved = match (self.sink_binding(root), rest) {
            (Some(SinkBinding::CodeExec(target) | SinkBinding::NodeModuleMember(target)), None) => {
                Some(target.clone())
            }
            (Some(SinkBinding::NodeModule(module)), Some(rest)) => {
                let rest = match rest.strip_prefix("promises.") {
                    Some(member) if module == "node:fs" => member,
                    _ => rest,
                };
                Some(format!("{module}.{rest}"))
            }
            _ => None,
        };
        Some(resolved.unwrap_or(path))
    }

    /// The sink binding that the innermost binding of `name` holds. A
    /// parameter, a catch parameter or a nested declaration of the same name
    /// without such a value hides an outer binding.
    fn sink_binding(&self, name: &str) -> Option<&SinkBinding> {
        if self
            .sink_aliases
            .catch_param_names
            .iter()
            .any(|names| names.contains(name))
        {
            return None;
        }
        let bindings = match self
            .nested_declaration_stack
            .iter()
            .rposition(|scope| scope.contains(name))
        {
            Some(index) => self.sink_aliases.scoped_bindings.get(index)?,
            None => &self.sink_aliases.bindings,
        };
        bindings.get(name)
    }

    /// Open the alias frames that match a new `nested_declaration_stack`
    /// frame: the `require` aliases and the code-exec aliases.
    pub(super) fn push_scoped_alias_frames(&mut self) {
        self.scoped_require_aliases.push(FxHashSet::default());
        self.sink_aliases.scoped_bindings.push(FxHashMap::default());
    }

    pub(super) fn pop_scoped_alias_frames(&mut self) {
        self.scoped_require_aliases.pop();
        self.sink_aliases.scoped_bindings.pop();
    }

    /// Open the scope of a `catch` clause that binds `names`.
    pub(super) fn push_catch_clause_scope(&mut self, names: FxHashSet<String>) {
        self.sink_aliases.catch_param_names.push(names.clone());
        self.push_direct_object_binding_scope(names);
    }

    pub(super) fn pop_catch_clause_scope(&mut self) {
        self.pop_direct_object_binding_scope();
        self.sink_aliases.catch_param_names.pop();
    }

    /// Flatten a callee or assignment object to a dotted path.
    ///
    /// Accepts identifiers, static members, computed members whose key folds to
    /// a constant string (`win["ev" + "al"]`), and the indirect-call sequence
    /// `(0, eval)` whose leading expressions are literals. Anything else is
    /// `None`, and the caller records a blind spot.
    fn static_callee_path(&self, expr: &Expression<'_>) -> Option<String> {
        let mut properties: Vec<Cow<'_, str>> = Vec::new();
        let mut expr = expr;
        let root = loop {
            crate::cost::walk_step();
            match unwrap_parens(expr) {
                Expression::Identifier(ident) => break ident.name.as_str(),
                Expression::StaticMemberExpression(member) => {
                    properties.push(Cow::Borrowed(member.property.name.as_str()));
                    expr = &member.object;
                }
                Expression::ComputedMemberExpression(member) => {
                    properties.push(Cow::Owned(self.static_property_key(&member.expression)?));
                    expr = &member.object;
                }
                Expression::SequenceExpression(seq) if properties.is_empty() => {
                    let (last, leading) = seq.expressions.split_last()?;
                    if !leading
                        .iter()
                        .all(|item| sink_literal_value(item).is_some())
                    {
                        return None;
                    }
                    expr = last;
                }
                _ => return None,
            }
        };
        Some(join_member_path(root, &properties))
    }

    /// Flatten the object of a member sink to a dotted path when
    /// [`Self::static_callee_path`] cannot: a `this` root, TypeScript
    /// wrappers (`el!`, `el as T`), optional chains (`a?.b`) and calls, which
    /// keep a `()` suffix (`document.createRange()`). Only member-assign
    /// objects and [`OBJECT_ROOTED_HTML_METHODS`] use this path, so call
    /// rows with broad patterns do not see these roots.
    fn sink_object_path(&self, expr: &Expression<'_>) -> Option<String> {
        let mut segments: Vec<String> = Vec::new();
        let mut calls = 0_usize;
        let mut expr = expr;
        loop {
            crate::cost::walk_step();
            let (segment, next): (Cow<'_, str>, Option<&Expression<'_>>) =
                match unwrap_static_expr(expr) {
                    Expression::Identifier(ident) => (Cow::Borrowed(ident.name.as_str()), None),
                    Expression::ThisExpression(_) => (Cow::Borrowed("this"), None),
                    Expression::StaticMemberExpression(member) => (
                        Cow::Borrowed(member.property.name.as_str()),
                        Some(&member.object),
                    ),
                    Expression::ComputedMemberExpression(member) => (
                        Cow::Owned(self.static_property_key(&member.expression)?),
                        Some(&member.object),
                    ),
                    Expression::CallExpression(call) => {
                        calls += 1;
                        expr = &call.callee;
                        continue;
                    }
                    Expression::ChainExpression(chain) => match &chain.expression {
                        ChainElement::StaticMemberExpression(member) => (
                            Cow::Borrowed(member.property.name.as_str()),
                            Some(&member.object),
                        ),
                        ChainElement::ComputedMemberExpression(member) => (
                            Cow::Owned(self.static_property_key(&member.expression)?),
                            Some(&member.object),
                        ),
                        ChainElement::CallExpression(call) => {
                            calls += 1;
                            expr = &call.callee;
                            continue;
                        }
                        ChainElement::TSNonNullExpression(non_null) => {
                            expr = &non_null.expression;
                            continue;
                        }
                        ChainElement::PrivateFieldExpression(_) => return None,
                    },
                    _ => return None,
                };
            let mut segment = segment.into_owned();
            for _ in 0..calls {
                segment.push_str("()");
            }
            calls = 0;
            segments.push(segment);
            match next {
                Some(object) => expr = object,
                None => break,
            }
        }
        segments.reverse();
        Some(segments.join("."))
    }

    /// The callee path of a call sink. A method in
    /// [`OBJECT_ROOTED_HTML_METHODS`] also resolves on an object that only
    /// [`Self::sink_object_path`] can flatten.
    fn call_sink_callee_path(&self, callee: &Expression<'_>) -> Option<String> {
        self.security_callee_path(callee).or_else(|| {
            let method = super::visit_helpers::terminal_static_member_name(callee)?;
            if !OBJECT_ROOTED_HTML_METHODS.contains(&method) {
                return None;
            }
            self.sink_object_path(callee)
        })
    }

    /// A computed property key that folds to a constant identifier-like string.
    /// Keys with a `.` are rejected, because they would split into two path
    /// segments.
    fn static_property_key(&self, expr: &Expression<'_>) -> Option<String> {
        let key = self.static_string_value(expr)?;
        (!key.is_empty() && !key.contains('.')).then_some(key)
    }

    /// Fold string literals, substitution-free templates, module constants and
    /// `+` concatenations of those to one string.
    fn static_string_value(&self, expr: &Expression<'_>) -> Option<String> {
        crate::cost::walk_step();
        if let Some(value) = static_string_literal_value(expr) {
            return Some(value);
        }
        if let Expression::BinaryExpression(bin) = unwrap_static_expr(expr) {
            if bin.operator != BinaryOperator::Addition {
                return None;
            }
            let mut value = self.static_string_value(&bin.left)?;
            value.push_str(&self.static_string_value(&bin.right)?);
            return Some(value);
        }
        match self.static_sink_literal_value(expr)? {
            SinkLiteralValue::String(value) => Some(value),
            SinkLiteralValue::Integer(_)
            | SinkLiteralValue::Boolean(_)
            | SinkLiteralValue::Null => None,
        }
    }

    pub(super) fn record_sink_alias_bindings(&mut self, name: &str, init: &Expression<'_>) {
        let binding = self.sink_binding_of_value(name, init);
        if let Some(bindings) = self.declaration_sink_bindings(name) {
            set_sink_binding(bindings, name, binding);
        }
        self.record_jsx_spread_object(name, init);
    }

    /// Record the members of a Node.js built-in module that an object pattern
    /// takes from it (`const { runInThisContext: run } = require("vm")`).
    pub(super) fn record_sink_pattern_bindings(
        &mut self,
        pattern: &ObjectPattern<'_>,
        init: &Expression<'_>,
    ) {
        let module = self.node_binding_module_of_require(init);
        for property in &pattern.properties {
            let BindingPattern::BindingIdentifier(local) = &property.value else {
                continue;
            };
            let name = local.name.as_str();
            let binding = module.as_ref().and_then(|module| {
                property
                    .key
                    .static_name()
                    .map(|key| SinkBinding::NodeModuleMember(format!("{module}.{}", key.text())))
            });
            if let Some(bindings) = self.declaration_sink_bindings(name) {
                set_sink_binding(bindings, name, binding);
            }
        }
    }

    /// Record a named ES import of a Node.js built-in module
    /// (`import { runInContext as run } from "node:vm"`).
    pub(super) fn record_import_sink_binding(&mut self, source: &str, imported: &str, local: &str) {
        if let Some(module) = node_binding_module(source) {
            self.sink_aliases.bindings.insert(
                local.to_string(),
                SinkBinding::NodeModuleMember(format!("{module}.{imported}")),
            );
        }
    }

    /// Update the sink binding of `name` after a plain assignment
    /// (`script = document.createElement("script")`). The binding lives in the
    /// frame that declares the name, or at module scope for an undeclared
    /// name.
    pub(super) fn record_sink_assignment_binding(&mut self, name: &str, value: &Expression<'_>) {
        if self.namespace_depth > 0 {
            return;
        }
        let binding = self.sink_binding_of_value(name, value);
        let frame = self
            .nested_declaration_stack
            .iter()
            .rposition(|scope| scope.contains(name));
        let bindings = match frame {
            Some(index) => match self.sink_aliases.scoped_bindings.get_mut(index) {
                Some(bindings) => bindings,
                None => return,
            },
            None => &mut self.sink_aliases.bindings,
        };
        set_sink_binding(bindings, name, binding);
    }

    /// The binding map of the frame that declares `name`. Inside a namespace
    /// the visitor keeps no frames, so no binding is recorded there.
    fn declaration_sink_bindings(
        &mut self,
        name: &str,
    ) -> Option<&mut FxHashMap<String, SinkBinding>> {
        if self.namespace_depth > 0 {
            return None;
        }
        let frame = if self.is_module_scope() {
            None
        } else {
            self.nested_declaration_stack
                .iter()
                .rposition(|scope| scope.contains(name))
                .or_else(|| self.nested_declaration_stack.len().checked_sub(1))
        };
        match frame {
            Some(index) => self.sink_aliases.scoped_bindings.get_mut(index),
            None => Some(&mut self.sink_aliases.bindings),
        }
    }

    /// The sink binding that a value gives to `name`: an alias of a
    /// code-executing global or of a Node.js module binding, a `require()` of
    /// a module in [`NODE_BINDING_MODULES`](fallow_types::security_sinks::NODE_BINDING_MODULES), or a new `<script>` element.
    fn sink_binding_of_value(&self, name: &str, value: &Expression<'_>) -> Option<SinkBinding> {
        if is_create_script_element_call(value) {
            return Some(SinkBinding::ScriptElement);
        }
        if let Some(module) = self.node_binding_module_of_require(value) {
            return Some(SinkBinding::NodeModule(module));
        }
        let target = match unwrap_parens(value) {
            Expression::Identifier(_)
            | Expression::StaticMemberExpression(_)
            | Expression::ComputedMemberExpression(_)
            | Expression::SequenceExpression(_) => self.security_callee_path(value)?,
            _ => return None,
        };
        if target == name {
            return None;
        }
        if CODE_EXEC_GLOBALS.contains(&target.as_str()) {
            return Some(SinkBinding::CodeExec(target));
        }
        if target.starts_with("node:") {
            return Some(if target.contains('.') {
                SinkBinding::NodeModuleMember(target)
            } else {
                SinkBinding::NodeModule(target)
            });
        }
        None
    }

    /// The `node:` path of `require("<module>")` for a module in
    /// [`NODE_BINDING_MODULES`](fallow_types::security_sinks::NODE_BINDING_MODULES).
    fn node_binding_module_of_require(&self, value: &Expression<'_>) -> Option<String> {
        let (_, source) = self.try_extract_require(unwrap_static_expr(value))?;
        node_binding_module(&source)
    }

    /// `el.setAttribute("srcdoc", html)` sets the same document as
    /// `el.srcdoc = html`, so it is captured as that member-assign shape.
    /// `script.setAttribute("src", url)` on a local bound to a new `<script>`
    /// element becomes `HTMLScriptElement.src`. The `src` of another element
    /// (`img.setAttribute("src", url)`) is not captured.
    fn capture_set_attribute_sink(&mut self, expr: &CallExpression<'_>, callee_path: &str) {
        let Some(object_path) = callee_path.strip_suffix(".setAttribute") else {
            return;
        };
        let [name_arg, value_arg, ..] = expr.arguments.as_slice() else {
            return;
        };
        let (Some(name_expr), Some(value_expr)) =
            (name_arg.as_expression(), value_arg.as_expression())
        else {
            return;
        };
        let Some(name) = self.static_string_value(name_expr) else {
            return;
        };
        let sink_path = if name.eq_ignore_ascii_case("srcdoc") {
            format!("{object_path}.srcdoc")
        } else if name.eq_ignore_ascii_case("src") {
            let Expression::StaticMemberExpression(member) = unwrap_parens(&expr.callee) else {
                return;
            };
            let Some(path) = self.script_element_assign_path(&member.object, "src") else {
                return;
            };
            path
        } else {
            return;
        };
        self.push_security_sink_arg(PushSinkArgInput {
            callee_path: &sink_path,
            sink_shape: SinkShape::MemberAssign,
            arg_index: 0,
            arg_expr: value_expr,
            url_arg_literal: None,
            span: expr.span,
        });
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
                classify_url_shape(&expr.right, self)
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
        if let Some(site) = self.jsx_prop_sink_site(name.name.as_str(), value_expr, attr.span) {
            self.security_sinks.push(site);
        }
    }

    /// Capture JSX props passed through a spread: an inline object literal
    /// (`<div {...{ dangerouslySetInnerHTML: x }} />`) or a local bound to one
    /// (`<div {...props} />`). The spread is the same as writing the props.
    pub(super) fn capture_jsx_spread_sink(&mut self, spread: &JSXSpreadAttribute<'_>) {
        match unwrap_parens(&spread.argument) {
            Expression::ObjectExpression(obj) => {
                for prop in &obj.properties {
                    let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                        continue;
                    };
                    let Some(key) = prop.key.static_name().and_then(|name| name.into_cow_str())
                    else {
                        continue;
                    };
                    if let Some(site) = self.jsx_prop_sink_site(&key, &prop.value, prop.span) {
                        self.security_sinks.push(site);
                    }
                }
            }
            Expression::Identifier(ident) => {
                if let Some(sites) = self
                    .sink_aliases
                    .jsx_spread_object_sinks
                    .remove(ident.name.as_str())
                {
                    self.security_sinks.extend(sites);
                }
            }
            _ => {}
        }
    }

    /// Remember the JSX sink props of a local bound to an object literal, so a
    /// later spread of the local can emit them. Only props in
    /// [`JSX_SPREAD_SINK_PROPS`] are kept; the site anchors on the property.
    fn record_jsx_spread_object(&mut self, name: &str, init: &Expression<'_>) {
        self.sink_aliases.jsx_spread_object_sinks.remove(name);
        let Expression::ObjectExpression(obj) = unwrap_static_expr(init) else {
            return;
        };
        let mut sites = Vec::new();
        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                continue;
            };
            let Some(key) = prop.key.static_name().and_then(|name| name.into_cow_str()) else {
                continue;
            };
            if !JSX_SPREAD_SINK_PROPS.contains(&key.as_ref()) {
                continue;
            }
            if let Some(site) = self.jsx_prop_sink_site(&key, &prop.value, prop.span) {
                sites.push(site);
            }
        }
        if !sites.is_empty() {
            self.sink_aliases
                .jsx_spread_object_sinks
                .insert(name.to_string(), sites);
        }
    }

    fn jsx_prop_sink_site(
        &mut self,
        name: &str,
        value_expr: &Expression<'_>,
        span: Span,
    ) -> Option<SinkSite> {
        if !is_non_literal_arg(value_expr) {
            return None;
        }
        self.record_sanitized_sink_arg(span.start, 0, value_expr);
        let object_keys = object_key_metadata(value_expr);
        Some(SinkSite {
            sink_shape: SinkShape::JsxAttr,
            callee_path: name.to_string(),
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
            span_start: span.start,
            span_end: span.end,
            url_arg_literal: None,
            url_shape: None,
        })
    }
}
