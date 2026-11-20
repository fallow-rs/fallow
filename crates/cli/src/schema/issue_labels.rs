//! The sentence-case display label of an issue type in the CLI reference.

use serde_json::Value;

/// Capitalized words that are proper names, so sentence case keeps them.
/// Acronyms and words with an inner capital (`CSS`, `ReDoS`, `MySQL`) are
/// kept without an entry here.
pub(super) const PROPER_NAMES: &[&str] = &[
    "Angular", "Buffer", "Effect", "Electron", "Next.js", "Svelte",
];

/// The display label of an issue type row: the summary label of the human
/// output when the row has one, else its registry label, else the name of
/// its explanation, in sentence case.
pub(super) fn display_label(row: &Value) -> Value {
    ["summary_label", "label"]
        .iter()
        .find_map(|field| row[*field].as_str())
        .or_else(|| row["explanation"]["name"].as_str())
        .map_or(Value::Null, |text| Value::from(sentence_case(text)))
}

/// Lower-case every word after the first, except acronyms, words with an
/// inner capital and [`PROPER_NAMES`]. The parts of a hyphenated word follow
/// the same rule.
fn sentence_case(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut first = true;
    for (index, word) in text.split(' ').enumerate() {
        if index > 0 {
            out.push(' ');
        }
        for (part_index, part) in word.split('-').enumerate() {
            if part_index > 0 {
                out.push('-');
            }
            if first || keeps_case(part) {
                out.push_str(part);
            } else {
                out.push_str(&part.to_lowercase());
            }
            first = false;
        }
    }
    out
}

fn keeps_case(word: &str) -> bool {
    let letters = word.trim_start_matches(|c: char| !c.is_alphanumeric());
    PROPER_NAMES.contains(&letters) || letters.chars().skip(1).any(char::is_uppercase)
}

#[cfg(test)]
mod tests {
    use super::sentence_case;

    #[test]
    fn sentence_case_keeps_acronyms_and_proper_names() {
        assert_eq!(sentence_case("High CRAP Score"), "High CRAP score");
        assert_eq!(sentence_case("Test-Only Branches"), "Test-only branches");
        assert_eq!(
            sentence_case("High Complexity (Both)"),
            "High complexity (both)"
        );
        assert_eq!(
            sentence_case("Unsafe Electron BrowserWindow preferences"),
            "Unsafe Electron BrowserWindow preferences"
        );
        assert_eq!(sentence_case("ReDoS regex sink"), "ReDoS regex sink");
    }
}
