//! The member access facts, the fixture, binding and mock facts, and the
//! `this` spread, dynamic custom element and misplaced directive facts that
//! the extractor records for the analysis.

/// A member name referenced from an Angular template surface.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AngularTemplateMemberAccessFact {
    /// Referenced class member name.
    pub member: String,
}

/// A typed Angular component field that exposes array elements to templates.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AngularComponentFieldArrayTypeFact {
    /// Component field name used as the template iterable.
    pub field: String,
    /// Array element class name.
    pub element_class: String,
}

/// Opaque Angular `{ ...this }` forwarding marker.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AngularThisSpreadFact;

/// A member access on a static factory call result.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FactoryCallMemberAccessFact {
    /// Local imported class or namespace object used as the factory callee.
    pub callee_object: String,
    /// Static factory method invoked on the callee object.
    pub callee_method: String,
    /// Member accessed on the returned instance-like object.
    pub member: String,
}

/// A factory-returned value consumed opaquely, so every member of the class it
/// returns must be treated as used.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FactoryFnWholeObjectFact {
    /// Local imported function used as the factory callee.
    pub callee_name: String,
}

/// A member access reached through a property of a value returned by an imported
/// factory that returns an object literal.
///
/// `const ui = createUi(); ui.orders.member` emits one fact per member read on a
/// factory-result property. The analyze layer resolves `callee_name` through the
/// consumer's imports to the factory's origin module, reads that module's
/// `exported_factory_return_object_shapes` to find the property whose path equals
/// `property_path` and its class local name, resolves THAT through the factory
/// module's own imports to the class export, and credits `member` on the class
/// (gated on the export actually being a class with members). See issue #1858.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FactoryReturnObjectPropertyAccessFact {
    /// Local imported function used as the factory callee.
    pub callee_name: String,
    /// Dotted property path between the factory-result local and the final member
    /// (e.g. `"orders"` for `ui.orders.member`, `"invoke.orders"` for `ui.invoke.orders.member`).
    pub property_path: String,
    /// Member accessed on the terminal property's instance.
    pub member: String,
}

/// A member access reached through a typed property hop that the extraction
/// layer could not resolve locally.
///
/// `constructor(private opts: Opts) { ... this.opts.c.optM() }` where `Opts`
/// is NOT declared in this file emits
/// `TypedPropertyMemberAccessFact { type_name: "Opts", property_path: "c", member: "optM" }`.
/// The analyze layer resolves `type_name` through the consumer's imports to the
/// declaring module, walks `property_path` through that module's
/// `type_member_types`, resolves the terminal type name through the declaring
/// module's own imports, and credits `member` on the resolved class (gated on
/// the export actually being a class with members). See issue #1785.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypedPropertyMemberAccessFact {
    /// Local (usually imported) named-type symbol the receiver is typed by.
    pub type_name: String,
    /// Remaining dotted property segments between the typed binding and the
    /// final member (e.g. `"c"` for `this.opts.c.optM()`).
    pub property_path: String,
    /// Member accessed on the terminal property's instance.
    pub member: String,
}

/// A class member directly required by a named structural type.
///
/// Optional properties and methods are excluded: removing an optional
/// implementation can preserve assignability, while removing a required one
/// from an explicit `implements` contract cannot.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RequiredTypeMemberFact {
    /// Module-local interface or object-type alias name.
    pub type_name: String,
    /// Static required property or method name.
    pub member: String,
}

/// A named interface that a module-local interface extends.
///
/// Analysis follows these edges to give an `implements` contract the required
/// members of each parent interface, also across modules.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InterfaceExtendsFact {
    /// Module-local interface declaration name.
    pub interface_name: String,
    /// Module-local or imported name of the parent interface.
    pub parent_name: String,
}

/// One direct contributor to a named type alias's receiver surface.
///
/// Nested property types are deliberately excluded: in
/// `{ nested: Nested }`, `Alias.member` does not access `Nested.member`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TypeAliasSurfaceTargetFact {
    /// Module-local alias declaration name.
    pub alias_name: String,
    /// Module-local or imported named type contributing the direct surface.
    pub target_name: String,
}

