//! `@keyframes` reference hints that the Sass/Less lowering cannot keep.
//!
//! The lowering replaces a preprocessor variable with a placeholder, drops
//! each `@include`, and drops a rule that CSS cannot parse, such as a
//! `%placeholder` rule. So `animation: $name 1s`, `@include animate(spin)` and
//! `%pulse { animation-name: pulsate; }` give no keyframes reference in the
//! virtual CSS. Without these hints, a keyframes block that the project uses
//! in such a place shows up as an unreferenced-keyframes candidate.

/// What a Sass/Less source says about keyframes references that the lowered
/// CSS does not contain.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct KeyframeHints {
    /// An `animation` or `animation-name` value holds an interpolation, so the
    /// referenced name is not known before compilation.
    pub(super) opaque_reference: bool,
    /// Bare identifiers in an `animation` or `animation-name` value and in the
    /// arguments of an `@include` or a Less mixin call. A caller counts one as
    /// a reference only when a `@keyframes` block defines that name.
    pub(super) possible_names: Vec<String>,
    /// Variables (`$name`, `@name`) in an `animation` or `animation-name`
    /// value that are no parameter of the enclosing mixin. Each one is opaque
    /// unless a literal assignment gives its value.
    pub(super) animation_variables: Vec<String>,
    /// Variables in mixin call arguments. The assigned names of each one are
    /// possible names. An unknown one is not opaque, because most mixin
    /// arguments have no relation to an animation.
    pub(super) argument_variables: Vec<String>,
    /// Variable assignments (`$fade: fade-in;`, `@fade: fade-in;`). The value
    /// is the identifier list of a literal value, or `None` when the value
    /// holds a variable or an interpolation.
    pub(super) assignments: Vec<(String, Option<Vec<String>>)>,
}

/// Scan a Sass/Less source for keyframes references that the lowering loses.
pub(super) fn preprocessor_keyframe_hints(source: &str) -> KeyframeHints {
    let mut hints = KeyframeHints::default();
    // One entry per open `{` block: the parameters of a mixin definition, or
    // an empty list for any other block.
    let mut scopes: Vec<Vec<String>> = Vec::new();
    for (statement, terminator) in statements(source) {
        let statement = statement.trim();
        match terminator {
            b'{' => scopes.push(mixin_parameters(statement)),
            b'}' => {
                scan_declaration(statement, &scopes, &mut hints);
                scopes.pop();
            }
            _ => scan_declaration(statement, &scopes, &mut hints),
        }
    }
    hints.possible_names.sort_unstable();
    hints.possible_names.dedup();
    hints
}

/// Scan one statement that ends at `;` or `}`.
fn scan_declaration(statement: &str, scopes: &[Vec<String>], hints: &mut KeyframeHints) {
    if let Some(rest) = statement.strip_prefix("@include") {
        push_call_arguments(rest, hints);
    } else if is_less_mixin_call(statement) {
        push_call_arguments(statement, hints);
    } else if let Some((name, value)) = variable_assignment(statement) {
        // A function call such as `unique-id()` gives a value that the scan
        // cannot know.
        let names = (!is_dynamic_value(value) && !value.contains('(')).then(|| {
            let mut names = Vec::new();
            push_identifiers(value, &mut names);
            names
        });
        hints.assignments.push((name.to_owned(), names));
    } else if let Some(value) = animation_value(statement) {
        if value.contains("#{") || value.contains("@{") {
            hints.opaque_reference = true;
            return;
        }
        push_identifiers(value, &mut hints.possible_names);
        for variable in variables(value) {
            if !scopes.iter().flatten().any(|param| *param == variable) {
                hints.animation_variables.push(variable.to_owned());
            }
        }
    }
}

/// Split a source into statements that end at `;`, `{` or `}`. A `#{...}` or
/// `@{...}` interpolation, a quoted string, a comment and a `;` inside
/// parentheses (Less mixin arguments) stay inside their statement.
fn statements(source: &str) -> Vec<(&str, u8)> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'#' | b'@' if bytes.get(i + 1) == Some(&b'{') => {
                i = skip_past(bytes, i + 2, b'}');
            }
            quote @ (b'"' | b'\'') => i = skip_string(bytes, i + 1, quote),
            b'/' if bytes.get(i + 1) == Some(&b'*') => {
                i = skip_block_comment(bytes, i + 2);
            }
            // `url(https://...)` holds `//` after a `:`, which is no comment.
            b'/' if bytes.get(i + 1) == Some(&b'/') && (i == 0 || bytes[i - 1] != b':') => {
                i = skip_past(bytes, i + 2, b'\n');
            }
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                depth = depth.saturating_sub(1);
                i += 1;
            }
            b';' if depth > 0 => i += 1,
            terminator @ (b';' | b'{' | b'}') => {
                out.push((&source[start..i], terminator));
                depth = 0;
                i += 1;
                start = i;
            }
            _ => i += 1,
        }
    }
    out.push((&source[start..], b';'));
    out
}

