//! Framework findings: injections, server actions, load data keys,
//! components, props, emits, events, effect layers, tagged errors, prop
//! drilling, thin wrappers, prop shapes, inputs, outputs and routes.

use super::*;

/// A Vue `inject(KEY)` or Svelte `getContext(KEY)` whose symbol KEY is
/// `provide`/`setContext`'d nowhere in the analyzed project. The key is a
/// symbol with cross-file identity, so an unmatched key is a real dead-half DI
/// link: at runtime the inject returns `undefined`, surfaced only at render.
/// The fix is binary: provide the key somewhere, or remove the dead inject.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnprovidedInject {
    /// The file carrying the orphan inject / getContext call.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The injected key identifier as written at the call site.
    pub key_name: String,
    /// Which framework's DI API this came from: `"vue"` or `"svelte"`.
    pub framework: String,
    /// 1-based line number of the inject / getContext call.
    pub line: u32,
    /// 0-based byte column offset of the inject / getContext call.
    pub col: u32,
}

/// A Next.js Server Action (an export of a `"use server"` file) that no code in
/// the analyzed project references: no import-and-call, no `action={fn}` JSX
/// binding, no `<form action={fn}>`. This is the cross-graph "declared but zero
/// consumers" direction, reclassified out of `unused-export` for `"use server"`
/// files so the finding carries the action-specific signal. It does NOT mean the
/// endpoint is unreachable: Next still registers the action id, so it stays
/// POST-able. It means no project code calls it (likely forgotten / dead, and a
/// candidate for removal to shrink surface area).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedServerAction {
    /// The `"use server"` file that exports the unreferenced action.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The exported action name as written, or `"default"` for a default export.
    pub action_name: String,
    /// 1-based line number of the export.
    pub line: u32,
    /// 0-based byte column offset of the export.
    pub col: u32,
}

/// A SvelteKit `+page.{ts,server.ts,js,server.js}` `load()` return-object key
/// read by no consumer: not off the sibling `+page.svelte`'s `data.<key>`, nor
/// project-wide via `page.data.<key>` / `$page.data.<key>`. A dead load key runs
/// a real server/DB fetch cost on every request for data nothing renders. The
/// fix is a human call (delete the key, or wire a consumer): a load fetch may
/// have side effects, so there is no safe auto-fix.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedLoadDataKey {
    /// The producer `+page.{ts,server.ts,js,server.js}` file declaring the key.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The returned-object key name read by no consumer.
    pub key_name: String,
    /// 1-based line number of the key in the return object.
    pub line: u32,
    /// 0-based byte column offset of the key.
    pub col: u32,
    /// The route directory relative to the project root (`src/routes/blog`), for
    /// agent remediation and per-route trend aggregation. `None` when not
    /// determinable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route_dir: Option<String>,
}

/// A Vue/Svelte single-file component (the default export of a `.vue`/`.svelte`
/// file) that is reachable in the module graph but rendered NOWHERE in the
/// project: no `<Tag>`, no `:is`/`this=` binding, no `components`/`app.component`
/// registration, no `h()`/auto-import use, and no script value-read. It survives
/// `unused-file` (a barrel re-export keeps it reachable) and `unused-export`
/// (the re-export counts as a use), yet no file actually instantiates it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnrenderedComponent {
    /// The component file that is reachable but rendered nowhere.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The component name. For `"vue"` / `"svelte"` / `"astro"` this is the SFC
    /// file stem (PascalCase); for `"angular"` it is the component class name; for
    /// `"lit"` it is the registered custom-element TAG (e.g. `x-foo`), not a file
    /// stem. Use `path` to anchor the file across all frameworks.
    pub component_name: String,
    /// Which framework this component belongs to: `"vue"`, `"svelte"`, `"astro"`,
    /// `"angular"`, or `"lit"`.
    pub framework: String,
    /// A barrel/file that re-exports this component, kept for the remediation
    /// trace ("reachable via X, rendered nowhere"). Absolute in memory,
    /// serialized workspace-relative (like `path`); `None` when not determinable.
    #[serde(
        serialize_with = "serde_path::serialize_option",
        skip_serializing_if = "Option::is_none"
    )]
    pub reachable_via: Option<PathBuf>,
    /// 1-based line number of the component (the file head; SFCs have no explicit
    /// default-export statement).
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
}

