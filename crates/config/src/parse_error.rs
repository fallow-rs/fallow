//! Parse error text for files that fallow reads from a project.
//!
//! The error `Display` of the `toml` crate prints the source line with a
//! caret, and serde messages print the bad value, for example
//! `invalid type: string "abc"`. An `extends` target can be any file, also
//! one outside the project, so the error text does not quote the file. The
//! functions here keep the error class, the line and the column,
//! and drop source lines and the values that the generic serde messages
//! quote. Field and key names stay, because a user needs them to find the
//! bad entry. A message that a fallow deserializer writes with
//! [`own_message`] stays as it is, because it applies only to a value of a
//! known config key. Any other message keeps its first line up to the first
//! quote character.

use std::fmt;

/// The first character of a message that a fallow deserializer writes. A
/// generic serde message starts with its own fixed words, so only a message
/// from [`own_message`] starts with this character.
const OWN_MESSAGE_MARKER: char = '\u{1f}';

/// The text that replaces an omitted value.
const OMITTED: &str = "(value omitted)";

/// `message` marked as text that a fallow deserializer writes. Pass the
/// result to `serde::de::Error::custom`, so the parse error text keeps the
/// message whole.
pub fn own_message(message: impl fmt::Display) -> String {
    format!("{OWN_MESSAGE_MARKER}{message}")
}

/// The part of a serde message after which the expected type follows. The
/// expected type comes from the schema, so it is safe to print.
const EXPECTED_MARKER: &str = ", expected ";

/// Prefixes of serde messages whose rest names a field or a length only.
const NAME_ONLY_PREFIXES: &[&str] = &[
    "unknown field `",
    "missing field `",
    "duplicate field `",
    "invalid length ",
];

/// Prefixes of serde messages that quote the bad value before the expected
/// type.
const VALUE_PREFIXES: &[&str] = &["invalid type: ", "invalid value: "];

/// `message` without the value that a generic serde message quotes.
pub fn redact_message(message: &str) -> String {
    if let Some(own) = message.trim_start().strip_prefix(OWN_MESSAGE_MARKER) {
        return own.replace(OWN_MESSAGE_MARKER, "");
    }
    let first_line = message.lines().next().unwrap_or_default().trim();
    if NAME_ONLY_PREFIXES
        .iter()
        .any(|prefix| first_line.starts_with(prefix))
    {
        return first_line.to_owned();
    }
    for prefix in VALUE_PREFIXES {
        if let Some(rest) = first_line.strip_prefix(prefix) {
            let kind = rest.split_whitespace().next().unwrap_or_default();
            let kind = kind.trim_end_matches(',');
            let head = prefix.trim_end_matches(' ');
            return match rest.rsplit_once(EXPECTED_MARKER) {
                Some((_, expected)) => format!("{head} {kind}{EXPECTED_MARKER}{expected}"),
                None => format!("{head} {kind}"),
            };
        }
    }
    if first_line.starts_with("unknown variant ") {
        return match first_line.rsplit_once(EXPECTED_MARKER) {
            Some((_, expected)) => format!("unknown variant{EXPECTED_MARKER}{expected}"),
            None => "unknown variant".to_owned(),
        };
    }
    match first_line.find(['"', '\'', '`']) {
        Some(cut) => {
            let head = first_line[..cut]
                .trim_end()
                .trim_end_matches([':', '.', ',']);
            format!("{head} {OMITTED}")
        }
        None => first_line.to_owned(),
    }
}

/// One-based line and column of the byte `offset` in `source`.
fn line_and_column(source: &str, offset: usize) -> (usize, usize) {
    let mut end = offset.min(source.len());
    while !source.is_char_boundary(end) {
        end -= 1;
    }
    let before = &source[..end];
    let line = before.matches('\n').count() + 1;
    let column = before
        .rsplit_once('\n')
        .map_or(before, |(_, tail)| tail)
        .chars()
        .count()
        + 1;
    (line, column)
}

/// The text of a TOML parse error, with a position and no source line.
pub fn describe_toml(err: &crate::toml_input::TomlInputError, source: &str) -> String {
    match err {
        crate::toml_input::TomlInputError::TooDeep(inner) => inner.to_string(),
        crate::toml_input::TomlInputError::Parse(inner) => {
            let message = redact_message(inner.message());
            match inner.span() {
                Some(span) => {
                    let (line, column) = line_and_column(source, span.start);
                    format!("{message} at line {line}, column {column}")
                }
                None => message,
            }
        }
    }
}

