use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};
use globset::GlobMatcher;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::discover::FileId;
use crate::extract::{
    ANGULAR_TPL_SENTINEL, ExportName, FACTORY_CALL_SENTINEL, FLUENT_CHAIN_NEW_SENTINEL,
    FLUENT_CHAIN_SENTINEL, INSTANCE_EXPORT_SENTINEL, MemberInfo, MemberKind, ModuleInfo,
    PLAYWRIGHT_FIXTURE_ALIAS_SENTINEL, PLAYWRIGHT_FIXTURE_DEF_SENTINEL,
    PLAYWRIGHT_FIXTURE_TYPE_SENTINEL, PLAYWRIGHT_FIXTURE_USE_SENTINEL,
};
use crate::graph::{ModuleGraph, ReferenceKind};
use crate::resolve::ResolvedModule;
use crate::results::UnusedMember;
use crate::suppress::{IssueKind, SuppressionContext};

use super::predicates::{is_angular_lifecycle_method, is_react_lifecycle_method};
use super::{LineOffsetsMap, byte_offset_to_line_col};

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
    allowlist: &'a ClassMemberAllowlist<'a>,
    super_class: Option<&'a str>,
    implemented_interfaces: &'a [String],
    is_public_api_class_export: bool,
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

fn heritage_clause(rule: &ScopedUsedClassMemberRule) -> String {
    match (rule.extends.as_deref(), rule.implements.as_deref()) {
        (Some(extends), Some(implements)) => {
            format!("extends='{extends}', implements='{implements}'")
        }
        (Some(extends), None) => format!("extends='{extends}'"),
        (None, Some(implements)) => format!("implements='{implements}'"),
        (None, None) => "unconstrained heritage".to_string(),
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

/// Credit one Angular external-template member access `<object>.<member>` whose
/// `object` is a component field bound to `type_name` (issue #920).
///
/// `type_name` is the field's binding target: a concrete class, an interface
/// (the typed `x: Greeter = inject(TOKEN)` form), or an `InjectionToken` const
/// (the untyped `x = inject(TOKEN)` form). The candidate interface set is the
/// binding name itself plus the interface declared by any `InjectionToken` the
/// binding resolves to; the member is credited on every class implementing any
/// candidate interface (and directly on the resolved export, harmless for a
/// memberless token const).
fn credit_angular_token_chain_member<'a>(
    graph: &ModuleGraph,
    type_name: &'a str,
    member: &str,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    token_to_interface: &FxHashMap<ExportKey, &'a str>,
    implementers_by_name: &FxHashMap<&'a str, Vec<ExportKey>>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    let mut interface_names: Vec<&str> = vec![type_name];
    if let Some(export_keys) = local_to_export_keys.get(type_name) {
        for export_key in export_keys {
            accessed_members
                .entry(export_key.clone())
                .or_default()
                .insert(member.to_string());
            for resolved in export_key_with_origins(graph, export_key) {
                if let Some(interface) = token_to_interface.get(&resolved) {
                    interface_names.push(interface);
                }
            }
        }
    }
    for interface in interface_names {
        let Some(implementers) = implementers_by_name.get(interface) else {
            continue;
        };
        for implementer_key in implementers {
            accessed_members
                .entry(implementer_key.clone())
                .or_default()
                .insert(member.to_string());
        }
    }
}

fn build_angular_template_refs(
    resolved_modules: &[ResolvedModule],
) -> FxHashMap<FileId, Vec<&str>> {
    resolved_modules
        .iter()
        .filter_map(|module| {
            let refs: Vec<&str> = module
                .member_accesses
                .iter()
                .filter(|access| access.object == ANGULAR_TPL_SENTINEL)
                .map(|access| access.member.as_str())
                .collect();
            if refs.is_empty() {
                None
            } else {
                Some((module.file_id, refs))
            }
        })
        .collect()
}

fn build_angular_template_chain_accesses(
    resolved_modules: &[ResolvedModule],
) -> FxHashMap<FileId, Vec<(&str, &str)>> {
    resolved_modules
        .iter()
        .filter_map(|module| {
            if !module
                .member_accesses
                .iter()
                .any(|access| access.object == ANGULAR_TPL_SENTINEL)
            {
                return None;
            }
            let chains: Vec<(&str, &str)> = module
                .member_accesses
                .iter()
                .filter(|access| {
                    access.object != ANGULAR_TPL_SENTINEL
                        && access.object != "this"
                        && !access.object.starts_with(INSTANCE_EXPORT_SENTINEL)
                        && !access.object.starts_with(FACTORY_CALL_SENTINEL)
                        && !access.object.starts_with(FLUENT_CHAIN_SENTINEL)
                        && !access.object.starts_with(FLUENT_CHAIN_NEW_SENTINEL)
                })
                .map(|access| (access.object.as_str(), access.member.as_str()))
                .collect();
            if chains.is_empty() {
                None
            } else {
                Some((module.file_id, chains))
            }
        })
        .collect()
}

struct AngularTemplateRefContext<'a, 'b> {
    refs: &'b FxHashMap<FileId, Vec<&'a str>>,
    self_accessed_members: &'b mut FxHashMap<FileId, FxHashSet<String>>,
}

impl AngularTemplateRefContext<'_, '_> {
    fn propagate(&mut self, resolved_modules: &[ResolvedModule]) {
        if self.refs.is_empty() {
            return;
        }

        for resolved in resolved_modules {
            if let Some(refs) = self.refs.get(&resolved.file_id) {
                let entry = self
                    .self_accessed_members
                    .entry(resolved.file_id)
                    .or_default();
                for &ref_name in refs {
                    entry.insert(ref_name.to_string());
                }
            }
            for import in resolved.all_resolved_imports() {
                if let Some(target_id) = import.target.internal_file_id()
                    && let Some(refs) = self.refs.get(&target_id)
                {
                    let entry = self
                        .self_accessed_members
                        .entry(resolved.file_id)
                        .or_default();
                    for &ref_name in refs {
                        entry.insert(ref_name.to_string());
                    }
                }
            }
        }
    }
}

fn component_instance_bindings(
    class_heritage: &[fallow_types::extract::ClassHeritageInfo],
) -> FxHashMap<&str, &str> {
    class_heritage
        .iter()
        .flat_map(|heritage| {
            heritage
                .instance_bindings
                .iter()
                .map(|(local, ty)| (local.as_str(), ty.as_str()))
        })
        .collect()
}

struct MemberHeritageContext<'a> {
    class_heritage_by_export: FxHashMap<ExportKey, (Option<String>, Vec<String>)>,
    class_heritage_by_file: FxHashMap<FileId, &'a [fallow_types::extract::ClassHeritageInfo]>,
    token_to_interface: FxHashMap<ExportKey, &'a str>,
    implementers_by_name: FxHashMap<&'a str, Vec<ExportKey>>,
    interface_to_implementers: FxHashMap<ExportKey, Vec<ExportKey>>,
}

fn build_member_heritage_context<'a>(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules: &'a [ModuleInfo],
) -> MemberHeritageContext<'a> {
    let mut class_heritage_by_export: FxHashMap<ExportKey, (Option<String>, Vec<String>)> =
        FxHashMap::default();
    let mut class_heritage_by_file = FxHashMap::default();
    let mut token_to_interface: FxHashMap<ExportKey, &str> = FxHashMap::default();
    let mut implementers_by_name: FxHashMap<&str, Vec<ExportKey>> = FxHashMap::default();

    for module in modules {
        class_heritage_by_file.insert(module.file_id, module.class_heritage.as_slice());
        class_heritage_by_export.extend(module.class_heritage.iter().map(|heritage| {
            (
                ExportKey::new(module.file_id, heritage.export_name.clone()),
                (heritage.super_class.clone(), heritage.implements.clone()),
            )
        }));
        for (token_name, interface_name) in &module.injection_tokens {
            token_to_interface.insert(
                ExportKey::new(module.file_id, token_name.clone()),
                interface_name.as_str(),
            );
        }
        for heritage in &module.class_heritage {
            let implementer_key = ExportKey::new(module.file_id, heritage.export_name.clone());
            for interface_name in &heritage.implements {
                implementers_by_name
                    .entry(interface_name.as_str())
                    .or_default()
                    .push(implementer_key.clone());
            }
        }
    }

    let interface_to_implementers =
        build_interface_to_implementers(graph, resolved_modules, &class_heritage_by_file);

    MemberHeritageContext {
        class_heritage_by_export,
        class_heritage_by_file,
        token_to_interface,
        implementers_by_name,
        interface_to_implementers,
    }
}

fn propagate_interface_member_accesses(
    interface_to_implementers: &FxHashMap<ExportKey, Vec<ExportKey>>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    if interface_to_implementers.is_empty() {
        return;
    }

    let mut propagations: Vec<(ExportKey, Vec<String>)> = Vec::new();
    for (interface_key, implementer_keys) in interface_to_implementers {
        let Some(interface_accesses) = accessed_members.get(interface_key) else {
            continue;
        };
        let accesses: Vec<String> = interface_accesses.iter().cloned().collect();
        for implementer_key in implementer_keys {
            propagations.push((implementer_key.clone(), accesses.clone()));
        }
    }

    for (implementer_key, accesses) in propagations {
        accessed_members
            .entry(implementer_key)
            .or_default()
            .extend(accesses);
    }
}

fn propagate_angular_template_member_accesses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    heritage_context: &MemberHeritageContext<'_>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
    self_accessed_members: &mut FxHashMap<FileId, FxHashSet<String>>,
) {
    let angular_tpl_refs = build_angular_template_refs(resolved_modules);
    let mut angular_ref_context = AngularTemplateRefContext {
        refs: &angular_tpl_refs,
        self_accessed_members,
    };
    angular_ref_context.propagate(resolved_modules);

    let angular_tpl_chain_accesses = build_angular_template_chain_accesses(resolved_modules);
    let mut angular_chain_context = AngularTemplateChainContext {
        graph,
        class_heritage_by_file: &heritage_context.class_heritage_by_file,
        chain_accesses: &angular_tpl_chain_accesses,
        token_to_interface: &heritage_context.token_to_interface,
        implementers_by_name: &heritage_context.implementers_by_name,
        accessed_members,
    };
    angular_chain_context.propagate(resolved_modules);
}

struct AngularTemplateChainContext<'a, 'b> {
    graph: &'b ModuleGraph,
    class_heritage_by_file: &'b FxHashMap<FileId, &'a [fallow_types::extract::ClassHeritageInfo]>,
    chain_accesses: &'b FxHashMap<FileId, Vec<(&'b str, &'b str)>>,
    token_to_interface: &'b FxHashMap<ExportKey, &'a str>,
    implementers_by_name: &'b FxHashMap<&'a str, Vec<ExportKey>>,
    accessed_members: &'b mut FxHashMap<ExportKey, FxHashSet<String>>,
}

