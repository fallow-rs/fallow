//! YAML parsing shared by every surface that reads user-authored YAML
//! (`pnpm-workspace.yaml`, `pnpm-lock.yaml`, prettier config).
//!
//! Callers see only the types of this module. The parser crate stays an
//! implementation detail, so a change of parser touches this file only.

use std::fmt;

use deser_core::{Context, TrackLocations};
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

/// Parse `source` as a single YAML document and keep the source position
/// of each node.
///
/// [`YamlNode::byte_range`] gives a range only for a document that this
/// function made. The document holds a copy of `source`, so use this
/// function for small files only.
///
/// # Errors
///
/// Returns an error in the same cases as [`parse`].
pub fn parse_with_locations(source: &str) -> Result<YamlDocument, YamlError> {
    let config = deser_yaml::DeserializerConfig::builder()
        .context(Context::with(TrackLocations(true)))
        .build();
    config
        .from_str::<Value>(source)
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

    /// The byte range of the node in the source. `None` when the document
    /// does not come from [`parse_with_locations`].
    #[must_use]
    pub fn byte_range(self) -> Option<std::ops::Range<usize>> {
        self.0.span().map(|span| span.range())
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
}

impl PartialEq for YamlNode<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
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
    use super::{YamlNode, parse, parse_documents, parse_with_locations};

    fn text<'s>(source: &'s str, node: YamlNode<'_>) -> &'s str {
        &source[node.byte_range().expect("node has a range")]
    }

    #[test]
    fn nodes_compare_by_value() {
        let first = parse("a: 1\nb: [x, y]\n").unwrap();
        let second = parse("a: 1\nb: [x, y]\n").unwrap();
        let other = parse("a: 2\nb: [x, y]\n").unwrap();
        assert_eq!(first.root(), second.root());
        assert_ne!(first.root(), other.root());
    }

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
    fn deep_nesting_parses_and_drops_without_recursion() {
        const DEPTH: usize = 1_000_000;
        let source = format!("a: {}{}\n", "[".repeat(DEPTH), "]".repeat(DEPTH));
        let document = parse(&source).unwrap();
        assert!(document.root().get("a").unwrap().as_sequence().is_some());
    }

    #[test]
    fn plain_parse_gives_no_byte_range() {
        let document = parse("a: 1\n").unwrap();
        assert_eq!(document.root().byte_range(), None);
        assert_eq!(document.root().get("a").unwrap().byte_range(), None);
    }

    #[test]
    fn block_keys_and_values_have_byte_ranges() {
        let source = "catalog:\n  react: ^18.2.0\n  \"@scope/lib\": '1'\n";
        let document = parse_with_locations(source).unwrap();
        let catalog = document.root().get("catalog").unwrap();
        let entries: Vec<_> = catalog.as_mapping().unwrap().iter().collect();
        let keys: Vec<_> = entries.iter().map(|(key, _)| text(source, *key)).collect();
        let values: Vec<_> = entries
            .iter()
            .map(|(_, value)| text(source, *value))
            .collect();
        assert_eq!(keys, ["react", "\"@scope/lib\""]);
        assert_eq!(values, ["^18.2.0", "'1'"]);
        assert!(text(source, catalog).starts_with("react:"));
    }

    #[test]
    fn flow_mapping_range_starts_at_the_brace() {
        let source = "catalogs:\n  ui: { react: \"18\", vue: \"3\" }\n";
        let document = parse_with_locations(source).unwrap();
        let ui = document.root().get("catalogs").unwrap().get("ui").unwrap();
        assert_eq!(text(source, ui), "{ react: \"18\", vue: \"3\" }");
        let keys: Vec<_> = ui
            .as_mapping()
            .unwrap()
            .keys()
            .map(|key| text(source, key))
            .collect();
        assert_eq!(keys, ["react", "vue"]);
    }

    #[test]
    fn flow_sequence_items_have_byte_ranges() {
        let source = "packages: [packages/*, 'apps/*']\n";
        let document = parse_with_locations(source).unwrap();
        let items: Vec<_> = document
            .root()
            .get("packages")
            .and_then(YamlNode::as_sequence)
            .unwrap()
            .map(|item| text(source, item))
            .collect();
        assert_eq!(items, ["packages/*", "'apps/*'"]);
    }

    #[test]
    fn key_before_a_null_value_has_a_byte_range() {
        let source = "catalogs:\n  a: {}\n  legacy:\n";
        let document = parse_with_locations(source).unwrap();
        let entries: Vec<_> = document
            .root()
            .get("catalogs")
            .and_then(YamlNode::as_mapping)
            .unwrap()
            .iter()
            .collect();
        let (key, value) = entries[1];
        assert_eq!(text(source, key), "legacy");
        assert!(value.is_null());
    }

    #[test]
    fn located_parse_reads_the_same_values_as_parse() {
        let source = "a: 1.10\nb: [x, y]\nc:\n  d: ~\n";
        for document in [
            parse(source).unwrap(),
            parse_with_locations(source).unwrap(),
        ] {
            let root = document.root();
            assert_eq!(root.get("a").and_then(|n| n.scalar_text()), Some("1.10"));
            let items: Vec<_> = root
                .get("b")
                .and_then(YamlNode::as_sequence)
                .unwrap()
                .filter_map(YamlNode::as_str)
                .collect();
            assert_eq!(items, ["x", "y"]);
            assert!(root.get("c").and_then(|c| c.get("d")).unwrap().is_null());
        }
    }

    #[test]
    fn located_parse_rejects_what_parse_rejects() {
        assert!(parse_with_locations("a: [1, 2\n").is_err());
        assert!(parse_with_locations("a: 1\na: 2\n").is_err());
        assert!(parse_with_locations("---\na: 1\n---\nb: 2\n").is_err());
        assert!(parse_with_locations("").unwrap().root().is_null());
    }

    #[test]
    fn byte_ranges_are_byte_offsets_after_non_ascii_text() {
        let source = "a: äöü\nb: x\n";
        let document = parse_with_locations(source).unwrap();
        let mapping = document.root().as_mapping().unwrap();
        let (key, value) = mapping.iter().nth(1).unwrap();
        assert_eq!(text(source, value), "x");
        assert_eq!(text(source, key), "b");
    }

    #[test]
    fn located_parse_handles_deep_flow_nesting() {
        const DEPTH: usize = 1_000_000;
        let source = format!("a: {}{}\n", "[".repeat(DEPTH), "]".repeat(DEPTH));
        let document = parse_with_locations(&source).unwrap();
        assert!(document.root().get("a").unwrap().as_sequence().is_some());
        assert!(document.root().get("a").unwrap().byte_range().is_some());
    }

    #[test]
    fn located_parse_handles_deep_block_nesting() {
        use std::fmt::Write as _;
        const DEPTH: usize = 2_000;
        let mut yaml = String::from("overrides:\n  deep:\n");
        for level in 0..DEPTH {
            let indent = " ".repeat(level + 4);
            write!(yaml, "{indent}? x\n{indent}:\n").expect("writing to a String cannot fail");
        }
        let document = parse_with_locations(&yaml).unwrap();
        assert!(
            document
                .root()
                .get("overrides")
                .and_then(|node| node.get("deep"))
                .is_some()
        );
    }
}
