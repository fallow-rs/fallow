//! Member references in the `host` object of an Angular `@Component` or
//! `@Directive` decorator.

use oxc_ast::ast::{Expression, ObjectExpression, ObjectPropertyKind};

use crate::js_text::JsText;

pub(super) fn extract_host_member_refs(host_obj: &ObjectExpression<'_>, refs: &mut Vec<String>) {
    for prop in &host_obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        if let Expression::StringLiteral(lit) = &p.value {
            extract_identifiers_from_host_expr(&lit.value.text(), refs);
        } else if let Expression::TemplateLiteral(tpl) = &p.value
            && tpl.expressions.is_empty()
            && let Some(quasi) = tpl.quasis.first()
        {
            extract_identifiers_from_host_expr(quasi.value.raw.as_str(), refs);
        }
    }
}

/// Extract every member-identifier referenced anywhere in a host-binding
/// expression. Scans the whole string (not just the leading token), so a complex
/// expression such as `'"mat-" + (color || "primary")'` credits `color`, and a
/// member tail in `'foo.bar'` credits only the root `foo` (segments after `.` are
/// skipped). String-literal contents are skipped so `"mat-"` / `"primary"` are
/// not mistaken for identifiers. Over-credits by design (a host ref is a use), so
/// scanning more of the expression can only reduce false positives.
pub(super) fn extract_identifiers_from_host_expr(expr: &str, refs: &mut Vec<String>) {
    let bytes = expr.as_bytes();
    let mut i = 0;
    let mut in_string: Option<u8> = None;
    let mut prev_significant: Option<u8> = None;
    while i < bytes.len() {
        let c = bytes[i];
        // Skip string-literal bodies (single, double, backtick).
        if let Some(quote) = in_string {
            if c == quote {
                in_string = None;
            }
            i += 1;
            continue;
        }
        if c == b'"' || c == b'\'' || c == b'`' {
            in_string = Some(c);
            prev_significant = Some(c);
            i += 1;
            continue;
        }
        let is_ident_start = c.is_ascii_alphabetic() || c == b'_' || c == b'$';
        if is_ident_start {
            let start = i;
            while i < bytes.len() {
                let cc = bytes[i];
                if cc.is_ascii_alphanumeric() || cc == b'_' || cc == b'$' {
                    i += 1;
                } else {
                    break;
                }
            }
            // A member tail (`foo.bar` -> skip `bar`) is not a component member.
            let is_member_tail = prev_significant == Some(b'.');
            let ident = &expr[start..i];
            if !is_member_tail
                && is_valid_member_identifier(ident)
                && !refs.iter().any(|r| r == ident)
            {
                refs.push(ident.to_string());
            }
            prev_significant = Some(b'a'); // any non-`.` significant byte
            continue;
        }
        if !c.is_ascii_whitespace() {
            prev_significant = Some(c);
        }
        i += 1;
    }
}

pub(super) fn is_valid_member_identifier(ident: &str) -> bool {
    !ident.is_empty()
        && ident
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
        && !matches!(
            ident,
            "true"
                | "false"
                | "null"
                | "undefined"
                | "this"
                | "event"
                | "window"
                | "document"
                | "console"
                | "Math"
                | "JSON"
                | "Object"
                | "Array"
                | "String"
                | "Number"
                | "Boolean"
                | "Date"
                | "RegExp"
                | "Error"
                | "Promise"
        )
}
