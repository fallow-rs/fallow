//! Persisted syntactic Effect facts.

/// Persisted syntactic Effect evidence for one module.
///
/// The layer keys, their `dependencies` and `layer_root_keys` use one
/// format: a plain binding name (`FooLive`) or a class static member
/// (`FooService.layer`). Import aliases resolve to the imported name, and a
/// namespace member (`Ns.FooLive`) resolves to the member name.
#[derive(Debug, Clone, Default, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct EffectFacts {
    /// Module-scope classes that extend a tagged error constructor.
    pub tagged_errors: Vec<EffectTaggedError>,
    /// `catchTag("X")` arguments and `catchTags({ X })` keys.
    pub caught_tags: Vec<EffectCaughtTag>,
    /// Tags written as an object literal `_tag: "X"` property.
    pub literal_tags: Vec<String>,
    /// Layer definitions: module-scope consts and class static properties.
    pub layers: Vec<EffectLayer>,
    /// Keys that a value reference outside every layer definition reads.
    pub layer_root_keys: Vec<String>,
    /// Sorted names of exports that an Effect schema declares.
    pub schema_exports: Vec<String>,
    /// Exports that are schemas only when an imported binding is a schema
    /// (`export const B = Imported.pipe(Schema.brand("B"))`). The analyze
    /// layer resolves the import to decide.
    pub derived_schema_exports: Vec<EffectDerivedSchema>,
}

/// An export whose chain starts at an imported binding.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct EffectDerivedSchema {
    /// The export name.
    pub export_name: String,
    /// The local name of the import binding at the root of the chain.
    pub import_local: String,
}

/// A module-scope class that extends a tagged error constructor.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct EffectTaggedError {
    /// Class binding name.
    pub name: String,
    /// The `_tag` string literal.
    pub tag: String,
    /// Byte offset of the class name.
    pub span_start: u32,
    /// The class is exported from its module.
    pub exported: bool,
    /// The module reads the class as a value outside its own declaration.
    pub local_value_reference: bool,
    /// The module reads the class in a type position outside its own declaration.
    pub local_type_reference: bool,
}

/// One tag that a `catchTag` or `catchTags` handler names.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct EffectCaughtTag {
    /// The handled tag.
    pub tag: String,
    /// Byte offset of the tag string or object key.
    pub span_start: u32,
}

/// A layer definition and the layer keys its initializer reads.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct EffectLayer {
    /// `FooLive` or `FooService.layer`.
    pub key: String,
    /// Byte offset of the binding name or static property key.
    pub span_start: u32,
    /// The const, or the class of a static property, is exported.
    pub exported: bool,
    /// Keys that value references inside the initializer read.
    pub dependencies: Vec<String>,
}