struct AngularTemplateComponentContext<'a, 'b> {
    component_bindings: FxHashMap<&'a str, &'a str>,
    local_to_export_keys: FxHashMap<&'b str, Vec<ExportKey>>,
}

impl<'a> AngularTemplateChainContext<'a, '_> {
    fn credit_members(
        &mut self,
        chains: &[(&str, &str)],
        component: &AngularTemplateComponentContext<'a, '_>,
    ) {
        for (object, member) in chains {
            let Some(type_name) = component.component_bindings.get(object) else {
                continue;
            };
            credit_angular_token_chain_member(
                self.graph,
                type_name,
                member,
                &component.local_to_export_keys,
                self.token_to_interface,
                self.implementers_by_name,
                self.accessed_members,
            );
        }
    }

    fn propagate(&mut self, resolved_modules: &[ResolvedModule]) {
        if self.chain_accesses.is_empty() {
            return;
        }

        for resolved in resolved_modules {
            let Some(class_heritage) = self.class_heritage_by_file.get(&resolved.file_id) else {
                continue;
            };
            if class_heritage.is_empty() {
                continue;
            }
            let component = AngularTemplateComponentContext {
                component_bindings: component_instance_bindings(class_heritage),
                local_to_export_keys: build_local_to_export_keys(resolved),
            };
            if component.component_bindings.is_empty() {
                continue;
            }
            if let Some(chains) = self.chain_accesses.get(&resolved.file_id) {
                self.credit_members(chains, &component);
            }
            for import in resolved.all_resolved_imports() {
                let Some(target_id) = import.target.internal_file_id() else {
                    continue;
                };
                let Some(chains) = self.chain_accesses.get(&target_id) else {
                    continue;
                };
                self.credit_members(chains, &component);
            }
        }
    }
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

fn parse_playwright_fixture_sentinel<'a>(
    object: &'a str,
    prefix: &str,
) -> Option<(&'a str, &'a str)> {
    object.strip_prefix(prefix)?.split_once(':')
}

fn build_playwright_fixture_targets(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
) -> FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>> {
    let type_targets = build_playwright_fixture_type_targets(graph, resolved_modules);
    let mut targets_by_test: FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>> =
        FxHashMap::default();
    let mut aliases_by_test: FxHashMap<ExportKey, Vec<ExportKey>> = FxHashMap::default();

    for resolved in resolved_modules {
        let local_to_export_keys = build_local_to_export_keys(resolved);
        for access in &resolved.member_accesses {
            let Some((test_local_name, fixture_name)) = parse_playwright_fixture_sentinel(
                access.object.as_str(),
                PLAYWRIGHT_FIXTURE_DEF_SENTINEL,
            ) else {
                continue;
            };
            let Some(test_keys) = local_to_export_keys.get(test_local_name) else {
                continue;
            };
            let Some(target_keys) = local_to_export_keys.get(access.member.as_str()) else {
                continue;
            };

            for test_key in test_keys {
                let fixture_targets = targets_by_test.entry(test_key.clone()).or_default();
                for target_key in target_keys {
                    push_playwright_fixture_target(
                        graph,
                        &type_targets,
                        fixture_targets,
                        fixture_name,
                        target_key,
                    );
                }
            }
        }

        for access in &resolved.member_accesses {
            let Some((test_local_name, _)) = parse_playwright_fixture_sentinel(
                access.object.as_str(),
                PLAYWRIGHT_FIXTURE_ALIAS_SENTINEL,
            ) else {
                continue;
            };
            let Some(test_keys) = local_to_export_keys.get(test_local_name) else {
                continue;
            };
            let Some(base_keys) = local_to_export_keys.get(access.member.as_str()) else {
                continue;
            };

            for test_key in test_keys {
                let aliases = aliases_by_test.entry(test_key.clone()).or_default();
                for base_key in base_keys {
                    for key in export_key_with_origins(graph, base_key) {
                        push_export_key(aliases, key);
                    }
                }
            }
        }
    }

    expand_playwright_fixture_aliases(&mut targets_by_test, &aliases_by_test);
    targets_by_test
}

fn expand_playwright_fixture_aliases(
    targets_by_test: &mut FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    aliases_by_test: &FxHashMap<ExportKey, Vec<ExportKey>>,
) {
    if aliases_by_test.is_empty() {
        return;
    }

    let max_iters = aliases_by_test.len() + 1;
    for _ in 0..max_iters {
        let snapshot = targets_by_test.clone();
        let mut changed = false;
        for (alias_key, base_keys) in aliases_by_test {
            for base_key in base_keys {
                let Some(base_targets) = snapshot.get(base_key) else {
                    continue;
                };
                let alias_targets = targets_by_test.entry(alias_key.clone()).or_default();
                for (fixture_name, target_keys) in base_targets {
                    let fixture_targets = alias_targets.entry(fixture_name.clone()).or_default();
                    for target_key in target_keys {
                        let before = fixture_targets.len();
                        push_export_key(fixture_targets, target_key.clone());
                        changed |= fixture_targets.len() != before;
                    }
                }
            }
        }
        if !changed {
            break;
        }
    }
}

fn push_playwright_fixture_target(
    graph: &ModuleGraph,
    type_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    fixture_targets: &mut FxHashMap<String, Vec<ExportKey>>,
    fixture_name: &str,
    target_key: &ExportKey,
) {
    let origin_keys = export_key_with_origins(graph, target_key);
    for key in &origin_keys {
        push_export_key(
            fixture_targets.entry(fixture_name.to_string()).or_default(),
            key.clone(),
        );
    }
    for alias_key in origin_keys {
        push_playwright_fixture_type_target(
            type_targets,
            fixture_targets,
            fixture_name,
            &alias_key,
        );
    }
}

fn push_playwright_fixture_type_target(
    type_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    fixture_targets: &mut FxHashMap<String, Vec<ExportKey>>,
    fixture_name: &str,
    alias_key: &ExportKey,
) {
    let Some(alias_targets) = type_targets.get(alias_key) else {
        return;
    };
    for (suffix, nested_targets) in alias_targets {
        let nested_fixture_name = format!("{fixture_name}.{suffix}");
        let fixture_targets = fixture_targets.entry(nested_fixture_name).or_default();
        for nested_target in nested_targets {
            push_export_key(fixture_targets, nested_target.clone());
        }
    }
}

fn build_playwright_fixture_type_targets(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
) -> FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>> {
    let mut targets_by_alias: FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>> =
        FxHashMap::default();

    for resolved in resolved_modules {
        let local_to_export_keys = build_local_to_export_keys(resolved);
        for access in &resolved.member_accesses {
            let Some((alias_name, fixture_name)) = parse_playwright_fixture_sentinel(
                access.object.as_str(),
                PLAYWRIGHT_FIXTURE_TYPE_SENTINEL,
            ) else {
                continue;
            };
            let Some(alias_keys) = local_to_export_keys.get(alias_name) else {
                continue;
            };
            let Some(target_keys) = local_to_export_keys.get(access.member.as_str()) else {
                continue;
            };

            for alias_key in alias_keys {
                let alias_targets = targets_by_alias.entry(alias_key.clone()).or_default();
                let fixture_targets = alias_targets.entry(fixture_name.to_string()).or_default();
                for target_key in target_keys {
                    for key in export_key_with_origins(graph, target_key) {
                        push_export_key(fixture_targets, key);
                    }
                }
            }
        }
    }

    targets_by_alias
}

fn propagate_playwright_fixture_accesses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    let targets_by_test = build_playwright_fixture_targets(graph, resolved_modules);
    if targets_by_test.is_empty() {
        return;
    }

    for resolved in resolved_modules {
        let local_to_export_keys = build_local_to_export_keys(resolved);
        for access in &resolved.member_accesses {
            let Some((test_local_name, fixture_name)) = parse_playwright_fixture_sentinel(
                access.object.as_str(),
                PLAYWRIGHT_FIXTURE_USE_SENTINEL,
            ) else {
                continue;
            };
            let Some(test_keys) = local_to_export_keys.get(test_local_name) else {
                continue;
            };

            for test_key in test_keys {
                let Some(fixture_targets) = targets_by_test.get(test_key) else {
                    continue;
                };
                let Some(target_keys) = fixture_targets.get(fixture_name) else {
                    continue;
                };
                for target_key in target_keys {
                    accessed_members
                        .entry(target_key.clone())
                        .or_default()
                        .insert(access.member.clone());
                }
            }
        }
    }
}

fn build_instance_export_targets(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
) -> FxHashMap<ExportKey, Vec<ExportKey>> {
    let mut targets_by_instance: FxHashMap<ExportKey, Vec<ExportKey>> = FxHashMap::default();

    for resolved in resolved_modules {
        let local_to_export_keys = build_local_to_export_keys(resolved);
        for access in &resolved.member_accesses {
            let Some(instance_export_name) = access.object.strip_prefix(INSTANCE_EXPORT_SENTINEL)
            else {
                continue;
            };
            let Some(target_keys) = local_to_export_keys.get(access.member.as_str()) else {
                continue;
            };

            let instance_key = ExportKey::new(resolved.file_id, instance_export_name);
            let instance_targets = targets_by_instance.entry(instance_key).or_default();
            for target_key in target_keys {
                for key in export_key_with_origins(graph, target_key) {
                    push_export_key(instance_targets, key);
                }
            }
        }
    }

    targets_by_instance
}

fn propagate_accesses_through_instance_exports(
    instance_targets: &FxHashMap<ExportKey, Vec<ExportKey>>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
    whole_object_used_exports: &mut FxHashSet<ExportKey>,
) {
    if instance_targets.is_empty() {
        return;
    }

    let accessed_snapshot: Vec<(ExportKey, Vec<String>)> = accessed_members
        .iter()
        .map(|(key, members)| (key.clone(), members.iter().cloned().collect()))
        .collect();
    for (instance_key, members) in accessed_snapshot {
        let Some(target_keys) = instance_targets.get(&instance_key) else {
            continue;
        };
        for target_key in target_keys {
            accessed_members
                .entry(target_key.clone())
                .or_default()
                .extend(members.iter().cloned());
        }
    }

    let whole_snapshot: Vec<ExportKey> = whole_object_used_exports.iter().cloned().collect();
    for instance_key in whole_snapshot {
        let Some(target_keys) = instance_targets.get(&instance_key) else {
            continue;
        };
        whole_object_used_exports.extend(target_keys.iter().cloned());
    }
}

