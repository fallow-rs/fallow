use serde::de::DeserializeOwned;

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

pub fn parse_to_value<T: DeserializeOwned>(
    content: &str,
) -> Result<T, jsonc_parser::errors::ParseError> {
    jsonc_parser::parse_to_serde_value(content, &parse_options())
}
