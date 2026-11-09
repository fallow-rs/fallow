//! Persisted semantic facts shared by extraction and analysis.
use super::*;

/// A typed extraction fact for cross-layer analysis.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SemanticFact {
    /// A class member referenced from an Angular template, host binding, or
    /// component metadata entry.
    AngularTemplateMemberAccess(AngularTemplateMemberAccessFact),
    /// An Angular component field whose value is an array of a class.
    AngularComponentFieldArrayType(AngularComponentFieldArrayTypeFact),
    /// An Angular component spreads `this` into an object literal, so component
    /// input/output usage is opaque.
    AngularThisSpread(AngularThisSpreadFact),
    /// A member access on a value returned by an imported static factory call.
    FactoryCallMemberAccess(FactoryCallMemberAccessFact),
    /// A member access on a value returned by an imported free-function factory
    /// (`const x = importedFactory(); x.member`). See issue #1441 (Part A).
    FactoryFnMemberAccess(FactoryFnMemberAccessFact),
    /// A member access reached through a property of a value whose declared
    /// type is an imported named type (`this.opts.c.optM()` where `opts` is
    /// typed by an imported interface). See issue #1785.
    TypedPropertyMemberAccess(TypedPropertyMemberAccessFact),
    /// A member access on a fluent chain rooted at an imported static factory.
    FluentChainMemberAccess(FluentChainMemberAccessFact),
    /// A member access on a fluent chain rooted at a `new` expression.
    FluentChainNewMemberAccess(FluentChainNewMemberAccessFact),
    /// A member access on a Playwright fixture object inside a test callback.
    PlaywrightFixtureUse(PlaywrightFixtureUseFact),
    /// A Playwright fixture definition declared by a typed `test.extend<T>()`.
    PlaywrightFixtureDefinition(PlaywrightFixtureDefinitionFact),
    /// A Playwright fixture wrapper alias declared by `mergeTests` or `.extend`.
    PlaywrightFixtureAlias(PlaywrightFixtureAliasFact),
    /// A nested Playwright fixture binding declared by a fixture type alias.
    PlaywrightFixtureType(PlaywrightFixtureTypeFact),
    /// An exported value whose runtime instance targets a local class or interface.
    InstanceExportBinding(InstanceExportBindingFact),
    /// A dynamic custom-element tag render that makes static Lit tag credit opaque.
    DynamicCustomElementRender(DynamicCustomElementRenderFact),
    /// A factory-returned value consumed in a way that can expose ANY property
    /// (`const { a, ...rest } = importedFactory()`, a computed destructure key).
    /// The returned class must be treated as wholly used: crediting only the
    /// visible keys would leave a live member reported as dead.
    ///
    /// Appended, never inserted: `bitcode` encodes an enum by ordinal, so moving an
    /// existing variant would make an old cache decode one fact as another.
    FactoryFnWholeObject(FactoryFnWholeObjectFact),
    /// A member access reached through a property of a value returned by an
    /// imported factory that returns an object literal (`const ui = createUi();
    /// ui.orders.member`). Appended after `FactoryFnWholeObject`, never inserted
    /// (bitcode encodes by ordinal). See issue #1858.
    FactoryReturnObjectPropertyAccess(FactoryReturnObjectPropertyAccessFact),
    /// A `this.<field>.<member>` access tied to its exact enclosing class.
    /// Appended because bitcode encodes enum variants by ordinal.
    ClassThisMemberAccess(ClassThisMemberAccessFact),
    /// A whole-object use of `this.<field>...` tied to its exact enclosing class.
    /// Appended because bitcode encodes enum variants by ordinal.
    ClassThisWholeObjectUse(ClassThisWholeObjectUseFact),
    /// An ordered Vitest module-mock operation with direct imported-`vi`
    /// provenance.
    ///
    /// Appended because bitcode encodes enum variants by ordinal. The ordinary
    /// dynamic-import fact for the same source remains authoritative for graph
    /// reachability and unresolved-import diagnostics.
    VitestModuleMockOperation(VitestModuleMockOperationFact),
    /// Direct declarations that form one legal TypeScript declaration merge.
    /// Appended because bitcode encodes enum variants by ordinal.
    DeclarationMerge(DeclarationMergeFact),
    /// A named type whose direct receiver surface is contributed by another
    /// named type (for example `Pick<Base, ...>` or a union constituent).
    /// Appended because bitcode encodes enum variants by ordinal.
    TypeAliasSurfaceTarget(TypeAliasSurfaceTargetFact),
    /// A string-valued TypeScript enum member available as a static property key.
    /// Appended because bitcode encodes enum variants by ordinal.
    StringEnumMemberValue(StringEnumMemberValueFact),
    /// A computed property access keyed by a static enum member.
    /// Appended because bitcode encodes enum variants by ordinal.
    ComputedEnumKeyUse(ComputedEnumKeyUseFact),
    /// A directly declared non-optional member required by a named interface
    /// or object type.
    /// Appended because bitcode encodes enum variants by ordinal.
    RequiredTypeMember(RequiredTypeMemberFact),
    /// The file's complete CommonJS assignment surface is exactly one
    /// top-level `module.exports = { ... }` object literal whose keys are all
    /// static and which has no transpilation marker or competing assignment.
    /// Appended because bitcode encodes enum variants by ordinal.
    CjsSingleStaticObjectMap,
    /// A default import binding was handed on as a whole object.
    /// Appended because bitcode encodes enum variants by ordinal.
    DefaultImportWholeObjectUse(DefaultImportWholeObjectUseFact),
    /// A property of an exported object contains a local class instance.
    /// Appended because bitcode encodes enum variants by ordinal.
    ExportedObjectInstanceProperty(ExportedObjectInstancePropertyFact),
    /// A proven class member read selected through a namespace import.
    /// Appended because bitcode encodes enum variants by ordinal.
    QualifiedClassMemberAccess(QualifiedClassMemberAccessFact),
    /// A Module Federation runtime call (`registerRemotes`, `loadRemote`,
    /// `init` or `createInstance`) imported from a Federation runtime package.
    /// Appended because bitcode encodes enum variants by ordinal.
    FederationRuntimeRemote(FederationRuntimeRemoteFact),
    /// A dynamic import or an import pattern whose load kind differs from the
    /// default of its list: `Dynamic` for `dynamic_imports` and
    /// `DynamicPattern` for `dynamic_import_patterns`.
    /// Appended because bitcode encodes enum variants by ordinal.
    ImportLoadKindOverride(ImportLoadKindOverrideFact),
    /// A source-owned class-producing property of an exported object factory.
    /// Appended because bitcode encodes enum variants by ordinal.
    FactoryReturnObjectPropertySource(Box<FactoryReturnObjectPropertySourceFact>),
    /// Property read whose bound factory receiver passed source-time lexical and value checks.
    ProvenFactoryReturnObjectPropertyAccess(FactoryReturnObjectPropertyAccessFact),
    /// Caller-owned callback origins for a source-proven returned property.
    CallbackFactoryPropertyAccess(Box<CallbackFactoryPropertyAccessFact>),
    /// A Playwright fixture definition from `<base>.extend<T>()` where `base`
    /// is imported. Analysis applies it only when `base` resolves to a
    /// Playwright test. Appended because bitcode encodes enum variants by ordinal.
    PlaywrightConditionalFixtureDefinition(Box<PlaywrightConditionalFixtureDefinitionFact>),
    /// A local name that the extractor proved holds a Playwright test.
    /// Appended because bitcode encodes enum variants by ordinal.
    PlaywrightTestBinding(PlaywrightTestBindingFact),
    /// A top-level statement or declaration emits runtime code beyond module
    /// imports and exports, which have their own extraction records.
    /// Appended because bitcode encodes enum variants by ordinal.
    TopLevelRuntimeStatement,
    /// A static member name probed with `"name" in value` or `Reflect.has(value, "name")`.
    /// Appended because bitcode encodes enum variants by ordinal.
    MemberNameProbe(MemberNameProbeFact),
    /// A use of a member of a TypeScript namespace that a plain member access
    /// does not record: a nested chain (`N.Inner.x`), a value that leaves the
    /// chain (`use(N)`), or a reference inside the namespace body.
    /// Appended because bitcode encodes enum variants by ordinal.
    NamespaceMemberUse(NamespaceMemberUseFact),
    /// Direct imported/global property access, collected only on request.
    SharedState(crate::shared_state::StateAccessFact),
    /// The per-file shared-state fact limit was reached.
    SharedStateTruncated,
}