fn build_typed_instance_binding_targets(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules: &[ModuleInfo],
) -> FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>> {
    let resolved_by_file: FxHashMap<FileId, &ResolvedModule> = resolved_modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();
    let mut targets_by_class: FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>> =
        FxHashMap::default();

    for module in modules {
        let Some(resolved) = resolved_by_file.get(&module.file_id) else {
            continue;
        };
        let local_to_export_keys = build_local_to_export_keys(resolved);
        for heritage in &module.class_heritage {
            if heritage.instance_bindings.is_empty() {
                continue;
            }
            let class_key = ExportKey::new(module.file_id, heritage.export_name.clone());
            let member_targets = targets_by_class.entry(class_key).or_default();

            for (member_name, type_name) in &heritage.instance_bindings {
                let Some(seed_keys) = local_to_export_keys.get(type_name.as_str()) else {
                    continue;
                };
                let targets = member_targets.entry(member_name.clone()).or_default();
                for seed_key in seed_keys {
                    for key in export_key_with_origins(graph, seed_key) {
                        push_export_key(targets, key);
                    }
                }
            }
        }
    }

    targets_by_class
}

fn chained_typed_instance_targets(
    graph: &ModuleGraph,
    typed_instance_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    seed_key: &ExportKey,
    segments: &[&str],
) -> Vec<ExportKey> {
    let mut current = export_key_with_origins(graph, seed_key);

    for segment in segments {
        let mut next = Vec::new();
        for class_key in &current {
            let Some(member_targets) = typed_instance_targets.get(class_key) else {
                continue;
            };
            let Some(targets) = member_targets.get(*segment) else {
                continue;
            };
            for target in targets {
                push_export_key(&mut next, target.clone());
            }
        }
        if next.is_empty() {
            return Vec::new();
        }
        current = next;
    }

    current
}

fn resolve_typed_instance_chain_targets(
    graph: &ModuleGraph,
    typed_instance_targets: &FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>>,
    local_to_export_keys: &FxHashMap<&str, Vec<ExportKey>>,
    object_name: &str,
) -> Vec<ExportKey> {
    let mut segments = object_name.split('.');
    let Some(root_local) = segments.next() else {
        return Vec::new();
    };
    let path_segments: Vec<&str> = segments.collect();
    if path_segments.is_empty() {
        return Vec::new();
    }
    let Some(root_keys) = local_to_export_keys.get(root_local) else {
        return Vec::new();
    };

    let mut targets = Vec::new();
    for root_key in root_keys {
        for target_key in
            chained_typed_instance_targets(graph, typed_instance_targets, root_key, &path_segments)
        {
            push_export_key(&mut targets, target_key);
        }
    }
    targets
}

fn propagate_accesses_through_typed_instance_bindings(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules: &[ModuleInfo],
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
    whole_object_used_exports: &mut FxHashSet<ExportKey>,
) {
    let typed_instance_targets =
        build_typed_instance_binding_targets(graph, resolved_modules, modules);
    if typed_instance_targets.is_empty() {
        return;
    }

    for resolved in resolved_modules {
        let local_to_export_keys = build_local_to_export_keys(resolved);
        for access in &resolved.member_accesses {
            if access.object.starts_with(INSTANCE_EXPORT_SENTINEL)
                || access.object.starts_with(FACTORY_CALL_SENTINEL)
                || access.object.starts_with(FLUENT_CHAIN_SENTINEL)
                || access.object.starts_with(FLUENT_CHAIN_NEW_SENTINEL)
                || access.object.starts_with(PLAYWRIGHT_FIXTURE_ALIAS_SENTINEL)
                || access.object.starts_with(PLAYWRIGHT_FIXTURE_DEF_SENTINEL)
                || access.object.starts_with(PLAYWRIGHT_FIXTURE_TYPE_SENTINEL)
                || access.object.starts_with(PLAYWRIGHT_FIXTURE_USE_SENTINEL)
                || access.object == ANGULAR_TPL_SENTINEL
            {
                continue;
            }

            for target_key in resolve_typed_instance_chain_targets(
                graph,
                &typed_instance_targets,
                &local_to_export_keys,
                &access.object,
            ) {
                accessed_members
                    .entry(target_key)
                    .or_default()
                    .insert(access.member.clone());
            }
        }

        for object_name in &resolved.whole_object_uses {
            if object_name.starts_with(INSTANCE_EXPORT_SENTINEL)
                || object_name.starts_with(FACTORY_CALL_SENTINEL)
                || object_name.starts_with(PLAYWRIGHT_FIXTURE_ALIAS_SENTINEL)
                || object_name.starts_with(PLAYWRIGHT_FIXTURE_DEF_SENTINEL)
                || object_name.starts_with(PLAYWRIGHT_FIXTURE_TYPE_SENTINEL)
                || object_name.starts_with(PLAYWRIGHT_FIXTURE_USE_SENTINEL)
                || object_name == ANGULAR_TPL_SENTINEL
            {
                continue;
            }

            for target_key in resolve_typed_instance_chain_targets(
                graph,
                &typed_instance_targets,
                &local_to_export_keys,
                object_name,
            ) {
                whole_object_used_exports.insert(target_key);
            }
        }
    }
}

/// Decode a `FACTORY_CALL_SENTINEL{callee_object}:{callee_method}` access
/// object into its components.
fn parse_factory_call_sentinel(object: &str) -> Option<(&str, &str)> {
    object
        .strip_prefix(FACTORY_CALL_SENTINEL)
        .and_then(|payload| payload.split_once(':'))
}

/// Credit member accesses produced by static-factory call bindings on the
/// originating class export.
fn propagate_factory_call_accesses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    let module_by_id: FxHashMap<FileId, &ResolvedModule> = resolved_modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();

    for resolved in resolved_modules {
        let local_to_export_keys = build_local_to_export_keys(resolved);
        for access in &resolved.member_accesses {
            let Some((callee_object, callee_method)) =
                parse_factory_call_sentinel(access.object.as_str())
            else {
                continue;
            };
            let Some(seed_keys) = local_to_export_keys.get(callee_object) else {
                continue;
            };
            for seed_key in seed_keys {
                for origin in
                    walk_re_export_origins(graph, seed_key.file_id, seed_key.export_name.as_str())
                {
                    let Some(origin_module) = module_by_id.get(&origin.file_id) else {
                        continue;
                    };
                    let matches_factory = origin_module.exports.iter().any(|export| {
                        export.name.matches_str(origin.export_name.as_str())
                            && export.members.iter().any(|member| {
                                member.is_instance_returning_static
                                    && member.kind == MemberKind::ClassMethod
                                    && member.name == callee_method
                            })
                    });
                    if !matches_factory {
                        continue;
                    }
                    accessed_members
                        .entry(origin)
                        .or_default()
                        .insert(access.member.clone());
                }
            }
        }
    }
}

/// Decode a `FLUENT_CHAIN_SENTINEL{root}:{root_method}:{chain}` access object
/// into its components.
fn parse_fluent_chain_sentinel(object: &str) -> Option<(&str, &str, Vec<&str>)> {
    let payload = object.strip_prefix(FLUENT_CHAIN_SENTINEL)?;
    let (root, rest) = payload.split_once(':')?;
    let (root_method, chain_str) = rest.split_once(':')?;
    let chain: Vec<&str> = if chain_str.is_empty() {
        Vec::new()
    } else {
        chain_str.split(',').collect()
    };
    Some((root, root_method, chain))
}

/// Validate a fluent chain against a single class export.
fn export_validates_fluent_chain(
    export: &crate::extract::ExportInfo,
    origin: &ExportKey,
    root_method: &str,
    chain: &[&str],
) -> bool {
    if !export.name.matches_str(origin.export_name.as_str()) {
        return false;
    }
    let has_factory = export.members.iter().any(|member| {
        member.is_instance_returning_static
            && member.kind == MemberKind::ClassMethod
            && member.name == root_method
    });
    if !has_factory {
        return false;
    }
    chain.iter().all(|step| {
        export.members.iter().any(|member| {
            member.kind == MemberKind::ClassMethod
                && member.name == *step
                && member.is_self_returning
        })
    })
}

/// Credit member accesses produced by fluent-builder chain calls.
fn propagate_fluent_chain_accesses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    let module_by_id: FxHashMap<FileId, &ResolvedModule> = resolved_modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();

    for resolved in resolved_modules {
        let local_to_export_keys = build_local_to_export_keys(resolved);
        for access in &resolved.member_accesses {
            let Some((root_local, root_method, chain)) =
                parse_fluent_chain_sentinel(access.object.as_str())
            else {
                continue;
            };
            let Some(seed_keys) = local_to_export_keys.get(root_local) else {
                continue;
            };
            for seed_key in seed_keys {
                for origin in
                    walk_re_export_origins(graph, seed_key.file_id, seed_key.export_name.as_str())
                {
                    let Some(origin_module) = module_by_id.get(&origin.file_id) else {
                        continue;
                    };
                    let chain_valid = origin_module.exports.iter().any(|export| {
                        export_validates_fluent_chain(export, &origin, root_method, &chain)
                    });
                    if !chain_valid {
                        continue;
                    }
                    accessed_members
                        .entry(origin)
                        .or_default()
                        .insert(access.member.clone());
                }
            }
        }
    }
}

/// Decode a `FLUENT_CHAIN_NEW_SENTINEL{class}:{chain}` access object into its
/// components.
fn parse_fluent_chain_new_sentinel(object: &str) -> Option<(&str, Vec<&str>)> {
    let payload = object.strip_prefix(FLUENT_CHAIN_NEW_SENTINEL)?;
    let (class, chain_str) = payload.split_once(':')?;
    if chain_str.is_empty() {
        return None;
    }
    Some((class, chain_str.split(',').collect()))
}

/// Validate a constructor-rooted fluent chain against a single class export.
fn export_validates_fluent_chain_new(
    export: &crate::extract::ExportInfo,
    origin: &ExportKey,
    chain: &[&str],
) -> bool {
    if !export.name.matches_str(origin.export_name.as_str()) {
        return false;
    }
    chain.iter().all(|step| {
        export.members.iter().any(|member| {
            member.kind == MemberKind::ClassMethod
                && member.name == *step
                && member.is_self_returning
        })
    })
}

