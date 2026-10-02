//! Read `&'static str` report fields back from a saved envelope.
//!
//! A live run fills these fields from string constants. A saved envelope holds
//! the same text as owned JSON strings. `fallow report --from` reads one
//! envelope, renders it once and exits, so the text is leaked into a static
//! string instead of a change of the field type on every live constructor.

use serde::{Deserialize, Deserializer};

/// A `&'static str` field that a saved envelope reads back through
/// [`deserialize`] or [`deserialize_option`].
///
/// serde borrows a field spelled `&str` from the input, which needs the input
/// to live for `'static`. The alias hides that spelling from the derive, so
/// the `deserialize_with` function owns the conversion.
pub type StaticStr = &'static str;

/// Deserialize a required `&'static str` field.
pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<&'static str, D::Error> {
    String::deserialize(deserializer)
        .map(String::leak)
        .map(|text| &*text)
}

/// Deserialize an optional `&'static str` field.
pub fn deserialize_option<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<&'static str>, D::Error> {
    Option::<String>::deserialize(deserializer)
        .map(|value| value.map(String::leak).map(|text| &*text))
}
