//! Read one `{ find, replacement }` alias entry.
//!
//! Vite and Rollup accept a string or a regular expression as `find`. The
//! resolver keeps aliases as `(prefix, replacement)` pairs, so only two regex
//! shapes map to it without a change of meaning:
//! - An anchored literal, such as `/^node:module$/`. The key is the literal.
//! - An anchored prefix with one trailing capture, such as `/^@\/(.*)$/`, when
//!   the replacement ends with `/$1`. The key is the prefix and the replacement
//!   loses the `/$1` suffix.
//!
//! Every other regex gives no alias, which keeps the old behavior.

use oxc_ast::ast::{Expression, ObjectExpression, RegExpFlags, RegExpLiteral, TemplateLiteral};

use super::{
    PathScope, alias_replacement_kinded, expression_to_string, find_property, path_string_in,
};

/// The capture group text that matches the rest of the specifier.
const REST_CAPTURES: &[&str] = &["(.*)", "(.+)"];

/// The replacement suffix that inserts the first capture after a slash.
const CAPTURE_SUFFIX: &str = "/$1";

/// The key that a regex `find` maps to.
#[derive(Debug, PartialEq, Eq)]
enum RegexAliasKey {
    /// The regex matches one specifier only.
    Exact(String),
    /// The regex matches `<prefix>/<rest>` and captures `<rest>` as `$1`.
    Prefix(String),
}

/// Build the `(find, replacement, is_bare)` tuple of one alias array entry.
///
/// A string `find` is the alias key as written. A regular expression `find`
/// maps to a key only when the prefix alias model can express it; see
/// [`regex_alias_pair`].
pub(super) fn object_alias_pair(
    obj: &ObjectExpression<'_>,
    scope: PathScope<'_>,
) -> Option<(String, String, bool)> {
    let find = &find_property(obj, "find")?.value;
    let (replacement, is_bare) = find_property(obj, "replacement")
        .and_then(|prop| alias_replacement_kinded(&prop.value, scope))?;
    match find.without_parentheses() {
        Expression::RegExpLiteral(regex) => regex_alias_pair(regex, &replacement)
            .map(|(key, replacement)| (key, replacement, is_bare)),
        other => expression_to_string(other).map(|key| (key, replacement, is_bare)),
    }
}

/// Evaluate an alias replacement path. A template literal that starts with a
/// path call, such as `${path.resolve(__dirname, 'src')}/$1`, is read too,
/// because regex aliases use it to append a capture reference to a directory.
pub(super) fn path_value(expr: &Expression<'_>, scope: PathScope<'_>) -> Option<String> {
    match expr {
        Expression::TemplateLiteral(template) if !template.expressions.is_empty() => {
            path_template_string(template, scope)
        }
        _ => path_string_in(expr, scope),
    }
}

/// Concatenate a template literal whose interpolations all evaluate to paths.
/// An unknown identifier, such as `${dirname}`, gives no value.
fn path_template_string(template: &TemplateLiteral<'_>, scope: PathScope<'_>) -> Option<String> {
    let starts_with_path_call = template.quasis.first()?.value.raw.is_empty()
        && matches!(
            template.expressions.first()?.without_parentheses(),
            Expression::CallExpression(_) | Expression::NewExpression(_)
        );
    if !starts_with_path_call {
        return None;
    }
    let mut value = String::new();
    for (index, quasi) in template.quasis.iter().enumerate() {
        value.push_str(quasi.value.raw.as_str());
        if let Some(expr) = template.expressions.get(index) {
            value.push_str(&path_string_in(expr, scope)?);
        }
    }
    Some(value)
}

/// Convert a regex `find` and its evaluated replacement to an alias pair.
///
/// Returns `None` when the regex has a shape that the prefix model can not
/// express. A replacement that keeps a `$` reference after the conversion is
/// also refused, because the resolver substitutes no capture groups.
fn regex_alias_pair(regex: &RegExpLiteral<'_>, replacement: &str) -> Option<(String, String)> {
    if !regex
        .regex
        .flags
        .difference(RegExpFlags::U | RegExpFlags::V)
        .is_empty()
    {
        return None;
    }
    let (key, replacement) = match parse_regex_alias_key(regex.regex.pattern.text.as_str())? {
        RegexAliasKey::Exact(key) => (key, replacement),
        RegexAliasKey::Prefix(key) => (key, replacement.strip_suffix(CAPTURE_SUFFIX)?),
    };
    if replacement.contains('$') {
        return None;
    }
    Some((key, replacement.to_owned()))
}

/// Parse the pattern text of a regex `find` into an alias key.
fn parse_regex_alias_key(pattern: &str) -> Option<RegexAliasKey> {
    let body = pattern.strip_prefix('^')?;
    for capture in REST_CAPTURES {
        let Some(head) = body.strip_suffix('$').unwrap_or(body).strip_suffix(capture) else {
            continue;
        };
        let prefix = unescape_literal(head)?;
        let prefix = prefix.strip_suffix('/')?;
        return (!prefix.is_empty()).then(|| RegexAliasKey::Prefix(prefix.to_owned()));
    }
    let literal = unescape_literal(body.strip_suffix('$')?)?;
    (!literal.is_empty()).then_some(RegexAliasKey::Exact(literal))
}

/// Read a regex fragment as literal text. Returns `None` when the fragment has
/// a metacharacter or an escape that is not a plain punctuation character.
fn unescape_literal(fragment: &str) -> Option<String> {
    let mut literal = String::with_capacity(fragment.len());
    let mut chars = fragment.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\\' => {
                let escaped = chars.next()?;
                if escaped.is_ascii_alphanumeric() {
                    return None;
                }
                literal.push(escaped);
            }
            '.' | '*' | '+' | '?' | '(' | ')' | '[' | ']' | '{' | '}' | '|' | '^' | '$' => {
                return None;
            }
            _ => literal.push(ch),
        }
    }
    Some(literal)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchored_literal_maps_to_an_exact_key() {
        assert_eq!(
            parse_regex_alias_key("^node:module$"),
            Some(RegexAliasKey::Exact("node:module".to_owned()))
        );
        assert_eq!(
            parse_regex_alias_key(r"^@scope\/pkg$"),
            Some(RegexAliasKey::Exact("@scope/pkg".to_owned()))
        );
        assert_eq!(
            parse_regex_alias_key(r"^lodash\.debounce$"),
            Some(RegexAliasKey::Exact("lodash.debounce".to_owned()))
        );
    }

    #[test]
    fn anchored_capture_maps_to_a_prefix_key() {
        for pattern in [r"^@\/(.*)$", "^@/(.*)$", r"^@\/(.+)$", r"^@\/(.*)"] {
            assert_eq!(
                parse_regex_alias_key(pattern),
                Some(RegexAliasKey::Prefix("@".to_owned())),
                "{pattern}"
            );
        }
        assert_eq!(
            parse_regex_alias_key(r"^~lib\/(.*)$"),
            Some(RegexAliasKey::Prefix("~lib".to_owned()))
        );
    }

    #[test]
    fn other_regex_shapes_give_no_key() {
        for pattern in [
            "node:module",
            "^lodash",
            r"\.svg$",
            "^lodash.debounce$",
            r"^(foo|bar)$",
            r"^@(.*)$",
            r"^\/(.*)$",
            r"^@\/(.*)\.js$",
            r"^\d+$",
            "^$",
        ] {
            assert_eq!(parse_regex_alias_key(pattern), None, "{pattern}");
        }
    }
}
