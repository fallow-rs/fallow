//! Persisted facts of free-function factories: the class that an exported
//! factory returns, and the member reads on the result of an imported one.

/// An exported free-function factory proven to return one class instance, or
/// one that declares an array of a class as its return type.
///
/// `export function useApi() { return new RESTApi() }` records
/// `FactoryReturnExport { export_name: "useApi", class_local_name: "RESTApi" }`.
/// The `class_local_name` is the factory module's own LOCAL name, resolved at
/// analyze time through that module's imports/exports to the real class export,
/// so a cross-module `const x = useApi(); x.member` consumer credits the class
/// across the boundary. See issue #1441 (Part A).
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
pub struct FactoryReturnExport {
    /// Public export name (honors `export { useApi as useRestApi }`).
    pub export_name: String,
    /// The returned class's local name within the factory module.
    pub class_local_name: String,
    /// The factory returns a promise of the class (an `async` function, or a
    /// `Promise<Class>` return type). Only an awaited call result is the class.
    #[serde(default)]
    pub is_async: bool,
    /// The factory declares an array return type (`make(): Sheet[]`), so an
    /// element of the call result is the class, not the result itself.
    #[serde(default)]
    pub returns_array: bool,
}

/// A member access on a value returned by an imported free-function factory.
///
/// `const x = importedFactory(); x.member` emits one fact per first-level read
/// on `x`. The analyze layer resolves `callee_name` through the consumer's
/// imports to the factory's origin module, reads that module's
/// `exported_factory_returns` to learn the returned class's local name, resolves
/// THAT through the factory module's own imports to the class export, and
/// credits `member` on the class. See issue #1441 (Part A).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FactoryFnMemberAccessFact {
    /// Local imported function used as the factory callee.
    pub callee_name: String,
    /// Member accessed on the returned instance-like object.
    pub member: String,
    /// The local holds the awaited call result (`const x = await factory()`).
    pub awaited: bool,
    /// The member is read on an element of the call result: an iteration
    /// callback parameter, a `for...of` variable, or an index
    /// (`make().forEach((sheet) => sheet.member)`).
    #[serde(default)]
    pub element: bool,
}
