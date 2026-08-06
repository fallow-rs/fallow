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
