use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};
use globset::GlobMatcher;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::discover::FileId;
use crate::extract::{ExportName, MemberInfo, MemberKind, ModuleInfo};
use crate::graph::{ModuleGraph, ReferenceKind};
use crate::resolve::ResolvedModule;
use crate::results::UnusedMember;
use crate::suppress::{IssueKind, SuppressionContext};
use fallow_types::extract::{
    FactoryCallMemberAccessFact, FactoryFnMemberAccessFact, FactoryFnWholeObjectFact,
    FactoryReturnObjectPropertyAccessFact, FluentChainMemberAccessFact,
    FluentChainNewMemberAccessFact, InstanceExportBindingFact, PlaywrightFixtureAliasFact,
    PlaywrightFixtureDefinitionFact, PlaywrightFixtureTypeFact, PlaywrightFixtureUseFact,
    SemanticFactView, TypedPropertyMemberAccessFact, ordinary_whole_object_uses,
};

use super::predicates::{is_angular_lifecycle_method, is_react_lifecycle_method};
use super::{LineOffsetsMap, byte_offset_to_line_col};

mod factory;
mod fluent_chain;
mod heritage;
mod instance;
mod playwright;
mod typed_property;

use factory::*;
use fluent_chain::*;
use heritage::*;
use instance::*;
use playwright::*;
use typed_property::*;

const NATIVE_CUSTOM_ELEMENT_LIFECYCLE_MEMBERS: &[&str] = &[
    "connectedCallback",
    "disconnectedCallback",
    "attributeChangedCallback",
    "adoptedCallback",
    "connectedMoveCallback",
    "observedAttributes",
    "formAssociated",
    "formAssociatedCallback",
    "formDisabledCallback",
    "formResetCallback",
    "formStateRestoreCallback",
];

fn is_native_custom_element_lifecycle_method(member_name: &str, super_class: Option<&str>) -> bool {
    super_class == Some("HTMLElement")
        && NATIVE_CUSTOM_ELEMENT_LIFECYCLE_MEMBERS.contains(&member_name)
}

/// Native ECMAScript `Error` constructors whose subclasses treat `name` as
/// runtime-used.
const NATIVE_ERROR_BASE_NAMES: &[&str] = &[
    "Error",
    "TypeError",
    "RangeError",
    "SyntaxError",
    "ReferenceError",
    "EvalError",
    "URIError",
    "AggregateError",
];

/// Runtime-used members on error subclasses. Kept narrow so unrelated members
/// on error classes still report.
const ERROR_SUBCLASS_RUNTIME_MEMBERS: &[&str] = &["name"];

fn is_native_error_base_name(name: &str) -> bool {
    NATIVE_ERROR_BASE_NAMES.contains(&name)
}

/// `name` is runtime-used when its declaring class is in the error-subclass
/// closure.
fn is_error_subclass_runtime_member(
    member_name: &str,
    export_key: &ExportKey,
    error_subclass_keys: &FxHashSet<ExportKey>,
) -> bool {
    ERROR_SUBCLASS_RUNTIME_MEMBERS.contains(&member_name)
        && error_subclass_keys.contains(export_key)
}

/// Methods OpenLayers calls by convention on an `ol/interaction/*` subclass.
///
/// `handleEvent` is the dispatcher the `Interaction` base exposes and the map
/// invokes per browser event; the `handle*Event` / `stopDown` set is the
/// `PointerInteraction` template-method protocol a drag/pointer interaction
/// overrides. None has an explicit `instance.method()` call site, so they are
/// otherwise reported as unused. Verified against the OpenLayers
/// `interaction/Interaction.js` and `interaction/Pointer.js` sources.
const OL_INTERACTION_DISPATCHED_MEMBERS: &[&str] = &[
    "handleEvent",
    "handleDownEvent",
    "handleDragEvent",
    "handleMoveEvent",
    "handleUpEvent",
    "stopDown",
];

/// A `super_class` import source naming an OpenLayers interaction base.
///
/// Matches the per-class subpath imports (`ol/interaction/Pointer`,
/// `ol/interaction/Interaction`, ...) and the barrel named import
/// (`import {Pointer} from 'ol/interaction'`). Anything else (a same-named
/// LOCAL `Pointer` class, an unrelated package) is not an OpenLayers base, so
/// its dispatched-name members still report.
fn is_ol_interaction_import_source(source: &str) -> bool {
    source == "ol/interaction" || source.starts_with("ol/interaction/")
}

/// A dispatched method is runtime-used when its declaring class is in the
/// OpenLayers-interaction-subclass closure.
fn is_ol_interaction_dispatched_member(
    member_name: &str,
    export_key: &ExportKey,
    ol_interaction_subclass_keys: &FxHashSet<ExportKey>,
) -> bool {
    OL_INTERACTION_DISPATCHED_MEMBERS.contains(&member_name)
        && ol_interaction_subclass_keys.contains(export_key)
}

/// Find unused enum and class members in exported symbols.
///
/// Collects `Identifier.member` accesses, resolves imports, and filters out
/// members that are accessed or explicitly allowlisted.
#[derive(Default)]
struct ClassMemberAllowlist<'a> {
    global: FxHashSet<&'a str>,
    global_patterns: Vec<MemberPattern<'a>>,
    scoped: FxHashMap<&'a str, Vec<&'a ScopedUsedClassMemberRule>>,
    scoped_patterns: Vec<ScopedMemberPattern<'a>>,
}

struct MemberPattern<'a> {
    raw: &'a str,
    matcher: GlobMatcher,
    matched: AtomicBool,
}

struct ScopedMemberPattern<'a> {
    raw: &'a str,
    matcher: GlobMatcher,
    rule: &'a ScopedUsedClassMemberRule,
    matched: AtomicBool,
}