/// Credit member accesses produced by fluent chains rooted at `new`.
fn propagate_fluent_chain_new_accesses(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    let module_by_id: FxHashMap<FileId, &ResolvedModule> = resolved_modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();

    for resolved in resolved_modules {
        let local_to_export_keys = build_local_to_export_keys(resolved);
        for access in &resolved.member_accesses {
            let Some((class_local, chain)) =
                parse_fluent_chain_new_sentinel(access.object.as_str())
            else {
                continue;
            };
            let Some(seed_keys) = local_to_export_keys.get(class_local) else {
                continue;
            };
            for seed_key in seed_keys {
                for origin in
                    walk_re_export_origins(graph, seed_key.file_id, seed_key.export_name.as_str())
                {
                    let Some(origin_module) = module_by_id.get(&origin.file_id) else {
                        continue;
                    };
                    let chain_valid = origin_module
                        .exports
                        .iter()
                        .any(|export| export_validates_fluent_chain_new(export, &origin, &chain));
                    if !chain_valid {
                        continue;
                    }
                    accessed_members
                        .entry(origin)
                        .or_default()
                        .insert(access.member.clone());
                }
            }
        }
    }
}

/// Build `parent_export -> [child_export, ...]` from each exported class's
/// `extends` clause.
fn build_parent_to_children(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
) -> FxHashMap<ExportKey, Vec<ExportKey>> {
    let mut parent_to_children: FxHashMap<ExportKey, Vec<ExportKey>> = FxHashMap::default();

    for resolved in resolved_modules {
        let local_to_export_keys = build_local_to_export_keys(resolved);

        for export in &resolved.exports {
            if let Some(super_local) = &export.super_class {
                let Some(parent_keys) = local_to_export_keys.get(super_local.as_str()) else {
                    continue;
                };
                let child_key = ExportKey::new(resolved.file_id, export.name.to_string());

                for parent_key in parent_keys {
                    for resolved_parent_key in export_key_with_origins(graph, parent_key) {
                        let children = parent_to_children.entry(resolved_parent_key).or_default();
                        if !children.contains(&child_key) {
                            children.push(child_key.clone());
                        }
                    }
                }
            }
        }
    }

    parent_to_children
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

/// Propagate member accesses through `extends` chains in both directions.
fn propagate_class_inheritance(
    parent_to_children: &FxHashMap<ExportKey, Vec<ExportKey>>,
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
    self_accessed_members: &mut FxHashMap<FileId, FxHashSet<String>>,
) {
    if parent_to_children.is_empty() {
        return;
    }

    let mut propagations: Vec<(FileId, Vec<String>)> = Vec::new();

    for (parent_key, children) in parent_to_children {
        if let Some(parent_self_accesses) = self_accessed_members.get(&parent_key.file_id) {
            let accesses: Vec<String> = parent_self_accesses.iter().cloned().collect();
            for child_key in children {
                propagations.push((child_key.file_id, accesses.clone()));
            }
        }

        let mut child_self_accesses_for_parent: FxHashSet<String> = FxHashSet::default();
        for child_key in children {
            if let Some(child_self_accesses) = self_accessed_members.get(&child_key.file_id) {
                child_self_accesses_for_parent.extend(child_self_accesses.iter().cloned());
            }
        }
        if !child_self_accesses_for_parent.is_empty() {
            propagations.push((
                parent_key.file_id,
                child_self_accesses_for_parent.into_iter().collect(),
            ));
        }

        let parent_accesses = accessed_members.get(parent_key).cloned();
        let mut child_accesses_to_propagate: FxHashSet<String> = FxHashSet::default();

        for child_key in children {
            if let Some(child_accesses) = accessed_members.get(child_key) {
                child_accesses_to_propagate.extend(child_accesses.iter().cloned());
            }
        }

        if let Some(ref parent_acc) = parent_accesses {
            for child_key in children {
                accessed_members
                    .entry(child_key.clone())
                    .or_default()
                    .extend(parent_acc.iter().cloned());
            }
        }

        if !child_accesses_to_propagate.is_empty() {
            accessed_members
                .entry(parent_key.clone())
                .or_default()
                .extend(child_accesses_to_propagate);
        }
    }

    for (file_id, members) in propagations {
        let entry = self_accessed_members.entry(file_id).or_default();
        for member in members {
            entry.insert(member);
        }
    }
}

#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_cli::programmatic::detect_dead_code instead. NOTE: replacement returns serde_json::Value, not typed AnalysisResults. See docs/fallow-core-migration.md and ADR-008."
)]
#[allow(dead_code, reason = "kept for the deprecated fallow_core helper API")]
pub fn find_unused_members(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules: &[ModuleInfo],
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    user_class_member_allowlist: &[UsedClassMemberRule],
    ignore_decorators: &[String],
) -> (Vec<UnusedMember>, Vec<UnusedMember>) {
    let results = find_unused_members_with_public_api_entry_points(UnusedMemberScanInput {
        graph,
        resolved_modules,
        modules,
        suppressions,
        line_offsets_by_file,
        user_class_member_allowlist,
        ignore_decorators,
        public_api_entry_points: &FxHashSet::default(),
    });
    (results.enum_members, results.class_members)
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
}

struct PreparedMemberScan<'a> {
    heritage_context: MemberHeritageContext<'a>,
    accessed_members: FxHashMap<ExportKey, FxHashSet<String>>,
    self_accessed_members: FxHashMap<FileId, FxHashSet<String>>,
    whole_object_used_exports: FxHashSet<ExportKey>,
    entry_star_targets: FxHashSet<FileId>,
    error_subclass_keys: FxHashSet<ExportKey>,
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
        if should_skip_export_member_scan(self.input.graph, module, export) {
            return;
        }
        if store_only_scan
            && !export
                .members
                .iter()
                .any(|m| m.kind == MemberKind::StoreMember)
        {
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
            .get(&export_key)
            .map_or((None, &[][..]), |(super_class, interfaces)| {
                (super_class.as_deref(), interfaces.as_slice())
            });

        for member in &export.members {
            self.collect_member(
                module,
                member,
                &export_name,
                &MemberSkipContext {
                    export_key: &export_key,
                    accessed_members: &self.prepared.accessed_members,
                    file_self_accesses,
                    ignore_decorators: self.ignore_decorators,
                    error_subclass_keys: &self.prepared.error_subclass_keys,
                    allowlist: self.allowlist,
                    super_class,
                    implemented_interfaces,
                    is_public_api_class_export,
                },
                store_only_scan,
                buckets,
            );
        }
    }

    fn collect_member(
        &self,
        module: &crate::graph::ModuleNode,
        member: &MemberInfo,
        export_name: &str,
        skip_context: &MemberSkipContext<'_>,
        store_only_scan: bool,
        buckets: &mut MemberScanBuckets,
    ) {
        if store_only_scan && member.kind != MemberKind::StoreMember {
            return;
        }
        if should_skip_member_for_unused_report(member, skip_context) {
            return;
        }

        let Some(unused) = build_unsuppressed_unused_member(
            module.file_id,
            &module.path,
            export_name,
            member,
            self.input.suppressions,
            self.input.line_offsets_by_file,
        ) else {
            return;
        };
        push_unused_member(buckets, unused, member.kind);
    }
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
    let heritage_context =
        build_member_heritage_context(input.graph, input.resolved_modules, input.modules);

    let MemberAccessCollections {
        mut accessed_members,
        mut self_accessed_members,
        mut whole_object_used_exports,
    } = collect_direct_member_accesses(input.resolved_modules);

    propagate_playwright_fixture_accesses(
        input.graph,
        input.resolved_modules,
        &mut accessed_members,
    );
    propagate_factory_call_accesses(input.graph, input.resolved_modules, &mut accessed_members);
    propagate_fluent_chain_accesses(input.graph, input.resolved_modules, &mut accessed_members);
    propagate_fluent_chain_new_accesses(input.graph, input.resolved_modules, &mut accessed_members);
    propagate_accesses_through_typed_instance_bindings(
        input.graph,
        input.resolved_modules,
        input.modules,
        &mut accessed_members,
        &mut whole_object_used_exports,
    );
    propagate_accesses_through_re_exports(input.graph, &mut accessed_members);
    propagate_whole_object_through_re_exports(input.graph, &mut whole_object_used_exports);
    let instance_targets = build_instance_export_targets(input.graph, input.resolved_modules);
    propagate_accesses_through_instance_exports(
        &instance_targets,
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
        &heritage_context,
        &mut accessed_members,
        &mut self_accessed_members,
    );

    let parent_to_children = build_parent_to_children(input.graph, input.resolved_modules);

    propagate_class_inheritance(
        &parent_to_children,
        &mut accessed_members,
        &mut self_accessed_members,
    );

    let entry_star_targets =
        entry_point_star_re_export_targets(input.graph, input.public_api_entry_points);

    let error_subclass_keys = build_error_subclass_export_keys(
        &parent_to_children,
        &heritage_context.class_heritage_by_export,
    );

    PreparedMemberScan {
        heritage_context,
        accessed_members,
        self_accessed_members,
        whole_object_used_exports,
        entry_star_targets,
        error_subclass_keys,
    }
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

    if member.has_decorator
        && (member.decorator_names.is_empty()
            || ctx.ignore_decorators.is_empty()
            || member
                .decorator_names
                .iter()
                .any(|name| !ctx.ignore_decorators.matches(name)))
    {
        return true;
    }

    is_class_member_kind(member.kind)
        && (is_react_lifecycle_method(&member.name)
            || is_angular_lifecycle_method(&member.name)
            || is_native_custom_element_lifecycle_method(&member.name, ctx.super_class)
            || is_error_subclass_runtime_member(
                &member.name,
                ctx.export_key,
                ctx.error_subclass_keys,
            )
            || ctx.allowlist.matches(
                member.name.as_str(),
                ctx.super_class,
                ctx.implemented_interfaces,
            ))
}