/// A Vue `<script setup>` `defineProps`, Svelte 5 `$props()`, or React declared
/// prop that is referenced NOWHERE inside its own component. Single-component
/// finding, zero-FP doctrine: the component abstains on any opaque public or
/// fallthrough signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedComponentProp {
    /// The component file declaring the unused prop.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The component name.
    pub component_name: String,
    /// The declared prop name that is never referenced.
    pub prop_name: String,
    /// 1-based line number of the prop declaration.
    pub line: u32,
    /// 0-based byte column offset of the prop declaration.
    pub col: u32,
}

/// A Vue `<script setup>` `defineEmits` declared event that is EMITTED nowhere
/// inside its own single-file component (no `emit('<name>')` call). Single-file
/// finding, zero-FP doctrine: the whole file abstains on any
/// unharvestable / dynamic-emit / whole-object-use / `defineModel` signal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedComponentEmit {
    /// The `.vue` SFC declaring the unused emit.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The component name (the `.vue` file stem).
    pub component_name: String,
    /// The declared emit event name that is never emitted.
    pub emit_name: String,
    /// 1-based line number of the emit declaration.
    pub line: u32,
    /// 0-based byte column offset of the emit declaration.
    pub col: u32,
}

/// A Svelte component dispatching a custom event via `createEventDispatcher()`
/// whose event name is listened to NOWHERE in the analyzed project. Cross-file
/// dead-output direction: the component fires an event nothing handles.
/// Zero-FP doctrine: the whole component abstains on any dynamic-dispatch or
/// whole-`dispatch`-value signal, and a listener on ANY component anywhere
/// credits the event name (the liberal over-credit direction).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedSvelteEvent {
    /// The `.svelte` component dispatching the unlistened event.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The component name (the `.svelte` file stem).
    pub component_name: String,
    /// The dispatched event name that is listened to nowhere.
    pub event_name: String,
    /// 1-based line number of the `dispatch('<name>')` call.
    pub line: u32,
    /// 0-based byte column offset of the `dispatch('<name>')` call.
    pub col: u32,
}

/// Kind of an Effect layer definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum EffectLayerKind {
    /// A module-scope `const` whose initializer is a layer.
    TopLevel,
    /// A class static property whose initializer is a layer.
    StaticMember,
}

/// An Effect layer whose every reference sits inside other layers that
/// nothing in the project uses. The layer is reachable only through a dead
/// composition, so it never reaches a runtime. A layer with zero references
/// is not reported here: `unused-export` or `unused-class-member` owns it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedEffectLayer {
    /// The module that defines the layer.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// `FooLive`, or `FooService.layer` for a static member.
    pub name: String,
    /// Whether the layer is a module-scope const or a class static member.
    pub kind: EffectLayerKind,
    /// 1-based line number of the layer name.
    pub line: u32,
    /// 0-based byte column offset of the layer name.
    pub col: u32,
    /// The unused layers that compose this layer, sorted by name.
    pub dead_consumers: Vec<String>,
}

/// A source location of a `catchTag` or `catchTags` handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EffectCatchSite {
    /// The module that contains the handler.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line number of the handled tag.
    pub line: u32,
    /// 0-based byte column offset of the handled tag.
    pub col: u32,
}

/// An Effect tagged error class that type positions or `catchTag` handlers
/// reference, but that no code constructs, fails with, or reads as a value.
/// Any value reference counts as a producer, including a schema union or an
/// API error declaration that decodes the error at a boundary.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedTaggedError {
    /// The module that declares the class.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The class name.
    pub name: String,
    /// The `_tag` value of the class.
    pub tag: String,
    /// 1-based line number of the class name.
    pub line: u32,
    /// 0-based byte column offset of the class name.
    pub col: u32,
    /// The `catchTag` / `catchTags` handlers that name the tag.
    pub catch_sites: Vec<EffectCatchSite>,
}

/// A `catchTag("X")` / `catchTags({ X })` handler whose tag `X` belongs to
/// project classes that no code produces. Reported only when no
/// `unused-tagged-error` finding covers those classes (the rule is off, or
/// the class finding is suppressed).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnproducedCatchTag {
    /// The module that contains the handler.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The handled tag.
    pub tag: String,
    /// 1-based line number of the handled tag.
    pub line: u32,
    /// 0-based byte column offset of the handled tag.
    pub col: u32,
    /// The project classes that declare the tag, sorted by name.
    pub candidate_classes: Vec<String>,
}