struct MemberSkipContext<'a> {
    export_key: &'a ExportKey,
    accessed_members: &'a FxHashMap<ExportKey, FxHashSet<String>>,
    file_self_accesses: Option<&'a FxHashSet<String>>,
    ignore_decorators: &'a IgnoreDecoratorSet,
    error_subclass_keys: &'a FxHashSet<ExportKey>,
    ol_interaction_subclass_keys: &'a FxHashSet<ExportKey>,
    allowlist: &'a ClassMemberAllowlist<'a>,
    super_class: Option<&'a str>,
    implemented_interfaces: &'a [String],
    is_public_api_class_export: bool,
    lit_active: bool,
}

impl<'a> ClassMemberAllowlist<'a> {
    fn from_rules(rules: &'a [UsedClassMemberRule]) -> Self {
        let mut allowlist = Self::default();
        for rule in rules {
            match rule {
                UsedClassMemberRule::Name(name) => {
                    allowlist.insert_global(name);
                }
                UsedClassMemberRule::Scoped(rule) => {
                    for member in &rule.members {
                        allowlist.insert_scoped(member, rule);
                    }
                }
            }
        }
        allowlist
    }

    fn insert_global(&mut self, member: &'a str) {
        if let Some(pattern) = compile_member_pattern(member) {
            self.global_patterns.push(MemberPattern {
                raw: member,
                matcher: pattern,
                matched: AtomicBool::new(false),
            });
        } else {
            self.global.insert(member);
        }
    }

    fn insert_scoped(&mut self, member: &'a str, rule: &'a ScopedUsedClassMemberRule) {
        if let Some(pattern) = compile_member_pattern(member) {
            self.scoped_patterns.push(ScopedMemberPattern {
                raw: member,
                matcher: pattern,
                rule,
                matched: AtomicBool::new(false),
            });
        } else {
            self.scoped.entry(member).or_default().push(rule);
        }
    }

    fn matches(
        &self,
        member_name: &str,
        super_class: Option<&str>,
        implemented_interfaces: &[String],
    ) -> bool {
        self.global.contains(member_name)
            || self
                .global_patterns
                .iter()
                .any(|pattern| pattern.matches(member_name))
            || self.scoped.get(member_name).is_some_and(|rules| {
                rules
                    .iter()
                    .any(|rule| rule.matches_heritage(super_class, implemented_interfaces))
            })
            || self
                .scoped_patterns
                .iter()
                .any(|pattern| pattern.matches(member_name, super_class, implemented_interfaces))
    }

    fn warn_unmatched_patterns(&self) {
        for pattern in self
            .global_patterns
            .iter()
            .filter(|pattern| !pattern.matched.load(Ordering::Relaxed))
        {
            tracing::warn!(
                "usedClassMembers glob pattern '{}' did not match any class member",
                pattern.raw
            );
        }

        for pattern in self
            .scoped_patterns
            .iter()
            .filter(|pattern| !pattern.matched.load(Ordering::Relaxed))
        {
            tracing::warn!(
                "usedClassMembers scoped glob pattern '{}' did not match any class member for {}",
                pattern.raw,
                heritage_clause(pattern.rule)
            );
        }
    }
}

impl MemberPattern<'_> {
    fn matches(&self, member_name: &str) -> bool {
        let matches = self.matcher.is_match(member_name);
        if matches {
            self.matched.store(true, Ordering::Relaxed);
        }
        matches
    }
}

impl ScopedMemberPattern<'_> {
    fn matches(
        &self,
        member_name: &str,
        super_class: Option<&str>,
        implemented_interfaces: &[String],
    ) -> bool {
        let matches = self.matcher.is_match(member_name)
            && self
                .rule
                .matches_heritage(super_class, implemented_interfaces);
        if matches {
            self.matched.store(true, Ordering::Relaxed);
        }
        matches
    }
}

fn compile_member_pattern(member: &str) -> Option<GlobMatcher> {
    if !member.contains('*') && !member.contains('?') {
        return None;
    }

    globset::Glob::new(member)
        .ok()
        .map(|glob| glob.compile_matcher())
}

/// User-supplied decorator names that should not count as reflective use.
///
/// Dotted entries match the full path; bare entries match the leftmost
/// segment. Unmatched entries are warned at end of run.
struct IgnoreDecoratorSet {
    entries: Vec<IgnoreDecoratorEntry>,
}

struct IgnoreDecoratorEntry {
    /// Original user input, after `@` strip + trim.
    raw: String,
    /// Whether the entry is dotted, which means exact-path matching.
    is_dotted: bool,
    matched: AtomicBool,
}

impl IgnoreDecoratorSet {
    fn from_config(ignore_decorators: &[String]) -> Self {
        let entries = ignore_decorators
            .iter()
            .filter_map(|raw| {
                let trimmed = raw.trim();
                let normalized = trimmed.strip_prefix('@').unwrap_or(trimmed);
                if normalized.is_empty() {
                    return None;
                }
                Some(IgnoreDecoratorEntry {
                    raw: normalized.to_string(),
                    is_dotted: normalized.contains('.'),
                    matched: AtomicBool::new(false),
                })
            })
            .collect();
        Self { entries }
    }

    fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns true when `decorator_path` matches any ignore-list entry.
    /// Empty paths never match. Matching entries are marked as seen.
    fn matches(&self, decorator_path: &str) -> bool {
        if decorator_path.is_empty() {
            return false;
        }
        let leftmost = decorator_path
            .split_once('.')
            .map_or(decorator_path, |(head, _)| head);
        for entry in &self.entries {
            let hit = if entry.is_dotted {
                entry.raw == decorator_path
            } else {
                entry.raw == leftmost
            };
            if hit {
                entry.matched.store(true, Ordering::Relaxed);
                return true;
            }
        }
        false
    }

    /// Mark matching entries as seen without returning the predicate result.
    /// Used by the pre-pass so used members do not trigger false warnings.
    fn record_seen(&self, decorator_path: &str) {
        if decorator_path.is_empty() {
            return;
        }
        let leftmost = decorator_path
            .split_once('.')
            .map_or(decorator_path, |(head, _)| head);
        for entry in &self.entries {
            let hit = if entry.is_dotted {
                entry.raw == decorator_path
            } else {
                entry.raw == leftmost
            };
            if hit {
                entry.matched.store(true, Ordering::Relaxed);
            }
        }
    }

