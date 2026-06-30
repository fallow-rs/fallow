use oxc_ast::ast::{
    Argument, AssignmentTarget, BinaryExpression, BinaryOperator, CallExpression, ChainElement,
    ChainExpression, Expression, ObjectPropertyKind, TemplateLiteral,
};
use rustc_hash::FxHashMap;

use fallow_types::extract::{SecurityUrlShape, SinkArgKind, SinkLiteralValue, SinkObjectProperty};

use super::{flatten_callee_path, unwrap_parens, unwrap_static_expr};

pub(super) fn risky_redos_fragment(pattern: &str) -> Option<String> {
    let mut cursor = 0;
    while let Some(relative) = pattern[cursor..].find('(') {
        let start = cursor + relative;
        let close = find_group_close(pattern, start)?;
        let Some(outer_end) = unbounded_quantifier_end(pattern, close + 1) else {
            cursor = close + 1;
            continue;
        };
        let body_start = group_body_start(pattern, start + 1);
        let body = &pattern[body_start..close];
        if has_unbounded_quantifier(body) || has_ambiguous_alternation(body) {
            return Some(pattern[start..outer_end].to_string());
        }
        cursor = close + 1;
    }
    None
}

fn find_group_close(pattern: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_class = false;
    let mut escaped = false;
    for (idx, ch) in pattern[open..].char_indices() {
        let absolute = open + idx;
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if in_class {
            if ch == ']' {
                in_class = false;
            }
            continue;
        }
        match ch {
            '[' => in_class = true,
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(absolute);
                }
            }
            _ => {}
        }
    }
    None
}

fn group_body_start(pattern: &str, body_start: usize) -> usize {
    let Some(rest) = pattern.get(body_start..) else {
        return body_start;
    };
    if rest.starts_with("?:") || rest.starts_with("?=") || rest.starts_with("?!") {
        return body_start + 2;
    }
    if rest.starts_with("?<=") || rest.starts_with("?<!") {
        return body_start + 3;
    }
    if rest.starts_with("?<")
        && let Some(end) = rest.find('>')
    {
        return body_start + end + 1;
    }
    body_start
}

fn unbounded_quantifier_end(pattern: &str, idx: usize) -> Option<usize> {
    let rest = pattern.get(idx..)?;
    if rest.starts_with('+') || rest.starts_with('*') {
        return Some(idx + 1);
    }
    if !rest.starts_with('{') {
        return None;
    }
    let close = rest.find('}')?;
    let body = &rest[1..close];
    let (min, max) = body.split_once(',')?;
    if min.chars().all(|ch| ch.is_ascii_digit()) && max.is_empty() {
        return Some(idx + close + 1);
    }
    None
}

fn has_unbounded_quantifier(pattern: &str) -> bool {
    let mut in_class = false;
    let mut escaped = false;
    let mut idx = 0;
    while idx < pattern.len() {
        let Some(ch) = pattern[idx..].chars().next() else {
            break;
        };
        if escaped {
            escaped = false;
            idx += ch.len_utf8();
            continue;
        }
        if ch == '\\' {
            escaped = true;
            idx += ch.len_utf8();
            continue;
        }
        if in_class {
            if ch == ']' {
                in_class = false;
            }
            idx += ch.len_utf8();
            continue;
        }
        if ch == '[' {
            in_class = true;
            idx += ch.len_utf8();
            continue;
        }
        if unbounded_quantifier_end(pattern, idx).is_some() {
            return true;
        }
        idx += ch.len_utf8();
    }
    false
}

fn has_ambiguous_alternation(pattern: &str) -> bool {
    let branches = top_level_alternation_branches(pattern);
    if branches.len() < 2 {
        return false;
    }
    let tokens = branches
        .iter()
        .map(|branch| regex_branch_tokens(branch))
        .collect::<Vec<_>>();
    tokens.iter().enumerate().any(|(left_idx, left)| {
        !left.is_empty()
            && tokens
                .iter()
                .enumerate()
                .any(|(right_idx, right)| left_idx != right_idx && is_prefix_tokens(left, right))
    })
}

