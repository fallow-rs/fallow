//! Dependency injection keys, component props, emits, events, inputs and
//! outputs, custom elements, load return keys, component functions, hooks,
//! render edges, and forwarded attributes.

/// Which side of a dependency-injection link a call site represents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum DiRole {
    /// `provide(KEY, value)` / `app.provide(KEY, value)` / `setContext(KEY, value)`.
    Provide,
    /// `inject(KEY)` / `getContext(KEY)`.
    Inject,
}

/// Which framework's DI API a call site came from (drives the finding message).
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum DiFramework {
    /// Vue `provide` / `inject` (from `vue` / `@vue/runtime-core`).
    Vue,
    /// Svelte `setContext` / `getContext` (from `svelte`).
    Svelte,
    /// Angular `inject(TOKEN)` / `@Inject(TOKEN)` (from `@angular/core`),
    /// matched against `{ provide: TOKEN, ... }` provider objects.
    Angular,
}

/// A Vue `provide`/`inject` or Svelte `setContext`/`getContext` call site keyed
/// by an identifier symbol. The `key_local` is resolved at analyze time through
/// the consuming module's import/export tables to a canonical defining-site
/// export key, so a provide and an inject of the same shared symbol unify even
/// across barrel re-exports. Consumed by the `unprovided-inject` detector.
#[derive(Debug, Clone, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub struct DiKeySite {
    /// The key identifier as written at the call site.
    pub key_local: String,
    /// Whether this is a provide or an inject.
    pub role: DiRole,
    /// Which framework's API this came from.
    pub framework: DiFramework,
    /// Start byte offset of the call expression (anchors the finding).
    pub span_start: u32,
}

/// A component prop declared by Vue `<script setup>` `defineProps` or Svelte 5
/// `$props()`. `used_in_script` / `used_in_template` are set during extraction;
/// the `unused-component-prop` detector flags a prop where neither is true. See
/// `harvest_define_props` and `harvest_svelte_props` in `sfc_props.rs`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct ComponentProp {
    /// The declared prop name.
    pub name: String,
    /// The template/script-visible local binding name: the destructure alias for
    /// `const { name: alias } = defineProps()` or
    /// `let { name: alias } = $props()`, otherwise the prop name itself. A
    /// renamed prop is read through this local, so usage must be checked against
    /// it, not the declared name.
    pub local: String,
    /// Start byte offset of the prop declaration (anchors the finding).
    pub span_start: u32,
    /// Whether this prop is referenced in the component's `<script>` (a
    /// destructured local binding with a resolved reference, or a `props.<name>`
    /// member access). For React, this is set-in-body: a resolved reference to the
    /// destructured local anywhere in the component function body.
    pub used_in_script: bool,
    /// Whether this prop name is referenced in the component's `<template>`.
    /// Set by `apply_template_usage` when the template scanner credits the name.
    /// Always false for React (no template; React uses `used_in_script`).
    pub used_in_template: bool,
    /// The enclosing component name. Empty for Vue SFCs (one component per file,
    /// the file stem is the component, set by the detector). For React this is the
    /// component function/arrow name a prop was declared on, so the detector can
    /// emit the right `component_name` and apply the per-component abstain ladder
    /// (a file can declare several React components).
    pub component: String,
    /// React-only: `true` when the destructured prop local is referenced at least
    /// once OUTSIDE a child-JSX attribute value expression (a substantive
    /// consumption: a hook arg, a host-element child, a non-JSX-attr read). When
    /// `used_in_script` is true but this is false, the prop is referenced ONLY as
    /// the root of forwarded child attribute values, i.e. a pure pass-through.
    /// Always `false` for Vue (no forward-vs-consume distinction is computed).
    pub used_outside_forward: bool,
}

/// A Vue `<script setup>` `defineEmits` declared event, harvested from the type
/// tuple-call form (`defineEmits<{ (e: 'foo'): void }>()`), the type object form
/// (`defineEmits<{ foo: [x: string] }>()`), or the runtime array form
/// (`defineEmits(['foo'])`). `used` is set during extraction when the bound emit
/// name is called as `emit('<name>')`. The `unused-component-emit` detector flags
/// an event where `used` is false. See `harvest_define_emits` in `sfc_props.rs`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct ComponentEmit {
    /// The declared emit event name.
    pub name: String,
    /// Start byte offset of the emit declaration (anchors the finding).
    pub span_start: u32,
    /// Whether this event is emitted via `emit('<name>')` somewhere in the
    /// component's `<script>`.
    pub used: bool,
}