    fn warn_unmatched(&self) {
        for entry in &self.entries {
            if !entry.matched.load(Ordering::Relaxed) {
                tracing::warn!(
                    "ignoreDecorators entry '{}' did not match any decorator in the analyzed codebase; remove if no longer needed",
                    entry.raw
                );
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct ExportKey {
    pub(super) file_id: FileId,
    pub(super) export_name: String,
}

impl ExportKey {
    pub(super) fn new(file_id: FileId, export_name: impl Into<String>) -> Self {
        Self {
            file_id,
            export_name: export_name.into(),
        }
    }
}

fn imported_export_name(imported_name: &crate::extract::ImportedName) -> Option<&str> {
    match imported_name {
        crate::extract::ImportedName::Named(name) => Some(name.as_str()),
        crate::extract::ImportedName::Default => Some("default"),
        crate::extract::ImportedName::Namespace | crate::extract::ImportedName::SideEffect => None,
    }
}

fn push_local_export_key<'a>(
    local_to_export_keys: &mut FxHashMap<&'a str, Vec<ExportKey>>,
    local_name: &'a str,
    export_key: ExportKey,
) {
    let entry = local_to_export_keys.entry(local_name).or_default();
    if !entry.contains(&export_key) {
        entry.push(export_key);
    }
}

pub(super) fn build_local_to_export_keys(
    resolved: &ResolvedModule,
) -> FxHashMap<&str, Vec<ExportKey>> {
    let mut local_to_export_keys = FxHashMap::default();

    for import in resolved.all_resolved_imports() {
        let Some(imported_name) = imported_export_name(&import.info.imported_name) else {
            continue;
        };
        let Some(target_file_id) = import.target.internal_file_id() else {
            continue;
        };
        push_local_export_key(
            &mut local_to_export_keys,
            import.info.local_name.as_str(),
            ExportKey::new(target_file_id, imported_name),
        );
    }

    for export in &resolved.exports {
        if let Some(local_name) = export.local_name.as_deref() {
            push_local_export_key(
                &mut local_to_export_keys,
                local_name,
                ExportKey::new(resolved.file_id, export.name.to_string()),
            );
        }
    }

    local_to_export_keys
}

/// Walk re-export chains to the defining-site `ExportKey`s.
///
/// Prefers real re-export edges over barrel stubs and handles renamed or
/// star re-exports.
pub(super) fn walk_re_export_origins(
    graph: &ModuleGraph,
    start_file: FileId,
    start_name: &str,
) -> Vec<ExportKey> {
    let mut origins: Vec<ExportKey> = Vec::new();
    let mut visited: FxHashSet<(FileId, String)> = FxHashSet::default();
    let mut stack: Vec<(FileId, String)> = vec![(start_file, start_name.to_string())];

    while let Some((file_id, name)) = stack.pop() {
        if !visited.insert((file_id, name.clone())) {
            continue;
        }
        let Some(module) = graph.modules.get(file_id.0 as usize) else {
            continue;
        };

        let mut matched_named = false;
        for re in &module.re_exports {
            if re.exported_name != "*" && re.imported_name != "*" && re.exported_name == name {
                stack.push((re.source_file, re.imported_name.clone()));
                matched_named = true;
            }
        }
        if matched_named {
            continue;
        }

        let locally_defined = module.exports.iter().any(|e| match &e.name {
            ExportName::Named(n) => n.as_str() == name,
            ExportName::Default => name == "default",
        });
        if locally_defined {
            origins.push(ExportKey::new(file_id, name));
            continue;
        }

        for re in &module.re_exports {
            if re.exported_name == "*" {
                stack.push((re.source_file, name.clone()));
            }
        }
    }

    origins
}

/// Copy access sets from barrel `ExportKey`s to every defining-site
/// `ExportKey` reachable through re-export chains.
fn propagate_accesses_through_re_exports(
    graph: &ModuleGraph,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    let snapshot: Vec<(ExportKey, Vec<String>)> = accessed_members
        .iter()
        .map(|(k, v)| (k.clone(), v.iter().cloned().collect()))
        .collect();
    for (key, members) in snapshot {
        let origins = walk_re_export_origins(graph, key.file_id, &key.export_name);
        for origin in origins {
            if origin == key {
                continue;
            }
            accessed_members
                .entry(origin)
                .or_default()
                .extend(members.iter().cloned());
        }
    }
}

/// Sibling of `propagate_accesses_through_re_exports` for whole-object use.
fn propagate_whole_object_through_re_exports(
    graph: &ModuleGraph,
    whole_object_used_exports: &mut FxHashSet<ExportKey>,
) {
    let snapshot: Vec<ExportKey> = whole_object_used_exports.iter().cloned().collect();
    for key in snapshot {
        let origins = walk_re_export_origins(graph, key.file_id, &key.export_name);
        for origin in origins {
            if origin == key {
                continue;
            }
            whole_object_used_exports.insert(origin);
        }
    }
}

fn push_export_key(keys: &mut Vec<ExportKey>, key: ExportKey) {
    if !keys.contains(&key) {
        keys.push(key);
    }
}

pub(super) fn export_key_with_origins(graph: &ModuleGraph, key: &ExportKey) -> Vec<ExportKey> {
    let mut keys = Vec::new();
    push_export_key(&mut keys, key.clone());
    for origin in walk_re_export_origins(graph, key.file_id, key.export_name.as_str()) {
        push_export_key(&mut keys, origin);
    }
    keys
}

pub(super) fn entry_point_star_re_export_targets(
    graph: &ModuleGraph,
    public_api_entry_points: &FxHashSet<FileId>,
) -> FxHashSet<FileId> {
    let mut targets: FxHashSet<FileId> = public_api_entry_points
        .iter()
        .filter_map(|file_id| graph.modules.get(file_id.0 as usize))
        .flat_map(|module| {
            module
                .re_exports
                .iter()
                .filter(|re_export| re_export.exported_name == "*")
                .map(|re_export| re_export.source_file)
        })
        .collect();

    let mut stack: Vec<FileId> = targets.iter().copied().collect();
    while let Some(file_id) = stack.pop() {
        let Some(module) = graph.modules.get(file_id.0 as usize) else {
            continue;
        };
        for re_export in module
            .re_exports
            .iter()
            .filter(|re_export| re_export.exported_name == "*")
        {
            if targets.insert(re_export.source_file) {
                stack.push(re_export.source_file);
            }
        }
    }

    targets
}

fn export_has_class_members(export: &crate::graph::ExportSymbol) -> bool {
    export.members.iter().any(|member| {
        matches!(
            member.kind,
            MemberKind::ClassMethod | MemberKind::ClassProperty
        )
    })
}

pub(super) fn export_has_entry_point_re_export_reference(
    graph: &ModuleGraph,
    export: &crate::graph::ExportSymbol,
    public_api_entry_points: &FxHashSet<FileId>,
) -> bool {
    export.references.iter().any(|reference| {
        reference.kind == ReferenceKind::ReExport
            && public_api_entry_points.contains(&reference.from_file)
            && graph
                .modules
                .get(reference.from_file.0 as usize)
                .is_some_and(|module| module.is_entry_point())
    })
}

fn is_entry_point_public_class_export(
    graph: &ModuleGraph,
    module: &crate::graph::ModuleNode,
    export: &crate::graph::ExportSymbol,
    entry_star_targets: &FxHashSet<FileId>,
    public_api_entry_points: &FxHashSet<FileId>,
) -> bool {
    export_has_class_members(export)
        && (entry_star_targets.contains(&module.file_id)
            || export_has_entry_point_re_export_reference(graph, export, public_api_entry_points))
}

fn playwright_fixture_uses(resolved: &ResolvedModule) -> Vec<PlaywrightFixtureUseFact> {
    let view = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    view.playwright_fixture_uses()
}

fn playwright_fixture_definitions(
    resolved: &ResolvedModule,
) -> Vec<PlaywrightFixtureDefinitionFact> {
    let view = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    view.playwright_fixture_definitions()
}

fn playwright_fixture_aliases(resolved: &ResolvedModule) -> Vec<PlaywrightFixtureAliasFact> {
    let view = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    view.playwright_fixture_aliases()
}

fn playwright_fixture_types(resolved: &ResolvedModule) -> Vec<PlaywrightFixtureTypeFact> {
    let view = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    view.playwright_fixture_types()
}

fn instance_export_bindings(resolved: &ResolvedModule) -> Vec<InstanceExportBindingFact> {
    let view = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    view.instance_export_bindings()
}

fn factory_call_member_accesses(resolved: &ResolvedModule) -> Vec<FactoryCallMemberAccessFact> {
    let view = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    view.factory_call_member_accesses()
}

fn factory_fn_member_accesses(resolved: &ResolvedModule) -> Vec<FactoryFnMemberAccessFact> {
    let view = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    view.factory_fn_member_accesses()
}

fn factory_fn_whole_objects(resolved: &ResolvedModule) -> Vec<FactoryFnWholeObjectFact> {
    let view = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    view.factory_fn_whole_objects()
}

fn factory_return_object_property_accesses(
    resolved: &ResolvedModule,
) -> Vec<FactoryReturnObjectPropertyAccessFact> {
    let view = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    view.factory_return_object_property_accesses()
}

fn typed_property_member_accesses(resolved: &ResolvedModule) -> Vec<TypedPropertyMemberAccessFact> {
    let view = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    view.typed_property_member_accesses()
}

fn fluent_chain_member_accesses(resolved: &ResolvedModule) -> Vec<FluentChainMemberAccessFact> {
    let view = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    view.fluent_chain_member_accesses()
}

fn fluent_chain_new_member_accesses(
    resolved: &ResolvedModule,
) -> Vec<FluentChainNewMemberAccessFact> {
    let view = SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses);
    view.fluent_chain_new_member_accesses()
}

/// Whether an export named `name` in `module` is a class carrying members, the
/// final over-credit gate for cross-module factory-fn credit. A class records
/// `ClassMethod`/`ClassProperty` members; enums, namespaces, and stores use
/// other `MemberKind`s, so a wrong resolution onto one of those credits nothing.
fn export_is_class_with_members(module: &ResolvedModule, name: &str) -> bool {
    module.exports.iter().any(|export| {
        export.name.matches_str(name)
            && export.members.iter().any(|member| {
                member.kind == MemberKind::ClassMethod || member.kind == MemberKind::ClassProperty
            })
    })
}

/// Build the set of exported class `ExportKey`s whose heritage chain reaches a
/// native JavaScript `Error` constructor.
fn build_error_subclass_export_keys(
    parent_to_children: &FxHashMap<ExportKey, Vec<ExportKey>>,
    class_heritage_by_export: &FxHashMap<ExportKey, (Option<String>, Vec<String>)>,
) -> FxHashSet<ExportKey> {
    let mut error_keys: FxHashSet<ExportKey> = class_heritage_by_export
        .iter()
        .filter(|(_, (super_class, _))| {
            super_class
                .as_deref()
                .is_some_and(is_native_error_base_name)
        })
        .map(|(key, _)| key.clone())
        .collect();

    if error_keys.is_empty() {
        return error_keys;
    }

    let mut stack: Vec<ExportKey> = error_keys.iter().cloned().collect();
    while let Some(parent_key) = stack.pop() {
        if let Some(children) = parent_to_children.get(&parent_key) {
            for child in children {
                if error_keys.insert(child.clone()) {
                    stack.push(child.clone());
                }
            }
        }
    }

    error_keys
}

/// Cross-file member-usage detection results, split by member kind. Store
/// members (Pinia `state` / `getters` / `actions` key, or a setup-store
/// returned key) are reported separately from enum and class members because
/// they default to `warn` (open declaration set) rather than `error`.
#[expect(
    clippy::struct_field_names,
    reason = "the `_members` suffix names the member kind and reads clearly at call sites"
)]
pub struct UnusedMemberResults {
    /// Unused TypeScript enum members.
    pub enum_members: Vec<UnusedMember>,
    /// Unused class methods / properties.
    pub class_members: Vec<UnusedMember>,
    /// Unused store members (Pinia stores).
    pub store_members: Vec<UnusedMember>,
}

