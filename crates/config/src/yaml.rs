//! YAML parsing shared by every surface that reads user-authored YAML
//! (`pnpm-workspace.yaml`, `pnpm-lock.yaml`, prettier config).
//!
//! Callers see only the types of this module. The parser crate stays an
//! implementation detail, so a change of parser touches this file only.

use std::fmt;

use deser_value::{Kind, Map, Value};

/// A YAML parse error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct YamlError(String);

impl fmt::Display for YamlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for YamlError {}

/// One parsed YAML document.
#[derive(Debug)]
pub struct YamlDocument(Value);

impl YamlDocument {
    /// The root node of the document.
    #[must_use]
    pub fn root(&self) -> YamlNode<'_> {
        YamlNode(&self.0)
    }
}

/// Parse `source` as a single YAML document.
///
/// An empty stream (for example an empty file) parses as a null document.
///
/// # Errors
///
/// Returns an error when `source` is not valid YAML, holds more than one
/// document, or has a duplicate mapping key.
pub fn parse(source: &str) -> Result<YamlDocument, YamlError> {
    deser_yaml::from_str::<Value>(source)
        .map(YamlDocument)
        .map_err(|error| YamlError(error.to_string()))
}

/// Parse every document of the YAML stream in `source`.
///
/// An empty stream returns no documents.
///
/// # Errors
///
/// Returns an error when a document is not valid YAML or has a duplicate
/// mapping key.
pub fn parse_documents(source: &str) -> Result<Vec<YamlDocument>, YamlError> {
    let mut deserializer = deser_yaml::Deserializer::from_str(source);
    deserializer
        .iter::<Value>()
        .map(|document| {
            document
                .map(YamlDocument)
                .map_err(|error| YamlError(error.to_string()))
        })
        .collect()
}

/// A borrowed node of a parsed YAML document.
#[derive(Debug, Clone, Copy)]
pub struct YamlNode<'a>(&'a Value);

impl<'a> YamlNode<'a> {
    /// Returns `true` for `null`, `~` and an empty value.
    #[must_use]
    pub fn is_null(self) -> bool {
        self.0.is_null()
    }

    /// The value of a boolean node.
    #[must_use]
    pub fn as_bool(self) -> Option<bool> {
        self.0.as_bool()
    }

    /// The value of a string node. Plain scalars that resolve to another type
    /// (`42`, `1.10`, `true`, `~`) are not strings and return `None`.
    #[must_use]
    pub fn as_str(self) -> Option<&'a str> {
        self.0.as_str()
    }

    /// The source text of a scalar node other than null.
    ///
    /// Strings return their value. Plain numbers and booleans return the text
    /// as written, so `1.10` stays `1.10` and does not become `1.1`.
    #[must_use]
    pub fn scalar_text(self) -> Option<&'a str> {
        match self.0.kind() {
            Kind::Implicit(implicit) if !self.0.is_null() => Some(implicit.text().as_str()),
            _ => self.as_str(),
        }
    }

    /// The node as a mapping.
    #[must_use]
    pub fn as_mapping(self) -> Option<YamlMapping<'a>> {
        self.0.as_map().map(YamlMapping)
    }

    /// The items of a sequence node.
    #[must_use]
    pub fn as_sequence(self) -> Option<impl Iterator<Item = YamlNode<'a>>> {
        self.0.as_seq().map(|seq| seq.iter().map(YamlNode))
    }

    /// The value of `key` when the node is a mapping.
    #[must_use]
    pub fn get(self, key: &str) -> Option<YamlNode<'a>> {
        self.as_mapping().and_then(|mapping| mapping.get(key))
    }

    /// The node written as YAML text, for values that have no scalar text.
    #[must_use]
    pub fn to_yaml_string(self) -> String {
        deser_yaml::to_string(self.0).unwrap_or_default()
    }
}

/// A borrowed YAML mapping, in source order.
#[derive(Debug, Clone, Copy)]
pub struct YamlMapping<'a>(&'a Map);

impl<'a> YamlMapping<'a> {
    /// The number of entries.
    #[must_use]
    pub fn len(self) -> usize {
        self.0.len()
    }