/// A Svelte custom event dispatched via `dispatch('<name>')`, where `dispatch`
/// is the binding from a `const dispatch = createEventDispatcher()` call. Only
/// literal-first-arg dispatches are recorded; a `dispatch(<nonLiteral>)` sets
/// `ModuleInfo::has_dynamic_dispatch` instead. Consumed by the
/// `unused-svelte-event` detector, which flags an event dispatched here but
/// listened to nowhere project-wide (the cross-file dead-output direction). The
/// span is a byte offset (not an `oxc_span::Span`) so the type round-trips
/// through the bitcode cache directly, mirroring `ComponentEmit::span_start`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct DispatchedEvent {
    /// The dispatched event name (the literal first argument).
    pub name: String,
    /// Start byte offset of the `dispatch(...)` call (anchors the finding).
    pub span_start: u32,
}

/// A declared Angular component/directive input, harvested from an `@Input()`
/// decorator or a signal `input()` / `input.required()` / `model()` initializer
/// on an Angular-decorated class. Consumed by the `unused-component-input`
/// detector, which flags an input read nowhere in its own component (neither the
/// template nor the class body). The span is stored as a byte offset (not an
/// `oxc_span::Span`) so the type is cheap to mirror onto the cache, matching
/// `ComponentEmit::span_start`. `ModuleInfo` is not serialized, so no serde
/// attrs are derived here. `bitcode` derives let the type be mirrored directly
/// onto `CachedModule` (the same pattern as `ComponentEmit`).
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct AngularInputMember {
    /// The declared input name (the property key).
    pub name: String,
    /// Start byte offset of the property key (anchors the finding).
    pub span_start: u32,
}

/// A declared Angular component/directive output, harvested from an `@Output()`
/// decorator or a signal `output()` / `outputFromObservable()` initializer on an
/// Angular-decorated class. Consumed by the `unused-component-output` detector,
/// which flags an output emitted nowhere in its own component. A `model()` is an
/// input and a framework-driven output, so it is recorded ONLY as an input and
/// never appears here (the implicit `update:` emit is framework-managed). The
/// span is a byte offset for the same reason as `AngularInputMember`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct AngularOutputMember {
    /// The declared output name (the property key).
    pub name: String,
    /// Start byte offset of the property key (anchors the finding).
    pub span_start: u32,
}

/// A declared Angular `@Component` and its `selector` value(s), harvested from a
/// `@Component({ selector: '...' })` decorator. Consumed by the Angular arm of
/// the `unrendered-component` detector, which flags a component whose every
/// element selector is used in NO template project-wide (and that is not
/// referenced by class name anywhere, e.g. routed / bootstrapped / dynamically
/// rendered). A multi-selector string (`'app-foo, [appBar]'`) is split into the
/// `selectors` list. The span is stored as a byte offset (not an
/// `oxc_span::Span`) so the type round-trips through the bitcode cache directly,
/// mirroring `AngularInputMember::span_start`. `@Directive` is intentionally NOT
/// harvested here (directives have no template render). `ModuleInfo` is not
/// serialized, so no serde attrs are derived.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct AngularComponentSelector {
    /// The declared selector strings for this component, split on `,`. A purely
    /// element-selector component has only `app-foo`-shaped entries; attribute
    /// (`[appFoo]`) and class (`.foo`) selectors are retained verbatim so the
    /// detector can abstain when ANY non-element selector is present.
    pub selectors: Vec<String>,
    /// Start byte offset of the component class declaration (anchors the
    /// finding).
    pub span_start: u32,
    /// The component class name (used to credit routed / bootstrapped / dynamic
    /// class-name references project-wide).
    pub class_name: String,
}