/// Per-run memoized lookup maps shared across every member-propagation pass.
///
/// Almost every access-propagation pass independently rebuilds the same two
/// derived structures for every module: the per-module local-name -> export-key
/// map (O(imports + exports) per module) and a `module_by_id` index over all
/// modules. Building both once per scan and threading a shared reference through
/// the passes removes the redundant constant-factor rebuilds on one of the
/// hottest analysis paths. Keys borrow from the `resolved_modules` slice (via
/// `build_local_to_export_keys`), so the struct is built where that slice
/// outlives all passes (`find_unused_members_with_public_api_entry_points` holds
/// `input` for the whole scan).
pub(super) struct MemberPassIndexes<'a> {
    pub(super) module_by_id: FxHashMap<FileId, &'a ResolvedModule>,
    local_keys_by_file: FxHashMap<FileId, FxHashMap<&'a str, Vec<ExportKey>>>,
    empty: FxHashMap<&'a str, Vec<ExportKey>>,
}

impl<'a> MemberPassIndexes<'a> {
    /// Build both maps eagerly in one loop over `resolved_modules`. Eager (not
    /// lazy) because every pass iterates all modules anyway, and eager building
    /// keeps the borrow lifetimes simple.
    pub(super) fn build(resolved_modules: &'a [ResolvedModule]) -> Self {
        let mut module_by_id: FxHashMap<FileId, &'a ResolvedModule> = FxHashMap::default();
        let mut local_keys_by_file: FxHashMap<FileId, FxHashMap<&'a str, Vec<ExportKey>>> =
            FxHashMap::default();
        for module in resolved_modules {
            module_by_id.insert(module.file_id, module);
            local_keys_by_file.insert(module.file_id, build_local_to_export_keys(module));
        }
        Self {
            module_by_id,
            local_keys_by_file,
            empty: FxHashMap::default(),
        }
    }

