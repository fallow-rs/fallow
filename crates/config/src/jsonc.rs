//! JSONC parsing helpers shared by every surface that reads user-authored
//! JSONC (config files, external plugin definitions, rule packs), so they all
//! accept exactly the same dialect.

use serde::de::DeserializeOwned;

/// The JSONC dialect fallow accepts: comments and trailing commas on top of
/// strict JSON. Loose extensions (unquoted keys, single quotes, hex numbers,
/// unary plus, missing commas) stay rejected so files remain portable to other
/// JSONC tooling.
pub fn parse_options() -> jsonc_parser::ParseOptions {
    jsonc_parser::ParseOptions {
        allow_comments: true,
        allow_loose_object_property_names: false,
        allow_trailing_commas: true,
        allow_missing_commas: false,
        allow_single_quoted_strings: false,
        allow_hexadecimal_numbers: false,
        allow_unary_plus_numbers: false,
    }
}

/// Parse JSONC `content` and deserialize it into `T` using [`parse_options`].
///
/// # Errors
///
/// Returns the parser's error when `content` is not valid JSONC under
/// [`parse_options`] or does not deserialize into `T`.
pub fn parse_to_value<T: DeserializeOwned>(
    content: &str,
) -> Result<T, jsonc_parser::errors::ParseError> {
    jsonc_parser::parse_to_serde_value(content, &parse_options())
}

#[cfg(test)]
mod tests {
    use super::parse_to_value;
    use serde_json::{Value, json};

    #[test]
    fn trailing_commas_preserve_json_values_and_string_contents() {
        let cases = [
            ("object", r#"{"a": 1, "b": 2,}"#, json!({"a": 1, "b": 2})),
            ("array", "[1, 2, 3,]", json!([1, 2, 3])),
            ("whitespace", "{\n  \"a\": 1,\n}", json!({"a": 1})),
            (
                "string comma",
                r#"{"a": "hello,}"}"#,
                json!({"a": "hello,}"}),
            ),
            (
                "nested references",
                r#"{"refs": [{"path": "./a",}, {"path": "./b",},],}"#,
                json!({"refs": [{"path": "./a"}, {"path": "./b"}]}),
            ),
            (
                "escaped quote",
                r#"{"a": "he\"llo,}",}"#,
                json!({"a": "he\"llo,}"}),
            ),
            (
                "without trailing commas",
                r#"{"a": 1, "b": [2, 3]}"#,
                json!({"a": 1, "b": [2, 3]}),
            ),
            ("empty", "", Value::Null),
            (
                "nested objects",
                "{\n  \"a\": {\n    \"b\": 1,\n    \"c\": 2,\n  },\n  \"d\": 3,\n}",
                json!({"a": {"b": 1, "c": 2}, "d": 3}),
            ),
            (
                "array of objects",
                r#"[{"a": 1,}, {"b": 2,},]"#,
                json!([{"a": 1}, {"b": 2}]),
            ),
            (
                "brackets inside string",
                r#"{"key": "value with ] and }",}"#,
                json!({"key": "value with ] and }"}),
            ),
            (
                "multiple levels",
                r#"{"a": {"b": [1, 2,], "c": 3,},}"#,
                json!({"a": {"b": [1, 2], "c": 3}}),
            ),
        ];

        for (case, input, expected) in cases {
            let actual: Value =
                parse_to_value(input).unwrap_or_else(|error| panic!("{case}: {error}"));
            assert_eq!(actual, expected, "{case}");
        }
    }
}
