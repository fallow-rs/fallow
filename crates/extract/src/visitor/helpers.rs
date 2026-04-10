//! Standalone helper functions for AST extraction.
//!
//! These functions don't require visitor state and operate purely on AST nodes.

use oxc_ast::ast::{
    Argument, ArrayExpressionElement, BinaryExpression, Class, ClassElement, Expression,
    ObjectPropertyKind, Statement,
};

use crate::{MemberInfo, MemberKind};

/// Metadata extracted from an Angular `@Component` decorator.
pub struct AngularComponentMetadata {
    /// The `templateUrl` value (e.g., `"./app.html"`).
    pub template_url: Option<String>,
    /// All style file URLs from `styleUrl` (singular) and `styleUrls` (array).
    pub style_urls: Vec<String>,
    /// Inline `template:` string literal content.
    pub inline_template: Option<String>,
    /// Class member names referenced in `host:` binding expressions.
    pub host_member_refs: Vec<String>,
    /// Class member names listed in `inputs:` and `outputs:` metadata arrays.
    pub input_output_members: Vec<String>,
}

/// Angular signal-based API function names that implicitly mark class properties
/// as framework-managed (Angular 17+). Properties initialized with these calls
/// should be treated like decorated members.
const ANGULAR_SIGNAL_APIS: &[&str] = &[
    "input",
    "output",
    "outputFromObservable",
    "model",
    "viewChild",
    "viewChildren",
    "contentChild",
    "contentChildren",
];

/// Extract all metadata from an Angular `@Component` decorator.
///
/// Walks the class's decorators looking for a `@Component({...})` call expression
/// and extracts template/style URLs, inline templates, host bindings, and
/// inputs/outputs metadata.
pub fn extract_angular_component_metadata(class: &Class<'_>) -> Option<AngularComponentMetadata> {
    for decorator in &class.decorators {
        let Expression::CallExpression(call) = &decorator.expression else {
            continue;
        };
        let Expression::Identifier(id) = &call.callee else {
            continue;
        };
        if !matches!(id.name.as_str(), "Component" | "Directive") {
            continue;
        }
        let Some(Argument::ObjectExpression(obj)) = call.arguments.first() else {
            continue;
        };

        let mut template_url = None;
        let mut style_urls = Vec::new();
        let mut inline_template = None;
        let mut host_member_refs = Vec::new();
        let mut input_output_members = Vec::new();

        for prop in &obj.properties {
            let ObjectPropertyKind::ObjectProperty(p) = prop else {
                continue;
            };
            let Some(key_name) = p.key.static_name() else {
                continue;
            };
            match key_name.as_ref() {
                "templateUrl" => {
                    if let Expression::StringLiteral(lit) = &p.value {
                        template_url = Some(lit.value.to_string());
                    }
                }
                "template" => {
                    if let Expression::StringLiteral(lit) = &p.value {
                        inline_template = Some(lit.value.to_string());
                    } else if let Expression::TemplateLiteral(tpl) = &p.value
                        && tpl.expressions.is_empty()
                        && let Some(quasi) = tpl.quasis.first()
                    {
                        inline_template = Some(quasi.value.raw.to_string());
                    }
                }
                "styleUrl" => {
                    if let Expression::StringLiteral(lit) = &p.value {
                        style_urls.push(lit.value.to_string());
                    }
                }
                "styleUrls" => {
                    if let Expression::ArrayExpression(arr) = &p.value {
                        for elem in &arr.elements {
                            if let ArrayExpressionElement::StringLiteral(lit) = elem {
                                style_urls.push(lit.value.to_string());
                            }
                        }
                    }
                }
                "host" => {
                    if let Expression::ObjectExpression(host_obj) = &p.value {
                        extract_host_member_refs(host_obj, &mut host_member_refs);
                    }
                }
                "inputs" | "outputs" => {
                    extract_input_output_members(&p.value, &mut input_output_members);
                }
                "queries" => {
                    extract_query_members(&p.value, &mut input_output_members);
                }
                _ => {}
            }
        }

        let has_data = template_url.is_some()
            || !style_urls.is_empty()
            || inline_template.is_some()
            || !host_member_refs.is_empty()
            || !input_output_members.is_empty();

        if has_data {
            return Some(AngularComponentMetadata {
                template_url,
                style_urls,
                inline_template,
                host_member_refs,
                input_output_members,
            });
        }
    }
    None
}