    /// The local-name -> export-key map for `file_id`, built once in `build`.
    /// Every module in `resolved_modules` is present; a `file_id` outside the
    /// slice (never reached by the passes, which only look up resolved modules)
    /// returns a shared empty map so callers stay branchless.
    pub(super) fn local_keys(&self, file_id: FileId) -> &FxHashMap<&'a str, Vec<ExportKey>> {
        self.local_keys_by_file.get(&file_id).unwrap_or(&self.empty)
    }
}

#[derive(Clone, Copy)]
pub(super) struct UnusedMemberScanInput<'a> {
    pub(super) graph: &'a ModuleGraph,
    pub(super) resolved_modules: &'a [ResolvedModule],
    pub(super) modules: &'a [ModuleInfo],
    pub(super) suppressions: &'a SuppressionContext<'a>,
    pub(super) line_offsets_by_file: &'a LineOffsetsMap<'a>,
    pub(super) user_class_member_allowlist: &'a [UsedClassMemberRule],
    pub(super) ignore_decorators: &'a [String],
    pub(super) public_api_entry_points: &'a FxHashSet<FileId>,
    /// Whether a Lit dependency is declared. When true, a `@state()`-decorated
    /// member on a direct `LitElement` / `ReactiveElement` subclass is made
    /// CHECKABLE (a never-read `@state` is dead internal reactive state). Other
    /// decorated members, and `@property` (the public attribute API), stay
    /// skipped.
    pub(super) lit_active: bool,
}

struct PreparedMemberScan<'a> {
    heritage_context: MemberHeritageContext<'a>,
    accessed_members: FxHashMap<ExportKey, FxHashSet<String>>,
    self_accessed_members: FxHashMap<FileId, FxHashSet<String>>,
    whole_object_used_exports: FxHashSet<ExportKey>,
    entry_star_targets: FxHashSet<FileId>,
    error_subclass_keys: FxHashSet<ExportKey>,
    ol_interaction_subclass_keys: FxHashSet<ExportKey>,
}

type MemberScanBuckets = (Vec<UnusedMember>, Vec<UnusedMember>, Vec<UnusedMember>);

struct MemberReportContext<'a, 'scan> {
    input: UnusedMemberScanInput<'a>,
    allowlist: &'scan ClassMemberAllowlist<'a>,
    ignore_decorators: &'scan IgnoreDecoratorSet,
    prepared: &'scan PreparedMemberScan<'a>,
}

pub(super) fn find_unused_members_with_public_api_entry_points(
    input: UnusedMemberScanInput<'_>,
) -> UnusedMemberResults {
    let mut unused_enum_members = Vec::new();
    let mut unused_class_members = Vec::new();
    let mut unused_store_members = Vec::new();
    let allowlist = ClassMemberAllowlist::from_rules(input.user_class_member_allowlist);
    let ignore_decorators = IgnoreDecoratorSet::from_config(input.ignore_decorators);

    record_seen_ignore_decorators(input.graph, &ignore_decorators);

    let prepared = prepare_member_scan(input);
    let member_results = MemberReportContext {
        input,
        allowlist: &allowlist,
        ignore_decorators: &ignore_decorators,
        prepared: &prepared,
    }
    .collect();

    for (enum_members, class_members, store_members) in member_results {
        unused_enum_members.extend(enum_members);
        unused_class_members.extend(class_members);
        unused_store_members.extend(store_members);
    }

    allowlist.warn_unmatched_patterns();
    ignore_decorators.warn_unmatched();

    UnusedMemberResults {
        enum_members: unused_enum_members,
        class_members: unused_class_members,
        store_members: unused_store_members,
    }
}

impl MemberReportContext<'_, '_> {
    fn collect(&self) -> Vec<MemberScanBuckets> {
        self.input
            .graph
            .modules
            .par_iter()
            .map(|module| self.collect_module(module))
            .collect()
    }

