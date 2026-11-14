//! Literal and option metadata that the extractor attaches to a sink site.

/// Literal values attached to literal-aware security sink captures.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
pub enum SinkLiteralValue {
    /// A string literal value.
    String(String),
    /// An integer numeric literal value.
    Integer(i64),
    /// A boolean literal value.
    Boolean(bool),
    /// A null literal value.
    Null,
}

/// Static object-literal property metadata attached to a captured sink
/// argument. Nested object paths are flattened with dot-separated keys.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
pub struct SinkObjectProperty {
    /// Static property name. Nested object properties use dot-separated paths.
    pub key: String,
    /// Literal property value when statically knowable.
    pub value: SinkLiteralValue,
}

/// Static metadata of the options object of a call whose options position the
/// extractor knows, attached to the command argument of the call (for example
/// `child_process.spawn(command, args, options)`). A call without an options
/// argument has empty, complete metadata.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
pub struct SinkCallOptions {
    /// Static literal properties of the options object.
    pub properties: Vec<SinkObjectProperty>,
    /// Static top-level keys of the options object, including keys whose
    /// values are not literal.
    pub keys: Vec<String>,
    /// Whether [`keys`](Self::keys) is complete. False when the options
    /// argument is not an object literal, or has a spread or a computed key.
    pub keys_complete: bool,
}