fn skip_past(bytes: &[u8], from: usize, end: u8) -> usize {
    bytes[from.min(bytes.len())..]
        .iter()
        .position(|&b| b == end)
        .map_or(bytes.len(), |offset| from + offset + 1)
}

fn skip_string(bytes: &[u8], mut i: usize, quote: u8) -> usize {
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    bytes.len()
}

fn skip_block_comment(bytes: &[u8], from: usize) -> usize {
    bytes[from.min(bytes.len())..]
        .windows(2)
        .position(|pair| pair == b"*/")
        .map_or(bytes.len(), |offset| from + offset + 2)
}

/// The parameters of a `@mixin name($a, $b)` or a Less `.name(@a; @b)`
/// definition. Any other block gives an empty list. A Less mixin also gets
/// the implicit `@arguments` parameter.
fn mixin_parameters(prelude: &str) -> Vec<String> {
    let less = is_less_mixin_head(prelude);
    if !less && !prelude.starts_with("@mixin") {
        return Vec::new();
    }
    let Some(open) = prelude.find('(') else {
        return Vec::new();
    };
    let mut parameters: Vec<String> = variables(&prelude[open..]).map(str::to_owned).collect();
    if less {
        parameters.push("@arguments".to_owned());
    }
    parameters
}

/// `.mixin(args);` or `#namespace.mixin(args);`: a Less mixin call. Only the
/// part before `(` must hold no `:`, so a named argument such as
/// `.m(@b: spin);` matches. A selector such as `.a:not(.b) {` never gets here,
/// because the caller passes only statements that end at `;` or `}`.
fn is_less_mixin_call(statement: &str) -> bool {
    is_less_mixin_head(statement)
}

/// A statement that starts with a Less mixin name and holds `(`, with no `:`
/// before the `(`.
fn is_less_mixin_head(statement: &str) -> bool {
    let mut chars = statement.chars();
    let starts_as_mixin = matches!(chars.next(), Some('.' | '#'))
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || matches!(c, '_' | '-'));
    starts_as_mixin
        && statement
            .split_once('(')
            .is_some_and(|(head, _)| !head.contains(':'))
}

/// `$name: value` or Less `@name: value`, also with `!default`.
fn variable_assignment(statement: &str) -> Option<(&str, &str)> {
    let (name, value) = statement.split_once(':')?;
    let name = name.trim();
    let sigil = name.bytes().next()?;
    let ident = &name[1..];
    let is_variable =
        matches!(sigil, b'$' | b'@') && !ident.is_empty() && ident.bytes().all(is_ident_continue);
    is_variable.then_some((name, value))
}

/// The value of an `animation` or `animation-name` declaration, also with a
/// vendor prefix.
fn animation_value(statement: &str) -> Option<&str> {
    let (property, value) = statement.split_once(':')?;
    let property = property.trim().to_ascii_lowercase();
    let property = ["-webkit-", "-moz-", "-o-", "-ms-"]
        .iter()
        .find_map(|prefix| property.strip_prefix(prefix))
        .unwrap_or(&property);
    matches!(property, "animation" | "animation-name").then_some(value)
}

/// A value that holds a Sass or Less variable or an interpolation.
fn is_dynamic_value(value: &str) -> bool {
    value.contains("#{") || value.contains("@{") || variables(value).next().is_some()
}

/// Each `$name` and `@name` token of `text`, with its sigil.
fn variables(text: &str) -> impl Iterator<Item = &str> {
    let bytes = text.as_bytes();
    bytes.iter().enumerate().filter_map(move |(i, &b)| {
        if !matches!(b, b'$' | b'@') {
            return None;
        }
        let first = *bytes.get(i + 1)?;
        if !(first.is_ascii_alphabetic() || first == b'_' || first == b'-') {
            return None;
        }
        let end = bytes[i + 1..]
            .iter()
            .position(|c| !is_ident_continue(*c))
            .map_or(bytes.len(), |offset| i + 1 + offset);
        Some(&text[i..end])
    })
}

/// Push each bare identifier and each variable inside the outer parentheses
/// of a mixin call.
fn push_call_arguments(call: &str, hints: &mut KeyframeHints) {
    let Some(open) = call.find('(') else {
        return;
    };
    let arguments = call[open + 1..]
        .rfind(')')
        .map_or(&call[open + 1..], |close| &call[open + 1..open + 1 + close]);
    push_identifiers(arguments, &mut hints.possible_names);
    hints
        .argument_variables
        .extend(variables(arguments).map(str::to_owned));
}