/// A Lit / web-component custom element registered in a module via
/// `@customElement('x-foo')` or `customElements.define('x-foo', C)`. Consumed by
/// the Lit arm of the `unrendered-component` detector. The span is stored as a
/// byte offset (not an `oxc_span::Span`) so the type round-trips through the
/// bitcode cache directly, mirroring `AngularComponentSelector::span_start`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct RegisteredCustomElement {
    /// The registered custom-element tag name (`x-foo`).
    pub tag: String,
    /// The registering class's local name, used for the public-API / export
    /// abstain (an exported / published element is rendered by a downstream
    /// consumer the scan cannot see). Empty for an anonymous
    /// `export default @customElement('x-foo') class extends LitElement {}`.
    pub class_local_name: String,
    /// Start byte offset of the registering class declaration (anchors the
    /// finding at the element, NOT line 1, since a `.ts` file can register
    /// several custom elements).
    pub span_start: u32,
}

/// A key returned from a SvelteKit route `load()` function's terminal return
/// object literal. Harvested from `+page.{ts,server.ts,js,server.js}` files
/// exporting a `load` function. Consumed by the `unused-load-data-key` detector,
/// which flags a key read by no consumer. The span is stored as byte offsets
/// (not an `oxc_span::Span`) so the type round-trips through the bitcode cache
/// directly, mirroring `DiKeySite::span_start` / `ComponentEmit::span_start`.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode, PartialEq, Eq)]
pub struct LoadReturnKey {
    /// The returned-object property key name.
    pub name: String,
    /// Start byte offset of the key (anchors the finding).
    pub span_start: u32,
    /// End byte offset of the key.
    pub span_end: u32,
}

/// The syntactic shape of an identified React component definition. Drives the
/// abstain ladder later phases apply: a `forwardRef` / `memo` wrapper whose
/// props come from an imported interface fallow cannot resolve must abstain
/// (ADR-001), not guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum ComponentFunctionKind {
    /// A `function Foo() { return <.../> }` declaration.
    FnDecl,
    /// A `const Foo = () => <.../>` arrow (or function-expression) binding.
    Arrow,
    /// A `const Foo = forwardRef((props, ref) => <.../>)` wrapper.
    ForwardRefWrapper,
    /// A `const Foo = memo((props) => <.../>)` wrapper.
    MemoWrapper,
}

/// An identified React component: a function/arrow whose body returns JSX.
/// Captured by `visit_jsx_element`'s enclosing-component tracking. The
/// `unused-component-prop` (React arm) and complexity-fold phases consume this;
/// the abstain flags keep zero-FP on the cases ADR-001 cannot resolve.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct ComponentFunction {
    /// The component name (the binding or declaration identifier).
    pub name: String,
    /// Start byte offset of the component definition (anchors findings).
    pub span_start: u32,
    /// The syntactic shape of the definition.
    pub kind: ComponentFunctionKind,
    /// Whether the component is exported from its module (a named export, a
    /// `export default`, or re-exported in the same module). Public-API
    /// components abstain in the prop phase.
    pub is_exported: bool,
    /// `true` when the component's props are not statically harvestable: a
    /// rest/spread in the signature (`{ ...rest }`), props passed wholesale to a
    /// hook/helper, or a `forwardRef` / `memo` wrapper whose props come from an
    /// imported interface generic fallow cannot resolve (ADR-001). The prop
    /// phase abstains on the whole component when set.
    pub has_unharvestable_props: bool,
    /// `true` when the component body calls `cloneElement` / `React.cloneElement`.
    /// `cloneElement` injects props by reflection, so the static forward-set is
    /// incomplete; the prop-drilling phase abstains on any chain through this
    /// component (ADR-001, zero-FP).
    pub uses_clone_element: bool,
    /// `true` when the component renders a `*.Provider` member-expression tag
    /// (`<FooContext.Provider>`). A context provider in the subtree means the
    /// drilling may be a deliberate non-context choice (or the prop is about to
    /// be provided); the prop-drilling phase downgrades/abstains.
    pub renders_provider: bool,
    /// `true` when the component passes a function as a child render value
    /// (render-props / children-as-function: `<Foo>{() => ...}</Foo>` or
    /// `<Foo render={() => ...}/>`). The forwarded shape is dynamic; the
    /// prop-drilling phase abstains on chains through this component.
    pub has_children_as_function: bool,
    /// `true` when the component body is pure structural indirection: a single
    /// statement returning exactly one capitalized/member-expression JSX element
    /// (no host wrapper, no extra children, optionally a fragment wrapping a
    /// single element) that forwards props via a bare spread of the component's
    /// own props binding / rest local (`<Child {...props}/>`), with NO named
    /// attributes alongside the spread and NO self-render. The cross-component
    /// `thin-wrapper` phase joins this with hook-density / cyclomatic checks and
    /// the resolved single render edge to flag a component that is a candidate
    /// for inlining. Computed from the component's own AST only, so it caches
    /// byte-identity-safe (ADR-001).
    pub is_pure_passthrough: bool,
}

