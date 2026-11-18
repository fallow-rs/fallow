//! Namespace member uses that the semantic pass finds, the keys of the uses
//! that have no local binding, and the values of exported object holders.

/// A use of a namespace member, found by the semantic pass.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NamespaceMemberUseFact {
    /// The local name of the root binding: an import binding, or a namespace
    /// that the module declares, or the key of an inline import type.
    pub object: String,
    /// The dotted member path after the root. Empty for a bare root.
    pub path: String,
    /// The code hands the value at `path` on (an argument, an exported
    /// alias, a spread). Every member below `path` then counts as used.
    pub whole: bool,
    /// The value goes into a class field (`levels = Level`). The member
    /// visitor follows the reads through `this.levels` and the component
    /// template, so an enum counts only those reads as used. A namespace
    /// still counts the field as an escape.
    pub class_field: bool,
}

impl NamespaceMemberUseFact {
    /// The path segment of a holder read with a computed key that is not a
    /// string literal: `holder[name].prototype.member` gives the path
    /// `[].prototype.member`. Such a key can select each value of the
    /// holder. An identifier cannot contain `[`, so no identifier key has
    /// this name. A string-literal key `'[]'` gives the same path, so a read
    /// through it selects each value too.
    pub const ANY_KEY: &'static str = "[]";

    /// The `object` of a use through the inline import type
    /// `import("./x").N.a`. An identifier cannot contain `(`, so the key
    /// never matches a local binding.
    #[must_use]
    pub fn import_type_object(source: &str) -> String {
        format!("import({source})")
    }
}

/// A value under a static key of an exported object literal:
/// `export default { websocket: WebSocket }` or
/// `export const transports = { websocket: WebSocket }`.
///
/// Another module can import the holder and read
/// `holder[name].prototype.member`. The member pass follows this fact one
/// hop to the value, so the read credits the instance member of the class.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HolderValueFact {
    /// The export name of the holder: `default`, or the name of the
    /// exported `const`.
    pub export_name: String,
    /// The static key of the property.
    pub key: String,
    /// The local name of the root binding of the value: an import binding,
    /// or a binding that the module declares.
    pub object: String,
    /// The dotted member path of the value after the root. Empty for a bare
    /// root.
    pub path: String,
}