/// Push each bare identifier of `text`. A variable (`$x`, `@x`), a member
/// (`.x`), a flag (`!default`) or a number is not a keyframes name.
fn push_identifiers(text: &str, into: &mut Vec<String>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if is_ident_continue(b) {
            let start = i;
            while i < bytes.len() && is_ident_continue(bytes[i]) {
                i += 1;
            }
            let preceded_by_sigil =
                start > 0 && matches!(bytes[start - 1], b'$' | b'@' | b'.' | b'!');
            let word = &text[start..i];
            let starts_as_ident = word
                .bytes()
                .find(|c| *c != b'-')
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_');
            if !preceded_by_sigil && starts_as_ident {
                into.push(word.to_owned());
            }
        } else {
            i += 1;
        }
    }
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolation_in_animation_value_is_opaque() {
        for source in [
            ".b { -webkit-animation-name: #{$prefix}-spin; }",
            ".b { animation-name: ~\"@{prefix}-spin\"; }",
            "@mixin m($name) { animation: #{$name}-in 1s; }",
        ] {
            assert!(
                preprocessor_keyframe_hints(source).opaque_reference,
                "{source}"
            );
        }
    }

    #[test]
    fn free_variable_in_animation_value_is_recorded() {
        let hints =
            preprocessor_keyframe_hints(".b { animation: $fade 2s; }\n.c { animation: @name 1s; }");
        assert!(!hints.opaque_reference);
        assert_eq!(hints.animation_variables, vec!["$fade", "@name"]);
    }

    #[test]
    fn mixin_parameter_in_animation_value_is_not_recorded() {
        for source in [
            "@mixin animate($name, $d: 1s) { animation: $name $d; }\n.a { @include animate(spin); }",
            ".m(@a; @b) { animation: @a 1s; }\n.x { .m(spin; 2s); }",
            ".m(@a) { animation: @arguments; }",
            "@mixin outer($n) { .x { animation: $n 1s; } }",
        ] {
            let hints = preprocessor_keyframe_hints(source);
            assert!(!hints.opaque_reference, "{source}");
            assert!(hints.animation_variables.is_empty(), "{source}: {hints:?}");
        }
    }

    #[test]
    fn mixin_parameter_does_not_leak_out_of_its_scope() {
        let source = "@mixin m($name) { animation: $name 1s; }\n.a { animation: $name 2s; }";
        let hints = preprocessor_keyframe_hints(source);
        assert_eq!(hints.animation_variables, vec!["$name"]);
    }

    #[test]
    fn literal_assignment_is_recorded() {
        let source = "$fade: fade-in !default;\n@pulse: pulse;\n$other: $fade;\n$id: unique-id();\n.a { animation: $fade 1s; }";
        let hints = preprocessor_keyframe_hints(source);
        assert_eq!(
            hints.assignments,
            vec![
                ("$fade".to_owned(), Some(vec!["fade-in".to_owned()])),
                ("@pulse".to_owned(), Some(vec!["pulse".to_owned()])),
                ("$other".to_owned(), None),
                ("$id".to_owned(), None),
            ]
        );
    }

    #[test]
    fn literal_animation_value_gives_possible_names() {
        let source =
            "%pulse { animation-name: pulsate; }\n.a { animation: spin 1s linear; color: $red; }";
        let hints = preprocessor_keyframe_hints(source);
        assert!(!hints.opaque_reference);
        assert_eq!(hints.possible_names, vec!["linear", "pulsate", "spin"]);
    }

    #[test]
    fn mixin_call_arguments_are_collected() {
        let source = ".a { @include animate(spin, $speed, 2s); }\n.b { .pulse(fade-in 1s); }\n.c:not(.d) { color: red; }\n";
        let hints = preprocessor_keyframe_hints(source);
        assert!(!hints.opaque_reference);
        assert_eq!(hints.possible_names, vec!["fade-in", "spin"]);
        assert_eq!(hints.argument_variables, vec!["$speed"]);
    }

    #[test]
    fn less_named_and_semicolon_arguments_are_collected() {
        let source = ".x { .m(@b: spin); }\n.y { .m(pulse; 2s); }\n.z { #ns.m(@a: fade); }";
        let hints = preprocessor_keyframe_hints(source);
        assert_eq!(hints.possible_names, vec!["fade", "pulse", "spin"]);
    }

    #[test]
    fn interpolation_braces_do_not_end_a_statement() {
        let source = ".a { animation: #{$name} 1s; }\n.b { @include x(spin); }";
        let hints = preprocessor_keyframe_hints(source);
        assert!(hints.opaque_reference);
        assert_eq!(hints.possible_names, vec!["spin"]);
    }

    #[test]
    fn comments_hide_their_content() {
        let source = "// animation: $x;\n/* @include y(spin); */\n.a { color: red; }";
        assert_eq!(
            preprocessor_keyframe_hints(source),
            KeyframeHints::default()
        );
    }
}
