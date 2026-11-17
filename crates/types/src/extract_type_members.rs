//! Persisted property types of named types, for the typed property join of
//! the member pass.

/// A named-type property whose declared type is a named type reference.
///
/// `interface Opts { c: OptDep }` (or `type Opts = { c: OptDep }`) records
/// `TypeMemberTypeEntry { type_name: "Opts", property: "c", property_type: "OptDep" }`.
/// Both `type_name` and `property_type` are the DECLARING module's own local
/// names; resolution through that module's imports/exports is deferred to
/// analyze time, mirroring `FactoryReturnExport.class_local_name`. Consumed by
/// the `unused-class-member` typed-property-hop join so a consumer's
/// `this.opts.c.optM()` credits `OptDep.optM` across module boundaries.
/// See issue #1785.
#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
    PartialEq,
    Eq,
)]
pub struct TypeMemberTypeEntry {
    /// Local interface or type-alias name declaring the property.
    pub type_name: String,
    /// Property name declared on the type.
    pub property: String,
    /// The property's declared type name (local to the declaring module).
    /// A type import such as `import("./room").Room` uses the form of
    /// [`import_type_reference`].
    pub property_type: String,
}

const IMPORT_TYPE_PREFIX: &str = "import(\"";
const IMPORT_TYPE_SEPARATOR: &str = "\").";

/// The `property_type` of a [`TypeMemberTypeEntry`] for the type import
/// `import("<source>").<name>`.
#[must_use]
pub fn import_type_reference(source: &str, name: &str) -> String {
    format!("{IMPORT_TYPE_PREFIX}{source}{IMPORT_TYPE_SEPARATOR}{name}")
}

/// The source and the export name of a type import that
/// [`import_type_reference`] makes. `None` for a plain type name.
#[must_use]
pub fn parse_import_type_reference(reference: &str) -> Option<(&str, &str)> {
    reference
        .strip_prefix(IMPORT_TYPE_PREFIX)?
        .rsplit_once(IMPORT_TYPE_SEPARATOR)
        .filter(|(source, name)| !source.is_empty() && !name.is_empty() && !name.contains('.'))
}