/// Extract identifier references from Angular `host:` binding expressions.
///
/// Host bindings use string keys like `'[class.active]': 'isActive'`,
/// `'(click)': 'onClick($event)'`, `'[style.--color]': 'customColor()'`.
/// The value strings contain expressions referencing class members.
fn extract_host_member_refs(host_obj: &oxc_ast::ast::ObjectExpression<'_>, refs: &mut Vec<String>) {
    for prop in &host_obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        if let Expression::StringLiteral(lit) = &p.value {
            extract_identifiers_from_host_expr(&lit.value, refs);
        }
    }
}

/// Extract property names from Angular `queries:` metadata object.
///
/// `queries: { myRef: new ViewChild('ref') }` declares class properties as
/// view/content queries. The object keys are the class member names.
fn extract_query_members(value: &Expression<'_>, members: &mut Vec<String>) {
    let Expression::ObjectExpression(obj) = value else {
        return;
    };
    for prop in &obj.properties {
        let ObjectPropertyKind::ObjectProperty(p) = prop else {
            continue;
        };
        if let Some(name) = p.key.static_name() {
            let name = name.to_string();
            if !name.is_empty() {
                members.push(name);
            }
        }
    }
}

/// Extract member names from Angular `inputs`/`outputs` metadata arrays.
///
/// Handles `inputs: ['memberName']` and `inputs: ['memberName: alias']`
/// (takes the part before the colon as the class member name).
fn extract_input_output_members(value: &Expression<'_>, members: &mut Vec<String>) {
    let Expression::ArrayExpression(arr) = value else {
        return;
    };
    for elem in &arr.elements {
        let ArrayExpressionElement::StringLiteral(lit) = elem else {
            continue;
        };
        let member = lit
            .value
            .as_ref()
            .split(':')
            .next()
            .unwrap_or_default()
            .trim();
        if !member.is_empty() {
            members.push(member.to_string());
        }
    }
}

/// Extract top-level identifier names from an Angular host binding expression string.
///
/// These are simple expressions like `'isActive'`, `'onClick($event)'`,
/// `'hostClass()'`, `'customColor()'`. We extract the leading identifier
/// before any `(` or `.` character.
fn extract_identifiers_from_host_expr(expr: &str, refs: &mut Vec<String>) {
    let expr = expr.trim();
    if expr.is_empty() {
        return;
    }
    // Extract the leading identifier (before any call parens, member access, etc.)
    let ident: String = expr
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '$')
        .collect();
    if !is_valid_member_identifier(&ident) || refs.contains(&ident) {
        return;
    }
    refs.push(ident);
}