fn top_level_alternation_branches(pattern: &str) -> Vec<&str> {
    let mut branches = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut in_class = false;
    let mut escaped = false;
    for (idx, ch) in pattern.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if in_class {
            if ch == ']' {
                in_class = false;
            }
            continue;
        }
        match ch {
            '[' => in_class = true,
            '(' => depth += 1,
            ')' => depth = depth.saturating_sub(1),
            '|' if depth == 0 => {
                branches.push(&pattern[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    branches.push(&pattern[start..]);
    branches
}

fn regex_branch_tokens(branch: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut chars = branch.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let escaped = chars
                    .next()
                    .map_or_else(|| "\\".to_string(), |next| format!("\\{next}"));
                tokens.push(escaped);
            }
            '[' => {
                let mut token = String::from("[");
                for next in chars.by_ref() {
                    token.push(next);
                    if next == ']' {
                        break;
                    }
                }
                tokens.push(token);
            }
            '^' | '$' | '+' | '*' | '?' => {}
            '{' => {
                for next in chars.by_ref() {
                    if next == '}' {
                        break;
                    }
                }
            }
            _ => tokens.push(ch.to_string()),
        }
    }
    tokens
}

fn is_prefix_tokens(left: &[String], right: &[String]) -> bool {
    left.len() < right.len() && right.starts_with(left)
}

pub(super) fn should_capture_hardcoded_secret_literal(context_name: &str, value: &str) -> bool {
    is_secret_shaped_context_name(context_name) || has_provider_prefix_capture_hint(value)
}

fn is_secret_shaped_context_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "apikey",
        "api_key",
        "accesskey",
        "access_key",
        "privatekey",
        "private_key",
        "clientsecret",
        "client_secret",
        "token",
        "secret",
        "password",
        "passwd",
        "credential",
        "jwt",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn has_provider_prefix_capture_hint(value: &str) -> bool {
    value.starts_with("AKIA")
        || value.starts_with("ASIA")
        || value.starts_with("ghp_")
        || value.starts_with("gho_")
        || value.starts_with("ghu_")
        || value.starts_with("ghs_")
        || value.starts_with("ghr_")
        || value.starts_with("github_pat_")
        || value.starts_with("glpat-")
        || value.starts_with("xoxb-")
        || value.starts_with("xoxp-")
        || value.starts_with("xoxa-")
        || value.starts_with("xoxr-")
        || value.starts_with("xoxs-")
        || value.starts_with("sk_live_")
        || value.starts_with("rk_live_")
        || value.starts_with("sk-ant-")
        || value.starts_with("sk-proj-")
        || value.starts_with("AIza")
        || value.starts_with("SG.")
        || value.starts_with("npm_")
        || value.starts_with("pypi-")
        || value.starts_with("sq0atp-")
        || value.starts_with("shpat_")
        || value.starts_with("shpss_")
        || value.starts_with("shpca_")
        || value.starts_with("shppa_")
        || value.starts_with("dp.pt.")
        || value.starts_with("doo_v1_")
        || value.starts_with("dop_v1_")
        || value.starts_with("dor_v1_")
        || value.starts_with("dot_v1_")
        || value.starts_with("dapi")
        || value.starts_with("lin_api_")
        || value.starts_with("PMAK-")
        || value.starts_with("hf_")
        || value.starts_with("AGE-SECRET-KEY-1")
        || value.contains("-----BEGIN")
}

/// Whether an expression is a non-literal argument (a conservative trigger for
/// sink capture). A fully-literal argument is never captured.
pub(super) fn is_non_literal_arg(expr: &Expression<'_>) -> bool {
    match unwrap_static_expr(expr) {
        Expression::StringLiteral(_)
        | Expression::NumericLiteral(_)
        | Expression::BooleanLiteral(_)
        | Expression::NullLiteral(_)
        | Expression::BigIntLiteral(_)
        | Expression::RegExpLiteral(_) => false,
        Expression::TemplateLiteral(tpl) => !tpl.expressions.is_empty(),
        _ => true,
    }
}

/// Classify a captured non-literal argument into a finer-grained [`SinkArgKind`]
/// so the catalogue can require unsafe shapes (concat, template-with-substitution)
/// and exclude safe ones (object literal, the parameterized form). Static
/// TypeScript wrappers are unwrapped first; a `+` `BinaryExpression` is treated
/// as a string concatenation.
pub(super) fn classify_arg_kind(expr: &Expression<'_>) -> SinkArgKind {
    match unwrap_static_expr(expr) {
        Expression::TemplateLiteral(_) => SinkArgKind::TemplateWithSubst,
        Expression::BinaryExpression(bin) if bin.operator == BinaryOperator::Addition => {
            SinkArgKind::Concat
        }
        Expression::ObjectExpression(_) => SinkArgKind::Object,
        Expression::CallExpression(_) => SinkArgKind::Call,
        _ => SinkArgKind::Other,
    }
}

pub(super) fn classify_url_shape(
    expr: &Expression<'_>,
    static_string_bindings: &FxHashMap<String, String>,
) -> Option<SecurityUrlShape> {
    match unwrap_static_expr(expr) {
        Expression::TemplateLiteral(tpl) => {
            classify_template_url_shape(tpl, static_string_bindings)
        }
        Expression::BinaryExpression(bin) if bin.operator == BinaryOperator::Addition => {
            classify_concat_url_shape(bin, static_string_bindings)
        }
        Expression::Identifier(ident) => Some(
            static_string_bindings
                .get(ident.name.as_str())
                .map_or(SecurityUrlShape::DynamicOrigin, |value| {
                    classify_url_prefix(value)
                }),
        ),
        Expression::StringLiteral(_) => None,
        _ => Some(SecurityUrlShape::DynamicOrigin),
    }
}

pub(super) fn sink_literal_value(expr: &Expression<'_>) -> Option<SinkLiteralValue> {
    match unwrap_static_expr(expr) {
        Expression::StringLiteral(lit) => Some(SinkLiteralValue::String(lit.value.to_string())),
        Expression::NumericLiteral(lit) if lit.value.is_finite() && lit.value.fract() == 0.0 => {
            #[expect(
                clippy::cast_possible_truncation,
                reason = "finite integer JS literals in the safe i64 range are the only admitted numeric sink metadata"
            )]
            let value = lit.value as i64;
            if (value as f64 - lit.value).abs() < f64::EPSILON {
                Some(SinkLiteralValue::Integer(value))
            } else {
                None
            }
        }
        Expression::BooleanLiteral(lit) => Some(SinkLiteralValue::Boolean(lit.value)),
        Expression::NullLiteral(_) => Some(SinkLiteralValue::Null),
        _ => None,
    }
}