/// The kind of a React hook call. `Custom` covers any `use*`-named call that is
/// not one of the built-in hooks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, bitcode::Encode, bitcode::Decode)]
pub enum HookUseKind {
    /// `useState(...)`.
    UseState,
    /// `useEffect(...)`.
    UseEffect,
    /// `useMemo(...)`.
    UseMemo,
    /// `useCallback(...)`.
    UseCallback,
    /// Any other `use*`-named call (a custom hook).
    Custom,
}

/// A React hook call site inside a component. Consumed by the complexity-fold
/// phase (hook density) and surfaced as descriptive hotspot context.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct HookUse {
    /// The hook kind.
    pub kind: HookUseKind,
    /// The dependency-array arity, recorded ONLY when a literal array is present
    /// at the dependency-array position (`[a, b]` -> `Some(2)`, `[]` ->
    /// `Some(0)`). `None` when the call has no dependency array argument or the
    /// argument is not a literal array (ADR-001: do not guess).
    pub dep_array_arity: Option<u32>,
    /// Start byte offset of the hook call (anchors findings).
    pub span_start: u32,
    /// The enclosing component name (the top of the visitor's component stack
    /// when the hook call was recorded). Lets the descriptive per-component hook
    /// summary attribute hooks exactly even when a file declares several
    /// components. A hook recorded outside any component carries an empty string
    /// (the visitor only records hooks inside a component, so this is the
    /// rare top-level / unattributed case).
    pub component: String,
}

/// A render edge: one component rendering another (a capitalized or
/// member-expression JSX tag). Captured at extraction time with the child's
/// written name; resolution of `child_component_name` to a `FileId`/export is
/// deferred to graph build via the existing import map.
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct RenderEdge {
    /// The name of the component that renders the child (the enclosing
    /// component). Empty when the JSX is not inside an identified component (a
    /// top-level render expression).
    pub parent_component: String,
    /// The rendered child component name as written (`Foo` or the full
    /// member-expression path `Foo.Bar`).
    pub child_component_name: String,
    /// The attribute (prop) names passed at the render site, in source order.
    pub attr_names: Vec<String>,
    /// `true` when the render site contains a JSX spread (`{...x}`), so the
    /// passed-prop set is not statically complete.
    pub has_spread: bool,
    /// The forwarded attributes at this render site: each pairs the child
    /// attribute NAME with the identifier ROOT of its value expression
    /// (`userName={user.name}` -> `{ attr: "userName", root: "user" }`;
    /// `value={x}` -> `{ attr: "value", root: "x" }`). ONLY plain identifier or
    /// member-root access values are recorded (`{x}`, `{x.y}`, `{x.y.z}`); a value
    /// that is a call, an arrow/function, a conditional, a JSX element, or any
    /// other complex expression is NOT recorded here (its root would not be a pure
    /// forward) and sets `has_complex_forward` instead. The prop-drilling chain
    /// walk uses this pairing to map "this component forwards prop P" to "the
    /// child receives it as attribute A".
    pub forward_attrs: Vec<ForwardAttr>,
    /// `true` when at least one attribute value at this render site is a complex
    /// expression (a call, an arrow/function render-prop, a conditional, a JSX
    /// element-as-prop, a template literal, etc.) whose identifier root was NOT
    /// recorded in `forward_attrs`. The prop-drilling phase abstains on a chain
    /// whose forwarded prop flows through such a value (ADR-001, zero-FP).
    pub has_complex_forward: bool,
}

/// One forwarded JSX attribute: the child attribute name plus the identifier
/// root of its value expression. See [`RenderEdge::forward_attrs`].
#[derive(Debug, Clone, bitcode::Encode, bitcode::Decode)]
pub struct ForwardAttr {
    /// The child attribute (prop) name as written (`userName`).
    pub attr: String,
    /// The identifier root of the attribute value expression (`user` for
    /// `userName={user.name}`).
    pub root: String,
}
