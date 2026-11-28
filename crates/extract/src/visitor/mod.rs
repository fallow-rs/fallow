mod binding_resolution;
mod deferred_records;
mod dynamic_import_shapes;
mod extractor_state;
mod factory_resolution;
mod member_access_records;
mod module_info_assembly;
pub use dynamic_import_shapes::extract_import_from_callable;
use dynamic_import_shapes::*;
mod bound_member_accesses;
mod callee_uses;
mod declarations;
mod decorator_metadata;
mod helpers;
mod independent_this;
mod local_class_heritage;
mod re_export_records;
mod record_extend;
mod remap;
mod requirements;
mod semantic_calls;
mod shared_state;
mod signature_value_aliases;
mod ssr_load_module;
mod this_scope;
mod walk_order;

use requirements::{BindingRequirements, RequirementArena};
use std::cell::{Cell, RefCell};
#[path = "visit_impl_interface_types.rs"]
mod interface_types;
pub(crate) mod react;
mod visit_impl;
#[path = "visit_impl_worker_env.rs"]
mod worker_env;
pub(crate) use record_extend::{extend_member_accesses, extend_whole_object_uses};
pub(crate) use semantic_calls::mark_template_callers;
pub(crate) use ssr_load_module::{ssr_load_module_call, ssr_load_module_source};
pub(crate) use visit_impl::{MAX_RECORDED_CHAIN_LINKS, push_member_tag_accesses};

use oxc_ast::ast::{
    Argument, ArrayExpressionElement, ArrowFunctionBody, BindingPattern, CallExpression,
    Expression, ImportExpression, JSXMemberExpression, JSXMemberExpressionObject, ObjectPattern,
    ObjectProperty, ObjectPropertyKind, Statement,
};
use oxc_span::Span;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    AngularTemplateMemberAccessFact, ComputedEnumKeyUseFact, DynamicImportInfo,
    DynamicImportPattern, ExportInfo, ExportName, ExportedObjectInstancePropertyFact,
    FactoryFnMemberAccessFact, FactoryFnWholeObjectFact, FactoryReturnObjectPropertyAccessFact,
    ImportInfo, ImportedName, InstanceExportBindingFact, MemberAccess, MemberInfo, MemberKind,
    ModuleInfo, PlaywrightFixtureDefinitionFact, PlaywrightTestBindingFact,
    QualifiedClassMemberAccessFact, ReExportInfo, RequireCallInfo, SemanticFact,
    TypedPropertyMemberAccessFact, VisibilityTag,
};
use crate::{js_text::JsText, suppress::ParsedSuppressions};
use fallow_types::extract::{
    AngularComponentSelector, AngularInputMember, AngularOutputMember, CalleeUse,
    ClassHeritageInfo, CommandInvocation, ComponentFunction, ComponentProp, DiKeySite,
    DispatchedEvent, HookUse, ImportBindingReference, ImportLoadKind, ImportLoadKindOverrideFact,
    ImportedCallSite, LocalTypeDeclaration, MisplacedDirectiveSite, PublicSignatureTypeReference,
    RenderEdge, SanitizedSinkArg, SanitizerScope, SecurityControlSite, SinkLiteralValue, SinkSite,
    SkippedSecurityCalleeSite, TaintedBinding,
};
use helpers::LitCustomElementDecorator;
pub(crate) use helpers::array_element_type_from_type;
use helpers::finish_package_resolve_sites;
use interface_types::InterfaceTypeLinks;
use worker_env::WorkerEnvBodies;
mod static_imports;
use static_imports::collect_static_import_specifiers;

pub(crate) const ROUTE_LOADER_DATA_OBJECT: &str = "$fallow.routeLoaderData";
const MAX_DIRECT_OBJECT_BINDING_MEMBER_ACCESSES: usize = 8192;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum RouteLoadHarvestMode {
    #[default]
    None,
    SvelteKitPage,
    ConventionalRoute,
}

/// A local function that returns dynamic `import()` results, with the count of
/// its references and of the references whose shape credits a precise set of
/// exports. A reference in any other shape credits the whole target module.
#[derive(Debug, Clone)]
struct LocalImportLoader {
    imports: Vec<LoaderImport>,
    references: usize,
    credited_references: usize,
    is_exported: bool,
}

/// One `import()` that a local loader returns, with its static specifiers.
#[derive(Debug, Clone)]
struct LoaderImport {
    span: Span,
    sources: Vec<String>,
}

#[derive(Debug, Clone)]
struct SubclassDeclaration {
    binding_span: Span,
    super_reference_span: Option<Span>,
}

#[derive(Debug, Clone)]
struct BindingClassProof {
    // Observations share admission conditions without new condition copies after refusal.
    builtin_containers: std::sync::Arc<[Span]>,
    target_name: String,
    target_reference_span: Span,
    requirements: BindingRequirements,
}

#[derive(Debug, Clone)]
struct TypePropertyTarget {
    name: String,
    reference_span: Option<Span>,
    owner_span: Option<Span>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DirectBindingOwner {
    Module,
    Scope(usize),
}

#[derive(Debug, Clone, Default)]
struct DirectBindingTarget {
    names: FxHashSet<String>,
    declaration_span: Option<Span>,
    constructor_owner: Option<Span>,
    value: Option<usize>,
    initial_producer_value: Option<usize>,
    /// Known function-body assignments remain possible targets for this older slot.
    possible_assignment_owner: Option<DirectBindingOwner>,
    requirements: BindingRequirements,
}

impl FromIterator<String> for DirectBindingTarget {
    fn from_iter<T: IntoIterator<Item = String>>(iter: T) -> Self {
        Self {
            names: iter.into_iter().collect(),
            ..Self::default()
        }
    }
}

impl std::ops::Deref for DirectBindingTarget {
    type Target = FxHashSet<String>;
    fn deref(&self) -> &Self::Target {
        &self.names
    }
}

impl std::ops::DerefMut for DirectBindingTarget {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.names
    }
}