pub(super) fn static_sink_literal_to_string(value: &SinkLiteralValue) -> String {
    match value {
        SinkLiteralValue::String(value) => value.clone(),
        SinkLiteralValue::Integer(value) => value.to_string(),
        SinkLiteralValue::Boolean(value) => value.to_string(),
        SinkLiteralValue::Null => "null".to_string(),
    }
}

pub(super) fn static_string_literal_value(expr: &Expression<'_>) -> Option<String> {
    match unwrap_static_expr(expr) {
        Expression::StringLiteral(lit) => Some(lit.value.to_string()),
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() && tpl.quasis.len() == 1 => {
            tpl.quasis
                .first()
                .and_then(|quasi| quasi.value.cooked.as_ref())
                .map(ToString::to_string)
        }
        _ => None,
    }
}

pub(super) fn object_literal_properties(expr: &Expression<'_>) -> Vec<SinkObjectProperty> {
    let mut properties = Vec::new();
    collect_object_literal_properties(expr, "", &mut properties);
    properties
}

pub(super) struct ObjectKeyMetadata {
    pub(super) keys: Vec<String>,
    pub(super) complete: bool,
}

pub(super) fn object_key_metadata(expr: &Expression<'_>) -> ObjectKeyMetadata {
    let Expression::ObjectExpression(obj) = unwrap_static_expr(expr) else {
        return ObjectKeyMetadata {
            keys: Vec::new(),
            complete: false,
        };
    };
    let mut keys = Vec::new();
    let mut complete = true;
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            complete = false;
            continue;
        };
        let Some(key) = prop.key.static_name() else {
            complete = false;
            continue;
        };
        let key = key.to_string();
        if !keys.iter().any(|existing| existing == &key) {
            keys.push(key);
        }
    }
    ObjectKeyMetadata { keys, complete }
}