    /// Returns `true` when the mapping has no entries.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.0.is_empty()
    }

    /// The value of the string key `key`.
    #[must_use]
    pub fn get(self, key: &str) -> Option<YamlNode<'a>> {
        self.0.get(key).map(YamlNode)
    }

    /// Returns `true` when the mapping has the string key `key`.
    #[must_use]
    pub fn contains_key(self, key: &str) -> bool {
        self.0.contains_key(key)
    }

    /// The entries, in source order.
    pub fn iter(self) -> impl Iterator<Item = (YamlNode<'a>, YamlNode<'a>)> {
        self.0
            .iter()
            .map(|(key, value)| (YamlNode(key), YamlNode(value)))
    }

    /// The keys, in source order.
    pub fn keys(self) -> impl Iterator<Item = YamlNode<'a>> {
        self.0.keys().map(YamlNode)
    }

    /// The string keys, in source order. Keys that are not strings are
    /// skipped.
    pub fn str_keys(self) -> impl Iterator<Item = &'a str> {
        self.0.keys().filter_map(|key| key.as_str())
    }

    /// The values, in source order.
    pub fn values(self) -> impl Iterator<Item = YamlNode<'a>> {
        self.0.values().map(YamlNode)
    }
}

#[cfg(test)]
mod tests {
    use super::{parse, parse_documents};

    #[test]
    fn reads_nested_mappings_and_sequences() {
        let document = parse("catalog:\n  react: ^18.2.0\nplugins:\n  - a\n  - b\n").unwrap();
        let root = document.root();
        let react = root.get("catalog").and_then(|catalog| catalog.get("react"));
        assert_eq!(react.and_then(|node| node.as_str()), Some("^18.2.0"));
        let plugins: Vec<_> = root
            .get("plugins")
            .and_then(|node| node.as_sequence())
            .unwrap()
            .filter_map(|node| node.as_str())
            .collect();
        assert_eq!(plugins, ["a", "b"]);
    }

    #[test]
    fn keeps_the_source_text_of_plain_scalars() {
        let document = parse("a: 1.10\nb: 0755\nc: true\nd: '1.10'\ne: ~\n").unwrap();
        let root = document.root();
        let text = |key: &str| root.get(key).and_then(|node| node.scalar_text());
        assert_eq!(text("a"), Some("1.10"));
        assert_eq!(text("b"), Some("0755"));
        assert_eq!(text("c"), Some("true"));
        assert_eq!(text("d"), Some("1.10"));
        assert_eq!(text("e"), None);
        assert!(root.get("e").unwrap().is_null());
        assert_eq!(root.get("a").and_then(|node| node.as_str()), None);
    }

    #[test]
    fn keeps_mapping_order_and_skips_non_string_keys() {
        let document = parse("b: 1\n1: x\na: 2\n").unwrap();
        let mapping = document.root().as_mapping().unwrap();
        assert_eq!(mapping.len(), 3);
        assert_eq!(mapping.str_keys().collect::<Vec<_>>(), ["b", "a"]);
    }

    #[test]
    fn rejects_invalid_yaml_and_duplicate_keys() {
        assert!(parse("a: [1, 2\n").is_err());
        assert!(parse("a:\n  b: 1\n c: 2\n").is_err());
        assert!(parse("a: 1\na: 2\n").is_err());
    }

    #[test]
    fn empty_input_is_one_null_document_or_no_documents() {
        assert!(parse("").unwrap().root().is_null());
        assert!(parse("# only a comment\n").unwrap().root().is_null());
        assert!(parse_documents("").unwrap().is_empty());
    }

    #[test]
    fn reads_every_document_of_a_stream() {
        let documents = parse_documents("---\na: 1\n---\nb: 2\n").unwrap();
        assert_eq!(documents.len(), 2);
        assert!(documents[1].root().get("b").is_some());
        assert!(parse("---\na: 1\n---\nb: 2\n").is_err());
    }

    #[test]
    fn writes_collections_as_yaml() {
        let document = parse("a:\n  - x\n  - y\n").unwrap();
        let text = document.root().get("a").unwrap().to_yaml_string();
        assert!(text.contains("- x"), "{text}");
    }

    #[test]
    fn deep_nesting_parses_writes_and_drops_without_recursion() {
        const DEPTH: usize = 1_000_000;
        let source = format!("a: {}{}\n", "[".repeat(DEPTH), "]".repeat(DEPTH));
        let document = parse(&source).unwrap();
        let text = document.root().get("a").unwrap().to_yaml_string();
        assert!(text.starts_with('['), "{}", &text[..text.len().min(20)]);
    }
}