/// A statically known string value of a TypeScript enum member.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StringEnumMemberValueFact {
    /// Module-local enum declaration name.
    pub enum_name: String,
    /// Static enum member name.
    pub member_name: String,
    /// Exact string initializer value.
    pub value: String,
}

/// A computed property access whose key is a static enum member.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ComputedEnumKeyUseFact {
    /// Local enum object used as the key source.
    pub key_object: String,
    /// Static member selected from the enum object.
    pub key_member: String,
}

/// A fluent chain rooted at a static factory call, one fact per chain. A step is
/// used when the root method is a factory and each earlier step returns `this`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FluentChainMemberAccessFact {
    /// Local imported class or namespace object used as the chain root.
    pub root_object: String,
    /// Static factory method that starts the fluent chain.
    pub root_method: String,
    /// The methods called after the root method, each once, in first-call order.
    pub steps: Vec<String>,
}

/// A fluent chain rooted at a `new` expression, one fact per chain. A step is
/// used when the first method and each earlier step return `this`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FluentChainNewMemberAccessFact {
    /// Local imported class constructed by the `new` expression.
    pub class_name: String,
    /// The method called on the new instance.
    pub first_method: String,
    /// The methods called after the first method, each once, in first-call order.
    pub steps: Vec<String>,
}

/// A member access on a Playwright fixture object inside a test callback.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlaywrightFixtureUseFact {
    /// Local test function or wrapper used as the callback callee.
    pub test_name: String,
    /// Fixture name or dotted fixture path referenced in the callback.
    pub fixture_name: String,
    /// Member accessed on the fixture target.
    pub member: String,
}

/// A Playwright fixture definition declared by a typed `test.extend<T>()`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlaywrightFixtureDefinitionFact {
    /// Local test function or wrapper receiving the fixture definition.
    pub test_name: String,
    /// Fixture name or dotted fixture path declared by the fixture type.
    pub fixture_name: String,
    /// Local type symbol used as the fixture target.
    pub type_name: String,
}

/// A Playwright fixture definition from `<base>.extend<T>()` where `base` is
/// an imported name. The extractor cannot prove that `base` holds a Playwright
/// test, so analysis applies the definition only when `base` resolves to one.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlaywrightConditionalFixtureDefinitionFact {
    /// Local test function or wrapper receiving the fixture definition.
    pub test_name: String,
    /// Imported local name that `.extend<T>()` is called on.
    pub base_name: String,
    /// Fixture name or dotted fixture path declared by the fixture type.
    pub fixture_name: String,
    /// Local type symbol used as the fixture target.
    pub type_name: String,
}

/// A local name that holds a Playwright test: the result of an `.extend()` on
/// the `@playwright/test` `test` import or of a `mergeTests(...)` call.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlaywrightTestBindingFact {
    /// Local name of the Playwright test.
    pub test_name: String,
}

/// A Playwright fixture wrapper alias declared by `mergeTests` or `.extend`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlaywrightFixtureAliasFact {
    /// Local test function or wrapper that inherits fixture definitions.
    pub test_name: String,
    /// Local test function or wrapper inherited by `test_name`.
    pub base_name: String,
}

/// A nested Playwright fixture binding declared by a fixture type alias.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PlaywrightFixtureTypeFact {
    /// Local type alias containing the nested fixture binding.
    pub alias_name: String,
    /// Fixture name or dotted fixture path declared inside the type alias.
    pub fixture_name: String,
    /// Local type symbol used as the nested fixture target.
    pub type_name: String,
}

/// An exported value whose runtime instance targets a local class or interface.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct InstanceExportBindingFact {
    /// Exported binding name.
    pub export_name: String,
    /// Local class or interface symbol used as the instance target.
    pub target_name: String,
    /// The static method of `target_name` whose call result the export
    /// holds (`export const sm = Cls.getInstance()`). `None` when the export
    /// is a direct instance. The class can be imported, so the analyze layer
    /// checks that the method returns an instance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub factory_method: Option<String>,
}

