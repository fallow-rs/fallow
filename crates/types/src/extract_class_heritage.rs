//! Heritage metadata of an exported class.

/// Additional heritage metadata for an exported class.
#[derive(
    Debug,
    Default,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    bitcode::Encode,
    bitcode::Decode,
    PartialEq,
    Eq,
)]
pub struct ClassHeritageInfo {
    /// Export name (`default` for default-exported classes).
    pub export_name: String,
    /// Parent class name from the `extends` clause, if any.
    pub super_class: Option<String>,
    /// Interface names from the class `implements` clause.
    pub implements: Vec<String>,
    /// Ordered class type-parameter names for multi-hop type arguments.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub type_parameters: Vec<String>,
    /// Typed instance bindings used to resolve member-access chains in external templates.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub instance_bindings: Vec<(String, String)>,
    /// Positional type arguments on the `extends` clause, such as
    /// `<DerivedClient>`. An empty string marks an argument that is not a
    /// plain type reference (issue #1910).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub super_class_type_args: Vec<String>,
    /// Instance-binding fields typed as exactly a class type parameter, as
    /// `(field_name, type_param_index)` (issue #1910).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generic_instance_bindings: Vec<(String, usize)>,
    /// The outer `extends` target and its type arguments after the fold of
    /// unexported local bases (issue #4199). The walk for inherited bindings
    /// reads it. `super_class` keeps the declared name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_super_class: Option<(String, Vec<String>)>,
    /// The first `extends` target past the base classes that the file
    /// declares but does not export, at any depth. Unlike
    /// `binding_super_class`, the walk also passes generic bases, because it
    /// reads only the names. `None` when `super_class` is not such a base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extends_root: Option<String>,
}