fn classify_template_url_shape(
    tpl: &TemplateLiteral<'_>,
    static_string_bindings: &FxHashMap<String, String>,
) -> Option<SecurityUrlShape> {
    let first = tpl.quasis.first()?.value.raw.as_ref();
    if !first.is_empty() {
        return Some(classify_url_prefix(first));
    }
    let first_expr = tpl.expressions.first()?;
    let Expression::Identifier(ident) = unwrap_static_expr(first_expr) else {
        return Some(SecurityUrlShape::DynamicOrigin);
    };
    Some(
        static_string_bindings
            .get(ident.name.as_str())
            .map_or(SecurityUrlShape::DynamicOrigin, |base| {
                classify_url_prefix(base)
            }),
    )
}

fn classify_concat_url_shape(
    bin: &BinaryExpression<'_>,
    static_string_bindings: &FxHashMap<String, String>,
) -> Option<SecurityUrlShape> {
    match unwrap_static_expr(&bin.left) {
        Expression::StringLiteral(lit) => Some(classify_url_prefix(lit.value.as_str())),
        Expression::TemplateLiteral(tpl) if tpl.expressions.is_empty() => tpl
            .quasis
            .first()
            .map(|quasi| classify_url_prefix(quasi.value.raw.as_ref())),
        Expression::Identifier(ident) => Some(
            static_string_bindings
                .get(ident.name.as_str())
                .map_or(SecurityUrlShape::DynamicOrigin, |value| {
                    classify_url_prefix(value)
                }),
        ),
        Expression::BinaryExpression(left) if left.operator == BinaryOperator::Addition => {
            classify_concat_url_shape(left, static_string_bindings)
        }
        _ => Some(SecurityUrlShape::DynamicOrigin),
    }
}

fn classify_url_prefix(prefix: &str) -> SecurityUrlShape {
    if has_fixed_url_origin_or_root(prefix) {
        SecurityUrlShape::FixedOriginDynamicPath
    } else {
        SecurityUrlShape::DynamicOrigin
    }
}

fn has_fixed_url_origin_or_root(prefix: &str) -> bool {
    let trimmed = prefix.trim_start();
    trimmed.starts_with('/')
        || trimmed.find("://").is_some_and(|scheme_end| {
            scheme_end > 0
                && !trimmed[scheme_end + 3..]
                    .split(['/', '?', '#'])
                    .next()
                    .unwrap_or_default()
                    .is_empty()
        })
}

fn collect_object_literal_properties(
    expr: &Expression<'_>,
    prefix: &str,
    properties: &mut Vec<SinkObjectProperty>,
) {
    let Expression::ObjectExpression(obj) = unwrap_static_expr(expr) else {
        return;
    };
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(prop) = prop else {
            continue;
        };
        let Some(key) = prop.key.static_name() else {
            continue;
        };
        let key = if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{prefix}.{key}")
        };
        if let Some(value) = sink_literal_value(&prop.value) {
            properties.push(SinkObjectProperty { key, value });
        } else {
            collect_object_literal_properties(&prop.value, &key, properties);
        }
    }
}