impl<'a> IntoIterator for &'a DirectBindingTarget {
    type Item = &'a String;
    type IntoIter = std::collections::hash_set::Iter<'a, String>;
    fn into_iter(self) -> Self::IntoIter {
        self.names.iter()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum ConstructorWriteTarget {
    Property { parent: usize, property: String },
    Root { path: String, declaration: Span },
}

#[derive(Debug, Clone, Default)]
struct ConstructorWrite {
    refused: bool,
    input: std::sync::Arc<[BindingClassProof]>,
    output: std::sync::Arc<[BindingClassProof]>,
}

#[derive(Debug, Default)]
struct DirectObjectBindingBranch {
    proof_writes: FxHashSet<String>,
    targets: FxHashMap<String, DirectBindingTarget>,
    paths_by_root: FxHashMap<String, FxHashSet<String>>,
    update_overwrites: FxHashMap<DirectBindingOwner, FxHashSet<String>>,
    projection_writes: FxHashMap<usize, FxHashSet<Option<String>>>,
    constructor_writes: FxHashMap<ConstructorWriteTarget, ConstructorWrite>,
}

#[derive(Debug, Default)]
struct DirectObjectBindingBranchScope {
    index: usize,
    proof_writes: FxHashSet<String>,
    update_overwrites: FxHashMap<DirectBindingOwner, FxHashSet<String>>,
    projection_views: FxHashMap<usize, usize>,
    projection_writes: FxHashMap<usize, FxHashSet<Option<String>>>,
    constructor_writes: FxHashMap<ConstructorWriteTarget, ConstructorWrite>,
}

#[derive(Debug)]
struct LoopUpdateAccess {
    object: String,
    member: String,
    owner: DirectBindingOwner,
}

#[derive(Debug, Default)]
struct LoopUpdateObservations {
    function_scope: Option<usize>,
    scope_index: usize,
    accesses: Vec<LoopUpdateAccess>,
    retained_paths: usize,
    exhausted: bool,
}

#[derive(Debug, Clone, Default)]
struct DirectBindingValue {
    constructor_owner: Option<Span>,
    constructor_alternatives: std::sync::Arc<[BindingClassProof]>,
    proof: Option<BindingClassProof>,
    factory_property: Option<FactoryPropertyProof>,
    factory_callee: Option<(String, Span)>,
    callback_result: Option<CallbackResultProof>,
    factory_constructed: bool,
    producer: Option<BindingProducer>,
    pending: Option<PendingBindingValue>,
    initial_projections: FxHashMap<String, usize>,
    requirements: BindingRequirements,
    iteration_proof: Option<BindingClassProof>,
    iteration_is_map: bool,
    array_element: Option<usize>,
    numeric_contract: bool,
    projection_contract: bool,
    projection_path: String,
    properties: FxHashMap<String, usize>,
    unknown_properties: bool,
}

impl DirectBindingValue {
    fn class(proof: BindingClassProof) -> Self {
        Self {
            proof: Some(proof),
            ..Self::default()
        }
    }

    fn unknown() -> Self {
        Self {
            unknown_properties: true,
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone)]
struct FactoryPropertyProof {
    projection: String,
    vue_only: bool,
    assigned_class: Option<BindingClassProof>,
    origin: FactoryPropertyOrigin,
    requirements: BindingRequirements,
}

#[derive(Debug, Clone)]
enum FactoryPropertyOrigin {
    CallbackParameter {
        parameter_index: u32,
        declaration: Span,
        reference: Span,
    },
    Typed {
        proof: BindingClassProof,
        path: String,
    },
    Call {
        name: String,
        declaration: Span,
        reference: Span,
        method: Option<String>,
        callbacks: std::sync::Arc<[(u32, CallbackResultProof)]>,
    },
}

#[derive(Debug, Clone)]
struct CallbackResultProof {
    class: BindingClassProof,
    lazy_reference: Option<Span>,
    capture: Option<(String, Span, Span)>,
    getter: bool,
}

#[derive(Debug, Clone)]
struct BindingProducer {
    name: String,
    declaration_span: Span,
    result: Option<BindingClassProof>,
    shape: Vec<(String, BindingClassProof)>,
    is_async: bool,
    is_generator: bool,
    is_static_method: bool,
}

#[derive(Debug, Clone)]
enum PendingBindingValue {
    Call {
        producer: usize,
        reference: Span,
        awaited: bool,
        result: Option<BindingClassProof>,
    },
    Projection {
        result: usize,
        path: String,
    },
}

#[derive(Debug, Clone)]
enum SubclassAccessProof {
    Known(BindingClassProof),
    Value {
        id: usize,
        requirements: BindingRequirements,
    },
}

#[derive(Debug, Clone)]
struct ProjectedMemberAccess {
    proof: BindingClassProof,
    property_path: String,
    member: String,
}

#[derive(Debug, Clone)]
struct PendingSubclassMemberAccess {
    proof: SubclassAccessProof,
    member: String,
}

#[derive(Debug, Clone)]
struct LocalClassExportInfo {
    members: Vec<MemberInfo>,
    super_class: Option<String>,
    implemented_interfaces: Vec<String>,
    type_parameters: Vec<String>,
    instance_bindings: Vec<(String, String)>,
    binding_origins: FxHashMap<String, Span>,
    binding_span: Span,
    super_class_type_args: Vec<String>,
    generic_instance_bindings: Vec<(String, usize)>,
}

#[derive(Debug, Clone)]
struct LocalSignatureTypeReference {
    owner_name: String,
    type_name: String,
    span: Span,
    from_satisfies: bool,
}

#[derive(Debug, Clone)]
struct ObjectBindingCandidate {
    binding_path: String,
    source_name: String,
}

#[derive(Debug, Clone)]
struct PendingLocalExportSpecifier {
    local_name: String,
    exported_name: String,
    is_type_only: bool,
    span: Span,
}

/// The parts of a named or default import that a re-export of it needs.
struct ForwardableImport {
    source: String,
    imported_name: String,
    is_type_only: bool,
}

/// A whole-module forward of one binding: `export default x` or `export = x`.
/// When `x` is a named or default import, the statement forwards the import,
/// exactly as `export { x as default }` does, so it resolves to a re-export.
#[derive(Debug, Clone)]
enum PendingBindingForward {
    /// `export default x`. The default `ExportInfo` is already recorded and is
    /// replaced by the re-export when `x` is an import.
    ExportDefault { local_name: String },
    /// `export = x`. TypeScript reads the binding as the default export.
    ExportAssignment { local_name: String, span: Span },
}

#[derive(Debug, Clone)]
struct PendingComputedEnumKeyUse {
    fact: ComputedEnumKeyUseFact,
    key_object_span: Span,
}

#[derive(Debug, Clone)]
struct StructuralParameterUse {
    // Scoped type parameters have no nominal receiver; call arguments can still supply one.
    type_name: Option<String>,
    members: FxHashSet<String>,
}

#[derive(Debug, Clone, Default)]
struct LocalStructuralFunction {
    params: FxHashMap<usize, StructuralParameterUse>,
}

struct ContextualParameterTypes {
    parameters: Vec<Option<String>>,
    type_parameters: Vec<String>,
}

enum FunctionTypeAliasBinding {
    Function(ContextualParameterTypes),
    /// `type X = ReturnType<typeof useFooStore>`, mapped to `useFooStore`.
    Store(String),
    ClassSelf,
    NonFunction,
}

#[derive(Debug, Clone)]
enum StructuralCallArgument {
    DirectClass(String),
    Binding(String),
}

#[derive(Debug, Clone)]
struct StructuralClassCallCandidate {
    callee_name: String,
    arguments: Vec<Option<StructuralCallArgument>>,
}

#[derive(Debug, Clone)]
struct FactoryCallCandidate {
    local_name: String,
    callee_object: String,
    callee_method: String,
}

/// `const local = useApi()` where `useApi` is a same-file function whose body
/// returns `new Class()`. Resolved against `factory_return_functions` at finalize
/// time so `local.member` credits the constructed class. See issue #1441.
#[derive(Debug, Clone)]
struct FactoryReturnCandidate {
    local_name: String,
    callee_name: String,
    /// `const local = await callee()`: the local holds the awaited result.
    awaited: bool,
    /// `const { key: local } = callee()`: the local holds the `key` property
    /// of the result, not the result itself (issue #5298).
    property_path: Option<String>,
}

/// An unresolved object-literal property value captured while visiting a factory
/// function's `return { ... }`. Resolved to a terminal class local name at finalize
/// time (after `binding_target_names` / `local_class_exports` are populated). See #1858.
#[derive(Debug, Clone)]
pub(crate) enum ObjectPropValueRef {
    /// A `new Class()` leaf: the class local name is known now.
    Class(String, Option<Span>),
    /// A flattened identifier / member-expression leaf (`o`, `factory.ordersPage`),
    /// resolved against `binding_target_names` (+ typed-property expansion) at finalize.
    Path(String),
}

/// A factory function returning an object literal, captured at visit time with
/// per-property value refs and its dotted path. Resolved into
/// `strict_factory_return_object_shapes` / `factory_return_object_shapes` at finalize.
/// See issue #1858.
#[derive(Debug, Clone)]
struct FactoryReturnObjectCandidate {
    fn_name: String,
    /// Whether this function qualifies for the cross-module (strict) map: sync,
    /// non-generator, unanimous-terminal object-literal return.
    is_strict_eligible: bool,
    /// `(dotted_property_path, value_ref)` for each statically-known property.
    properties: Vec<(String, ObjectPropValueRef)>,
}

/// The classified right-hand side of a module-local assignment, used to build a
/// VALUE proof that an aliased factory's returned local really holds a class
/// instance, not merely a type annotation. See issue #1441.
#[derive(Debug, Clone)]
pub(crate) enum FactoryAssignedValue {
    /// `id = new Class()`, directly a class instance.
    NewClass(String),
    /// `id = callee(...)`, a class instance only if `callee` is a strict
    /// same-file factory (resolved at finalize).
    Call(String),
    /// Anything else (a literal, a mock, `as any`, …), poisons the proof.
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum BindingTarget {
    Class(String),
    FactoryCall {
        callee_object: String,
        callee_method: String,
    },
    /// The same unqualified binding name could not be proven to have one target
    /// across lexical scopes or assignments. Module-flat fallback resolution
    /// must abstain; scope-owned extraction records precise parameter accesses.
    Ambiguous,
}

impl BindingTarget {
    pub(crate) fn class_name(&self) -> Option<&str> {
        match self {
            Self::Class(name) => Some(name),
            Self::FactoryCall { .. } | Self::Ambiguous => None,
        }
    }

    fn class_with_suffix(&self, suffix: &str) -> Option<String> {
        self.class_name()
            .map(|class_name| format!("{class_name}.{suffix}"))
    }
}

/// Outcome of expanding a compound binding target (`Opts.c`) through the
/// file's named-type property maps. See issue #1785.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TypedPropertyExpansion {
    /// Every hop resolved locally; the terminal name is an ordinary
    /// local / imported identifier the analyze layer can resolve.
    Resolved(String, Option<Span>, Vec<(Span, Span)>),
    /// A hop's type name is not declared in this file (imported); the
    /// remainder must be joined cross-module at analyze time.
    CrossModule {
        type_name: String,
        property_path: String,
        origin: Option<Span>,
        requirements: Vec<(Span, Span)>,
    },
    /// Not an interface/alias hop (e.g. a local-class compound handled by
    /// `instance_bindings`); leave the compound access untouched.
    Opaque,
}

#[derive(Debug, Clone)]
struct PendingPlaywrightFactory {
    test_name: String,
    /// The identifier that the helper extends. `None` when the receiver is a
    /// `mergeTests(...)` call, which is always a Playwright test.
    base_name: Option<String>,
    type_bindings: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
struct SourceReturnPath {
    arg_index: usize,
    suffixes: Vec<String>,
}

#[derive(Debug, Clone, Default)]
struct SourceReturningHelper {
    paths: Vec<SourceReturnPath>,
}

#[derive(Debug, Clone)]
enum SideEffectRegistrationTarget {
    LocalClass(String),
    AnonymousDefaultExport(usize),
}

#[derive(Debug, Clone)]
struct LitCustomElementCandidate {
    decorator: LitCustomElementDecorator,
    target: SideEffectRegistrationTarget,
    /// The `@customElement('x-foo')` tag literal, if statically recoverable.
    /// `None` for a computed / non-literal tag argument (which the Lit
    /// `unrendered-component` arm cannot key on, so no registration is recorded).
    tag: Option<String>,
    /// Start byte offset of the decorated class (anchors the finding).
    span_start: u32,
}

/// How a pending mock operation's recorded object span must be proven before
/// the operation is trusted (issue #2082).
///
/// `Binding` covers `X.mock(...)` where `X` is an identifier: the span must
/// resolve to a `vi` import from `vitest` (any local alias), a `jest` import
/// from `@jest/globals` (any local alias), or the unresolved `jest` global.
/// `VitestNamespace` covers `ns.vi.mock(...)`: the span must resolve to a
/// namespace import of `vitest`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MockObjectProvenance {
    Binding,
    VitestNamespace,
}

#[derive(Debug, Clone)]
struct PendingVitestMockOperation {
    source: String,
    /// Span of the identifier whose provenance the semantic pass must confirm
    /// (the `vi`/`jest`/alias object, or the namespace in `ns.vi.mock`).
    object_span: Span,
    provenance: MockObjectProvenance,
    call_span: Span,
    has_factory: bool,
    /// Aliased and namespace forms defer their dynamic-import credit edges to
    /// `resolve_vitest_mock_operations`, where provenance is proven; literal
    /// `vi`/`jest` objects push edges eagerly (they may be injected globals).
    needs_deferred_edges: bool,
    proof: PendingVitestMockProof,
}

#[derive(Debug, Clone)]
enum PendingVitestMockProof {
    ClosedFactory {
        binding_requirement_spans: Vec<Span>,
        namespace_requirement_spans: Vec<Span>,
    },
    UnprovenMock,
    Unmock,
    /// `vi.doMock` / `jest.doMock` (issue #2082): unhoisted and
    /// order-sensitive, the call affects only module requests evaluated after
    /// it runs and may sit inside a test callback that never executes, so it
    /// never enters the mask fact stream. It still deserves the
    /// dynamic-import credit edges (target plus speculative `__mocks__`
    /// sibling) once provenance is proven, so a manual mock referenced only
    /// through `doMock` does not surface as an unused file.
    CreditOnly,
}

#[derive(Debug, Clone)]
pub(crate) struct InlineTemplateFinding {
    pub(crate) template_source: String,
    pub(crate) decorator_start: u32,
}

#[derive(Default)]
pub(crate) struct ModuleInfoExtractor {
    pub(crate) exports: Vec<ExportInfo>,
    pub(crate) imports: Vec<ImportInfo>,
    pub(crate) re_exports: Vec<ReExportInfo>,
    dynamic_imports: Vec<DynamicImportInfo>,
    dynamic_import_patterns: Vec<DynamicImportPattern>,
    /// Spans of dynamic imports and patterns whose load kind differs from the
    /// default of their list. Kept apart from `semantic_facts` until the spans
    /// are final, so a component-file remap moves them with the edges.
    import_load_kind_marks: Vec<(Span, ImportLoadKind)>,
    require_calls: Vec<RequireCallInfo>,
    package_path_references: Vec<String>,
    type_package_references: Vec<String>,
    bin_path_references: Vec<String>,
    /// Direct `require.resolve('pkg')` calls: the package name and the call
    /// span. Kept as a span until the spans are final, so a component-file
    /// remap moves them.
    package_resolve_sites: Vec<(String, Span)>,
    pub(crate) member_accesses: Vec<MemberAccess>,
    semantic_facts: Vec<SemanticFact>,
    /// Inner calls of a fluent chain that its outermost call already walked.
    fluent_chain_inner_calls: FxHashSet<Span>,
    pending_computed_enum_key_uses: Vec<PendingComputedEnumKeyUse>,
    pending_vitest_mock_operations: Vec<PendingVitestMockOperation>,
    pub(crate) whole_object_uses: Vec<String>,
    has_cjs_exports: bool,
    cjs_assignment_count: u32,
    cjs_single_static_object_map: bool,
    has_cjs_es_module_marker: bool,
    has_angular_component_template_url: bool,
    handled_require_spans: FxHashSet<Span>,
    handled_import_spans: FxHashSet<Span>,
    /// `import()` spans whose value an expression statement discards.
    discarded_import_spans: FxHashSet<Span>,
    /// Bindings of an `import()` without `await`. Such a binding holds a
    /// promise, so a member read on it, such as `p.then(cb)`, hands the
    /// namespace to code that the walk does not follow.
    promise_import_bindings: FxHashSet<String>,
    /// Top-level local loader functions keyed by binding name: a `const`
    /// arrow or function expression, or a function declaration, whose body
    /// returns an `import()`. Registered before the body walk because a
    /// function declaration is hoisted above the code that calls it.
    local_import_loaders: FxHashMap<String, LocalImportLoader>,
    /// Imported component loaders whose proven contract reads the default export.
    component_loader_imports: FxHashMap<String, visit_impl::ComponentLoaderImport>,
    namespace_binding_names: Vec<String>,
    module_namespace_binding_names: FxHashSet<String>,
    scoped_namespace_binding_names: Vec<FxHashSet<String>>,
    /// The `import * as NS from '...'` locals of this program, pre-registered
    /// from the top-level statement list before the body walk: an import
    /// declaration is legal after the code that reads it, so walk order cannot
    /// answer whether a reference names a namespace object. See issue #2377.
    namespace_import_locals: FxHashSet<String>,
    /// Test-framework spy APIs that a top-level binding or a global names in
    /// this program (`vi`, `jest`, `spyOn`, the `mock` of `node:test`), keyed by
    /// local name. Pre-registered before the body walk, so a declaration later
    /// in the file still removes the global meaning of its name.
    spy_api_locals: FxHashMap<String, visit_impl::SpyApi>,
    /// Default-import locals whose target shape is known only after graph
    /// resolution. Bare uses become semantic facts instead of joining the
    /// namespace-wide `whole_object_uses` stream.
    default_import_locals: FxHashSet<String>,
    default_import_whole_object_uses: FxHashSet<String>,
    /// Names seeded into an isolated template snippet. Directly calling an
    /// imported function is not a whole-object handover, so call-callee spans
    /// for this broader name set are excluded during that snippet's walk.
    template_object_locals: FxHashSet<String>,
    /// Module-scope `const` names bound to an object literal.
    module_plain_object_consts: FxHashSet<String>,
    /// Spans of references to those locals that a more precise pass already
    /// resolved to a member, so `visit_identifier_reference` leaves them alone:
    /// the object of a static or string-computed access, the root of a JSX
    /// member tag, and the left side of a dotted type name, plus a re-export
    /// specifier local the graph already treats as a whole-object observation.
    /// See issue #2377.
    structured_namespace_reference_spans: FxHashSet<Span>,
    /// Context of `new URL(path, import.meta.url)` edges in this file.
    new_url_context: visit_impl::NewUrlContext,
    /// `(root local, namespace local)` for a namespace placed in an object
    /// literal bound to that root (`const api = { ns }`). The placement itself
    /// is precise, so a bare reference to the root is what hands the namespace
    /// object on. See issue #2377.
    object_literal_namespace_placements: Vec<(String, String)>,
    /// Local bindings introduced by `import X = require('./y')`.
    pub(crate) import_equals_bindings: Vec<String>,
    /// Names declared by `export import X = require('./y')`. The walk credits
    /// each one with a whole-object use as it records it, the same credit
    /// `export { X }` earns for a namespace import. The list exempts them from
    /// the unused-import-binding verdict: the exported form is the file's
    /// public API and has no local reference by construction.
    pub(crate) exported_import_equals_names: Vec<String>,
    binding_target_names: FxHashMap<String, BindingTarget>,
    /// The class each binding name most recently named at this point in the walk,
    /// never poisoned to `Ambiguous`. `binding_target_names` is module-flat, so a
    /// name bound to two classes in sibling scopes abstains there; this map keeps
    /// the walk-order answer, which is the correct one for the scope the access
    /// was written in.
    walk_order_class_bindings: FxHashMap<String, String>,
    /// The class paths of a local whose init is a conditional with branches
    /// of different classes (`const r = f ? p.rotate() : p`). A read on the
    /// local credits each path. The map is module-flat like
    /// `binding_target_names`, so a read in another scope with the same
    /// local name can get extra credit, never less.
    receiver_alias_alternatives: FxHashMap<String, Vec<String>>,
    /// A scoped target snapshot has no module-flat binding to resolve after scope
    /// exit. Other snapshots only credit bindings that eventually become ambiguous.
    walk_order_member_accesses: Vec<(Option<String>, String, String)>,
    /// The `(binding, class)` pair of each whole-object use of a binding,
    /// taken at the read. It credits the class when sibling scopes make the
    /// module-flat binding ambiguous.
    walk_order_whole_object_uses: Vec<(String, String)>,
    interface_property_types: FxHashMap<String, FxHashMap<String, TypePropertyTarget>>,
    interface_type_links: InterfaceTypeLinks,
    worker_env_bodies: WorkerEnvBodies,
    pending_typed_destructures: Vec<(String, String, String)>,
    iterable_element_types: FxHashMap<String, String>,
    /// Module-scope value bindings whose type is an array (or Vue reactive array)
    /// of a non-builtin class, keyed by binding name -> element class name. Read
    /// by the Vue SFC template scanner to type a `v-for` loop variable to its
    /// source iterable's element class so template member accesses on the item
    /// (`{{ util.getter }}`) credit the class. Transient extractor state, not a
    /// cached `ModuleInfo` field (mirrors `binding_target_names`).
    array_binding_element_types: FxHashMap<String, String>,
    /// Block/function-scoped array element types for local iteration receivers.
    /// This lets `const utils: Util[]` inside a function type `.map((util) => …)`
    /// and `for (const util of utils)` in the same lexical scope without leaking
    /// the binding to sibling functions.
    scoped_array_binding_element_types: Vec<FxHashMap<String, String>>,
    /// Module-scope bindings typed `Map<K, V>` / `ReadonlyMap<K, V>` (or
    /// initialized with `new Map<K, V>()`) of a non-builtin class `V`, keyed by
    /// binding name -> value class name. Class fields use the class-scoped
    /// `this@<id>.` key. A `<binding>.get(...)` result is then a receiver of `V`.
    map_binding_value_types: FxHashMap<String, String>,
    /// Map value types aligned with `scoped_direct_object_binding_roots`, so
    /// every lexical binding can hide an outer map even when its type is unknown.
    scoped_map_binding_value_types: Vec<FxHashMap<String, String>>,
    /// Map callbacks awaiting their function scope, keyed by callback start.
    map_callback_value_bindings: FxHashMap<u32, (String, Span, String, Option<BindingClassProof>)>,
    /// `import('./x').then(m => ...)` namespace params awaiting their
    /// callback scope, keyed by callback start. The param shadows a binding
    /// that is recorded in the outer scope, so that binding gets no credit.
    then_callback_namespace_bindings: FxHashMap<u32, String>,
    /// Top-level local function name -> declared return element class (`Promise<T>`
    /// / `T`, non-builtin), from a `visit_program` pre-pass, so `Promise.all(arr.map(cb))`
    /// types its result also when the callee follows the consumer. Transient
    /// extractor state, not a cached `ModuleInfo` field. See issue #1793.
    local_function_return_types: FxHashMap<String, String>,
    /// Top-level local function name -> array return element class (issue #4313).
    local_function_array_return_types: FxHashMap<String, String>,
    object_binding_candidates: Vec<ObjectBindingCandidate>,
    /// Direct `new Class()` object-property target associations recorded during the AST
    /// walk. Combined with `object_binding_candidates` to keep both object
    /// binding channels under the same breadth cap.
    direct_object_binding_target_count: usize,
    /// Object-property paths whose value is a direct class instance. Module
    /// targets remain available throughout the walk, while scoped targets are
    /// pushed and popped with the matching declaration scopes.
    module_direct_object_binding_targets: FxHashMap<String, DirectBindingTarget>,
    module_direct_object_binding_generations: FxHashMap<String, u64>,
    module_direct_object_binding_paths_by_root: FxHashMap<String, FxHashSet<String>>,
    scoped_direct_object_binding_targets: Vec<FxHashMap<String, DirectBindingTarget>>,
    scoped_direct_object_binding_generations: Vec<FxHashMap<String, u64>>,
    scoped_direct_object_binding_paths_by_root: Vec<FxHashMap<String, FxHashSet<String>>>,
    scoped_direct_object_binding_roots: Vec<FxHashSet<String>>,
    /// Function frames own possible outer writes until that body exits.
    direct_object_binding_function_scopes: Vec<usize>,
    direct_object_binding_branch_scopes: Vec<DirectObjectBindingBranchScope>,
    loop_update_observations: Vec<LoopUpdateObservations>,
    direct_object_binding_member_accesses: FxHashSet<(String, String)>,
    direct_object_binding_access_generations: FxHashMap<String, FxHashMap<String, u64>>,
    dynamic_direct_object_binding_access_generations: FxHashMap<(String, String), u64>,
    dynamic_direct_object_binding_abstention_generations: FxHashMap<String, u64>,
    direct_object_binding_generation: u64,
    abstained_direct_object_binding_paths: FxHashSet<String>,
    direct_object_binding_whole_object_uses: FxHashSet<String>,
    deferred_function_var_binding_roots: Vec<FxHashSet<String>>,
    /// Working state for the object-binding fixed-point (issue #1843 follow-up):
    /// an ancestor-prefix index over `binding_target_names`, mapping every proper
    /// dotted prefix `P` of a key to the keys that start with `P.`. Built once per
    /// `resolve_object_binding_candidates` run and kept current as that pass
    /// inserts. `copy_nested_binding_targets` uses it to enumerate keys under a
    /// source prefix in O(matches) instead of scanning all of
    /// `binding_target_names`. `None` outside the pass. NOT persisted.
    binding_target_prefix_index: Option<FxHashMap<String, Vec<String>>>,
    local_declaration_names: FxHashSet<String>,
    /// Top-level names that an `interface` or a `type` alias declares, and
    /// top-level names that a value declaration declares. A local
    /// `export { X }` of a name with only a type declaration is a type-only
    /// export (issue #5262).
    module_type_names: FxHashSet<String>,
    module_value_names: FxHashSet<String>,
    pending_local_export_specifiers: Vec<PendingLocalExportSpecifier>,
    pending_binding_forwards: Vec<PendingBindingForward>,
    local_structural_functions: FxHashMap<String, LocalStructuralFunction>,
    /// Parameter types declared by module-local function type aliases, used to
    /// recover contextual types for unannotated arrow/function expressions.
    function_type_alias_params: FxHashMap<String, ContextualParameterTypes>,
    /// Nearest lexical type alias declarations for function bodies and blocks.
    /// A `NonFunction` binding still shadows an outer function alias. Transient
    /// visitor state; never persisted.
    function_type_alias_scopes: Vec<FxHashMap<String, FunctionTypeAliasBinding>>,
    /// Local function bodies pre-scanned by `record_local_structural_function`.
    /// Prevents a second AST walk when the ordinary visitor reaches the body.
    scoped_typed_parameter_body_spans: FxHashSet<Span>,
    /// Module-local const aliases proven to select the browser-global
    /// `HTMLElement` when it exists, with an SSR-only class fallback.
    native_html_element_alias_candidates: FxHashSet<String>,
    structural_class_call_candidates: Vec<StructuralClassCallCandidate>,
    namespace_depth: u32,
    /// Depth of `declare module '<specifier>' { ... }` ambient-module and
    /// module-augmentation bodies currently being walked. Inner `export`
    /// statements describe the named module, not the containing file, so
    /// file-level export recording is suppressed while non-zero (issue #2349).
    /// Transient visitor state; never persisted.
    ambient_module_depth: u32,
    /// Whether the walked program has top-level import or export syntax.
    /// TypeScript reads `declare module 'pkg'` as an augmentation only in a
    /// module file. Transient visitor state; never persisted.
    is_module_file: bool,
    /// Depth of namespace bodies declared without the `export` keyword
    /// (`namespace Foo {}`, `declare namespace Foo {}`, legacy `module Foo {}`,
    /// dotted `namespace A.B.C {}`, and namespaces nested in those or in
    /// `declare global`) currently being walked. The namespace is a local
    /// binding, so its inner `export` statements are members of that binding
    /// rather than file-level exports; export recording is suppressed while
    /// non-zero and nothing is queued in `pending_namespace_members` because
    /// there is no exported owner to attach to (issue #2356). Kept separate
    /// from `namespace_depth`, which only tracks exported namespace bodies.
    /// Transient visitor state; never persisted.
    local_namespace_depth: u32,
    pending_namespace_members: Vec<MemberInfo>,
    /// Names of the nested exported namespaces between the exported root
    /// namespace and the body that the visitor walks. A member of a nested
    /// namespace gets the dotted path `Inner.x`, relative to the root.
    /// Transient visitor state; never persisted.
    namespace_member_path: Vec<String>,
    /// Span of the exported root namespace declaration (`export namespace N`)
    /// that `visit_ts_namespace_declaration` reaches next. Transient visitor
    /// state; never persisted.
    exported_namespace_root: Option<Span>,
    /// Span of the nested namespace declaration that is an exported member of
    /// the namespace around it (`export namespace Inner`, or a segment of a
    /// dotted `namespace A.B`). Transient visitor state; never persisted.
    exported_namespace_member: Option<Span>,
    /// Spans of exported namespaces that are type-only only because their
    /// body emits no JavaScript (see `namespace_is_instantiated`). When such a
    /// namespace merges with a value export of the same name, the merged
    /// symbol is one runtime binding, so the namespace entry stays a value.
    /// Transient visitor state; never persisted.
    structural_type_only_namespace_spans: Vec<oxc_span::Span>,
    class_heritage: Vec<ClassHeritageInfo>,
    /// `(token_export_name, interface_name)` for `new InjectionToken<I>(...)`
    /// declarations imported from `@angular/core`. See issue #920.
    injection_tokens: Vec<(String, String)>,
    local_type_declarations: Vec<LocalTypeDeclaration>,
    public_signature_type_references: Vec<PublicSignatureTypeReference>,
    local_signature_type_references: Vec<LocalSignatureTypeReference>,
    /// `(owner, held)`: the inferred type of `owner` holds `held` as a value.
    /// Both are a binding, a path (`a.b`), a call result (`f()`) or `default`.
    local_value_aliases: Vec<(String, String)>,
    local_class_exports: FxHashMap<String, LocalClassExportInfo>,
    subclass_declarations: FxHashMap<String, SubclassDeclaration>,
    direct_binding_values: Vec<DirectBindingValue>,
    capture_subclass_provenance: bool,
    capture_factory_properties: bool,
    factory_property_observations: FxHashMap<Span, (Span, String, Option<FactoryPropertyProof>)>,
    factory_property_result_observations: FxHashMap<Span, (String, Span)>,
    factory_completed_value_results: FxHashSet<Span>,
    factory_property_destinations: FxHashMap<Span, String>,
    factory_property_return_reads: FxHashMap<Span, Vec<(Span, String, Span)>>,
    factory_property_accesses: Vec<(FactoryPropertyProof, String, String)>,
    factory_property_sources:
        FxHashMap<Span, FxHashMap<String, fallow_types::extract::FactoryPropertySource>>,
    factory_property_classes: FxHashMap<Span, FxHashMap<String, BindingClassProof>>,
    independent_this_depth: usize,
    /// Whether the walk is in a `static` class method, where `this` is the
    /// class constructor and not an instance.
    in_static_method: bool,
    class_method_function_span: Option<Span>,
    independent_this: independent_this::IndependentThisState,
    this_result_locals: FxHashMap<String, Vec<visit_impl::visit_signature::ThisResultLocal>>,
    /// String `const` locals that a dynamic `import()` can name.
    const_specifiers: visit_impl::visit_dynamic_imports::ConstSpecifierScopes,
    pending_subclass_member_accesses: Vec<PendingSubclassMemberAccess>,
    projected_member_accesses: Vec<ProjectedMemberAccess>,
    projection_retained_work: usize,
    possible_constructor_work: usize,
    active_constructor_function: Option<Span>,
    projection_exhausted: Cell<bool>,
    binding_requirements: RefCell<RequirementArena>,
    projection_unknown_value: Option<usize>,
    projection_array_returns: FxHashMap<Span, (usize, Span)>,
    subclass_binding_references: FxHashMap<Span, Span>,
    subclass_namespace_references: FxHashSet<Span>,
    subclass_immutable_references: FxHashSet<Span>,
    subclass_binding_writes: FxHashMap<Span, Vec<Span>>,
    builtin_container_references: FxHashSet<Span>,
    playwright_fixture_types: FxHashMap<String, Vec<(String, String)>>,
    block_depth: u32,
    function_depth: u32,
    /// True when the program top level is a component body, not module level.
    /// A framework compiles a Vue `<script setup>` block, a Svelte instance
    /// script and an Astro frontmatter into a setup or render function, so a
    /// top-level `await import()` there runs per instance, on demand.
    top_level_is_component_body: bool,
    /// True for a Vue or Svelte script block. An SFC script is never Angular, so
    /// a `@Component` decorator there (Vue class components) is not analyzed.
    pub(crate) sfc_script: bool,
    /// True while walking the immediate quasi of a tagged template. Tagged
    /// templates receive raw values, so interpolation should not credit
    /// `toString` coercion for the quasi itself.
    in_tagged_template_quasi: bool,
    /// The module binds `html` to its own value, so `` html`...` `` is plain
    /// text and not page markup.
    html_tag_is_project_value: bool,
    class_super_stack: Vec<Option<(String, Option<Span>)>>,
    /// Monotonic per-module class-scope id source: incremented each time a
    /// class body is entered so every class gets a unique id, paired with
    /// `class_scope_stack`. A `this.<field>` binding key is qualified with the
    /// enclosing class's id (`this@<id>.<field>`) during the walk so two
    /// classes declaring a same-named field do not collide in the module-flat
    /// `binding_target_names` map; the qualifier is stripped back to `this.`
    /// before ModuleInfo emission. See issue #1821 (Fix B).
    class_scope_counter: u32,
    /// Stack of active class-scope ids. The top is the enclosing class of the
    /// current walk position; `this.<field>` keys and receiver spellings are
    /// qualified with it. Empty at module scope, so module-level `this` stays
    /// unqualified (behavior unchanged). See issue #1821 (Fix B).
    class_scope_stack: Vec<u32>,
    pub(crate) inline_template_findings: Vec<InlineTemplateFinding>,
    side_effect_registered_class_names: FxHashSet<String>,
    lit_custom_element_candidates: Vec<LitCustomElementCandidate>,
    registered_custom_elements: Vec<fallow_types::extract::RegisteredCustomElement>,
    used_custom_element_tags: FxHashSet<String>,
    /// Auto-import candidates for the OG image templates that
    /// `defineOgImage('Name')` calls name. See [`crate::og_image`].
    og_image_template_candidates: Vec<String>,
    factory_call_candidates: Vec<FactoryCallCandidate>,
    /// Same-file functions whose body returns `new Class()`, mapped to the class
    /// name, plus the `const x = fn()` bindings to resolve against them. See #1441.
    factory_return_functions: FxHashMap<String, String>,
    /// Callees of a factory call destructured opaquely (`const { a, ...rest } = f()`,
    /// a computed key). The returned class must be credited wholesale.
    factory_whole_object_candidates: Vec<String>,
    /// `(callee, member, awaited)` for a factory result read without being named:
    /// `f().member`, `const { member } = await f()`.
    factory_unnamed_result_accesses: Vec<(String, String, bool)>,
    factory_return_candidates: Vec<FactoryReturnCandidate>,
    /// How many binding identifiers in the module declare each name. A
    /// destructured factory property credits only through a local name that
    /// the module binds once, because the member accesses are not scoped
    /// (issue #5298).
    binding_name_counts: FxHashMap<String, u32>,
    /// Same-file functions whose body returns a bare identifier (e.g.
    /// `useApi() { return api }`). Resolved against `binding_target_names` at
    /// finalize: a typed local (`let api: RESTApi`) promotes the function to a
    /// `factory_return_functions` entry, so `const x = useApi()` credits the
    /// class without tracing the assignment chain. See issue #1441 (var-return).
    factory_return_alias_functions: FxHashMap<String, String>,
    /// Same-file pass-through wrappers (`const setup = () => newService()`),
    /// keyed by wrapper name, valued by the callee the wrapper returns. A
    /// factory call candidate on a wrapper resolves through the callee
    /// (issue #5366).
    factory_pass_through_functions: FxHashMap<String, String>,
    /// Subset of factory-return functions (by local name) whose body provably
    /// returns a SINGLE class across ALL static return paths, the all-paths
    /// unanimity proof required before a factory may be exported as cross-module
    /// metadata. Stricter than `factory_return_functions` (which keeps the
    /// same-file last-return leniency). Only entries here become
    /// `ModuleInfo.exported_factory_returns`, bounding the cross-module
    /// over-credit blast radius. See issue #1441 (Part A).
    strict_factory_return_functions: FxHashMap<String, String>,
    /// The `async` counterpart of `strict_factory_return_functions`: factories
    /// proven to resolve to one class. Exported with `is_async`, so only an
    /// awaited cross-module call result credits the class.
    strict_async_factory_return_functions: FxHashMap<String, String>,
    /// Module-scope `async` (non-generator) functions. Their plain call result is
    /// a promise, so only an awaited result binds the class they resolve to.
    promise_factory_functions: FxHashSet<String>,
    /// Module-scope generator functions. Awaiting the call gives the iterator,
    /// so an awaited result never binds a class.
    generator_factory_functions: FxHashSet<String>,
    /// Factory functions returning an object literal, captured at visit time and
    /// resolved at finalize (`resolve_factory_return_object_shapes`). See #1858.
    factory_return_object_candidates: Vec<FactoryReturnObjectCandidate>,
    /// Resolved object-literal factory-return shapes, LOOSE (same-file) form:
    /// `fn_name -> [(property_path, class_local_name)]`. Feeds the same-file
    /// consumer arm in `resolve_factory_return_candidates`. See issue #1858.
    factory_return_object_shapes:
        FxHashMap<String, Vec<fallow_types::extract::FactoryReturnObjectProperty>>,
    /// Resolved object-literal factory-return shapes, STRICT (cross-module) form:
    /// only sync/non-generator/unanimous-terminal factories. Joined against exports
    /// into `ModuleInfo.exported_factory_return_object_shapes`. See issue #1858.
    strict_factory_return_object_shapes:
        FxHashMap<String, Vec<fallow_types::extract::FactoryReturnObjectProperty>>,
    /// Alias factory functions (by local name) whose body is eligible for STRICT
    /// (cross-module) promotion: it returns synchronously (not async/generator)
    /// and cannot fall through to `undefined` (terminal last statement). The
    /// same-file (loose) alias promotion does not require this. See #1441 (A).
    strict_alias_eligible: FxHashSet<String>,
    /// Module-scope identifier -> classified initializer right-hand sides, from
    /// MODULE-SCOPE declarators and MODULE-SCOPE assignment expressions
    /// (`let api = new RESTApi()`, `api = init()` at top level). A module-scope
    /// write runs at load and dominates any later call, usable as PROOF and
    /// checked for poison. See #1441 (Part A).
    module_scope_initializers: FxHashMap<String, Vec<FactoryAssignedValue>>,
    /// Identifier -> classified right-hand sides of EVERY assignment expression
    /// to it, in ANY scope (sibling functions, non-dominating branches, …). Used
    /// ONLY as a POISON input for the strict alias proof: a write to the returned
    /// module binding that is `Other`/unresolved/a conflicting class (e.g.
    /// `poison() { api = {} as any }`) makes the strict export abstain, since it
    /// can leave the binding holding a non-class at return time. Declarations
    /// (which introduce a separate binding) are intentionally excluded. See #1441
    /// (Part A).
    identifier_write_values: FxHashMap<String, Vec<FactoryAssignedValue>>,
    /// Alias factory function (by local name) -> classified right-hand sides of
    /// assignments to its RETURNED identifier, collected from that function's OWN
    /// body only (not nested functions). Ties the value-proof to the alias
    /// function itself, so an assignment in an unrelated/sibling function does not
    /// falsely prove it. See #1441 (Part A).
    alias_in_body_assignments: FxHashMap<String, Vec<FactoryAssignedValue>>,
    node_module_register_url_bindings: FxHashMap<String, Vec<String>>,
    child_process_fork_bindings: FxHashSet<String>,
    child_process_namespace_bindings: FxHashSet<String>,
    node_path_namespace_bindings: FxHashSet<String>,
    node_url_file_url_to_path_bindings: FxHashSet<String>,
    current_module_path_bindings: FxHashMap<String, declarations::ModulePathBinding>,
    child_process_fork_target_bindings: FxHashMap<String, Vec<String>>,
    static_string_bindings: FxHashMap<String, Vec<String>>,
    static_string_arrays: FxHashMap<String, Vec<String>>,
    static_object_property_values: FxHashMap<String, FxHashMap<String, Vec<String>>>,
    loop_string_bindings: Vec<FxHashMap<String, Vec<String>>>,
    loop_object_property_values: Vec<FxHashMap<String, FxHashMap<String, Vec<String>>>>,
    package_resolution_function_args: FxHashMap<String, usize>,
    /// Other local names of `createRequire` from `node:module`.
    create_require_aliases: FxHashSet<String>,
    /// Module-level bindings of `createRequire(<this file>)`.
    module_require_aliases: FxHashSet<String>,
    /// Per `nested_declaration_stack` frame: the bindings of
    /// `createRequire(<this file>)` declared there.
    scoped_require_aliases: Vec<FxHashSet<String>>,
    nested_declaration_stack: Vec<FxHashSet<String>>,
    /// Per `nested_declaration_stack` frame: the names declared there that
    /// provably hold no class instance, such as `const service = { m() {} }` or
    /// a parameter `service: number`.
    scoped_non_instance_bindings: Vec<FxHashSet<String>>,
    /// The reads in `member_accesses` whose receiver root is one of those
    /// shadows or a nested shadow of a module alias. `resolve_bound_member_accesses`
    /// does not resolve them through the module-flat class bindings.
    shadowed_reads: visit_impl::visit_value_alias::ShadowedReads,
    class_type_param_constraints: Vec<FxHashMap<String, Option<TypePropertyTarget>>>,
    pending_playwright_factory_calls: Vec<PendingPlaywrightFactory>,
    pending_playwright_factory_aliases: Vec<(String, String)>,
    /// Local `const X = base.extend<T>({...})` fixture definitions keyed by the
    /// const's local name. A helper wrapping `<X>.extend(...)` (issue #1791)
    /// inherits these bindings through `pending_playwright_factory_aliases`, even
    /// when the wrapping `.extend({})` carries no type argument of its own.
    playwright_local_fixture_defs: FxHashMap<String, Vec<(String, String)>>,
    /// Local consts that hold a Playwright test: `base.extend(...)` on the
    /// `@playwright/test` `test` import, a `mergeTests(...)` result, or an
    /// `.extend(...)` of another such const. The type argument of an
    /// `.extend<T>(...)` on one of these consts defines fixtures (issue #3462).
    playwright_local_tests: FxHashSet<String>,
    /// Local consts made by `.extend(...)` on a base that only analysis can
    /// prove: an import, or another const in this set. An `.extend<T>(...)`
    /// on one of them records conditional fixture definitions.
    playwright_conditional_tests: FxHashSet<String>,
    source_returning_helpers: FxHashMap<String, SourceReturningHelper>,
    /// The `Program::directives` (`"use client"`) for the `client-server-leak` rule.
    directives: Vec<String>,
    /// The `import()` starts of `next/dynamic(..., { ssr: false })`, which the leak cone skips.
    client_only_dynamic_import_spans: Vec<u32>,
    /// The non-public env reads, for the `client-server-leak` rule.
    env_reads: Vec<fallow_types::extract::EnvRead>,
    /// Captured security sink sites (category-blind). Consumed by the
    /// catalogue-driven `tainted_sink` detector.
    security_sinks: Vec<SinkSite>,
    /// Count of sink-shaped nodes whose callee is a security blind spot.
    security_sinks_skipped: u32,
    sink_aliases: visit_impl::SinkAliasState,
    /// Span-level diagnostics for skipped security sink callees.
    security_unresolved_callee_sites: Vec<SkippedSecurityCalleeSite>,
    /// Export names of the shallow-safe module-scope functions.
    inert_function_exports: Vec<String>,
    /// Local bindings tied to the member-access path they were sourced from
    /// (e.g. `const id = req.query.id`). Feeds the security `tainted_sink`
    /// source-to-sink association in the analyze layer.
    tainted_bindings: Vec<TaintedBinding>,
    /// Chain-hop depth per recorded tainted binding, aligned 1:1 with
    /// `tainted_bindings` (index `i` describes `tainted_bindings[i]`, so the
    /// depth is tracked per `(local, source_path)` pair, never approximated
    /// across a local's candidate paths). Hop 1 is a direct capture (source
    /// read, framework param, helper return, destructure-from-source); each
    /// #1146 chain step through another local binding adds 1, capped by
    /// `MAX_TAINT_BINDING_HOPS`. Working state only: NOT persisted in the
    /// extract cache and NOT carried across SFC script blocks (`merge_into`
    /// drops it together with this extractor's binding lookup, so a
    /// cross-block chain cannot form and the hop accounting cannot drift from
    /// the bindings it describes).
    tainted_binding_hops: Vec<u8>,
    /// Direct sink arguments recognized as sanitizer calls.
    sanitized_sink_args: Vec<SanitizedSinkArg>,
    /// Defensive control call sites for security surface output.
    security_control_sites: Vec<SecurityControlSite>,
    /// Statically flattenable callee paths, one entry per path and form with
    /// the start of every site. Consumed by the callee rules.
    callee_uses: Vec<CalleeUse>,
    /// Literal commands of process-spawning calls.
    command_invocations: Vec<CommandInvocation>,
    /// Call facts paired with the root reference span until semantic admission.
    pending_imported_call_sites: Vec<(Span, ImportedCallSite)>,
    /// Runtime ESM roots precollected before the body walk.
    runtime_import_locals: FxHashSet<String>,
    /// Imported calls admitted by semantic scope identity.
    imported_call_sites: Vec<ImportedCallSite>,
    /// Value references to runtime ESM import bindings, set by the semantic
    /// pass. `import_index` points into `imports`.
    import_binding_references: Vec<ImportBindingReference>,
    /// Call evidence of plain functions, set by the semantic pass.
    /// `CallTarget::Import` points into `imports`.
    function_contracts: fallow_types::extract::FunctionContractFacts,
    opaque_loads: semantic_calls::OpaqueLoadState,
    /// Index into `callee_uses` of the entry for each callee path and form.
    /// The second key element is the `is_new` form. Working state only: not
    /// persisted and not merged across SFC script blocks, so each block has
    /// its own entries.
    callee_use_index: FxHashMap<(String, bool), usize>,
    /// `"use client"` / `"use server"` directive strings written as expression
    /// statements in `program.body` (misplaced, NOT in the leading
    /// prologue), so the RSC bundler silently ignores them. Captured by
    /// `visit_program` and consumed by the `misplaced-directive` detector.
    misplaced_directives: Vec<MisplacedDirectiveSite>,
    /// Export LOCAL NAMES of exported functions / const-arrows whose body has an
    /// inline `"use server"` directive. Captured by `extract_declaration_exports`
    /// and consumed by the `unused-server-action` reclassifier. Only EXPORTED
    /// declarations are captured (the capture sits on the exported-declaration
    /// path), so a non-exported local function with a use-server body is never
    /// recorded; even if it were, the reclassifier only matches against
    /// unused-EXPORT names, so a stray name is inert.
    inline_server_action_exports: Vec<String>,
    /// Vue `provide`/`inject` and Svelte `setContext`/`getContext` call sites
    /// keyed by a stable identifier symbol. Consumed by the `unprovided-inject` detector.
    di_key_sites: Vec<DiKeySite>,
    /// `true` when a `provide`/`setContext` keyed by an unknowable key (a
    /// non-identifier, a spread, or a transient nested-scope local) was seen.
    /// Forces the `unprovided-inject` detector to abstain project-wide.
    has_dynamic_provide: bool,
    /// All-action `"use server"` module flag, set in `visit_program`.
    is_server_action_module: bool,
    /// Global-scope declaration flag, set in `visit_program`.
    has_global_declarations: bool,
    /// `/// <reference path>` values, set in `visit_program`.
    triple_slash_reference_paths: Vec<String>,
    /// Whether the program has a JSX element or fragment. The automatic JSX
    /// runtime adds its import only to such a file.
    has_jsx: bool,
    /// Whether a bundler or test config may select the JSX runtime for this
    /// file: it has JSX and no pragma that selects the runtime or its source.
    jsx_runtime_from_config: bool,
    /// Module-scope `const NAME = "literal"` names: a DI key bound to a string
    /// literal has STRING identity (a provider supplying the literal, often
    /// inside a package, matches it), so its `di_key_sites` are dropped at
    /// finalize. Working state, not persisted.
    string_keyed_di_consts: FxHashSet<String>,
    /// Module-scope default, namespace, or require bindings imported from
    /// DOMPurify-compatible packages.
    dompurify_bindings: FxHashSet<String>,
    /// Module-scope local helpers whose return value is a proven sanitizer
    /// output for a narrow sink domain.
    module_sanitizer_helpers: FxHashMap<String, SanitizerScope>,
    /// Module-scope local sanitizer bindings. `None` means the name is declared
    /// but not sanitizer-backed, shadowing any outer match.
    module_sanitizer_bindings: FxHashMap<String, Option<SanitizerScope>>,
    /// Nested lexical sanitizer binding stack for functions and blocks.
    sanitizer_binding_stack: Vec<FxHashMap<String, Option<SanitizerScope>>>,
    literal_keys: visit_impl::visit_computed_keys::LiteralKeyBindings,
    /// Module-scope literal-backed string allowlists; `false` shadows without trust.
    module_literal_allowlist_bindings: FxHashMap<String, bool>,
    /// Module-scope literal constants for security sink argument classification.
    module_static_sink_literals: FxHashMap<String, SinkLiteralValue>,
    /// Module-scope `const` locals bound to a trusted module-relative path.
    module_trusted_path_bindings: FxHashSet<String>,
    /// Nested lexical literal allowlist bindings.
    literal_allowlist_binding_stack: Vec<FxHashMap<String, bool>>,
    /// Module-scope locals initialized from risky literal regex patterns.
    /// `None` means the name shadows an outer risky regex but is not itself risky.
    module_risky_regex_bindings: FxHashMap<String, Option<String>>,
    /// Nested lexical risky regex bindings.
    risky_regex_binding_stack: Vec<FxHashMap<String, Option<String>>>,
    /// Module-scope locals initialized from a path sink call.
    module_path_sink_bindings: FxHashMap<String, Option<SecurityPathSinkBinding>>,
    /// Nested lexical locals initialized from a path sink call.
    path_sink_binding_stack: Vec<FxHashMap<String, Option<SecurityPathSinkBinding>>>,
    /// Module-scope `path.relative(base, resolved)` aliases.
    module_path_relative_bindings: FxHashMap<String, Option<String>>,
    /// Nested lexical `path.relative(base, resolved)` aliases.
    path_relative_binding_stack: Vec<FxHashMap<String, Option<String>>>,
    /// Harvested Pinia store members keyed by the store binding's local name
    /// (`state` / `getters` / `actions` keys, or setup-store returned keys).
    /// `enrich_store_exports` copies them onto the matching `ExportInfo`, the same
    /// shape as `enrich_local_class_exports`. Working state only; not persisted.
    store_member_decls: FxHashMap<String, Vec<MemberInfo>>,
    /// Locals bound to a store-factory call (`const s = useFooStore()`). Gates the
    /// destructure and `storeToRefs(s)` crediting so it never fires on a plain
    /// `new`-instance or object binding. Working state only.
    store_instance_locals: FxHashSet<String>,
    /// Span starts of store `$state` expressions that a key read, a destructure or
    /// a whole write consumes. Any other one marks the whole store as used.
    pinia_state_named_uses: FxHashSet<u32>,
    /// Local type aliases of the form `type X = ReturnType<typeof useFooStore>`
    /// mapped to the store factory name (`X -> useFooStore`). Lets a param typed
    /// as the store (`(s: X)` or `(props: { store: X })`) bind to the factory so
    /// `s.member` / `props.store.member` credit the store member through the
    /// existing `binding_target_names` remap (issue #1489 Case 2). Working state
    /// only; never persisted. An alias in a function body or block is a
    /// `FunctionTypeAliasBinding::Store` in `function_type_alias_scopes`.
    type_alias_store_factory: FxHashMap<String, String>,
    /// SvelteKit `load()` return-object keys harvested from a `load` export.
    /// Basename-gated to page-load producers in `parse.rs` (cleared for any
    /// non-`+page.{ts,server.ts,js,server.js}` file). Consumed by the
    /// `unused-load-data-key` detector.
    load_return_keys: Vec<fallow_types::extract::LoadReturnKey>,
    /// Which route-data producer names should be harvested for this file. Set by
    /// `parse.rs` before the AST walk because SvelteKit and conventional route
    /// modules use different export names but share the same cached field.
    route_load_harvest_mode: RouteLoadHarvestMode,
    /// `true` when a `load` export was seen whose body could not be harvested
    /// safely (spread/non-literal/multi-return/computed-key/wrapped). Forces the
    /// `unused-load-data-key` detector to abstain on the whole file.
    has_unharvestable_load: bool,
    /// `true` when this file passes the whole `data` binding opaquely
    /// (`const X = data`, `fn(data)` / `fn(...data)`). Name-gated on `data`.
    /// Consumed only by the `unused-load-data-key` detector (FP-1).
    has_load_data_whole_use: bool,
    /// Locals bound to `useLoaderData()`. Reads on these locals are mirrored to
    /// the reserved route-data marker for React Router and Remix route modules.
    /// Working state only, not persisted.
    route_loader_data_bindings: FxHashSet<String>,
    /// `true` when the parse is JSX-capable (`.jsx`/`.tsx`, or a `.js`/`.ts`
    /// file re-parsed through the JSX retry). Gates the React/JSX structural
    /// walk so it is a no-op on non-JSX files (perf: `audit` hot path on
    /// non-React repos must not regress). Set by `parse.rs` after construction.
    pub(crate) jsx_capable: bool,
    /// React component definitions captured during the JSX walk. Empty unless
    /// `jsx_capable`.
    component_functions: Vec<ComponentFunction>,
    /// React component props (reuses `ComponentProp`; `used_in_template` always
    /// false, `used_in_script` = used-in-body). Empty unless `jsx_capable`.
    react_props: Vec<ComponentProp>,
    /// React hook call sites. Empty unless `jsx_capable`.
    hook_uses: Vec<HookUse>,
    /// React render edges (child name captured; resolution deferred to graph
    /// build). Empty unless `jsx_capable`.
    render_edges: Vec<RenderEdge>,
    /// Stack of enclosing React component names, pushed when a component
    /// function/arrow is entered and popped on exit. The top is the
    /// `parent_component` for any render edge or hook captured inside.
    component_stack: Vec<String>,
    /// Pending React-component metadata for a named arrow / function-expression
    /// binding, keyed by the arrow/function span. Populated in
    /// `visit_variable_declaration` BEFORE the walk descends into the init, then
    /// consumed by `visit_arrow_function_expression` /
    /// `visit_function` to push the component stack with the binding name. Working
    /// state only (not persisted, not merged across SFC blocks).
    pending_component_arrows: FxHashMap<Span, PendingComponentArrow>,
    /// Same-file object-type declarations eligible to back a React component's
    /// bare-identifier typed props param (`(props: Props) => ...`). Maps the
    /// type/interface name to its `(prop_name, span_start)` members, populated
    /// ONLY for a plain object shape: an `interface X { ... }` with no `extends`
    /// and no type parameters, or a `type X = { ... }` whose annotation is a bare
    /// object type literal with no type parameters. An `extends` / intersection /
    /// generic / mapped / imported type never enters this map, so a pending typed
    /// props harvest that misses it abstains (ADR-001, zero-FP). Working state,
    /// not persisted.
    react_object_type_props: FxHashMap<String, Vec<(String, u32)>>,
    /// React components whose first param is a bare identifier with a
    /// same-file-resolvable object-type annotation, deferred to finalize because
    /// the backing interface/type may hoist (be declared after the component).
    /// The `props.<name>` member-access usage is computed at capture time (the
    /// body is in hand then); the prop-name SET is resolved in finalize against
    /// `react_object_type_props`. Working state, not persisted.
    pending_typed_react_props: Vec<PendingTypedReactProps>,
    /// Angular component/directive inputs harvested from Angular-decorated
    /// classes (`@Input()` decorators and signal `input()` / `model()`
    /// initializers). Accumulated across every Angular class in the module and
    /// copied onto `ModuleInfo.angular_inputs`. Consumed by the
    /// `unused-component-input` detector.
    angular_inputs: Vec<AngularInputMember>,
    /// Angular component/directive outputs harvested from Angular-decorated
    /// classes (`@Output()` decorators and signal `output()` /
    /// `outputFromObservable()` initializers). Accumulated across every Angular
    /// class in the module and copied onto `ModuleInfo.angular_outputs`. Consumed
    /// by the `unused-component-output` detector.
    angular_outputs: Vec<AngularOutputMember>,
    /// Spans of Angular classes already harvested into `angular_inputs` /
    /// `angular_outputs`. An `export class FooComponent` is visited by both the
    /// named-export declaration path and the top-level class-declaration path, so
    /// this dedups the harvest and prevents one declared input/output from being
    /// flagged twice.
    harvested_angular_class_spans: FxHashSet<Span>,
    /// Angular `@Component` declarations with their `selector` value(s), harvested
    /// from `@Component({ selector })` decorators. Accumulated across every
    /// Angular component class in the module and copied onto
    /// `ModuleInfo.angular_component_selectors`. Consumed by the Angular arm of
    /// the `unrendered-component` detector.
    angular_component_selectors: Vec<AngularComponentSelector>,
    /// Custom element selector tags referenced in this file's Angular templates
    /// (inline `template:` blocks). External `templateUrl` `.html` files are
    /// scanned separately when that file is parsed. Copied onto
    /// `ModuleInfo.angular_used_selectors`.
    angular_used_selectors: Vec<String>,
    /// Angular component class names referenced as a route entry or bootstrap
    /// target (route `component:` / `loadComponent`, `bootstrapApplication` /
    /// `bootstrap: [...]`). Copied onto `ModuleInfo.angular_entry_component_refs`
    /// (the Angular `unrendered-component` entry-point abstain).
    angular_entry_component_refs: Vec<String>,
    /// `true` when a dynamic component render was seen
    /// (`*.createComponent(<ident>)`). Copied onto
    /// `ModuleInfo.has_dynamic_component_render` (the Angular
    /// `unrendered-component` project-wide abstain).
    has_dynamic_component_render: bool,
    /// Local binding names bound from `const dispatch = createEventDispatcher()`
    /// (where `createEventDispatcher` is imported from `svelte`). A
    /// `dispatch('<name>')` call through one of these bindings records a
    /// `DispatchedEvent`. Working state, then copied as the gate for the dispatch
    /// harvest. Not persisted.
    event_dispatch_bindings: FxHashSet<String>,
    /// Svelte custom events dispatched via `dispatch('<name>')` (literal arg).
    /// Copied onto `ModuleInfo.svelte_dispatched_events`. Consumed by the
    /// `unused-svelte-event` detector.
    svelte_dispatched_events: Vec<DispatchedEvent>,
    /// `true` when a `dispatch(<nonLiteral>)` call was seen, or a `dispatch`
    /// binding was used as a whole value (passed / returned). Forces the
    /// `unused-svelte-event` detector to abstain on the whole component.
    has_dynamic_dispatch: bool,
    /// The script binding context and the emit calls of a markup snippet.
    markup: visit_impl::MarkupContext,
}

/// Metadata for a named arrow / function-expression that may be a React
/// component, captured at the declarator before the function body is walked.
#[derive(Debug, Clone)]
struct PendingComponentArrow {
    /// The binding name.
    name: String,
    /// The component kind (`Arrow`, or a `forwardRef` / `memo` wrapper).
    kind: fallow_types::extract::ComponentFunctionKind,
    /// Whether the binding is exported.
    is_exported: bool,
    /// For a `forwardRef<Ref, Props>((props, ref) => ...)` wrapper, the bare
    /// single-name SECOND generic type argument (`Props`). The inner render
    /// function's `props` param carries no annotation in this shape, so the props
    /// type lives on the wrapper call's type arguments instead. Resolved against
    /// `react_object_type_props` in finalize, exactly like an inline
    /// `(props: Props)` annotation. `None` for every non-generic / unresolvable
    /// shape (the inner param's own annotation, if any, still wins).
    props_type_name: Option<String>,
}

/// A React component whose first param is a bare identifier carrying a
/// same-file object-type annotation (`(props: Props) => ...`). Captured during
/// the body walk so `props.<name>` member-access usage is recorded against the
/// props local while the body is in hand; the prop-name set is resolved in
/// finalize because the backing interface/type may be declared after the
/// component (TypeScript hoists type declarations).
#[derive(Debug, Clone)]
struct PendingTypedReactProps {
    /// The enclosing component name (`ComponentProp.component`).
    component: String,
    /// The bare-identifier props parameter local (e.g. `props`).
    props_local: String,
    /// The annotation type name to resolve against `react_object_type_props`.
    type_name: String,
    /// Prop names read via `<props_local>.<name>` member access (or via a
    /// `const { name } = props` destructure local) anywhere in the body. A name
    /// in this set is credited `used_in_script = true`.
    member_uses: FxHashSet<String>,
    /// `true` when the props binding is consumed as a whole object (passed to a
    /// call/hook, spread, returned, or assigned). The prop set is then opaque, so
    /// the whole component abstains (`has_unharvestable_props`).
    has_whole_object_use: bool,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SecurityPathSinkBinding {
    span_start: u32,
    arg_count: u32,
}

#[cfg(all(test, not(miri)))]
mod tests;