    fn collect_module(&self, module: &crate::graph::ModuleNode) -> MemberScanBuckets {
        let mut buckets = (Vec::new(), Vec::new(), Vec::new());
        if !module.is_reachable() {
            return buckets;
        }

        let store_only_scan = module.is_entry_point();
        for export in &module.exports {
            self.collect_export(module, export, store_only_scan, &mut buckets);
        }
        buckets
    }

    fn collect_export(
        &self,
        module: &crate::graph::ModuleNode,
        export: &crate::graph::ExportSymbol,
        store_only_scan: bool,
        buckets: &mut MemberScanBuckets,
    ) {
        if self.export_member_scan_skipped(module, export, store_only_scan) {
            return;
        }

        let export_name = export.name.to_string();
        let export_key = ExportKey::new(module.file_id, export_name.clone());
        if self
            .prepared
            .whole_object_used_exports
            .contains(&export_key)
        {
            return;
        }

        self.collect_export_members(
            &MemberScanTarget {
                module,
                export_name: &export_name,
                store_only_scan,
            },
            export,
            &export_key,
            buckets,
        );
    }

    /// Whether this export is skipped for member scanning: not member-scannable,
    /// or a store-only (entry-point) scan with no store members.
    fn export_member_scan_skipped(
        &self,
        module: &crate::graph::ModuleNode,
        export: &crate::graph::ExportSymbol,
        store_only_scan: bool,
    ) -> bool {
        if should_skip_export_member_scan(self.input.graph, module, export) {
            return true;
        }
        store_only_scan
            && !export
                .members
                .iter()
                .any(|m| m.kind == MemberKind::StoreMember)
    }

    /// Build the shared per-export skip context and scan each declared member.
    fn collect_export_members(
        &self,
        target: &MemberScanTarget<'_>,
        export: &crate::graph::ExportSymbol,
        export_key: &ExportKey,
        buckets: &mut MemberScanBuckets,
    ) {
        let module = target.module;
        let file_self_accesses = self.prepared.self_accessed_members.get(&module.file_id);
        let is_public_api_class_export = is_entry_point_public_class_export(
            self.input.graph,
            module,
            export,
            &self.prepared.entry_star_targets,
            self.input.public_api_entry_points,
        );
        let (super_class, implemented_interfaces) = self
            .prepared
            .heritage_context
            .class_heritage_by_export
            .get(export_key)
            .map_or((None, &[][..]), |(super_class, interfaces)| {
                (super_class.as_deref(), interfaces.as_slice())
            });

        for member in &export.members {
            self.collect_member(
                target,
                member,
                &MemberSkipContext {
                    export_key,
                    accessed_members: &self.prepared.accessed_members,
                    file_self_accesses,
                    ignore_decorators: self.ignore_decorators,
                    error_subclass_keys: &self.prepared.error_subclass_keys,
                    ol_interaction_subclass_keys: &self.prepared.ol_interaction_subclass_keys,
                    allowlist: self.allowlist,
                    super_class,
                    implemented_interfaces,
                    is_public_api_class_export,
                    lit_active: self.input.lit_active,
                },
                buckets,
            );
        }
    }

    fn collect_member(
        &self,
        target: &MemberScanTarget<'_>,
        member: &MemberInfo,
        skip_context: &MemberSkipContext<'_>,
        buckets: &mut MemberScanBuckets,
    ) {
        if target.store_only_scan && member.kind != MemberKind::StoreMember {
            return;
        }
        if should_skip_member_for_unused_report(member, skip_context) {
            return;
        }

        let Some(unused) = build_unsuppressed_unused_member(
            target.module.file_id,
            &target.module.path,
            target.export_name,
            member,
            self.input.suppressions,
            self.input.line_offsets_by_file,
        ) else {
            return;
        };
        push_unused_member(buckets, unused, member.kind);
    }
}

/// Shared per-export scan target: the module, the export's rendered name, and
/// whether this is an entry-point store-only scan.
struct MemberScanTarget<'a> {
    module: &'a crate::graph::ModuleNode,
    export_name: &'a str,
    store_only_scan: bool,
}

fn push_unused_member(buckets: &mut MemberScanBuckets, unused: UnusedMember, kind: MemberKind) {
    match kind {
        MemberKind::EnumMember => buckets.0.push(unused),
        MemberKind::ClassMethod | MemberKind::ClassProperty => buckets.1.push(unused),
        MemberKind::StoreMember => buckets.2.push(unused),
        MemberKind::NamespaceMember => unreachable!(),
    }
}