/// One hop in a prop-drilling chain: a component that received the prop and
/// passed it along (or, at the chain ends, the source that owns it and the
/// consumer that substantively reads it).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PropDrillHop {
    /// The file containing this hop's component.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// 1-based line of the component definition (or the prop declaration at the
    /// source hop). Anchors a jump-to-source for the agent.
    pub line: u32,
    /// The component name at this hop.
    pub component: String,
}

/// A located prop-drilling chain: a received prop forwarded unchanged through
/// `>= N` intermediate pass-through components, each of which only re-passes it,
/// until a component that substantively consumes it. The high-confidence signal
/// is "the received identifier is used ONLY as the root of forwarded child-JSX
/// attribute values", not the attribute name matching. Health signal (rule
/// defaults to `off`, opt-in): a small capped penalty plus a `health --hotspots`
/// surface, and located per-chain records so CI / an agent can act ("colocate or
/// lift to context at hop B"). Zero-FP doctrine: any spread / `cloneElement` /
/// element-as-prop / render-prop / context-provider / dynamic shape in the path
/// abstains the whole chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PropDrillingChain {
    /// The drilled prop name as declared at the chain source.
    pub prop: String,
    /// The chain depth = the number of components the prop is forwarded through
    /// (source + intermediates + consumer = `hops.len()`). Always `>= N`.
    pub depth: u32,
    /// The ordered hop trail from source to consumer. The first hop owns the
    /// prop, the middle hops are pass-throughs, the last hop consumes it. The
    /// finding anchor is the first hop (`path` / `line` for suppression + CI).
    pub hops: Vec<PropDrillHop>,
}

/// A located thin-wrapper / passthrough component: a React/Preact component
/// whose entire body is `return <Child {...props}/>` (a single spread-forwarded
/// child render, no host wrapper, no own value-add). It is pure structural
/// indirection, a CANDIDATE for inlining at call sites or deleting. Health
/// signal (rule defaults to `off`, opt-in): never a correctness error. Zero-FP
/// doctrine: `forwardRef` / `memo` / exported / context-provider /
/// `cloneElement` / render-prop / named-attr / unresolved-child wrappers all
/// abstain (each is an intentional indirection or unprovable shape).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ThinWrapper {
    /// The file containing the wrapper component.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// 1-based line of the wrapper component definition (the finding anchor for
    /// jump-to-source and line-level suppression).
    pub line: u32,
    /// The wrapper component name.
    pub component: String,
    /// The single child component the wrapper forwards its props to (as written
    /// at the render site).
    pub child_component: String,
}

/// One member of a duplicate-prop-shape group: the other components that share
/// the same significant prop-name set, listed in each member's
/// `sharing_components`. Path-sorted for stable output. A located reference (no
/// `shape`, which is carried once on the owning [`DuplicatePropShape`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicatePropShapeMember {
    /// The file containing the sibling component.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// 1-based line of the sibling component definition.
    pub line: u32,
    /// The sibling component name.
    pub component: String,
}

/// A React/Preact component that participates in a duplicate-prop-shape GROUP:
/// three or more distinct components across two or more files whose
/// statically-harvested, fully-known prop NAME set is byte-for-byte IDENTICAL
/// after excluding a fixed denylist of ubiquitous DOM / render-passthrough prop
/// names, with the REMAINING significant set holding four or more members. This
/// is a structural-refactor health signal (extract a shared `Props` type or a
/// base component), never a correctness error and never an auto-fix. One finding
/// is emitted per participating component; `sharing_components` lists the other
/// members of the same group. Health signal: the rule defaults to `off`
/// (opt-in), so this is dormant until enabled. Exact full-set identity only: a
/// superset / subset relationship does NOT group (so the finding always fits one
/// extracted shared type).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DuplicatePropShape {
    /// The file containing this component.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// 1-based line of this component definition (the finding anchor for
    /// jump-to-source and line-level suppression).
    pub line: u32,
    /// This component name.
    pub component: String,
    /// The shared significant prop-name set (sorted, denylist-stripped). The
    /// unit being grouped; identical across every member of the group.
    pub shape: Vec<String>,
    /// The total number of components in this group (this one plus every
    /// sibling).
    pub group_size: u32,
    /// The other components sharing this exact prop shape (path-sorted). A
    /// file-level-suppressed member drops from its own finding but still appears
    /// here, because the group is real regardless of suppression.
    pub sharing_components: Vec<DuplicatePropShapeMember>,
}

