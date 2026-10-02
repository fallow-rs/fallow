//! Read `&'static str` report fields back from a saved envelope.
//!
//! A live run fills these fields from string constants. A saved envelope holds
//! the same text as owned JSON strings. `fallow report --from` reads one
//! envelope, renders it once and exits, so the text is leaked into a static
//! string instead of a change of the field type on every live constructor.
//! Each distinct text is interned, so a process that reads many envelopes
//! leaks every distinct value once, not once for each read.

use std::sync::{Mutex, OnceLock};

use rustc_hash::FxHashSet;
use serde::{Deserialize, Deserializer};

/// Return the interned `&'static str` for `text`, leaking it on first use.
fn intern(text: String) -> &'static str {
    static INTERNED: OnceLock<Mutex<FxHashSet<&'static str>>> = OnceLock::new();
    let mut interned = INTERNED
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = interned.get(text.as_str()) {
        return existing;
    }
    let leaked: &'static str = String::leak(text);
    interned.insert(leaked);
    leaked
}

/// A `&'static str` field that a saved envelope reads back through
/// [`deserialize`] or [`deserialize_option`].
///
/// serde borrows a field spelled `&str` from the input, which needs the input
/// to live for `'static`. The alias hides that spelling from the derive, so
/// the `deserialize_with` function owns the conversion.
pub type StaticStr = &'static str;

/// Deserialize a required `&'static str` field.
pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<&'static str, D::Error> {
    String::deserialize(deserializer).map(intern)
}

/// Deserialize an optional `&'static str` field.
pub fn deserialize_option<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<&'static str>, D::Error> {
    Option::<String>::deserialize(deserializer).map(|value| value.map(intern))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize)]
    struct Field {
        #[serde(deserialize_with = "deserialize")]
        text: StaticStr,
    }

    #[test]
    fn the_same_text_is_leaked_once() {
        let first: Field =
            serde_json::from_str(r#"{"text":"interned-value"}"#).expect("valid field");
        let second: Field =
            serde_json::from_str(r#"{"text":"interned-value"}"#).expect("valid field");
        assert_eq!(first.text, "interned-value");
        assert!(std::ptr::eq(first.text, second.text));
    }
}