/// The text of a JSONC parse error, with a position and no value.
pub fn describe_jsonc(err: &jsonc_parser::errors::ParseError) -> String {
    use jsonc_parser::errors::ParseErrorKind;
    // This kind quotes the bad escape sequence of the file.
    let message = match err.kind() {
        ParseErrorKind::String(
            jsonc_parser::ParseStringErrorKind::InvalidUnicodeEscapeSequence(_),
        ) => "Invalid unicode escape sequence".to_owned(),
        kind => redact_message(&kind.to_string()),
    };
    format!(
        "{message} at line {}, column {}",
        err.line_display(),
        err.column_display()
    )
}

/// The text of a `serde_json` error, with a position when one exists and no
/// value.
pub fn describe_json(err: &serde_json::Error) -> String {
    let full = err.to_string();
    // `serde_json` appends " at line L column C" to the message itself.
    let message = full
        .rsplit_once(" at line ")
        .map_or(full.as_str(), |(head, _)| head);
    let message = redact_message(message);
    if err.line() == 0 {
        message
    } else {
        format!("{message} at line {}, column {}", err.line(), err.column())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_field_names_and_expected_types() {
        assert_eq!(
            redact_message("unknown field `reason`, expected `files` or `rules`"),
            "unknown field `reason`, expected `files` or `rules`"
        );
        assert_eq!(
            redact_message("invalid type: string \"marker-7531\", expected a boolean"),
            "invalid type: string, expected a boolean"
        );
        assert_eq!(
            redact_message("invalid value: integer `42`, expected a string"),
            "invalid value: integer, expected a string"
        );
        assert_eq!(
            redact_message("unknown variant `marker-7531`, expected one of `a`, `b`"),
            "unknown variant, expected one of `a`, `b`"
        );
    }

    #[test]
    fn drops_quoted_values_from_unmarked_messages() {
        let text = redact_message("bad glob \"marker-7531\": unclosed brace\nline two marker-7531");
        assert!(!text.contains("marker-7531"), "{text}");
        assert!(text.starts_with("bad glob"), "{text}");
    }

    #[test]
    fn keeps_messages_of_fallow_deserializers() {
        let text = "keepExportTags entry '-internal' is a tag name without a `+` or `-` prefix";
        assert_eq!(redact_message(&own_message(text)), text);
        assert!(!redact_message(text).contains("-internal"));
    }

    #[test]
    fn jsonc_escape_errors_omit_the_sequence() {
        let err =
            crate::jsonc::parse_to_value::<serde_json::Value>(r#"["\uD800\u0041"]"#).unwrap_err();
        let text = describe_jsonc(&err);
        assert!(!text.contains("0041"), "{text}");
        assert!(!text.contains("D800"), "{text}");
    }

    #[test]
    fn toml_errors_name_the_position_and_not_the_line() {
        let source = "a = 1\nkey=marker-7531\n";
        let err = crate::toml_input::from_str::<toml::Value>(source).unwrap_err();
        let text = describe_toml(&err, source);
        assert!(!text.contains("marker-7531"), "{text}");
        assert!(text.contains("line 2"), "{text}");
    }

    #[test]
    fn jsonc_and_json_errors_drop_values() {
        #[derive(Debug, serde::Deserialize)]
        struct Shape {
            #[expect(dead_code, reason = "only the error matters")]
            flag: bool,
        }
        let source = "{\n  \"flag\": \"marker-7531\"\n}";
        let err = crate::jsonc::parse_to_value::<Shape>(source).unwrap_err();
        let text = describe_jsonc(&err);
        assert!(!text.contains("marker-7531"), "{text}");
        assert!(text.contains("line 2"), "{text}");
        let err = serde_json::from_str::<Shape>(source).unwrap_err();
        let text = describe_json(&err);
        assert!(!text.contains("marker-7531"), "{text}");
        assert!(text.contains("line 2"), "{text}");
    }

    #[test]
    fn positions_count_characters_from_one() {
        assert_eq!(line_and_column("ab\ncd", 4), (2, 2));
        assert_eq!(line_and_column("ab", 0), (1, 1));
        assert_eq!(line_and_column("é", 1), (1, 1));
    }
}
