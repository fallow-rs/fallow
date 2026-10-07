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
        allow_bare_decimal_point_numbers: false,
        allow_extended_string_escapes: false,
        allow_non_finite_numbers: false,
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

/// Parse a Deno config (`deno.json` / `deno.jsonc`) and deserialize it into `T`.
///
/// Deno reads its own config with the fully loose JSONC dialect: single
/// quotes, unquoted keys, missing commas, hexadecimal numbers and unary plus
/// all load. This function matches Deno's own reader, so fallow accepts every
/// config that Deno runs. Use [`parse_to_value`] for every other file.
///
/// # Errors
///
/// Returns the parser's error when `content` is not valid loose JSONC or does
/// not deserialize into `T`.
pub fn parse_deno_to_value<T: DeserializeOwned>(
    content: &str,
) -> Result<T, jsonc_parser::errors::ParseError> {
    jsonc_parser::parse_to_serde_value(content, &jsonc_parser::ParseOptions::default())
}

#[cfg(test)]
mod tests {
    use super::{parse_deno_to_value, parse_to_value};
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

    #[test]
    fn loose_extensions_stay_rejected() {
        // The other half of the dialect contract in this module's doc: these
        // shapes are what the five `false` flags in `parse_options` buy, and
        // flipping any of them must fail a test rather than pass silently.
        let cases = [
            ("unquoted key", "{a: 1}"),
            ("single-quoted string", r#"{"a": 'x'}"#),
            ("hexadecimal number", r#"{"a": 0x1F}"#),
            ("unary plus number", r#"{"a": +1}"#),
            ("missing comma", r#"{"a": 1 "b": 2}"#),
        ];

        for (case, input) in cases {
            assert!(
                parse_to_value::<Value>(input).is_err(),
                "{case}: `{input}` must stay rejected so files stay portable"
            );
        }
    }

    #[test]
    fn deno_dialect_accepts_what_deno_runs() {
        let cases = [
            (
                "single-quoted strings",
                r"{ 'imports': { '@/': './src/' } }",
            ),
            ("unquoted keys", r#"{ imports: { "@/": "./src/" } }"#),
            (
                "missing comma",
                r#"{ "imports": { "@/": "./src/" } "tasks": {} }"#,
            ),
        ];

        for (case, input) in cases {
            let actual: Value =
                parse_deno_to_value(input).unwrap_or_else(|error| panic!("{case}: {error}"));
            assert_eq!(actual["imports"], json!({"@/": "./src/"}), "{case}");
        }
    }

    #[test]
    fn deno_dialect_reads_strict_configs_like_the_strict_dialect() {
        let input = "{\n  // comment\n  \"imports\": { \"@/\": \"./src/\", },\n  \"workspace\": [\"a\"],\n}";
        let strict: Value = parse_to_value(input).expect("strict dialect parses");
        let deno: Value = parse_deno_to_value(input).expect("deno dialect parses");
        assert_eq!(deno, strict);
    }
}