fn is_class_member_kind(kind: MemberKind) -> bool {
    matches!(kind, MemberKind::ClassMethod | MemberKind::ClassProperty)
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

fn build_interface_to_implementers(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    class_heritage_by_file: &FxHashMap<FileId, &[fallow_types::extract::ClassHeritageInfo]>,
) -> FxHashMap<ExportKey, Vec<ExportKey>> {
    let mut interface_to_implementers: FxHashMap<ExportKey, Vec<ExportKey>> = FxHashMap::default();
    for resolved in resolved_modules {
        let Some(class_heritage) = class_heritage_by_file.get(&resolved.file_id) else {
            continue;
        };
        if class_heritage.is_empty() {
            continue;
        }

        let local_to_export_keys = build_local_to_export_keys(resolved);
        for heritage in *class_heritage {
            if heritage.implements.is_empty() {
                continue;
            }

            let implementer_key = ExportKey::new(resolved.file_id, heritage.export_name.clone());
            for interface_name in &heritage.implements {
                let Some(interface_keys) = local_to_export_keys.get(interface_name.as_str()) else {
                    continue;
                };
                for interface_key in interface_keys {
                    for resolved_interface_key in export_key_with_origins(graph, interface_key) {
                        let implementers = interface_to_implementers
                            .entry(resolved_interface_key)
                            .or_default();
                        if !implementers.contains(&implementer_key) {
                            implementers.push(implementer_key.clone());
                        }
                    }
                }
            }
        }
    }
    interface_to_implementers
}

struct MemberAccessCollections {
    accessed_members: FxHashMap<ExportKey, FxHashSet<String>>,
    self_accessed_members: FxHashMap<FileId, FxHashSet<String>>,
    whole_object_used_exports: FxHashSet<ExportKey>,
}

fn collect_direct_member_accesses(resolved_modules: &[ResolvedModule]) -> MemberAccessCollections {
    let mut accessed_members: FxHashMap<ExportKey, FxHashSet<String>> = FxHashMap::default();
    let mut self_accessed_members: FxHashMap<FileId, FxHashSet<String>> = FxHashMap::default();
    let mut whole_object_used_exports: FxHashSet<ExportKey> = FxHashSet::default();

    for resolved in resolved_modules {
        let local_to_export_keys = build_local_to_export_keys(resolved);
        for access in &resolved.member_accesses {
            if access.object.starts_with(INSTANCE_EXPORT_SENTINEL)
                || access.object.starts_with(FACTORY_CALL_SENTINEL)
                || access.object.starts_with(FLUENT_CHAIN_SENTINEL)
                || access.object.starts_with(FLUENT_CHAIN_NEW_SENTINEL)
            {
                continue;
            }
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
#[expect(
    deprecated,
    reason = "ADR-008 keeps direct detector unit tests while the public warning targets external callers"
)]
mod tests {
    use super::*;
    use crate::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};
    use crate::extract::{
        ExportInfo, ExportName, ImportInfo, ImportedName, MemberAccess, MemberInfo, MemberKind,
        ModuleInfo, VisibilityTag,
    };
    use crate::graph::{ExportSymbol, ModuleGraph, SymbolReference};
    use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule};
    use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};
    use fallow_types::extract::ClassHeritageInfo;
    use oxc_span::Span;
    use std::path::PathBuf;

    #[expect(
        clippy::cast_possible_truncation,
        reason = "test file counts are trivially small"
    )]
    fn build_graph(file_specs: &[(&str, bool)]) -> ModuleGraph {
        let files: Vec<DiscoveredFile> = file_specs
            .iter()
            .enumerate()
            .map(|(i, (path, _))| DiscoveredFile {
                id: FileId(i as u32),
                path: PathBuf::from(path),
                size_bytes: 0,
            })
            .collect();

        let entry_points: Vec<EntryPoint> = file_specs
            .iter()
            .filter(|(_, is_entry)| *is_entry)
            .map(|(path, _)| EntryPoint {
                path: PathBuf::from(path),
                source: EntryPointSource::ManualEntry,
            })
            .collect();

        let resolved_modules: Vec<ResolvedModule> = files
            .iter()
            .map(|f| ResolvedModule {
                file_id: f.id,
                path: f.path.clone(),
                ..Default::default()
            })
            .collect();

        ModuleGraph::build(&resolved_modules, &entry_points, &files)
    }

    fn make_member(name: &str, kind: MemberKind) -> MemberInfo {
        MemberInfo {
            name: name.to_string(),
            kind,
            span: Span::new(10, 20),
            has_decorator: false,
            decorator_names: Vec::new(),
            is_instance_returning_static: false,
            is_self_returning: false,
        }
    }

    fn make_export_with_members(
        name: &str,
        members: Vec<MemberInfo>,
        ref_from: Option<u32>,
    ) -> ExportSymbol {
        let references = ref_from
            .map(|from| {
                vec![SymbolReference {
                    from_file: FileId(from),
                    kind: crate::graph::ReferenceKind::NamedImport,
                    import_span: Span::new(0, 10),
                }]
            })
            .unwrap_or_default();
        ExportSymbol {
            name: ExportName::Named(name.to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: Span::new(0, 10),
            references,
            members,
        }
    }

    fn make_module_with_class_heritage(
        file_id: u32,
        export_name: &str,
        super_class: Option<&str>,
        implements: &[&str],
    ) -> ModuleInfo {
        ModuleInfo {
            file_id: FileId(file_id),
            exports: vec![],
            imports: vec![],
            re_exports: vec![],
            dynamic_imports: vec![],
            dynamic_import_patterns: vec![],
            require_calls: vec![],
            package_path_references: vec![],
            member_accesses: vec![],
            whole_object_uses: vec![],
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            content_hash: 0,
            suppressions: vec![],
            unknown_suppression_kinds: vec![],
            unused_import_bindings: vec![],
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            line_offsets: vec![],
            complexity: vec![],
            flag_uses: vec![],
            class_heritage: vec![ClassHeritageInfo {
                export_name: export_name.to_string(),
                super_class: super_class.map(str::to_string),
                implements: implements.iter().map(ToString::to_string).collect(),
                instance_bindings: Vec::new(),
            }],
            injection_tokens: Vec::new(),
            local_type_declarations: vec![],
            public_signature_type_references: vec![],
            namespace_object_aliases: vec![],
            iconify_prefixes: vec![],
            iconify_icon_names: vec![],
            auto_import_candidates: Vec::new(),
            directives: Vec::new(),
            client_only_dynamic_import_spans: Vec::new(),
            security_sinks: Vec::new(),
            security_sinks_skipped: 0,
            security_unresolved_callee_sites: Vec::new(),
            tainted_bindings: Vec::new(),
            sanitized_sink_args: Vec::new(),
            security_control_sites: Vec::new(),
            callee_uses: Vec::new(),
            misplaced_directives: Vec::new(),
            di_key_sites: Vec::new(),
            has_dynamic_provide: false,
            referenced_import_bindings: Vec::new(),
            component_props: Vec::new(),
            has_props_attrs_fallthrough: false,
            has_define_expose: false,
            has_define_model: false,
            has_unharvestable_props: false,
            component_emits: Vec::new(),
            has_unharvestable_emits: false,
            has_dynamic_emit: false,
            has_emit_whole_object_use: false,
            load_return_keys: Vec::new(),
            has_unharvestable_load: false,
            has_load_data_whole_use: false,
            has_page_data_store_whole_use: false,
            component_functions: Vec::new(),
            react_props: Vec::new(),
            hook_uses: Vec::new(),
            render_edges: Vec::new(),
        }
    }

    #[test]
    fn unused_members_empty_graph() {
        let graph = build_graph(&[]);

        let (enum_members, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert!(enum_members.is_empty());
        assert!(class_members.is_empty());
    }

    #[test]
    fn unused_enum_member_detected() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Status",
            vec![
                make_member("Active", MemberKind::EnumMember),
                make_member("Inactive", MemberKind::EnumMember),
            ],
            Some(0), // referenced from entry
        )];

        let (enum_members, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert_eq!(enum_members.len(), 2);
        assert!(class_members.is_empty());
        let names: FxHashSet<&str> = enum_members
            .iter()
            .map(|m| m.member_name.as_str())
            .collect();
        assert!(names.contains("Active"));
        assert!(names.contains("Inactive"));
    }

    #[test]
    fn accessed_enum_member_not_flagged() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Status",
            vec![
                make_member("Active", MemberKind::EnumMember),
                make_member("Inactive", MemberKind::EnumMember),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/src/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./enums".to_string(),
                    imported_name: ImportedName::Named("Status".to_string()),
                    local_name: "Status".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            member_accesses: vec![MemberAccess {
                object: "Status".to_string(),
                member: "Active".to_string(),
            }],
            ..Default::default()
        }];

        let (enum_members, _) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert_eq!(enum_members.len(), 1);
        assert_eq!(enum_members[0].member_name, "Inactive");
    }

    #[test]
    fn accessed_enum_member_via_re_export_not_flagged() {
        let mut graph = build_graph(&[
            ("/app/consumer.ts", true),
            ("/lib/index.ts", true),
            ("/lib/types.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[2].set_reachable(true);

        graph.modules[1].exports = vec![make_export_with_members(
            "Status",
            vec![],
            Some(0), // referenced from consumer
        )];
        graph.modules[1].re_exports = vec![crate::graph::ReExportEdge {
            source_file: FileId(2),
            imported_name: "Status".to_string(),
            exported_name: "Status".to_string(),
            is_type_only: false,
            span: Span::default(),
        }];

        graph.modules[2].exports = vec![make_export_with_members(
            "Status",
            vec![
                make_member("Active", MemberKind::EnumMember),
                make_member("Inactive", MemberKind::EnumMember),
                make_member("Archived", MemberKind::EnumMember),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/app/consumer.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "@scope/lib".to_string(),
                    imported_name: ImportedName::Named("Status".to_string()),
                    local_name: "Status".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            member_accesses: vec![
                MemberAccess {
                    object: "Status".to_string(),
                    member: "Active".to_string(),
                },
                MemberAccess {
                    object: "Status".to_string(),
                    member: "Inactive".to_string(),
                },
            ],
            ..Default::default()
        }];

        let (enum_members, _) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );

        assert_eq!(enum_members.len(), 1, "{enum_members:?}");
        assert_eq!(enum_members[0].member_name, "Archived");
        assert_eq!(enum_members[0].parent_name, "Status");
    }

    #[test]
    fn accessed_class_static_member_via_re_export_not_flagged() {
        let mut graph = build_graph(&[
            ("/app/consumer.ts", true),
            ("/lib/index.ts", true),
            ("/lib/utils.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[2].set_reachable(true);

        graph.modules[1].exports = vec![make_export_with_members("StringUtils", vec![], Some(0))];
        graph.modules[1].re_exports = vec![crate::graph::ReExportEdge {
            source_file: FileId(2),
            imported_name: "StringUtils".to_string(),
            exported_name: "StringUtils".to_string(),
            is_type_only: false,
            span: Span::default(),
        }];

        graph.modules[2].exports = vec![make_export_with_members(
            "StringUtils",
            vec![
                make_member("toUpper", MemberKind::ClassMethod),
                make_member("toLower", MemberKind::ClassMethod),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/app/consumer.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "@scope/lib".to_string(),
                    imported_name: ImportedName::Named("StringUtils".to_string()),
                    local_name: "StringUtils".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            member_accesses: vec![MemberAccess {
                object: "StringUtils".to_string(),
                member: "toUpper".to_string(),
            }],
            ..Default::default()
        }];

        let (_, class_members) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );

        assert_eq!(class_members.len(), 1, "{class_members:?}");
        assert_eq!(class_members[0].member_name, "toLower");
    }

    #[test]
    fn accessed_member_via_renamed_re_export_not_flagged() {
        let mut graph = build_graph(&[
            ("/app/consumer.ts", true),
            ("/lib/index.ts", true),
            ("/lib/types.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[2].set_reachable(true);

        graph.modules[1].exports = vec![make_export_with_members("Renamed", vec![], Some(0))];
        graph.modules[1].re_exports = vec![crate::graph::ReExportEdge {
            source_file: FileId(2),
            imported_name: "Original".to_string(),
            exported_name: "Renamed".to_string(),
            is_type_only: false,
            span: Span::default(),
        }];

        graph.modules[2].exports = vec![make_export_with_members(
            "Original",
            vec![
                make_member("A", MemberKind::EnumMember),
                make_member("B", MemberKind::EnumMember),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/app/consumer.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "@scope/lib".to_string(),
                    imported_name: ImportedName::Named("Renamed".to_string()),
                    local_name: "Renamed".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            member_accesses: vec![MemberAccess {
                object: "Renamed".to_string(),
                member: "A".to_string(),
            }],
            ..Default::default()
        }];

        let (enum_members, _) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );

        assert_eq!(enum_members.len(), 1, "{enum_members:?}");
        assert_eq!(enum_members[0].member_name, "B");
        assert_eq!(enum_members[0].parent_name, "Original");
    }

    #[test]
    fn accessed_member_via_star_re_export_not_flagged() {
        let mut graph = build_graph(&[
            ("/app/consumer.ts", true),
            ("/lib/index.ts", true),
            ("/lib/types.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[2].set_reachable(true);

        graph.modules[1].re_exports = vec![crate::graph::ReExportEdge {
            source_file: FileId(2),
            imported_name: "*".to_string(),
            exported_name: "*".to_string(),
            is_type_only: false,
            span: Span::default(),
        }];

        graph.modules[2].exports = vec![make_export_with_members(
            "Status",
            vec![
                make_member("Active", MemberKind::EnumMember),
                make_member("Inactive", MemberKind::EnumMember),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/app/consumer.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "@scope/lib".to_string(),
                    imported_name: ImportedName::Named("Status".to_string()),
                    local_name: "Status".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            member_accesses: vec![MemberAccess {
                object: "Status".to_string(),
                member: "Active".to_string(),
            }],
            ..Default::default()
        }];

        let (enum_members, _) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );

        assert_eq!(enum_members.len(), 1, "{enum_members:?}");
        assert_eq!(enum_members[0].member_name, "Inactive");
    }

    #[test]
    fn whole_object_use_skips_all_members() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Status",
            vec![
                make_member("Active", MemberKind::EnumMember),
                make_member("Inactive", MemberKind::EnumMember),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/src/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./enums".to_string(),
                    imported_name: ImportedName::Named("Status".to_string()),
                    local_name: "Status".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            whole_object_uses: vec!["Status".to_string()],
            ..Default::default()
        }];

        let (enum_members, class_members) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert!(enum_members.is_empty());
        assert!(class_members.is_empty());
    }

    #[test]
    fn decorated_class_member_not_flagged() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/entity.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "User",
            vec![MemberInfo {
                name: "name".to_string(),
                kind: MemberKind::ClassProperty,
                span: Span::new(10, 20),
                has_decorator: true, // @Column() etc.
                decorator_names: vec!["Column".to_string()],
                is_instance_returning_static: false,
                is_self_returning: false,
            }],
            Some(0),
        )];

        let (_, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert!(class_members.is_empty());
    }

    #[test]
    fn ignore_decorator_set_record_seen_marks_entries() {
        let set = IgnoreDecoratorSet::from_config(&["@step".to_string()]);
        assert!(!set.entries[0].matched.load(Ordering::Relaxed));
        set.record_seen("step");
        assert!(
            set.entries[0].matched.load(Ordering::Relaxed),
            "record_seen should mark a bare-name entry as seen on a matching decorator path"
        );
    }

    #[test]
    fn ignore_decorator_set_dotted_record_seen_distinct_from_bare() {
        let set = IgnoreDecoratorSet::from_config(&[
            "decorators.log".to_string(),
            "decorators.audit".to_string(),
        ]);
        set.record_seen("decorators.log");
        assert!(
            set.entries[0].matched.load(Ordering::Relaxed),
            "decorators.log entry should be marked seen by an exact dotted match"
        );
        assert!(
            !set.entries[1].matched.load(Ordering::Relaxed),
            "decorators.audit entry must NOT be marked seen by record_seen('decorators.log')"
        );
    }

    #[test]
    fn react_lifecycle_method_not_flagged() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/component.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "MyComponent",
            vec![
                make_member("render", MemberKind::ClassMethod),
                make_member("componentDidMount", MemberKind::ClassMethod),
                make_member("customMethod", MemberKind::ClassMethod),
            ],
            Some(0),
        )];

        let (_, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert_eq!(class_members.len(), 1);
        assert_eq!(class_members[0].member_name, "customMethod");
    }

    #[test]
    fn angular_lifecycle_method_not_flagged() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/component.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "AppComponent",
            vec![
                make_member("ngOnInit", MemberKind::ClassMethod),
                make_member("ngOnDestroy", MemberKind::ClassMethod),
                make_member("myHelper", MemberKind::ClassMethod),
            ],
            Some(0),
        )];

        let (_, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert_eq!(class_members.len(), 1);
        assert_eq!(class_members[0].member_name, "myHelper");
    }

    #[test]
    fn user_class_member_allowlist_not_flagged() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/renderer.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "MyRendererComponent",
            vec![
                make_member("agInit", MemberKind::ClassMethod),
                make_member("refresh", MemberKind::ClassMethod),
                make_member("customHelper", MemberKind::ClassMethod),
            ],
            Some(0),
        )];

        let allowlist = vec![
            UsedClassMemberRule::from("agInit"),
            UsedClassMemberRule::from("refresh"),
        ];

        let (_, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &allowlist,
            &[],
        );
        assert_eq!(
            class_members.len(),
            1,
            "only customHelper should remain unused"
        );
        assert_eq!(class_members[0].member_name, "customHelper");
    }

    #[test]
    fn user_class_member_allowlist_globs_match_member_names() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/listener.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "GrammarListener",
            vec![
                make_member("enterRule", MemberKind::ClassMethod),
                make_member("exitRule", MemberKind::ClassMethod),
                make_member("onNodeEvent", MemberKind::ClassMethod),
                make_member("customHelper", MemberKind::ClassMethod),
            ],
            Some(0),
        )];

        let allowlist = vec![
            UsedClassMemberRule::from("enter*"),
            UsedClassMemberRule::from("exit*"),
            UsedClassMemberRule::from("on?odeEvent"),
        ];

        let (_, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &allowlist,
            &[],
        );
        assert_eq!(
            class_members.len(),
            1,
            "only customHelper should remain unused"
        );
        assert_eq!(class_members[0].member_name, "customHelper");
    }

    #[test]
    fn member_glob_patterns_track_whether_they_matched() {
        let rules = vec![
            UsedClassMemberRule::from("enter*"),
            UsedClassMemberRule::from("missing*"),
        ];
        let allowlist = ClassMemberAllowlist::from_rules(&rules);

        assert!(allowlist.matches("enterRule", None, &[]));

        assert!(allowlist.global_patterns[0].matched.load(Ordering::Relaxed));
        assert!(!allowlist.global_patterns[1].matched.load(Ordering::Relaxed));
    }

    #[test]
    fn user_class_member_allowlist_does_not_affect_enums() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/status.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Status",
            vec![make_member("refresh", MemberKind::EnumMember)],
            Some(0),
        )];

        let allowlist = vec![UsedClassMemberRule::from("refresh")];

        let (enum_members, _) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &allowlist,
            &[],
        );
        assert_eq!(enum_members.len(), 1);
        assert_eq!(enum_members[0].member_name, "refresh");
    }

    #[test]
    fn scoped_allowlist_matches_implements_only() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/renderer.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "MyRendererComponent",
            vec![
                make_member("refresh", MemberKind::ClassMethod),
                make_member("customHelper", MemberKind::ClassMethod),
            ],
            Some(0),
        )];

        let modules = vec![make_module_with_class_heritage(
            1,
            "MyRendererComponent",
            None,
            &["ICellRendererAngularComp"],
        )];
        let allowlist = vec![UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
            extends: None,
            implements: Some("ICellRendererAngularComp".to_string()),
            members: vec!["refresh".to_string()],
        })];

        let (_, class_members) = find_unused_members(
            &graph,
            &[],
            &modules,
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &allowlist,
            &[],
        );

        assert_eq!(class_members.len(), 1);
        assert_eq!(class_members[0].member_name, "customHelper");
    }

    #[test]
    fn scoped_allowlist_globs_match_only_matching_heritage() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/listener.ts", false),
            ("/src/unrelated.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "GrammarListener",
            vec![
                make_member("enterRule", MemberKind::ClassMethod),
                make_member("exitRule", MemberKind::ClassMethod),
                make_member("customHelper", MemberKind::ClassMethod),
            ],
            Some(0),
        )];
        graph.modules[2].set_reachable(true);
        graph.modules[2].exports = vec![make_export_with_members(
            "DashboardComponent",
            vec![make_member("enterRule", MemberKind::ClassMethod)],
            Some(0),
        )];

        let modules = vec![make_module_with_class_heritage(
            1,
            "GrammarListener",
            Some("BaseListener"),
            &[],
        )];
        let allowlist = vec![UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
            extends: Some("BaseListener".to_string()),
            implements: None,
            members: vec!["enter*".to_string(), "exit*".to_string()],
        })];

        let (_, class_members) = find_unused_members(
            &graph,
            &[],
            &modules,
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &allowlist,
            &[],
        );
        assert_eq!(
            class_members.len(),
            2,
            "only unrelated enterRule and listener customHelper should remain unused: {class_members:?}"
        );
        assert!(
            class_members
                .iter()
                .any(|member| member.parent_name == "DashboardComponent"
                    && member.member_name == "enterRule"),
            "scoped glob must not suppress unrelated classes: {class_members:?}"
        );
        assert!(
            class_members
                .iter()
                .any(|member| member.parent_name == "GrammarListener"
                    && member.member_name == "customHelper"),
            "scoped glob must not suppress unmatched members: {class_members:?}"
        );
        assert!(
            !class_members
                .iter()
                .any(|member| member.parent_name == "GrammarListener"
                    && (member.member_name == "enterRule" || member.member_name == "exitRule")),
            "scoped glob should suppress matching listener members: {class_members:?}"
        );
    }

    #[test]
    fn scoped_allowlist_matches_extends_only() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/command.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "GenerateReport",
            vec![
                make_member("execute", MemberKind::ClassMethod),
                make_member("customHelper", MemberKind::ClassMethod),
            ],
            Some(0),
        )];

        let modules = vec![make_module_with_class_heritage(
            1,
            "GenerateReport",
            Some("BaseCommand"),
            &[],
        )];
        let allowlist = vec![UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
            extends: Some("BaseCommand".to_string()),
            implements: None,
            members: vec!["execute".to_string()],
        })];

        let (_, class_members) = find_unused_members(
            &graph,
            &[],
            &modules,
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &allowlist,
            &[],
        );

        assert_eq!(class_members.len(), 1);
        assert_eq!(class_members[0].member_name, "customHelper");
    }

    fn make_export_info(name: &str, super_class: Option<&str>) -> ExportInfo {
        ExportInfo {
            name: ExportName::Named(name.to_string()),
            local_name: Some(name.to_string()),
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            span: Span::new(0, 10),
            members: vec![],
            super_class: super_class.map(str::to_string),
        }
    }

    #[test]
    fn is_native_error_base_name_recognizes_native_errors() {
        for base in [
            "Error",
            "TypeError",
            "RangeError",
            "SyntaxError",
            "ReferenceError",
            "EvalError",
            "URIError",
            "AggregateError",
        ] {
            assert!(
                is_native_error_base_name(base),
                "{base} should be a native error base"
            );
        }
        assert!(!is_native_error_base_name("Person"));
        assert!(!is_native_error_base_name("HttpException"));
        assert!(!is_native_error_base_name("error")); // case-sensitive
        assert!(!is_native_error_base_name("DOMException")); // out of scope
    }

    #[test]
    fn error_subclass_name_member_not_flagged_but_other_members_are() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/errors.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "DomainError",
            vec![
                make_member("name", MemberKind::ClassProperty),
                make_member("unusedHelper", MemberKind::ClassMethod),
            ],
            Some(0),
        )];

        let modules = vec![make_module_with_class_heritage(
            1,
            "DomainError",
            Some("Error"),
            &[],
        )];

        let (_, class_members) = find_unused_members(
            &graph,
            &[],
            &modules,
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );

        assert_eq!(class_members.len(), 1);
        assert_eq!(class_members[0].member_name, "unusedHelper");
    }

    #[test]
    fn ordinary_class_name_member_still_flagged() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/person.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Person",
            vec![make_member("name", MemberKind::ClassProperty)],
            Some(0),
        )];

        let modules = vec![make_module_with_class_heritage(1, "Person", None, &[])];

        let (_, class_members) = find_unused_members(
            &graph,
            &[],
            &modules,
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );

        assert_eq!(class_members.len(), 1);
        assert_eq!(class_members[0].member_name, "name");
    }

    #[test]
    fn transitive_error_subclass_name_member_not_flagged() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/errors.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![
            make_export_with_members(
                "DomainError",
                vec![make_member("name", MemberKind::ClassProperty)],
                Some(0),
            ),
            make_export_with_members(
                "ApiError",
                vec![make_member("name", MemberKind::ClassProperty)],
                Some(0),
            ),
        ];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/src/errors.ts"),
            exports: vec![
                make_export_info("DomainError", Some("Error")),
                make_export_info("ApiError", Some("DomainError")),
            ],
            ..Default::default()
        }];

        let mut errors_module =
            make_module_with_class_heritage(1, "DomainError", Some("Error"), &[]);
        errors_module.class_heritage.push(ClassHeritageInfo {
            export_name: "ApiError".to_string(),
            super_class: Some("DomainError".to_string()),
            implements: Vec::new(),
            instance_bindings: Vec::new(),
        });
        let modules = vec![errors_module];

        let (_, class_members) = find_unused_members(
            &graph,
            &resolved_modules,
            &modules,
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );

        assert!(
            class_members.is_empty(),
            "both DomainError.name and ApiError.name should be credited, got {class_members:?}"
        );
    }

    #[test]
    fn this_member_access_not_flagged() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/service.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Service",
            vec![
                make_member("label", MemberKind::ClassProperty),
                make_member("unused_prop", MemberKind::ClassProperty),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(1), // same file as the service
            path: PathBuf::from("/src/service.ts"),
            member_accesses: vec![MemberAccess {
                object: "this".to_string(),
                member: "label".to_string(),
            }],
            ..Default::default()
        }];

        let (_, class_members) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert_eq!(class_members.len(), 1);
        assert_eq!(class_members[0].member_name, "unused_prop");
    }

    #[test]
    fn unreferenced_export_skips_member_analysis() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Status",
            vec![make_member("Active", MemberKind::EnumMember)],
            None, // no references
        )];

        let (enum_members, _) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert!(enum_members.is_empty());
    }

    #[test]
    fn unreachable_module_skips_member_analysis() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/dead.ts", false)]);
        graph.modules[1].exports = vec![make_export_with_members(
            "DeadEnum",
            vec![make_member("X", MemberKind::EnumMember)],
            Some(0),
        )];

        let (enum_members, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert!(enum_members.is_empty());
        assert!(class_members.is_empty());
    }

    #[test]
    fn entry_point_module_skips_member_analysis() {
        let mut graph = build_graph(&[("/src/entry.ts", true)]);
        graph.modules[0].exports = vec![make_export_with_members(
            "EntryEnum",
            vec![make_member("X", MemberKind::EnumMember)],
            None,
        )];

        let (enum_members, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert!(enum_members.is_empty());
        assert!(class_members.is_empty());
    }

    #[test]
    fn enum_member_kind_routed_to_enum_results() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Status",
            vec![make_member("Active", MemberKind::EnumMember)],
            Some(0),
        )];

        let (enum_members, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert_eq!(enum_members.len(), 1);
        assert_eq!(enum_members[0].kind, MemberKind::EnumMember);
        assert!(class_members.is_empty());
    }

    #[test]
    fn class_member_kind_routed_to_class_results() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/class.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "MyClass",
            vec![
                make_member("myMethod", MemberKind::ClassMethod),
                make_member("myProp", MemberKind::ClassProperty),
            ],
            Some(0),
        )];

        let (enum_members, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert!(enum_members.is_empty());
        assert_eq!(class_members.len(), 2);
        assert!(
            class_members
                .iter()
                .any(|m| m.kind == MemberKind::ClassMethod)
        );
        assert!(
            class_members
                .iter()
                .any(|m| m.kind == MemberKind::ClassProperty)
        );
    }

    #[test]
    fn instance_member_access_not_flagged() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/service.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "MyService",
            vec![
                make_member("greet", MemberKind::ClassMethod),
                make_member("unusedMethod", MemberKind::ClassMethod),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/src/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./service".to_string(),
                    imported_name: ImportedName::Named("MyService".to_string()),
                    local_name: "MyService".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            member_accesses: vec![MemberAccess {
                object: "MyService".to_string(),
                member: "greet".to_string(),
            }],
            ..Default::default()
        }];

        let (_, class_members) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert_eq!(class_members.len(), 1);
        assert_eq!(class_members[0].member_name, "unusedMethod");
    }

    #[test]
    fn this_access_does_not_skip_enum_members() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Direction",
            vec![
                make_member("Up", MemberKind::EnumMember),
                make_member("Down", MemberKind::EnumMember),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(1),
            path: PathBuf::from("/src/enums.ts"),
            member_accesses: vec![MemberAccess {
                object: "this".to_string(),
                member: "Up".to_string(),
            }],
            ..Default::default()
        }];

        let (enum_members, _) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert_eq!(enum_members.len(), 2);
    }

    #[test]
    fn mixed_enum_and_class_in_same_module() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/mixed.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![
            make_export_with_members(
                "Status",
                vec![make_member("Active", MemberKind::EnumMember)],
                Some(0),
            ),
            make_export_with_members(
                "Service",
                vec![make_member("doWork", MemberKind::ClassMethod)],
                Some(0),
            ),
        ];

        let (enum_members, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert_eq!(enum_members.len(), 1);
        assert_eq!(enum_members[0].parent_name, "Status");
        assert_eq!(class_members.len(), 1);
        assert_eq!(class_members[0].parent_name, "Service");
    }

    #[test]
    fn local_name_mapped_to_imported_name() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Status",
            vec![
                make_member("Active", MemberKind::EnumMember),
                make_member("Inactive", MemberKind::EnumMember),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/src/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./enums".to_string(),
                    imported_name: ImportedName::Named("Status".to_string()),
                    local_name: "S".to_string(), // aliased
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            member_accesses: vec![MemberAccess {
                object: "S".to_string(), // uses local alias
                member: "Active".to_string(),
            }],
            ..Default::default()
        }];

        let (enum_members, _) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert_eq!(enum_members.len(), 1);
        assert_eq!(enum_members[0].member_name, "Inactive");
    }

    #[test]
    fn default_import_maps_to_default_export() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "default",
            vec![
                make_member("X", MemberKind::EnumMember),
                make_member("Y", MemberKind::EnumMember),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/src/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./enums".to_string(),
                    imported_name: ImportedName::Default,
                    local_name: "MyEnum".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            member_accesses: vec![MemberAccess {
                object: "MyEnum".to_string(),
                member: "X".to_string(),
            }],
            ..Default::default()
        }];

        let (enum_members, _) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert_eq!(enum_members.len(), 1);
        assert_eq!(enum_members[0].member_name, "Y");
    }

    #[test]
    fn suppressed_enum_member_not_flagged() {
        use crate::suppress::{IssueKind, Suppression};

        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Status",
            vec![make_member("Active", MemberKind::EnumMember)],
            Some(0),
        )];

        let supps = vec![Suppression::issue(1, 0, IssueKind::UnusedEnumMember)];
        let mut supp_map: FxHashMap<FileId, &[Suppression]> = FxHashMap::default();
        supp_map.insert(FileId(1), &supps);
        let suppressions = SuppressionContext::from_map(supp_map);

        let (enum_members, _) = find_unused_members(
            &graph,
            &[],
            &[],
            &suppressions,
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert!(
            enum_members.is_empty(),
            "suppressed enum member should not be flagged"
        );
    }

    #[test]
    fn suppressed_class_member_not_flagged() {
        use crate::suppress::{IssueKind, Suppression};

        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/service.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Service",
            vec![make_member("doWork", MemberKind::ClassMethod)],
            Some(0),
        )];

        let supps = vec![Suppression::issue(1, 0, IssueKind::UnusedClassMember)];
        let mut supp_map: FxHashMap<FileId, &[Suppression]> = FxHashMap::default();
        supp_map.insert(FileId(1), &supps);
        let suppressions = SuppressionContext::from_map(supp_map);

        let (_, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &suppressions,
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert!(
            class_members.is_empty(),
            "suppressed class member should not be flagged"
        );
    }

    #[test]
    fn whole_object_use_via_aliased_import() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Status",
            vec![
                make_member("A", MemberKind::EnumMember),
                make_member("B", MemberKind::EnumMember),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/src/entry.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./enums".to_string(),
                    imported_name: ImportedName::Named("Status".to_string()),
                    local_name: "S".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            whole_object_uses: vec!["S".to_string()], // aliased local name
            ..Default::default()
        }];

        let (enum_members, _) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert!(
            enum_members.is_empty(),
            "whole object use via alias should suppress all members"
        );
    }

    #[test]
    fn this_field_chained_access_not_flagged() {
        let mut graph = build_graph(&[("/src/main.ts", true), ("/src/service.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "MyService",
            vec![
                make_member("doWork", MemberKind::ClassMethod),
                make_member("unusedMethod", MemberKind::ClassMethod),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/src/main.ts"),
            resolved_imports: vec![ResolvedImport {
                info: ImportInfo {
                    source: "./service".to_string(),
                    imported_name: ImportedName::Named("MyService".to_string()),
                    local_name: "MyService".to_string(),
                    is_type_only: false,
                    from_style: false,
                    span: Span::new(0, 30),
                    source_span: Span::default(),
                },
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            member_accesses: vec![MemberAccess {
                object: "MyService".to_string(),
                member: "doWork".to_string(),
            }],
            ..Default::default()
        }];

        let (_, class_members) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert_eq!(class_members.len(), 1);
        assert_eq!(class_members[0].member_name, "unusedMethod");
    }

    #[test]
    fn interface_member_usage_propagates_to_implementers() {
        let mut graph = build_graph(&[
            ("/src/main.ts", true),
            ("/src/scroll-strategy.interface.ts", false),
            ("/src/fixed-size-strategy.ts", false),
            ("/src/scroll-viewport.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[2].set_reachable(true);
        graph.modules[3].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "VirtualScrollStrategy",
            vec![],
            Some(3),
        )];
        graph.modules[2].exports = vec![make_export_with_members(
            "FixedSizeScrollStrategy",
            vec![
                make_member("attached", MemberKind::ClassProperty),
                make_member("attach", MemberKind::ClassMethod),
                make_member("detach", MemberKind::ClassMethod),
                make_member("unusedHelper", MemberKind::ClassMethod),
            ],
            Some(0),
        )];

        let modules = vec![make_module_with_class_heritage(
            2,
            "FixedSizeScrollStrategy",
            None,
            &["VirtualScrollStrategy"],
        )];

        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(2),
                path: PathBuf::from("/src/fixed-size-strategy.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./scroll-strategy.interface".to_string(),
                        imported_name: ImportedName::Named("VirtualScrollStrategy".to_string()),
                        local_name: "VirtualScrollStrategy".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: Span::new(0, 30),
                        source_span: Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(3),
                path: PathBuf::from("/src/scroll-viewport.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./scroll-strategy.interface".to_string(),
                        imported_name: ImportedName::Named("VirtualScrollStrategy".to_string()),
                        local_name: "VirtualScrollStrategy".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: Span::new(0, 30),
                        source_span: Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                member_accesses: vec![
                    MemberAccess {
                        object: "VirtualScrollStrategy".to_string(),
                        member: "attach".to_string(),
                    },
                    MemberAccess {
                        object: "VirtualScrollStrategy".to_string(),
                        member: "attached".to_string(),
                    },
                    MemberAccess {
                        object: "VirtualScrollStrategy".to_string(),
                        member: "detach".to_string(),
                    },
                ],
                ..Default::default()
            },
        ];

        let (_, class_members) = find_unused_members(
            &graph,
            &resolved_modules,
            &modules,
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );

        let unused_names: FxHashSet<String> = class_members
            .iter()
            .map(|member| format!("{}.{}", member.parent_name, member.member_name))
            .collect();

        assert!(
            !unused_names.contains("FixedSizeScrollStrategy.attach"),
            "attach should be credited through interface usage: {unused_names:?}"
        );
        assert!(
            !unused_names.contains("FixedSizeScrollStrategy.attached"),
            "attached should be credited through interface usage: {unused_names:?}"
        );
        assert!(
            !unused_names.contains("FixedSizeScrollStrategy.detach"),
            "detach should be credited through interface usage: {unused_names:?}"
        );
        assert!(
            unused_names.contains("FixedSizeScrollStrategy.unusedHelper"),
            "unrelated members should still be reported: {unused_names:?}"
        );
    }

    #[test]
    fn same_named_interfaces_do_not_share_member_usage() {
        let mut graph = build_graph(&[
            ("/src/main.ts", true),
            ("/src/one-interface.ts", false),
            ("/src/two-interface.ts", false),
            ("/src/one-impl.ts", false),
            ("/src/two-impl.ts", false),
            ("/src/consumer.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[2].set_reachable(true);
        graph.modules[3].set_reachable(true);
        graph.modules[4].set_reachable(true);
        graph.modules[5].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members("Strategy", vec![], Some(5))];
        graph.modules[2].exports = vec![make_export_with_members("Strategy", vec![], Some(0))];
        graph.modules[3].exports = vec![make_export_with_members(
            "OneStrategy",
            vec![make_member("attach", MemberKind::ClassMethod)],
            Some(0),
        )];
        graph.modules[4].exports = vec![make_export_with_members(
            "TwoStrategy",
            vec![make_member("attach", MemberKind::ClassMethod)],
            Some(0),
        )];

        let modules = vec![
            make_module_with_class_heritage(3, "OneStrategy", None, &["Strategy"]),
            make_module_with_class_heritage(4, "TwoStrategy", None, &["Strategy"]),
        ];

        let resolved_modules = vec![
            ResolvedModule {
                file_id: FileId(3),
                path: PathBuf::from("/src/one-impl.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./one-interface".to_string(),
                        imported_name: ImportedName::Named("Strategy".to_string()),
                        local_name: "Strategy".to_string(),
                        is_type_only: true,
                        from_style: false,
                        span: Span::new(0, 30),
                        source_span: Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(4),
                path: PathBuf::from("/src/two-impl.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./two-interface".to_string(),
                        imported_name: ImportedName::Named("Strategy".to_string()),
                        local_name: "Strategy".to_string(),
                        is_type_only: true,
                        from_style: false,
                        span: Span::new(0, 30),
                        source_span: Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                }],
                ..Default::default()
            },
            ResolvedModule {
                file_id: FileId(5),
                path: PathBuf::from("/src/consumer.ts"),
                resolved_imports: vec![ResolvedImport {
                    info: ImportInfo {
                        source: "./one-interface".to_string(),
                        imported_name: ImportedName::Named("Strategy".to_string()),
                        local_name: "Strategy".to_string(),
                        is_type_only: true,
                        from_style: false,
                        span: Span::new(0, 30),
                        source_span: Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                }],
                member_accesses: vec![MemberAccess {
                    object: "Strategy".to_string(),
                    member: "attach".to_string(),
                }],
                ..Default::default()
            },
        ];

        let (_, class_members) = find_unused_members(
            &graph,
            &resolved_modules,
            &modules,
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );

        let unused_names: FxHashSet<String> = class_members
            .iter()
            .map(|member| format!("{}.{}", member.parent_name, member.member_name))
            .collect();

        assert!(
            !unused_names.contains("OneStrategy.attach"),
            "OneStrategy.attach should be credited through its own interface export: {unused_names:?}"
        );
        assert!(
            unused_names.contains("TwoStrategy.attach"),
            "TwoStrategy.attach should remain unused when only the other interface export is used: {unused_names:?}"
        );
    }

    #[test]
    fn same_named_exports_do_not_share_member_usage() {
        let mut graph = build_graph(&[
            ("/src/entry.ts", true),
            ("/src/one.ts", false),
            ("/src/two.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[2].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "Widget",
            vec![
                make_member("refresh", MemberKind::ClassMethod),
                make_member("unusedOne", MemberKind::ClassMethod),
            ],
            Some(0),
        )];
        graph.modules[2].exports = vec![make_export_with_members(
            "Widget",
            vec![
                make_member("refresh", MemberKind::ClassMethod),
                make_member("unusedTwo", MemberKind::ClassMethod),
            ],
            Some(0),
        )];

        let resolved_modules = vec![ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/src/entry.ts"),
            resolved_imports: vec![
                ResolvedImport {
                    info: ImportInfo {
                        source: "./one".to_string(),
                        imported_name: ImportedName::Named("Widget".to_string()),
                        local_name: "FirstWidget".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: Span::new(0, 30),
                        source_span: Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(1)),
                },
                ResolvedImport {
                    info: ImportInfo {
                        source: "./two".to_string(),
                        imported_name: ImportedName::Named("Widget".to_string()),
                        local_name: "SecondWidget".to_string(),
                        is_type_only: false,
                        from_style: false,
                        span: Span::new(31, 62),
                        source_span: Span::default(),
                    },
                    target: ResolveResult::InternalModule(FileId(2)),
                },
            ],
            member_accesses: vec![MemberAccess {
                object: "FirstWidget".to_string(),
                member: "refresh".to_string(),
            }],
            ..Default::default()
        }];

        let (_, class_members) = find_unused_members(
            &graph,
            &resolved_modules,
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );

        let unused_members: FxHashSet<(String, String)> = class_members
            .iter()
            .map(|member| {
                (
                    member.path.display().to_string(),
                    format!("{}.{}", member.parent_name, member.member_name),
                )
            })
            .collect();

        assert_eq!(
            unused_members.len(),
            3,
            "unexpected members: {unused_members:?}"
        );
        assert!(
            unused_members.contains(&("/src/one.ts".to_string(), "Widget.unusedOne".to_string()))
        );
        assert!(
            unused_members.contains(&("/src/two.ts".to_string(), "Widget.refresh".to_string()))
        );
        assert!(
            unused_members.contains(&("/src/two.ts".to_string(), "Widget.unusedTwo".to_string()))
        );
        assert!(
            !unused_members.contains(&("/src/one.ts".to_string(), "Widget.refresh".to_string())),
            "member usage from /src/one.ts should not leak into /src/two.ts: {unused_members:?}"
        );
    }

    #[test]
    fn export_with_no_members_skipped() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/utils.ts", false)]);
        graph.modules[1].set_reachable(true);
        graph.modules[1].exports = vec![make_export_with_members(
            "helper",
            vec![], // no members
            Some(0),
        )];

        let (enum_members, class_members) = find_unused_members(
            &graph,
            &[],
            &[],
            &SuppressionContext::empty(),
            &FxHashMap::default(),
            &[],
            &[],
        );
        assert!(enum_members.is_empty());
        assert!(class_members.is_empty());
    }
}