/// Check if a string is a valid class member identifier (not a keyword or built-in).
fn is_valid_member_identifier(ident: &str) -> bool {
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

/// Check if a class has any Angular decorator (`@Component`, `@Directive`,
/// `@Injectable`, `@Pipe`).
pub fn has_angular_class_decorator(class: &Class<'_>) -> bool {
    class.decorators.iter().any(|d| {
        if let Expression::CallExpression(call) = &d.expression
            && let Expression::Identifier(id) = &call.callee
        {
            matches!(
                id.name.as_str(),
                "Component" | "Directive" | "Injectable" | "Pipe"
            )
        } else {
            false
        }
    })
}

/// Check if a property initializer is an Angular signal API call.
///
/// Matches `input()`, `input.required()`, `output()`, `outputFromObservable()`,
/// `model()`, `viewChild()`, `viewChildren()`, `contentChild()`, `contentChildren()`.
fn is_angular_signal_initializer(value: &Expression<'_>) -> bool {
    let Expression::CallExpression(call) = value else {
        return false;
    };
    match &call.callee {
        // Direct call: `input()`, `output()`, `model()`, etc.
        Expression::Identifier(id) => ANGULAR_SIGNAL_APIS.contains(&id.name.as_str()),
        // Static member call: `input.required()`
        Expression::StaticMemberExpression(member) => {
            if let Expression::Identifier(obj) = &member.object {
                ANGULAR_SIGNAL_APIS.contains(&obj.name.as_str())
                    && member.property.name == "required"
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Extract class members (methods and properties) from a class declaration.
///
/// When `is_angular_class` is true, properties initialized with Angular signal
/// APIs (`input()`, `output()`, `outputFromObservable()`, `model()`, `viewChild()`, etc.) are treated as
/// decorated (framework-managed) to prevent false unused-member reports.
pub fn extract_class_members(class: &Class<'_>, is_angular_class: bool) -> Vec<MemberInfo> {
    let mut members = Vec::new();
    for element in &class.body.body {
        match element {
            ClassElement::MethodDefinition(method) => {
                if let Some(name) = method.key.static_name() {
                    let name_str = name.to_string();
                    // Skip constructor, private, and protected methods
                    if name_str != "constructor"
                        && !matches!(
                            method.accessibility,
                            Some(
                                oxc_ast::ast::TSAccessibility::Private
                                    | oxc_ast::ast::TSAccessibility::Protected
                            )
                        )
                    {
                        members.push(MemberInfo {
                            name: name_str,
                            kind: MemberKind::ClassMethod,
                            span: method.span,
                            has_decorator: !method.decorators.is_empty(),
                        });
                    }
                }
            }
            ClassElement::PropertyDefinition(prop) => {
                if let Some(name) = prop.key.static_name()
                    && !matches!(
                        prop.accessibility,
                        Some(
                            oxc_ast::ast::TSAccessibility::Private
                                | oxc_ast::ast::TSAccessibility::Protected
                        )
                    )
                {
                    let has_decorator = !prop.decorators.is_empty()
                        || (is_angular_class
                            && prop
                                .value
                                .as_ref()
                                .is_some_and(is_angular_signal_initializer));
                    members.push(MemberInfo {
                        name: name.to_string(),
                        kind: MemberKind::ClassProperty,
                        span: prop.span,
                        has_decorator,
                    });
                }
            }
            _ => {}
        }
    }
    members
}

/// Extract the parent class name from an `extends` clause, if present.
///
/// Returns `Some("ParentClass")` for `class Foo extends ParentClass { ... }`.
/// Only handles simple identifier references — complex expressions like
/// `extends mixin(Base)` return `None`.
pub fn extract_super_class_name(class: &Class<'_>) -> Option<String> {
    match class.super_class.as_ref()? {
        Expression::Identifier(ident) => Some(ident.name.to_string()),
        _ => None,
    }
}

/// Check if an argument expression is `import.meta.url`.
pub(super) fn is_meta_url_arg(arg: &Argument<'_>) -> bool {
    if let Argument::StaticMemberExpression(member) = arg
        && member.property.name == "url"
        && matches!(member.object, Expression::MetaProperty(_))
    {
        return true;
    }
    false
}

/// Extract static prefix and optional suffix from a binary addition chain.
pub(super) fn extract_concat_parts(
    expr: &BinaryExpression<'_>,
) -> Option<(String, Option<String>)> {
    let prefix = extract_leading_string(&expr.left)?;
    let suffix = extract_trailing_string(&expr.right);
    Some((prefix, suffix))
}

fn extract_leading_string(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::StringLiteral(lit) => Some(lit.value.to_string()),
        Expression::BinaryExpression(bin)
            if bin.operator == oxc_ast::ast::BinaryOperator::Addition =>
        {
            extract_leading_string(&bin.left)
        }
        _ => None,
    }
}

fn extract_trailing_string(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::StringLiteral(lit) => {
            let s = lit.value.to_string();
            if s.is_empty() { None } else { Some(s) }
        }
        _ => None,
    }
}

/// Convert a simple regex extension filter pattern to a glob suffix.
///
/// Handles common `require.context()` patterns like:
/// - `\.vue$` → `".vue"`
/// - `\.tsx?$` → uses `".ts"` / `".tsx"` via glob `".{ts,tsx}"`
/// - `\.(js|ts)$` → `".{js,ts}"`
/// - `\.(js|jsx|ts|tsx)$` → `".{js,jsx,ts,tsx}"`
///
/// Returns `None` for patterns that are too complex to convert.
pub(super) fn regex_pattern_to_suffix(pattern: &str) -> Option<String> {
    // Strip leading `^` or `.*` anchors (they don't affect extension matching)
    let p = pattern.strip_prefix('^').unwrap_or(pattern);
    let p = p.strip_prefix(".*").unwrap_or(p);

    // Must start with `\.` (escaped dot for extension)
    let p = p.strip_prefix("\\.")?;

    // Must end with `$`
    let p = p.strip_suffix('$')?;

    // Pattern: `ext?` — e.g., `tsx?` → {ts,tsx}
    if let Some(base) = p.strip_suffix('?') {
        // base must be simple alphanumeric (e.g., "tsx" from "tsx?")
        if base.chars().all(|c| c.is_ascii_alphanumeric()) && !base.is_empty() {
            let without_last = &base[..base.len() - 1];
            if without_last.is_empty() {
                // Single char like `x?` → matches "" or "x", too ambiguous
                return None;
            }
            return Some(format!(".{{{without_last},{base}}}"));
        }
        return None;
    }

    // Pattern: `(ext1|ext2|...)` — e.g., `(js|ts)` → {js,ts}
    if let Some(inner) = p.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        let exts: Vec<&str> = inner.split('|').collect();
        if exts
            .iter()
            .all(|e| e.chars().all(|c| c.is_ascii_alphanumeric()) && !e.is_empty())
        {
            return Some(format!(".{{{}}}", exts.join(",")));
        }
        return None;
    }

    // Pattern: simple extension like `vue`, `json`, `css`
    if p.chars().all(|c| c.is_ascii_alphanumeric()) && !p.is_empty() {
        return Some(format!(".{p}"));
    }

    None
}

/// Try to extract a class name from a factory function argument.
///
/// Matches patterns where a call argument is an arrow function or function
/// expression whose body returns a `new ClassName(...)` expression:
///
/// - `() => new Foo()`  (arrow expression body)
/// - `() => { return new Foo(); }` (arrow block body)
/// - `function() { return new Foo(); }` (function expression)
///
/// Returns the class name if found and it's not a built-in constructor.
pub(super) fn try_extract_factory_new_class(arguments: &[Argument<'_>]) -> Option<String> {
    for arg in arguments {
        let class_name = match arg {
            Argument::ArrowFunctionExpression(arrow) => {
                if arrow.expression {
                    // Expression body: `() => new Foo()`
                    extract_new_class_from_statement(arrow.body.statements.first()?)
                } else {
                    // Block body: `() => { return new Foo(); }`
                    extract_new_class_from_return_body(&arrow.body.statements)
                }
            }
            Argument::FunctionExpression(func) => {
                // `function() { return new Foo(); }`
                extract_new_class_from_return_body(&func.body.as_ref()?.statements)
            }
            _ => None,
        };
        if let Some(name) = class_name
            && !is_builtin_constructor(&name)
        {
            return Some(name);
        }
    }
    None
}

/// Extract a class name from a `new ClassName(...)` in an expression statement.
fn extract_new_class_from_statement(stmt: &Statement<'_>) -> Option<String> {
    if let Statement::ExpressionStatement(expr_stmt) = stmt
        && let Expression::NewExpression(new_expr) = &expr_stmt.expression
        && let Expression::Identifier(callee) = &new_expr.callee
    {
        return Some(callee.name.to_string());
    }
    None
}

/// Extract a class name from the last `return new ClassName(...)` in a function body.
fn extract_new_class_from_return_body(stmts: &[Statement<'_>]) -> Option<String> {
    for stmt in stmts.iter().rev() {
        if let Statement::ReturnStatement(ret) = stmt
            && let Some(Expression::NewExpression(new_expr)) = &ret.argument
            && let Expression::Identifier(callee) = &new_expr.callee
        {
            return Some(callee.name.to_string());
        }
    }
    None
}

/// Check if a name is a well-known JavaScript/DOM built-in constructor.
///
/// Used to avoid creating spurious instance bindings for `new URL()`, `new Map()`,
/// etc. These are never user-exported classes and would only create noise in the
/// member access tracking pipeline.
pub(super) fn is_builtin_constructor(name: &str) -> bool {
    matches!(
        name,
        "Array"
            | "ArrayBuffer"
            | "Blob"
            | "Boolean"
            | "DataView"
            | "Date"
            | "Error"
            | "EvalError"
            | "Event"
            | "Float32Array"
            | "Float64Array"
            | "FormData"
            | "Headers"
            | "Int8Array"
            | "Int16Array"
            | "Int32Array"
            | "Map"
            | "Number"
            | "Object"
            | "Promise"
            | "Proxy"
            | "RangeError"
            | "ReferenceError"
            | "RegExp"
            | "Request"
            | "Response"
            | "Set"
            | "SharedArrayBuffer"
            | "String"
            | "SyntaxError"
            | "TypeError"
            | "URIError"
            | "URL"
            | "URLSearchParams"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Uint16Array"
            | "Uint32Array"
            | "WeakMap"
            | "WeakRef"
            | "WeakSet"
            | "Worker"
            | "AbortController"
            | "ReadableStream"
            | "WritableStream"
            | "TransformStream"
            | "TextEncoder"
            | "TextDecoder"
            | "MutationObserver"
            | "IntersectionObserver"
            | "ResizeObserver"
            | "PerformanceObserver"
            | "MessageChannel"
            | "BroadcastChannel"
            | "WebSocket"
            | "XMLHttpRequest"
            | "EventEmitter"
            | "Buffer"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── regex_pattern_to_suffix ──────────────────────────────

    #[test]
    fn regex_suffix_with_caret_anchor() {
        // Leading `^` should be stripped — result matches bare pattern
        assert_eq!(
            regex_pattern_to_suffix(r"^\.vue$"),
            Some(".vue".to_string())
        );
        assert_eq!(
            regex_pattern_to_suffix(r"^\.json$"),
            Some(".json".to_string())
        );
    }

    #[test]
    fn regex_suffix_with_dotstar_anchor() {
        // Leading `.*` should be stripped
        assert_eq!(
            regex_pattern_to_suffix(r".*\.css$"),
            Some(".css".to_string())
        );
    }

    #[test]
    fn regex_suffix_with_both_anchors() {
        // Both `^` and `.*` as prefix
        assert_eq!(
            regex_pattern_to_suffix(r"^.*\.ts$"),
            Some(".ts".to_string())
        );
    }

    #[test]
    fn regex_suffix_single_char_optional_returns_none() {
        // `\.x?$` — single char base "x" minus last char = "" which is too ambiguous
        assert_eq!(regex_pattern_to_suffix(r"\.x?$"), None);
    }

    #[test]
    fn regex_suffix_two_char_optional() {
        // `\.ts?$` — base "ts" minus last = "t", result: .{t,ts}
        assert_eq!(
            regex_pattern_to_suffix(r"\.ts?$"),
            Some(".{t,ts}".to_string())
        );
    }

    #[test]
    fn regex_suffix_no_dollar_sign_returns_none() {
        // Missing trailing `$` should return None
        assert_eq!(regex_pattern_to_suffix(r"\.vue"), None);
    }

    #[test]
    fn regex_suffix_no_escaped_dot_returns_none() {
        // Missing `\.` prefix should return None
        assert_eq!(regex_pattern_to_suffix(r"vue$"), None);
    }

    #[test]
    fn regex_suffix_empty_alternation_returns_none() {
        // Empty group `()` should return None (no extensions)
        assert_eq!(regex_pattern_to_suffix(r"\.()$"), None);
    }

    #[test]
    fn regex_suffix_alternation_with_special_chars_returns_none() {
        // Special characters in alternation group
        assert_eq!(regex_pattern_to_suffix(r"\.(j.s|ts)$"), None);
    }

    #[test]
    fn regex_suffix_complex_wildcard_returns_none() {
        assert_eq!(regex_pattern_to_suffix(r"\..+$"), None);
        assert_eq!(regex_pattern_to_suffix(r"\.[a-z]+$"), None);
    }

    // ── is_builtin_constructor ───────────────────────────────

    #[test]
    fn builtin_constructors_recognized() {
        assert!(is_builtin_constructor("Array"));
        assert!(is_builtin_constructor("Map"));
        assert!(is_builtin_constructor("Set"));
        assert!(is_builtin_constructor("WeakMap"));
        assert!(is_builtin_constructor("WeakSet"));
        assert!(is_builtin_constructor("Promise"));
        assert!(is_builtin_constructor("URL"));
        assert!(is_builtin_constructor("URLSearchParams"));
        assert!(is_builtin_constructor("RegExp"));
        assert!(is_builtin_constructor("Date"));
        assert!(is_builtin_constructor("Error"));
        assert!(is_builtin_constructor("TypeError"));
        assert!(is_builtin_constructor("Request"));
        assert!(is_builtin_constructor("Response"));
        assert!(is_builtin_constructor("Headers"));
        assert!(is_builtin_constructor("FormData"));
        assert!(is_builtin_constructor("Blob"));
        assert!(is_builtin_constructor("AbortController"));
        assert!(is_builtin_constructor("ReadableStream"));
        assert!(is_builtin_constructor("WritableStream"));
        assert!(is_builtin_constructor("TransformStream"));
        assert!(is_builtin_constructor("TextEncoder"));
        assert!(is_builtin_constructor("TextDecoder"));
        assert!(is_builtin_constructor("Worker"));
        assert!(is_builtin_constructor("WebSocket"));
        assert!(is_builtin_constructor("EventEmitter"));
        assert!(is_builtin_constructor("Buffer"));
        assert!(is_builtin_constructor("MutationObserver"));
        assert!(is_builtin_constructor("IntersectionObserver"));
        assert!(is_builtin_constructor("ResizeObserver"));
        assert!(is_builtin_constructor("MessageChannel"));
        assert!(is_builtin_constructor("BroadcastChannel"));
    }

    #[test]
    fn user_defined_classes_not_builtin() {
        assert!(!is_builtin_constructor("MyService"));
        assert!(!is_builtin_constructor("UserRepository"));
        assert!(!is_builtin_constructor("AppController"));
        assert!(!is_builtin_constructor("DatabaseConnection"));
        assert!(!is_builtin_constructor("Logger"));
        assert!(!is_builtin_constructor("Config"));
        assert!(!is_builtin_constructor(""));
    }

    #[test]
    fn builtin_names_are_case_sensitive() {
        assert!(!is_builtin_constructor("array"));
        assert!(!is_builtin_constructor("map"));
        assert!(!is_builtin_constructor("url"));
        assert!(!is_builtin_constructor("MAP"));
        assert!(!is_builtin_constructor("ARRAY"));
    }
}