pub(super) fn is_token_like_security_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [
        "token",
        "secret",
        "session",
        "jwt",
        "auth",
        "csrf",
        "nonce",
        "salt",
        "password",
        "credential",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

/// Whether a call expression is, or contains in its callee / arguments, a
/// `Math.random()` use.
pub(super) fn call_contains_math_random(call: &CallExpression<'_>) -> bool {
    is_math_random_zero_arg_call(call)
        || expression_callee_contains_math_random(&call.callee)
        || call
            .arguments
            .iter()
            .filter_map(Argument::as_expression)
            .any(expression_contains_math_random_call)
}

pub(super) fn expression_contains_math_random_call(expr: &Expression<'_>) -> bool {
    match unwrap_parens(expr) {
        Expression::CallExpression(call) => call_contains_math_random(call),
        Expression::BinaryExpression(bin) => {
            expression_contains_math_random_call(&bin.left)
                || expression_contains_math_random_call(&bin.right)
        }
        Expression::LogicalExpression(logical) => {
            expression_contains_math_random_call(&logical.left)
                || expression_contains_math_random_call(&logical.right)
        }
        Expression::ConditionalExpression(cond) => {
            expression_contains_math_random_call(&cond.test)
                || expression_contains_math_random_call(&cond.consequent)
                || expression_contains_math_random_call(&cond.alternate)
        }
        Expression::TemplateLiteral(tpl) => tpl
            .expressions
            .iter()
            .any(expression_contains_math_random_call),
        Expression::ArrayExpression(array) => array.elements.iter().any(|element| {
            element
                .as_expression()
                .is_some_and(expression_contains_math_random_call)
        }),
        Expression::ObjectExpression(obj) => obj.properties.iter().any(|prop| {
            let ObjectPropertyKind::ObjectProperty(prop) = prop else {
                return false;
            };
            expression_contains_math_random_call(&prop.value)
        }),
        Expression::ParenthesizedExpression(paren) => {
            expression_contains_math_random_call(&paren.expression)
        }
        Expression::StaticMemberExpression(member) => {
            expression_contains_math_random_call(&member.object)
        }
        Expression::ChainExpression(chain) => chain_element_contains_math_random(chain),
        _ => false,
    }
}

pub(super) fn assignment_target_security_context_name(
    target: &AssignmentTarget<'_>,
) -> Option<String> {
    match target {
        AssignmentTarget::AssignmentTargetIdentifier(id) => Some(id.name.to_string()),
        AssignmentTarget::StaticMemberExpression(member) => Some(member.property.name.to_string()),
        _ => None,
    }
}

/// A zero-argument `Math.random()` call (the literal insecure-randomness shape).
fn is_math_random_zero_arg_call(call: &CallExpression<'_>) -> bool {
    call.arguments.is_empty() && flatten_callee_path(&call.callee).as_deref() == Some("Math.random")
}

/// Whether an optional-chain element is, or contains, a `Math.random()` use.
fn chain_element_contains_math_random(chain: &ChainExpression<'_>) -> bool {
    match &chain.expression {
        ChainElement::CallExpression(call) => {
            is_math_random_zero_arg_call(call)
                || expression_callee_contains_math_random(&call.callee)
        }
        ChainElement::StaticMemberExpression(member) => {
            expression_contains_math_random_call(&member.object)
        }
        _ => false,
    }
}

fn expression_callee_contains_math_random(callee: &Expression<'_>) -> bool {
    match callee {
        Expression::StaticMemberExpression(member) => {
            expression_contains_math_random_call(&member.object)
        }
        Expression::ChainExpression(chain) => match &chain.expression {
            ChainElement::StaticMemberExpression(member) => {
                expression_contains_math_random_call(&member.object)
            }
            ChainElement::CallExpression(call) => {
                expression_callee_contains_math_random(&call.callee)
            }
            _ => false,
        },
        _ => false,
    }
}
