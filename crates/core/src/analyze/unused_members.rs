use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};
use globset::GlobMatcher;
use rayon::prelude::*;
use rustc_hash::{FxHashMap, FxHashSet};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::discover::FileId;
use crate::extract::{
    ANGULAR_TPL_SENTINEL, ExportName, FACTORY_CALL_SENTINEL, FLUENT_CHAIN_SENTINEL,
    INSTANCE_EXPORT_SENTINEL, MemberKind, ModuleInfo, PLAYWRIGHT_FIXTURE_DEF_SENTINEL,
    PLAYWRIGHT_FIXTURE_USE_SENTINEL,
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

/// Find unused enum and class members in exported symbols.
///
/// Collects all `Identifier.member` static member accesses from all modules,
/// maps them to their imported names, and filters out members that are accessed.
///
/// `user_class_member_allowlist` extends the built-in Angular/React lifecycle
/// allowlist with framework-invoked method names contributed by plugins and
/// top-level config (see `FallowConfig::used_class_members` and
/// `Plugin::used_class_members`). Plain string entries suppress matching member
/// names or glob patterns globally; scoped object entries only suppress classes
/// whose heritage clause matches the configured `extends` / `implements`
/// constraints.
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

/// User-supplied decorator names that should NOT count as evidence of
/// reflective use. Built from `FallowConfig::ignore_decorators`.
///
/// Matching rule: entries containing `.` match the full dotted path of a
/// decorator (e.g. `"decorators.log"` matches `@decorators.log` but not
/// `@decorators.audit`). Bare entries match the leftmost segment of the path
/// (e.g. `"decorators"` matches `@decorators.log` AND `@decorators.audit`;
/// `"step"` matches `@step` and `@step("x")`). Both `"@step"` and `"step"`
/// round-trip equivalently because a leading `@` is stripped at construction.
///
/// Each entry tracks whether it matched at least one decorator during the
/// run; unmatched entries surface as a `tracing::warn!` at end of run,
/// mirroring `ClassMemberAllowlist::warn_unmatched_patterns`.
struct IgnoreDecoratorSet {
    entries: Vec<IgnoreDecoratorEntry>,
}

struct IgnoreDecoratorEntry {
    /// Original user-provided string (after `@` strip + trim). Used in the
    /// unmatched-pattern warning so the message echoes the user's input.
    raw: String,
    /// Whether the entry contains `.` (dotted = exact-path match; bare =
    /// leftmost-segment match).
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

    /// Returns true when `decorator_path` matches any ignore-list entry under
    /// the dual matching rule. An empty `decorator_path` (the silent fallback
    /// for decorators whose expression is not an identifier ladder) never
    /// matches. Side effect: marks every matching entry as seen for the
    /// end-of-run `warn_unmatched` report.
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

    /// Mark every entry matching `decorator_path` as seen, without returning
    /// the predicate result. Used during the pre-pass over all class-member
    /// decorators (including those on members that never reach the skip
    /// predicate because they were already credited as used). Without this,
    /// the `warn_unmatched` report falsely flags entries whose decorators
    /// only appear on used members. Caught 2026-05-20 by /fallow-review.
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
struct ExportKey {
    file_id: FileId,
    export_name: String,
}

impl ExportKey {
    fn new(file_id: FileId, export_name: impl Into<String>) -> Self {
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

fn build_local_to_export_keys(resolved: &ResolvedModule) -> FxHashMap<&str, Vec<ExportKey>> {
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

/// Walk the re-export chain starting at `(start_file, start_name)` and return
/// every defining-site `ExportKey` reachable from it.
///
/// A barrel like `lib/index.ts` with `export { Foo } from './types'` produces
/// a `ReExportEdge { source_file: types.ts, imported_name: "Foo", exported_name: "Foo" }`
/// on the barrel module, AND Phase 4 chain resolution synthesizes an
/// `ExportSymbol` for `Foo` on the barrel as a stub for reference tracking.
/// We must prefer re-export edges over the local stub so the walk reaches
/// the file where the enum/class is actually defined (and where `members`
/// are populated). Cross-package consumers resolve their import to the
/// barrel's `file_id`, so the access map keys at the barrel; without this
/// chain walk, `find_unused_members` looks up accesses at the origin file
/// and finds nothing (issue #178).
///
/// Handles named re-exports (with renames) and `export *` re-exports as a
/// fallback when no named edge matches. Cycle-protected via a visited set.
fn walk_re_export_origins(
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

        // Prefer re-export edges over the local export stub: Phase 4 chain
        // resolution synthesizes an `ExportSymbol` for chained re-exports so
        // reference propagation can attach SymbolReferences. That stub is
        // indistinguishable from a real `export const X = ...` declaration
        // by name alone, so we follow the named edge first whenever both
        // are present.
        let mut matched_named = false;
        for re in &module.re_exports {
            // `export * as ns from './mod'` produces an edge with
            // `exported_name = "ns"` and `imported_name = "*"`. Following
            // that edge would push `(source_file, "*")` and dead-end on the
            // next iteration. Member access through a re-exported namespace
            // (`ns.Foo.member`) is two property accesses deep and isn't
            // tracked at extract time anyway, so skipping the edge here
            // matches the existing extraction contract.
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

/// Copy access sets from each barrel `ExportKey` in `accessed_members` to
/// every defining-site `ExportKey` reachable through re-export chains.
///
/// Without this, a cross-package consumer of an enum or class re-exported
/// through a barrel file (e.g. `lib/index.ts` re-exporting `lib/types.ts`)
/// has its `Foo.bar` accesses recorded at the barrel and never reaches the
/// origin where `members` are populated. See issue #178.
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

/// Sibling of `propagate_accesses_through_re_exports` for the
/// "whole-object-used" set (e.g. `Object.values(StatusCode)` on a re-exported
/// enum should mark every member of the originating enum as used).
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

fn export_key_with_origins(graph: &ModuleGraph, key: &ExportKey) -> Vec<ExportKey> {
    let mut keys = Vec::new();
    push_export_key(&mut keys, key.clone());
    for origin in walk_re_export_origins(graph, key.file_id, key.export_name.as_str()) {
        push_export_key(&mut keys, origin);
    }
    keys
}

fn entry_point_star_re_export_targets(
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

fn export_has_entry_point_re_export_reference(
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
    let mut targets_by_test: FxHashMap<ExportKey, FxHashMap<String, Vec<ExportKey>>> =
        FxHashMap::default();

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
                let fixture_targets = targets_by_test
                    .entry(test_key.clone())
                    .or_default()
                    .entry(fixture_name.to_string())
                    .or_default();
                for target_key in target_keys {
                    for key in export_key_with_origins(graph, target_key) {
                        push_export_key(fixture_targets, key);
                    }
                }
            }
        }
    }

    targets_by_test
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
                || access.object.starts_with(PLAYWRIGHT_FIXTURE_DEF_SENTINEL)
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
                || object_name.starts_with(PLAYWRIGHT_FIXTURE_DEF_SENTINEL)
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

/// Decode a `FACTORY_CALL_SENTINEL{callee_object}:{callee_method}` access object
/// into its `(callee_object, callee_method)` components. Returns `None` when the
/// object is not sentinel-prefixed or the embedded delimiter is missing.
fn parse_factory_call_sentinel(object: &str) -> Option<(&str, &str)> {
    object
        .strip_prefix(FACTORY_CALL_SENTINEL)
        .and_then(|payload| payload.split_once(':'))
}

/// Credit member accesses produced by static-factory call bindings on the
/// originating class export.
///
/// Each `const <local> = <ID>.<METHOD>()` site emitted (via the visitor's
/// `resolve_factory_call_candidates` and `resolve_bound_member_accesses`)
/// sentinel-encoded `MemberAccess { object: "{sentinel}{ID}:{METHOD}", member }`
/// entries on the consumer module. This pass resolves `<ID>` through the
/// consumer's `local_to_export_keys` (same map used for direct accesses, so it
/// covers both same-file local classes and cross-file imports). The matched
/// `ExportKey` is then walked through `walk_re_export_origins` to reach every
/// defining-site export. For each origin whose `MemberInfo` array contains a
/// member named `<METHOD>` with `is_instance_returning_static == true`, the
/// consumed `member` is inserted into `accessed_members` at the origin key.
///
/// Origins lacking the matching flagged method are skipped silently: the
/// sentinel was recorded speculatively at extract time, so this is the
/// intended drop point for imports that turn out not to name a factory class.
/// See issue #346.
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
/// into its components. `chain` is a comma-separated list of intermediate
/// method names walked since the root call (empty when the credited member is
/// the first call after the root). Returns `None` when the object is not
/// sentinel-prefixed or the structure is malformed. See issue #387.
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

/// Validate a fluent chain against a single class export: the export must
/// match the resolved origin name, declare `root_method` with
/// `is_instance_returning_static`, and contain every `chain` step as a
/// `is_self_returning` `ClassMethod`. See issue #387.
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
///
/// At extract time, each call expression chained off a previous call emits a
/// sentinel-encoded `MemberAccess` of shape `FLUENT_CHAIN_SENTINEL:<ID>:<root_method>:<chain_prefix>`
/// with `member` set to the method being called now. This pass:
///
/// 1. Resolves `<ID>` through each consumer's `local_to_export_keys` (covering
///    both same-file local classes and cross-file imports).
/// 2. Walks the matched `ExportKey` through `walk_re_export_origins` to reach
///    every defining-site class export.
/// 3. Validates the origin's `<root_method>` carries `is_instance_returning_static`.
/// 4. Walks each name in `<chain_prefix>`: every step must exist on the origin
///    class with `is_self_returning`. If any step is absent or non-self-returning,
///    the chain has left the class type and the credit is skipped (e.g., the
///    `.toString()` after a `.build()` that returns a different type).
/// 5. Credits the access's `member` on the origin class only when every check
///    above passes.
///
/// Origins lacking the matching flagged root method are skipped silently: the
/// sentinel was recorded speculatively at extract time, so non-class imports
/// or imports of factory-less classes drop here. See issue #387.
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

/// Build `parent_export -> [child_export, ...]` from each exported class's
/// `extends` clause (resolved through the importing module's
/// `local_to_export_keys`). Output is deduplicated per-parent.
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

/// Propagate member accesses through `extends` chains in both directions.
///
/// - Parent `this.*` accesses flow down to child files, so a base class method
///   calling `this.getArea()` credits `Circle.getArea()` / `Rectangle.getArea()`.
/// - Child `this.*` accesses (and Angular template refs bridged into
///   `self_accessed_members`) flow UP to parent files, so a child component
///   template referencing an inherited method credits the base class's method
///   as used.
/// - External accesses on a parent export flow down to every child export.
/// - External accesses on any child export flow up to the parent export.
///
/// Self-access propagations are computed on a snapshot first and applied after
/// the external-access loop so the mutable borrows stay disjoint.
fn propagate_class_inheritance(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    accessed_members: &mut FxHashMap<ExportKey, FxHashSet<String>>,
    self_accessed_members: &mut FxHashMap<FileId, FxHashSet<String>>,
) {
    let parent_to_children = build_parent_to_children(graph, resolved_modules);
    if parent_to_children.is_empty() {
        return;
    }

    let mut propagations: Vec<(FileId, Vec<String>)> = Vec::new();

    for (parent_key, children) in &parent_to_children {
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
    find_unused_members_with_public_api_entry_points(
        graph,
        resolved_modules,
        modules,
        suppressions,
        line_offsets_by_file,
        user_class_member_allowlist,
        ignore_decorators,
        &FxHashSet::default(),
    )
}

#[expect(
    clippy::too_many_arguments,
    clippy::too_many_lines,
    reason = "member tracking requires many graph traversal steps; further splitting is possible but not yet a priority"
)]
pub(super) fn find_unused_members_with_public_api_entry_points(
    graph: &ModuleGraph,
    resolved_modules: &[ResolvedModule],
    modules: &[ModuleInfo],
    suppressions: &SuppressionContext<'_>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
    user_class_member_allowlist: &[UsedClassMemberRule],
    ignore_decorators: &[String],
    public_api_entry_points: &FxHashSet<FileId>,
) -> (Vec<UnusedMember>, Vec<UnusedMember>) {
    let mut unused_enum_members = Vec::new();
    let mut unused_class_members = Vec::new();
    let allowlist = ClassMemberAllowlist::from_rules(user_class_member_allowlist);
    let ignore_decorators = IgnoreDecoratorSet::from_config(ignore_decorators);

    // Pre-pass: mark every ignore-decorator entry as seen against every
    // decorator name in the codebase, regardless of whether the decorated
    // member ever reaches the skip predicate. Without this, the per-member
    // path that calls `ignore_decorators.matches(...)` is short-circuited
    // for members already credited as used (external access, this.* access,
    // suppressed, etc.), so an entry whose decorator appears only on USED
    // decorated members would falsely surface in the end-of-run
    // `warn_unmatched` report.
    if !ignore_decorators.is_empty() {
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

    let mut class_heritage_by_export: FxHashMap<ExportKey, (Option<String>, Vec<String>)> =
        FxHashMap::default();
    let mut class_heritage_by_file = FxHashMap::default();
    for module in modules {
        class_heritage_by_file.insert(module.file_id, module.class_heritage.as_slice());
        class_heritage_by_export.extend(module.class_heritage.iter().map(|heritage| {
            (
                ExportKey::new(module.file_id, heritage.export_name.clone()),
                (heritage.super_class.clone(), heritage.implements.clone()),
            )
        }));
    }

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

    // Map exported symbol identity -> set of member names that are accessed across all modules.
    let mut accessed_members: FxHashMap<ExportKey, FxHashSet<String>> = FxHashMap::default();

    // Also build a per-file set of `this.member` accesses. These indicate internal usage
    // within a class body — class members accessed via `this.foo` are used internally
    // even if no external code accesses them via `ClassName.foo`.
    let mut self_accessed_members: FxHashMap<crate::discover::FileId, FxHashSet<String>> =
        FxHashMap::default();

    // Build a set of exported symbols that are used as whole objects
    // (Object.values, for..in, etc.). All members of these exports should be
    // considered used.
    let mut whole_object_used_exports: FxHashSet<ExportKey> = FxHashSet::default();

    for resolved in resolved_modules {
        let local_to_export_keys = build_local_to_export_keys(resolved);

        for access in &resolved.member_accesses {
            if access.object.starts_with(INSTANCE_EXPORT_SENTINEL)
                || access.object.starts_with(FACTORY_CALL_SENTINEL)
                || access.object.starts_with(FLUENT_CHAIN_SENTINEL)
            {
                continue;
            }
            // Track `this.member` accesses per-file for internal class usage
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

    // Propagate accesses through re-export chains so cross-package consumers
    // that import a barrel-re-exported enum/class credit the originating
    // file's `members`. See issue #178.
    propagate_playwright_fixture_accesses(graph, resolved_modules, &mut accessed_members);
    propagate_factory_call_accesses(graph, resolved_modules, &mut accessed_members);
    propagate_fluent_chain_accesses(graph, resolved_modules, &mut accessed_members);
    propagate_accesses_through_typed_instance_bindings(
        graph,
        resolved_modules,
        modules,
        &mut accessed_members,
        &mut whole_object_used_exports,
    );
    propagate_accesses_through_re_exports(graph, &mut accessed_members);
    propagate_whole_object_through_re_exports(graph, &mut whole_object_used_exports);
    let instance_targets = build_instance_export_targets(graph, resolved_modules);
    propagate_accesses_through_instance_exports(
        &instance_targets,
        &mut accessed_members,
        &mut whole_object_used_exports,
    );

    if !interface_to_implementers.is_empty() {
        let mut propagations: Vec<(ExportKey, Vec<String>)> = Vec::new();

        for (interface_key, implementer_keys) in &interface_to_implementers {
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

    // Bridge Angular template member refs to their owning components.
    //
    // Sentinel member accesses come from two sources:
    // 1. External templates: HTML files scanned for Angular syntax, with sentinel
    //    accesses stored on the HTML file's ModuleInfo. Bridged to the component
    //    via the SideEffect import edge from @Component({ templateUrl }).
    // 2. Inline templates/host/inputs/outputs: sentinel accesses stored directly
    //    on the component's own ModuleInfo (same file as the class).
    //
    // Bridged BEFORE `propagate_class_inheritance` so child-template refs
    // propagate up to base-class files, crediting inherited members used in a
    // child component's external template.
    let angular_tpl_refs: FxHashMap<FileId, Vec<&str>> = resolved_modules
        .iter()
        .filter_map(|m| {
            let refs: Vec<&str> = m
                .member_accesses
                .iter()
                .filter(|a| a.object == ANGULAR_TPL_SENTINEL)
                .map(|a| a.member.as_str())
                .collect();
            if refs.is_empty() {
                None
            } else {
                Some((m.file_id, refs))
            }
        })
        .collect();

    // Non-sentinel member-access chains from HTML template scanners
    // (`dataService.getTotal` where `dataService` is an unresolved top-level
    // identifier). Keyed by the HTML file's id.
    let angular_tpl_chain_accesses: FxHashMap<FileId, Vec<(&str, &str)>> = resolved_modules
        .iter()
        .filter_map(|m| {
            let has_sentinel = m
                .member_accesses
                .iter()
                .any(|a| a.object == ANGULAR_TPL_SENTINEL);
            if !has_sentinel {
                return None;
            }
            let chains: Vec<(&str, &str)> = m
                .member_accesses
                .iter()
                .filter(|a| {
                    a.object != ANGULAR_TPL_SENTINEL
                        && a.object != "this"
                        && !a.object.starts_with(INSTANCE_EXPORT_SENTINEL)
                        && !a.object.starts_with(FACTORY_CALL_SENTINEL)
                        && !a.object.starts_with(FLUENT_CHAIN_SENTINEL)
                })
                .map(|a| (a.object.as_str(), a.member.as_str()))
                .collect();
            if chains.is_empty() {
                None
            } else {
                Some((m.file_id, chains))
            }
        })
        .collect();

    if !angular_tpl_refs.is_empty() {
        for resolved in resolved_modules {
            // Case 1: sentinel accesses on the same file (inline template, host, inputs/outputs)
            if let Some(refs) = angular_tpl_refs.get(&resolved.file_id) {
                let entry = self_accessed_members.entry(resolved.file_id).or_default();
                for &ref_name in refs {
                    entry.insert(ref_name.to_string());
                }
            }
            // Case 2: sentinel accesses on an imported file (external templateUrl)
            for import in resolved.all_resolved_imports() {
                if let Some(target_id) = import.target.internal_file_id()
                    && let Some(refs) = angular_tpl_refs.get(&target_id)
                {
                    let entry = self_accessed_members.entry(resolved.file_id).or_default();
                    for &ref_name in refs {
                        entry.insert(ref_name.to_string());
                    }
                }
            }
        }
    }

    // Resolve HTML template chain accesses (`dataService.getTotal`) through
    // the importing component's typed instance bindings to credit the target
    // class's member as used.
    //
    // For inline templates, the chain accesses are stored on the component's
    // own `member_accesses` and resolved via the visitor's
    // `resolve_bound_member_accesses` at extract time -- so they flow through
    // the regular member-access pipeline above and need no special handling
    // here.
    if !angular_tpl_chain_accesses.is_empty() {
        for resolved in resolved_modules {
            let Some(class_heritage) = class_heritage_by_file.get(&resolved.file_id) else {
                continue;
            };
            if class_heritage.is_empty() {
                continue;
            }
            let component_bindings: FxHashMap<&str, &str> = class_heritage
                .iter()
                .flat_map(|h| {
                    h.instance_bindings
                        .iter()
                        .map(|(local, ty)| (local.as_str(), ty.as_str()))
                })
                .collect();
            if component_bindings.is_empty() {
                continue;
            }
            let local_to_export_keys = build_local_to_export_keys(resolved);
            for import in resolved.all_resolved_imports() {
                let Some(target_id) = import.target.internal_file_id() else {
                    continue;
                };
                let Some(chains) = angular_tpl_chain_accesses.get(&target_id) else {
                    continue;
                };
                for (object, member) in chains {
                    let Some(type_name) = component_bindings.get(object) else {
                        continue;
                    };
                    let Some(export_keys) = local_to_export_keys.get(type_name) else {
                        continue;
                    };
                    for export_key in export_keys {
                        accessed_members
                            .entry(export_key.clone())
                            .or_default()
                            .insert((*member).to_string());
                    }
                }
            }
        }
    }

    propagate_class_inheritance(
        graph,
        resolved_modules,
        &mut accessed_members,
        &mut self_accessed_members,
    );

    let entry_star_targets = entry_point_star_re_export_targets(graph, public_api_entry_points);

    let member_results: Vec<(Vec<UnusedMember>, Vec<UnusedMember>)> = graph
        .modules
        .par_iter()
        .map(|module| {
            let mut unused_enum_members = Vec::new();
            let mut unused_class_members = Vec::new();

            if !module.is_reachable() || module.is_entry_point() {
                return (unused_enum_members, unused_class_members);
            }

            for export in &module.exports {
                if export.members.is_empty() {
                    continue;
                }

                // If the export itself is unused, skip member analysis (whole export is dead).
                // Side-effect-registered exports (Lit @customElement, customElements.define)
                // are alive at runtime even with empty cross-file references; their members
                // are runtime-invoked by the browser/Lit framework so member analysis must run.
                if export.references.is_empty()
                    && !export.is_side_effect_used
                    && !graph.has_namespace_import(module.file_id)
                {
                    continue;
                }

                let export_name = export.name.to_string();
                let export_key = ExportKey::new(module.file_id, export_name.clone());
                let (super_class, implemented_interfaces) = class_heritage_by_export
                    .get(&export_key)
                    .map_or((None, &[][..]), |(super_class, interfaces)| {
                        (super_class.as_deref(), interfaces.as_slice())
                    });

                // If this export is used as a whole object (Object.values, for..in, etc.),
                // all members are considered used — skip individual member analysis.
                if whole_object_used_exports.contains(&export_key) {
                    continue;
                }

                let is_public_api_class_export = is_entry_point_public_class_export(
                    graph,
                    module,
                    export,
                    &entry_star_targets,
                    public_api_entry_points,
                );

                // Get `this.member` accesses from this file (internal class usage)
                let file_self_accesses = self_accessed_members.get(&module.file_id);

                for member in &export.members {
                    // Per-member unused detection on TS namespaces is not yet
                    // wired; the namespace as a whole is still tracked via the
                    // unused-export detector, so a fully-unused namespace remains
                    // reported and only the per-member granularity is missing.
                    if matches!(member.kind, MemberKind::NamespaceMember) {
                        continue;
                    }

                    if is_public_api_class_export
                        && matches!(
                            member.kind,
                            MemberKind::ClassMethod | MemberKind::ClassProperty
                        )
                    {
                        continue;
                    }

                    // Check if this member is accessed anywhere via external import
                    if accessed_members
                        .get(&export_key)
                        .is_some_and(|s| s.contains(&member.name))
                    {
                        continue;
                    }

                    // Check if this member is accessed via `this.member` within the same file
                    // (internal class usage — e.g., constructor sets this.label, methods use this.label)
                    if matches!(
                        member.kind,
                        MemberKind::ClassMethod | MemberKind::ClassProperty
                    ) && file_self_accesses
                        .is_some_and(|accesses| accesses.contains(&member.name))
                    {
                        continue;
                    }

                    // Skip decorated class members. Decorators like @Column(),
                    // @ApiProperty(), @Inject() indicate runtime usage by
                    // frameworks (NestJS, TypeORM, class-validator,
                    // class-transformer). These members are accessed
                    // reflectively and should not be flagged as unused.
                    //
                    // Users can opt specific decorators out of this skip via
                    // FallowConfig.ignore_decorators (issue #471). A member
                    // whose every decorator path is in the ignore set is
                    // checked normally; any non-ignored decorator restores
                    // the conservative skip. Members where `has_decorator` is
                    // true but `decorator_names` is empty (Angular signal
                    // initializer properties, which set the boolean without
                    // a literal decorator AST node) always skip; there is no
                    // name to match against the ignore set.
                    if member.has_decorator
                        && (member.decorator_names.is_empty()
                            || ignore_decorators.is_empty()
                            || member
                                .decorator_names
                                .iter()
                                .any(|name| !ignore_decorators.matches(name)))
                    {
                        continue;
                    }

                    // Skip lifecycle methods called by runtimes or the browser, not user code:
                    // React class component lifecycle, Angular lifecycle hooks, and native
                    // Custom Elements lifecycle on direct HTMLElement subclasses. The user
                    // allowlist extends these built-ins with framework-invoked names contributed
                    // by plugins and top-level config (ag-Grid's `agInit`, etc.).
                    if matches!(
                        member.kind,
                        MemberKind::ClassMethod | MemberKind::ClassProperty
                    ) && (is_react_lifecycle_method(&member.name)
                        || is_angular_lifecycle_method(&member.name)
                        || is_native_custom_element_lifecycle_method(&member.name, super_class)
                        || allowlist.matches(
                            member.name.as_str(),
                            super_class,
                            implemented_interfaces,
                        ))
                    {
                        continue;
                    }

                    let (line, col) = byte_offset_to_line_col(
                        line_offsets_by_file,
                        module.file_id,
                        member.span.start,
                    );

                    // Check inline suppression
                    let issue_kind = match member.kind {
                        MemberKind::EnumMember => IssueKind::UnusedEnumMember,
                        MemberKind::ClassMethod | MemberKind::ClassProperty => {
                            IssueKind::UnusedClassMember
                        }
                        MemberKind::NamespaceMember => unreachable!(),
                    };
                    if suppressions.is_suppressed(module.file_id, line, issue_kind) {
                        continue;
                    }

                    let unused = UnusedMember {
                        path: module.path.clone(),
                        parent_name: export_name.clone(),
                        member_name: member.name.clone(),
                        kind: member.kind,
                        line,
                        col,
                    };

                    match member.kind {
                        MemberKind::EnumMember => unused_enum_members.push(unused),
                        MemberKind::ClassMethod | MemberKind::ClassProperty => {
                            unused_class_members.push(unused);
                        }
                        MemberKind::NamespaceMember => unreachable!(),
                    }
                }
            }

            (unused_enum_members, unused_class_members)
        })
        .collect();

    for (enum_members, class_members) in member_results {
        unused_enum_members.extend(enum_members);
        unused_class_members.extend(class_members);
    }

    allowlist.warn_unmatched_patterns();
    ignore_decorators.warn_unmatched();

    (unused_enum_members, unused_class_members)
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
        ExportName, ImportInfo, ImportedName, MemberAccess, MemberInfo, MemberKind, ModuleInfo,
        VisibilityTag,
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
            local_type_declarations: vec![],
            public_signature_type_references: vec![],
            namespace_object_aliases: vec![],
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

        // No member accesses at all — both should be unused
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

        // Consumer accesses Status.Active
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
        // Only Inactive should be unused
        assert_eq!(enum_members.len(), 1);
        assert_eq!(enum_members[0].member_name, "Inactive");
    }

    #[test]
    fn accessed_enum_member_via_re_export_not_flagged() {
        // Mirror of issue #178: enum defined in `types.ts`, re-exported by a
        // barrel `index.ts`, consumed cross-package via the barrel. Without
        // re-export chain propagation in `find_unused_members`, the access
        // map keys at the barrel and the origin-keyed lookup at detection
        // time misses every cross-barrel access, falsely flagging every
        // member as unused.
        let mut graph = build_graph(&[
            ("/app/consumer.ts", true),
            ("/lib/index.ts", true),
            ("/lib/types.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[2].set_reachable(true);

        // Phase 4 chain resolution synthesizes a stub `ExportSymbol` on the
        // barrel for chained re-exports (so reference tracking can hang
        // SymbolReferences off it). Replicate the shape here.
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
            // In production, Phase 4 chain resolution propagates the
            // barrel's `references` back to the source. Simulate that here.
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
        // Cross-package class static method case from the issue #178 comment:
        // `ClassName.method()` cross-barrel must credit the originating
        // class member.
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
        // `export { Original as Renamed } from './types'` chains must
        // walk back to the original name on the source file.
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
        // `export * from './types'` must fan out to source file when no
        // named edge matches.
        let mut graph = build_graph(&[
            ("/app/consumer.ts", true),
            ("/lib/index.ts", true),
            ("/lib/types.ts", false),
        ]);
        graph.modules[1].set_reachable(true);
        graph.modules[2].set_reachable(true);

        // Star re-export: barrel has no synthesized stub for "Status"; just
        // a `*` edge pointing at types.ts.
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

        // Consumer uses Object.values(Status) — whole object use
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
        // Direct test of the pre-pass primitive. After `record_seen("step")`,
        // the `@step` entry is considered matched even when no skip predicate
        // has been evaluated yet, so the end-of-run `warn_unmatched` report
        // does not falsely flag it. Caught 2026-05-20 by /fallow-review.
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
        // `record_seen("decorators.log")` marks a dotted entry but does NOT
        // mark a sibling dotted entry `decorators.audit`. Pins the dual-match
        // semantics for the pre-pass primitive. Caught 2026-05-20 by
        // /fallow-review (extends the false-warn regression coverage).
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
        // Only customMethod should be flagged
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
        // Third-party framework contract: library calls `agInit` and `refresh`
        // on the consumer class. The user allowlist (from config or a plugin)
        // extends the built-in Angular/React lifecycle check so these names are
        // treated as always-used. See issue #98 (ag-Grid `AgFrameworkComponent`).
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
        // The allowlist is scoped to class members; matching enum member names
        // must still be flagged as unused.
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

        // The service file itself accesses this.label
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
        // Only unused_prop should be flagged (label is accessed via this)
        assert_eq!(class_members.len(), 1);
        assert_eq!(class_members[0].member_name, "unused_prop");
    }

    #[test]
    fn unreferenced_export_skips_member_analysis() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/enums.ts", false)]);
        graph.modules[1].set_reachable(true);
        // Export has members but NO references — whole export is dead, members skipped
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
        // Member analysis skipped because export itself is unreferenced
        assert!(enum_members.is_empty());
    }

    #[test]
    fn unreachable_module_skips_member_analysis() {
        let mut graph = build_graph(&[("/src/entry.ts", true), ("/src/dead.ts", false)]);
        // Module 1 stays unreachable
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

        // Consumer imports MyService and accesses greet via instance.
        // The visitor maps `svc.greet()` → `MyService.greet` at extraction time,
        // so the analysis layer sees it as a direct member access on the export name.
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
                // Already mapped by the visitor from `svc.greet()` → `MyService.greet`
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
        // Only unusedMethod should be flagged; greet is used via instance access
        assert_eq!(class_members.len(), 1);
        assert_eq!(class_members[0].member_name, "unusedMethod");
    }

    #[test]
    fn this_access_does_not_skip_enum_members() {
        // `this.member` accesses only suppress class members, not enum members.
        // Enums don't have `this` — this test ensures the check is scoped to class kinds.
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

        // File accesses this.Up — but for enum members, this should NOT suppress
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
        // Both enum members should be flagged — `this` access doesn't apply to enums
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
        // import { Status as S } from './enums'
        // S.Active → should map "S" back to "Status" for member access matching
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
        // S.Active maps back to Status.Active, so only Inactive is unused
        assert_eq!(enum_members.len(), 1);
        assert_eq!(enum_members[0].member_name, "Inactive");
    }

    #[test]
    fn default_import_maps_to_default_export() {
        // import MyEnum from './enums' → local "MyEnum", imported "default"
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
        // MyEnum.X maps to default.X, so only Y is unused
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

        // Suppress on line 1 (byte offset 10 => line 1 with no offsets)
        let supps = vec![Suppression {
            line: 1,
            comment_line: 0,
            kind: Some(IssueKind::UnusedEnumMember),
        }];
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

        let supps = vec![Suppression {
            line: 1,
            comment_line: 0,
            kind: Some(IssueKind::UnusedClassMember),
        }];
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
        // import { Status as S } from './enums'
        // Object.values(S) → should map S back to Status and suppress all members
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
        // Object.values(S) maps S→Status, so all members of Status should be considered used
        assert!(
            enum_members.is_empty(),
            "whole object use via alias should suppress all members"
        );
    }

    #[test]
    fn this_field_chained_access_not_flagged() {
        // `this.service = new MyService()` then `this.service.doWork()`
        // should recognize doWork as a used member of MyService.
        // The visitor emits MemberAccess { object: "MyService", member: "doWork" }
        // after resolving the `this.service` binding via binding_target_names.
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

        // Consumer imports MyService, stores in a field, and calls through it.
        // The visitor resolves `this.service.doWork()` → `MyService.doWork`.
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
                // Already resolved by visitor from `this.service.doWork()` → `MyService.doWork`
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
        // Only unusedMethod should be flagged; doWork is used via this.service.doWork()
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
