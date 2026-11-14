//! Namespace member uses that the semantic pass finds, and the keys of the
//! uses that have no local binding.

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
    /// The `object` of a use through the inline import type
    /// `import("./x").N.a`. An identifier cannot contain `(`, so the key
    /// never matches a local binding.
    #[must_use]
    pub fn import_type_object(source: &str) -> String {
        format!("import({source})")
    }
}