/// A property of an exported object whose value is an instance of a local class.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExportedObjectInstancePropertyFact {
    /// Public export name of the containing object.
    pub export_name: String,
    /// Dotted path from the exported object to the instance property.
    pub property_path: String,
    /// Local class symbol instantiated at that property.
    pub class_local_name: String,
}

/// A proven class member read through a namespace-qualified constructor or local subclass.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct QualifiedClassMemberAccessFact {
    /// Local namespace-import binding.
    pub namespace_local: String,
    /// Class export selected from that namespace.
    pub class_export_name: String,
    /// Member accessed through the proven class or instance binding.
    pub member: String,
}

/// Opaque marker for a dynamic custom-element render site.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DynamicCustomElementRenderFact;

/// The action performed by a Vitest module-mock operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum VitestModuleMockAction {
    /// Register a mock. `factory_replaces_original` is true only when the
    /// factory is structurally closed and cannot load the original module.
    ///
    /// Automock (`vi.mock` / `jest.mock` without a factory) is always
    /// `factory_replaces_original: false` by decision (issue #2082). For
    /// Vitest, the runner derives the mocked shape by importing the original
    /// module, so its top-level code executes at collection time, and
    /// file-level masking cannot express "module evaluated but exports
    /// stubbed". For Jest, a `__mocks__` sibling takes precedence and the
    /// original is genuinely not required, but the manual mock itself may
    /// load the original (`jest.requireActual`), and proving it never does
    /// would need a cross-file factory proof. Both runners therefore keep
    /// coverage credit for the automock form.
    Mock {
        /// Whether the factory provably replaces the original module.
        factory_replaces_original: bool,
    },
    /// Remove a registered mock and restore the original module.
    Unmock,
}

impl VitestModuleMockAction {
    /// Whether this operation registers a proven complete replacement.
    #[must_use]
    pub const fn replaces_original(self) -> bool {
        matches!(
            self,
            Self::Mock {
                factory_replaces_original: true
            }
        )
    }
}

/// Ordered Vitest module-mock operation with a static source target.
///
/// The declaring [`ModuleInfo::file_id`](crate::extract::ModuleInfo::file_id) owns the test-root provenance. The
/// resolver consumes `source` through its canonical specifier pipeline; this
/// fact deliberately carries no resolved path or duplicate diagnostic span.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct VitestModuleMockOperationFact {
    /// Static module specifier passed to `vi.mock` or `vi.unmock`.
    pub source: String,
    /// Source-order position of the call within the declaring module.
    pub call_start: u32,
    /// Typed mock or unmock action.
    pub action: VitestModuleMockAction,
}

/// A `this`-rooted member access with exact enclosing-class provenance.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ClassThisMemberAccessFact {
    /// Enclosing class local name, or `default` for an anonymous default class.
    pub class_local_name: String,
    /// Dotted receiver spelling beginning with `this.`.
    pub object: String,
    /// Terminal member being accessed.
    pub member: String,
}

/// A whole-object use of a `this`-rooted chain with enclosing-class provenance.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ClassThisWholeObjectUseFact {
    /// Enclosing class local name, or `default` for an anonymous default class.
    pub class_local_name: String,
    /// Dotted receiver spelling beginning with `this.`.
    pub object: String,
}

/// A `"use client"` / `"use server"` directive string written as an expression
/// statement in `program.body` (NOT the leading prologue), so the RSC bundler
/// silently ignores it. One entry per offending occurrence. Consumed by the
/// `misplaced-directive` detector.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct MisplacedDirectiveSite {
    /// `true` for `"use server"`, `false` for `"use client"`.
    pub is_server: bool,
    /// Start byte offset of the misplaced directive statement.
    pub span_start: u32,
}