/// An Angular `@Input()` / signal `input()` / `model()` declared input that is
/// read NOWHERE inside its own component (neither the inline/external template
/// nor the class body). Single-file dead-input direction; the Angular analogue
/// of [`UnusedComponentProp`]. The whole component abstains on an unresolved
/// `extends` heritage clause (a base class in another file may read `this.foo`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedComponentInput {
    /// The Angular component/directive `.ts` file declaring the unused input.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The component name (the `.ts` file stem).
    pub component_name: String,
    /// The declared input name that is never read.
    pub input_name: String,
    /// 1-based line number of the input declaration.
    pub line: u32,
    /// 0-based byte column offset of the input declaration.
    pub col: u32,
}

/// An Angular `@Output()` / signal `output()` declared output that is EMITTED
/// nowhere inside its own component (no `this.<output>.emit(...)`). Single-file
/// dead-output direction; the Angular analogue of [`UnusedComponentEmit`]. A
/// `model()` is recorded as an input only, so its framework-driven `update:`
/// emit is never flagged here. The whole component abstains on an unresolved
/// `extends` heritage clause.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct UnusedComponentOutput {
    /// The Angular component/directive `.ts` file declaring the unused output.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The component name (the `.ts` file stem).
    pub component_name: String,
    /// The declared output name that is never emitted.
    pub output_name: String,
    /// 1-based line number of the output declaration.
    pub line: u32,
    /// 0-based byte column offset of the output declaration.
    pub col: u32,
}

/// Two or more Next.js App Router route files that resolve to the SAME URL
/// within one app-root. Next.js fails the build ("You cannot have two parallel
/// pages that resolve to the same path"); fallow catches it statically and
/// names every colliding file at once. One finding is emitted per colliding
/// file; `conflicting_paths` lists the sibling files that share the URL.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RouteCollision {
    /// This colliding route file (a `page` or `route` leaf).
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The URL pathname this file resolves to within its app-root, after
    /// stripping route groups `(x)` and parallel-slot `@slot` prefixes (e.g.
    /// `/about`, `/api/health`, `/blog/:slug`).
    pub url: String,
    /// The other route files that resolve to the same URL within the same
    /// app-root. Path-sorted for stable output / fingerprints.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub conflicting_paths: Vec<PathBuf>,
    /// 1-based line number (file-level finding, always 1).
    pub line: u32,
    /// 0-based byte column offset (file-level finding, always 0).
    pub col: u32,
}

/// Two or more sibling dynamic route segments at the SAME App Router tree
/// position using different param spellings (`[id]` vs `[slug]`, or `[...x]`
/// vs `[[...x]]`). Next.js throws "You cannot use different slug names for the
/// same dynamic path" at dev / production RUNTIME when the position is hit;
/// `next build` does NOT catch it, so fallow's static catch surfaces a route
/// that would otherwise pass CI and crash at request time. One finding is
/// emitted per involved file.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DynamicSegmentNameConflict {
    /// This route file living under one of the conflicting dynamic segments.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The tree position (parent URL after group/slot normalization) where the
    /// dynamic segments conflict, e.g. `/shop` for `/shop/[id]` vs
    /// `/shop/[slug]`. The app-root prefix is stripped.
    pub position: String,
    /// The distinct conflicting dynamic-segment spellings at this position, as
    /// written (e.g. `["[id]", "[slug]"]`). Sorted for stable output.
    pub conflicting_segments: Vec<String>,
    /// The other route files at the same position under a conflicting dynamic
    /// segment. Path-sorted for stable output / fingerprints.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub conflicting_paths: Vec<PathBuf>,
    /// 1-based line number (file-level finding, always 1).
    pub line: u32,
    /// 0-based byte column offset (file-level finding, always 0).
    pub col: u32,
}