fn prepare_member_scan(input: UnusedMemberScanInput<'_>) -> PreparedMemberScan<'_> {
    // Build the shared local-export-key and module-index maps once per scan; the
    // passes below all borrow them instead of rebuilding per module.
    let indexes = MemberPassIndexes::build(input.resolved_modules);
    let heritage_context =
        build_member_heritage_context(input.graph, input.resolved_modules, input.modules, &indexes);
    let parent_to_children =
        build_parent_to_children(input.graph, input.resolved_modules, &indexes);

    let MemberAccessCollections {
        accessed_members,
        self_accessed_members,
        whole_object_used_exports,
    } = collect_propagated_member_accesses(input, &heritage_context, &parent_to_children, &indexes);

    let entry_star_targets =
        entry_point_star_re_export_targets(input.graph, input.public_api_entry_points);

    let error_subclass_keys = build_error_subclass_export_keys(
        &parent_to_children,
        &heritage_context.class_heritage_by_export,
    );

    let ol_interaction_subclass_keys =
        build_ol_interaction_subclass_keys(input.resolved_modules, &parent_to_children);

    PreparedMemberScan {
        heritage_context,
        accessed_members,
        self_accessed_members,
        whole_object_used_exports,
        entry_star_targets,
        error_subclass_keys,
        ol_interaction_subclass_keys,
    }
}

/// Build the set of exported class `ExportKey`s whose heritage chain reaches an
/// OpenLayers interaction base, verified by the `super_class` local name
/// resolving through the module's imports to an `ol/interaction/*` specifier.
///
/// Seeds direct subclasses (the imported base is an external package, never a
/// local export, so `parent_to_children` cannot reach it), then walks
/// `parent_to_children` downward so a transitive local subclass
/// (`class B extends A` where `A extends PointerInteraction`) is covered too.
fn build_ol_interaction_subclass_keys(
    resolved_modules: &[ResolvedModule],
    parent_to_children: &FxHashMap<ExportKey, Vec<ExportKey>>,
) -> FxHashSet<ExportKey> {
    let mut ol_keys: FxHashSet<ExportKey> = FxHashSet::default();

    for resolved in resolved_modules {
        let ol_import_locals: FxHashSet<&str> = resolved
            .resolved_imports
            .iter()
            .filter(|import| is_ol_interaction_import_source(&import.info.source))
            .map(|import| import.info.local_name.as_str())
            .collect();
        if ol_import_locals.is_empty() {
            continue;
        }

        for export in &resolved.exports {
            if let Some(super_local) = &export.super_class
                && ol_import_locals.contains(super_local.as_str())
            {
                ol_keys.insert(ExportKey::new(resolved.file_id, export.name.to_string()));
            }
        }
    }

    if ol_keys.is_empty() {
        return ol_keys;
    }

    let mut stack: Vec<ExportKey> = ol_keys.iter().cloned().collect();
    while let Some(parent_key) = stack.pop() {
        if let Some(children) = parent_to_children.get(&parent_key) {
            for child in children {
                if ol_keys.insert(child.clone()) {
                    stack.push(child.clone());
                }
            }
        }
    }

    ol_keys
}

/// Collect direct member accesses and run every access-propagation pass
/// (fixtures, factory/fluent chains, typed instances, re-exports, instance
/// exports, interfaces, Angular templates, class inheritance) into one populated
/// `MemberAccessCollections`.
fn collect_propagated_member_accesses(
    input: UnusedMemberScanInput<'_>,
    heritage_context: &MemberHeritageContext<'_>,
    parent_to_children: &FxHashMap<ExportKey, Vec<ExportKey>>,
    indexes: &MemberPassIndexes<'_>,
) -> MemberAccessCollections {
    let MemberAccessCollections {
        mut accessed_members,
        mut self_accessed_members,
        mut whole_object_used_exports,
    } = collect_direct_member_accesses(input.resolved_modules, indexes);

    propagate_common_member_accesses(
        input,
        indexes,
        &heritage_context.interface_to_implementers,
        &mut accessed_members,
        &mut whole_object_used_exports,
    );

    propagate_interface_member_accesses(
        &heritage_context.interface_to_implementers,
        &mut accessed_members,
    );

    propagate_angular_template_member_accesses(
        input.graph,
        input.resolved_modules,
        heritage_context,
        indexes,
        &mut accessed_members,
        &mut self_accessed_members,
    );

    propagate_class_inheritance(
        parent_to_children,
        &mut accessed_members,
        &mut self_accessed_members,
    );

    MemberAccessCollections {
        accessed_members,
        self_accessed_members,
        whole_object_used_exports,
    }
}

fn propagate_common_member_accesses(
    input: UnusedMemberScanInput<'_>,
    indexes: &MemberPassIndexes<'_>,
    interface_to_implementers: &FxHashMap<ExportKey, Vec<ExportKey>>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
    whole_object_used_exports: &mut FxHashSet<ExportKey>,
) {
    propagate_playwright_fixture_accesses(
        input.graph,
        input.resolved_modules,
        indexes,
        accessed_members,
    );
    propagate_factory_call_accesses(
        input.graph,
        input.resolved_modules,
        indexes,
        accessed_members,
    );
    propagate_factory_fn_accesses(
        input.graph,
        input.resolved_modules,
        indexes,
        accessed_members,
    );
    propagate_factory_return_object_accesses(
        input.graph,
        input.resolved_modules,
        indexes,
        accessed_members,
    );
    propagate_typed_property_accesses(
        input.graph,
        input.resolved_modules,
        indexes,
        interface_to_implementers,
        accessed_members,
    );
    propagate_fluent_chain_accesses(
        input.graph,
        input.resolved_modules,
        indexes,
        accessed_members,
    );
    propagate_fluent_chain_new_accesses(
        input.graph,
        input.resolved_modules,
        indexes,
        accessed_members,
    );
    propagate_accesses_through_typed_instance_bindings(
        input.graph,
        input.resolved_modules,
        input.modules,
        indexes,
        accessed_members,
        whole_object_used_exports,
    );
    // Before the re-export propagation below, so a class suppressed through an
    // opaque destructure is suppressed at every name it is re-exported under.
    propagate_factory_fn_whole_object_uses(
        input.graph,
        input.resolved_modules,
        indexes,
        whole_object_used_exports,
    );
    propagate_accesses_through_re_exports(input.graph, accessed_members);
    propagate_whole_object_through_re_exports(input.graph, whole_object_used_exports);
    let instance_targets =
        build_instance_export_targets(input.graph, input.resolved_modules, indexes);
    propagate_accesses_through_instance_exports(
        &instance_targets,
        accessed_members,
        whole_object_used_exports,
    );
}

fn should_skip_export_member_scan(
    graph: &ModuleGraph,
    module: &crate::graph::ModuleNode,
    export: &crate::graph::ExportSymbol,
) -> bool {
    export.members.is_empty()
        || (export.references.is_empty()
            && !export.is_side_effect_used
            && !graph.has_namespace_import(module.file_id))
}

fn build_unsuppressed_unused_member(
    file_id: FileId,
    path: &Path,
    export_name: &str,
    member: &MemberInfo,
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> Option<UnusedMember> {
    let (line, col) = byte_offset_to_line_col(line_offsets_by_file, file_id, member.span.start);
    let issue_kind = match member.kind {
        MemberKind::EnumMember => IssueKind::UnusedEnumMember,
        MemberKind::ClassMethod | MemberKind::ClassProperty => IssueKind::UnusedClassMember,
        MemberKind::StoreMember => IssueKind::UnusedStoreMember,
        MemberKind::NamespaceMember => unreachable!(),
    };
    if suppressions.is_suppressed(file_id, line, issue_kind) {
        return None;
    }

    Some(UnusedMember {
        path: path.to_path_buf(),
        parent_name: export_name.to_string(),
        member_name: member.name.clone(),
        kind: member.kind,
        line,
        col,
    })
}

fn should_skip_member_for_unused_report(member: &MemberInfo, ctx: &MemberSkipContext<'_>) -> bool {
    if matches!(member.kind, MemberKind::NamespaceMember) {
        return true;
    }

    if ctx.is_public_api_class_export && is_class_member_kind(member.kind) {
        return true;
    }

    if ctx
        .accessed_members
        .get(ctx.export_key)
        .is_some_and(|s| s.contains(&member.name))
    {
        return true;
    }

    // Intra-store self-access credit: an option-store getter/action consumed
    // only by a sibling getter/action via `this.<member>` lands in the file's
    // self-access set. Without this, a store member used solely inside its own
    // store would be falsely flagged. Class members already credit this way;
    // store members must too. (Setup stores do not use `this`; a returned key
    // used only internally is a genuinely dead PUBLIC member.)
    if (is_class_member_kind(member.kind) || matches!(member.kind, MemberKind::StoreMember))
        && ctx
            .file_self_accesses
            .is_some_and(|accesses| accesses.contains(&member.name))
    {
        return true;
    }

    if member_decorator_requires_skip(member, ctx) {
        return true;
    }

    class_member_runtime_credit_applies(member, ctx)
}

/// Whether a member is a Lit `@state()` reactive property that should be CHECKED
/// for deadness (overriding the generic decorated-member skip). Lit `@state` is
/// INTERNAL reactive state, not a framework-reflected public API like
/// `@property` (which is settable via HTML attribute / parent property binding /
/// `setAttribute` / CSS, all invisible here). Gated on a Lit dependency + a
/// direct `LitElement` / `ReactiveElement` base, and requires `@state` to be the
/// member's ONLY decorator (a `@state` combined with another framework decorator
/// stays skipped).
fn is_lit_checkable_state_member(member: &MemberInfo, ctx: &MemberSkipContext<'_>) -> bool {
    ctx.lit_active
        && matches!(ctx.super_class, Some("LitElement" | "ReactiveElement"))
        && !member.decorator_names.is_empty()
        && member.decorator_names.iter().all(|name| name == "state")
}

fn member_decorator_requires_skip(member: &MemberInfo, ctx: &MemberSkipContext<'_>) -> bool {
    if is_lit_checkable_state_member(member, ctx) {
        return false;
    }
    let ignore_decorators = ctx.ignore_decorators;
    member.has_decorator
        && (member.decorator_names.is_empty()
            || ignore_decorators.is_empty()
            || member
                .decorator_names
                .iter()
                .any(|name| !ignore_decorators.matches(name)))
}

fn is_class_member_kind(kind: MemberKind) -> bool {
    matches!(kind, MemberKind::ClassMethod | MemberKind::ClassProperty)
}

fn class_member_runtime_credit_applies(member: &MemberInfo, ctx: &MemberSkipContext<'_>) -> bool {
    is_class_member_kind(member.kind)
        && (is_react_lifecycle_method(&member.name)
            || is_angular_lifecycle_method(&member.name)
            || is_native_custom_element_lifecycle_method(&member.name, ctx.super_class)
            || is_error_subclass_runtime_member(
                &member.name,
                ctx.export_key,
                ctx.error_subclass_keys,
            )
            || is_ol_interaction_dispatched_member(
                &member.name,
                ctx.export_key,
                ctx.ol_interaction_subclass_keys,
            )
            || ctx.allowlist.matches(
                member.name.as_str(),
                ctx.super_class,
                ctx.implemented_interfaces,
            ))
}

fn record_seen_ignore_decorators(graph: &ModuleGraph, ignore_decorators: &IgnoreDecoratorSet) {
    if ignore_decorators.is_empty() {
        return;
    }
    for module in &graph.modules {
        for export in &module.exports {
            for member in &export.members {
                for decorator in &member.decorator_names {
                    ignore_decorators.record_seen(decorator);
                }
            }
        }
    }
}

struct MemberAccessCollections {
    accessed_members: FxHashMap<ExportKey, FxHashSet<String>>,
    self_accessed_members: FxHashMap<FileId, FxHashSet<String>>,
    whole_object_used_exports: FxHashSet<ExportKey>,
}

fn collect_direct_member_accesses(
    resolved_modules: &[ResolvedModule],
    indexes: &MemberPassIndexes<'_>,
) -> MemberAccessCollections {
    let mut accessed_members: FxHashMap<ExportKey, FxHashSet<String>> = FxHashMap::default();
    let mut self_accessed_members: FxHashMap<FileId, FxHashSet<String>> = FxHashMap::default();
    let mut whole_object_used_exports: FxHashSet<ExportKey> = FxHashSet::default();

    for resolved in resolved_modules {
        let local_to_export_keys = indexes.local_keys(resolved.file_id);
        for access in SemanticFactView::new(&resolved.semantic_facts, &resolved.member_accesses)
            .ordinary_member_accesses()
        {
            if access.object == "this" {
                self_accessed_members
                    .entry(resolved.file_id)
                    .or_default()
                    .insert(access.member.clone());
                continue;
            }
            if let Some(export_keys) = local_to_export_keys.get(access.object.as_str()) {
                for export_key in export_keys {
                    accessed_members
                        .entry(export_key.clone())
                        .or_default()
                        .insert(access.member.clone());
                }
            }
        }

        for local_name in &resolved.whole_object_uses {
            if let Some(export_keys) = local_to_export_keys.get(local_name.as_str()) {
                whole_object_used_exports.extend(export_keys.iter().cloned());
            }
        }
    }

    MemberAccessCollections {
        accessed_members,
        self_accessed_members,
        whole_object_used_exports,
    }
}

#[cfg(test)]
mod tests;
