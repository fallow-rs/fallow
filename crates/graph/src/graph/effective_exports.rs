//! Canonical effective export bindings for direct and transitive re-exports.

use std::collections::{VecDeque, hash_map::Entry};

use fallow_types::discover::FileId;
use fallow_types::extract::ExportName;
use rustc_hash::{FxBuildHasher, FxHashMap, FxHashSet};

use super::types::ExportSymbol;
use crate::resolve::ResolvedModule;

/// The namespace in which an exported name is resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum ExportNamespace {
    /// Type declarations and the type side of dual-space declarations.
    Type,
    /// Runtime value declarations.
    Value,
}

/// One canonical declaration that an exported name resolves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EffectiveExportBinding {
    file_id: FileId,
    kind: EffectiveExportBindingKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
enum EffectiveExportBindingKind {
    Declaration(usize),
    NamespaceObject { source: FileId },
    ImplicitDefault,
}

impl EffectiveExportBinding {
    /// Module that owns the resolved declaration.
    #[must_use]
    pub const fn origin_file(&self) -> FileId {
        self.file_id
    }

    pub(in crate::graph) const fn origin_slot(self) -> Option<usize> {
        match self.kind {
            EffectiveExportBindingKind::Declaration(slot) => Some(slot),
            EffectiveExportBindingKind::NamespaceObject { .. }
            | EffectiveExportBindingKind::ImplicitDefault => None,
        }
    }

    /// Source module represented by a namespace-object export.
    #[must_use]
    pub const fn namespace_source(self) -> Option<FileId> {
        match self.kind {
            EffectiveExportBindingKind::NamespaceObject { source } => Some(source),
            EffectiveExportBindingKind::Declaration(_)
            | EffectiveExportBindingKind::ImplicitDefault => None,
        }
    }

    /// Whether this binding is the implicit default export of an SFC file.
    #[must_use]
    pub const fn is_implicit_default(self) -> bool {
        matches!(self.kind, EffectiveExportBindingKind::ImplicitDefault)
    }
}

/// Effective resolution for one module/name/namespace tuple.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EffectiveExportResolution {
    /// The module does not export this name in the requested namespace.
    #[default]
    Missing,
    /// Exactly one canonical declaration supplies the exported name.
    Unique(EffectiveExportBinding),
    /// Multiple distinct star-exported declarations supply the same name.
    Ambiguous,
}

impl EffectiveExportResolution {
    fn merged_with(self, incoming: Self) -> Self {
        match (self, incoming) {
            (Self::Missing, resolution) | (resolution, Self::Missing) => resolution,
            (Self::Unique(left), Self::Unique(right)) if left == right => self,
            (Self::Ambiguous, _) | (_, Self::Ambiguous) | (Self::Unique(_), Self::Unique(_)) => {
                Self::Ambiguous
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct ExportNameId(usize);

impl ExportNameId {
    const DEFAULT: Self = Self(0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct ExportKey {
    file_id: FileId,
    name: ExportNameId,
}

impl ExportKey {
    const fn new(file_id: FileId, name: ExportNameId) -> Self {
        Self { file_id, name }
    }

    const fn with_file(self, file_id: FileId) -> Self {
        Self { file_id, ..self }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ExportLookup {
    key: ExportKey,
    namespace: ExportNamespace,
}

impl ExportLookup {
    const fn new(file_id: FileId, name: ExportNameId, namespace: ExportNamespace) -> Self {
        Self {
            key: ExportKey::new(file_id, name),
            namespace,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
struct NamespaceResolutions {
    r#type: EffectiveExportResolution,
    value: EffectiveExportResolution,
}

impl NamespaceResolutions {
    const fn get(self, namespace: ExportNamespace) -> EffectiveExportResolution {
        match namespace {
            ExportNamespace::Type => self.r#type,
            ExportNamespace::Value => self.value,
        }
    }

    const fn set(&mut self, namespace: ExportNamespace, resolution: EffectiveExportResolution) {
        match namespace {
            ExportNamespace::Type => self.r#type = resolution,
            ExportNamespace::Value => self.value = resolution,
        }
    }

    const fn is_missing(self) -> bool {
        matches!(self.r#type, EffectiveExportResolution::Missing)
            && matches!(self.value, EffectiveExportResolution::Missing)
    }
}

struct ExportNameInterner {
    ids: FxHashMap<Box<str>, ExportNameId>,
    next_id: usize,
}

impl ExportNameInterner {
    fn with_capacity(capacity: usize) -> Self {
        let mut ids = FxHashMap::with_capacity_and_hasher(capacity, FxBuildHasher);
        ids.insert(Box::<str>::from("default"), ExportNameId::DEFAULT);
        Self { ids, next_id: 1 }
    }

    fn intern_export_name(&mut self, name: &ExportName) -> ExportNameId {
        match name {
            ExportName::Default => ExportNameId::DEFAULT,
            ExportName::Named(name) => self.intern(name),
        }
    }

    fn intern(&mut self, name: &str) -> ExportNameId {
        if let Some(id) = self.ids.get(name) {
            return *id;
        }
        let id = ExportNameId(self.next_id);
        self.next_id += 1;
        self.ids.insert(Box::from(name), id);
        id
    }
}

#[derive(Clone, Copy)]
struct StarObserver {
    barrel: FileId,
    type_only: bool,
}

#[derive(Clone, Copy)]
struct NamedObserver {
    namespaces: ExportNamespaces,
    destination: DenseExportId,
}

#[derive(Clone, Copy, Default)]
pub(super) struct ExportNamespaces {
    r#type: bool,
    value: bool,
}

impl ExportNamespaces {
    pub(super) const fn contains(self, namespace: ExportNamespace) -> bool {
        match namespace {
            ExportNamespace::Type => self.r#type,
            ExportNamespace::Value => self.value,
        }
    }

    pub(super) const fn insert(&mut self, namespace: ExportNamespace) -> bool {
        let slot = match namespace {
            ExportNamespace::Type => &mut self.r#type,
            ExportNamespace::Value => &mut self.value,
        };
        let inserted = !*slot;
        *slot = true;
        inserted
    }

    pub(super) const fn extend(&mut self, namespaces: Self) {
        self.r#type |= namespaces.r#type;
        self.value |= namespaces.value;
    }

    const fn without(self, blocked: Self) -> Self {
        Self {
            r#type: self.r#type && !blocked.r#type,
            value: self.value && !blocked.value,
        }
    }

    pub(super) const fn is_empty(self) -> bool {
        !self.r#type && !self.value
    }

    const fn for_re_export(type_only: bool) -> Self {
        Self {
            r#type: true,
            value: !type_only,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct DenseExportId(usize);

struct PropagationObservers {
    star: FxHashMap<FileId, Vec<StarObserver>>,
}

struct ObserverBuildState<'a> {
    interner: &'a mut ExportNameInterner,
    arena: &'a mut DenseExportArena,
}

struct DenseExportEntry {
    key: ExportKey,
    resolutions: NamespaceResolutions,
    direct: ExportNamespaces,
    explicit: ExportNamespaces,
    named: Vec<NamedObserver>,
    queued: bool,
}

struct DenseExportArena {
    ids: FxHashMap<ExportKey, DenseExportId>,
    entries: Vec<DenseExportEntry>,
    queue: VecDeque<DenseExportId>,
}

impl DenseExportArena {
    fn with_capacity(key_capacity: usize, queue_capacity: usize) -> Self {
        Self {
            ids: FxHashMap::with_capacity_and_hasher(key_capacity, FxBuildHasher),
            entries: Vec::with_capacity(key_capacity),
            queue: VecDeque::with_capacity(queue_capacity),
        }
    }

    fn intern(&mut self, key: ExportKey) -> DenseExportId {
        match self.ids.entry(key) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(entry) => {
                let id = DenseExportId(self.entries.len());
                entry.insert(id);
                self.entries.push(DenseExportEntry {
                    key,
                    resolutions: NamespaceResolutions::default(),
                    direct: ExportNamespaces::default(),
                    explicit: ExportNamespaces::default(),
                    named: Vec::new(),
                    queued: false,
                });
                id
            }
        }
    }

    fn direct_namespaces(&self, id: DenseExportId) -> ExportNamespaces {
        self.entries[id.0].direct
    }

    fn mark_direct(&mut self, id: DenseExportId, namespace: ExportNamespace) -> bool {
        let entry = &mut self.entries[id.0];
        entry.explicit.insert(namespace);
        entry.direct.insert(namespace)
    }

    fn mark_explicit(&mut self, id: DenseExportId, namespaces: ExportNamespaces) {
        self.entries[id.0].explicit.extend(namespaces);
    }

    fn add_named_observer(&mut self, source: DenseExportId, observer: NamedObserver) {
        self.entries[source.0].named.push(observer);
    }

    fn merge_resolution(
        &mut self,
        id: DenseExportId,
        namespace: ExportNamespace,
        incoming: EffectiveExportResolution,
    ) {
        let entry = &mut self.entries[id.0];
        let current = entry.resolutions.get(namespace);
        let next = current.merged_with(incoming);
        if current == next {
            return;
        }
        entry.resolutions.set(namespace, next);
        self.enqueue(id);
    }

    fn merge_resolutions(
        &mut self,
        id: DenseExportId,
        incoming: NamespaceResolutions,
        namespaces: ExportNamespaces,
    ) {
        let entry = &mut self.entries[id.0];
        let mut changed = false;
        for namespace in [ExportNamespace::Type, ExportNamespace::Value] {
            if !namespaces.contains(namespace) {
                continue;
            }
            let before = entry.resolutions.get(namespace);
            let after = before.merged_with(incoming.get(namespace));
            if before != after {
                entry.resolutions.set(namespace, after);
                changed = true;
            }
        }
        if changed {
            self.enqueue(id);
        }
    }

    fn enqueue(&mut self, id: DenseExportId) {
        let entry = &mut self.entries[id.0];
        if entry.queued {
            return;
        }
        entry.queued = true;
        self.queue.push_back(id);
    }

    fn pop(&mut self) -> Option<DenseExportId> {
        let id = self.queue.pop_front()?;
        self.entries[id.0].queued = false;
        Some(id)
    }

    fn into_resolutions(self, capacity: usize) -> FxHashMap<ExportKey, NamespaceResolutions> {
        let mut resolutions = FxHashMap::with_capacity_and_hasher(capacity, FxBuildHasher);
        for entry in self.entries {
            if !entry.resolutions.is_missing() {
                resolutions.insert(entry.key, entry.resolutions);
            }
        }
        resolutions
    }
}

/// Immutable effective export bindings for one resolved project.
///
/// Resolution is a finite monotone propagation: each key advances at most from
/// missing to one binding and then to ambiguous. Cycles therefore terminate,
/// while multiple paths to the same binding remain unique.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct EffectiveExportIndex {
    name_ids: FxHashMap<Box<str>, ExportNameId>,
    resolutions: FxHashMap<ExportKey, NamespaceResolutions>,
    declaration_merge_groups: DeclarationMergeGroups,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct DeclarationMergeGroups {
    groups: Vec<Box<[usize]>>,
    group_by_slot: FxHashMap<(FileId, usize), usize>,
}

#[derive(Clone, Copy)]
struct DeclarationBindingContext {
    file_id: FileId,
    namespace: ExportNamespace,
    binding: EffectiveExportBinding,
}

#[derive(Clone, Copy, Default)]
struct EffectiveExportCapacity {
    direct_exports: usize,
    implicit_defaults: usize,
    named_re_exports: usize,
    star_re_exports: usize,
}

impl EffectiveExportCapacity {
    fn for_modules(modules: &[ResolvedModule]) -> Self {
        let mut capacity = Self::default();
        for module in modules {
            capacity.direct_exports = capacity.direct_exports.saturating_add(module.exports.len());
            capacity.implicit_defaults += usize::from(is_sfc_path(&module.path));
            for re_export in &module.re_exports {
                if re_export.target.internal_file_id().is_none() {
                    continue;
                }
                if re_export.info.exported_name == "*" {
                    capacity.star_re_exports = capacity.star_re_exports.saturating_add(1);
                } else {
                    capacity.named_re_exports = capacity.named_re_exports.saturating_add(1);
                }
            }
        }
        capacity
    }

    const fn has_work(self) -> bool {
        self.direct_exports > 0
            || self.implicit_defaults > 0
            || self.named_re_exports > 0
            || self.star_re_exports > 0
    }

    const fn build_keys(self) -> usize {
        self.direct_exports
            .saturating_add(self.implicit_defaults)
            .saturating_add(self.named_re_exports.saturating_mul(2))
    }

    const fn resolution_keys(self) -> usize {
        self.direct_exports
            .saturating_add(self.implicit_defaults)
            .saturating_add(self.named_re_exports)
    }

    const fn interned_names(self) -> usize {
        self.direct_exports
            .saturating_add(self.named_re_exports.saturating_mul(2))
            .saturating_add(1)
    }
}

impl EffectiveExportIndex {
    pub(super) fn build(modules: &[ResolvedModule]) -> Self {
        let capacity = EffectiveExportCapacity::for_modules(modules);
        if !capacity.has_work() {
            return Self::default();
        }

        let mut interner = ExportNameInterner::with_capacity(capacity.interned_names());
        let mut arena =
            DenseExportArena::with_capacity(capacity.build_keys(), capacity.resolution_keys());
        seed_direct_bindings(modules, capacity.direct_exports, &mut interner, &mut arena);
        let observers = collect_observers(modules, capacity, &mut interner, &mut arena);
        propagate_bindings(&mut arena, &observers);

        Self {
            name_ids: interner.ids,
            resolutions: arena.into_resolutions(capacity.resolution_keys()),
            declaration_merge_groups: collect_declaration_merge_groups(modules),
        }
    }

    pub(super) fn resolve(
        &self,
        file_id: FileId,
        name: &str,
        namespace: ExportNamespace,
    ) -> EffectiveExportResolution {
        let Some(name) = self.name_ids.get(name) else {
            return EffectiveExportResolution::Missing;
        };
        self.resolutions
            .get(&ExportKey::new(file_id, *name))
            .map_or(EffectiveExportResolution::Missing, |resolutions| {
                resolutions.get(namespace)
            })
    }

    pub(super) fn resolves_through(
        &self,
        barrel: FileId,
        barrel_name: &str,
        source: FileId,
        source_name: &str,
        namespace: ExportNamespace,
    ) -> bool {
        matches!(
            (
                self.resolve(barrel, barrel_name, namespace),
                self.resolve(source, source_name, namespace),
            ),
            (
                EffectiveExportResolution::Unique(barrel_binding),
                EffectiveExportResolution::Unique(source_binding),
            ) if barrel_binding == source_binding
        )
    }

    pub(super) fn resolving_namespaces_through(
        &self,
        barrel: FileId,
        barrel_name: &str,
        source: FileId,
        source_name: &str,
    ) -> ExportNamespaces {
        let Some(barrel_name_id) = self.name_ids.get(barrel_name).copied() else {
            return ExportNamespaces::default();
        };
        let source_name_id = if barrel_name == source_name {
            barrel_name_id
        } else {
            let Some(source_name_id) = self.name_ids.get(source_name).copied() else {
                return ExportNamespaces::default();
            };
            source_name_id
        };
        let barrel_resolutions = self
            .resolutions
            .get(&ExportKey::new(barrel, barrel_name_id))
            .copied()
            .unwrap_or_default();
        let source_resolutions = self
            .resolutions
            .get(&ExportKey::new(source, source_name_id))
            .copied()
            .unwrap_or_default();
        let mut matching = ExportNamespaces::default();
        for namespace in [ExportNamespace::Type, ExportNamespace::Value] {
            if matches!(
                (
                    barrel_resolutions.get(namespace),
                    source_resolutions.get(namespace),
                ),
                (
                    EffectiveExportResolution::Unique(barrel_binding),
                    EffectiveExportResolution::Unique(source_binding),
                ) if barrel_binding == source_binding
            ) {
                matching.insert(namespace);
            }
        }
        matching
    }

    pub(super) fn contributes_through(
        &self,
        barrel: FileId,
        barrel_name: &str,
        source: FileId,
        source_name: &str,
        namespace: ExportNamespace,
    ) -> bool {
        match (
            self.resolve(barrel, barrel_name, namespace),
            self.resolve(source, source_name, namespace),
        ) {
            (
                EffectiveExportResolution::Unique(barrel_binding),
                EffectiveExportResolution::Unique(source_binding),
            ) => barrel_binding == source_binding,
            (
                EffectiveExportResolution::Ambiguous,
                EffectiveExportResolution::Unique(_) | EffectiveExportResolution::Ambiguous,
            ) => true,
            _ => false,
        }
    }

    pub(super) fn unique_bindings(
        &self,
        file_id: FileId,
        namespace: ExportNamespace,
    ) -> FxHashSet<EffectiveExportBinding> {
        self.resolutions
            .iter()
            .filter_map(|(key, resolutions)| {
                if key.file_id != file_id {
                    return None;
                }
                match resolutions.get(namespace) {
                    EffectiveExportResolution::Unique(binding) => Some(binding),
                    EffectiveExportResolution::Missing | EffectiveExportResolution::Ambiguous => {
                        None
                    }
                }
            })
            .collect()
    }

    pub(super) fn declaration_group_slots(&self, binding: EffectiveExportBinding) -> &[usize] {
        binding
            .origin_slot()
            .and_then(|slot| {
                self.declaration_merge_groups
                    .group_by_slot
                    .get(&(binding.origin_file(), slot))
            })
            .and_then(|group| self.declaration_merge_groups.groups.get(*group))
            .map_or(&[], Box::as_ref)
    }

    pub(super) fn is_declaration_slot(
        &self,
        exports: &[ExportSymbol],
        file_id: FileId,
        name: &str,
        namespace: ExportNamespace,
        export_index: usize,
    ) -> bool {
        let Some(export) = exports.get(export_index) else {
            return false;
        };
        if !export.name.matches_str(name) {
            return false;
        }
        let EffectiveExportResolution::Unique(binding) = self.resolve(file_id, name, namespace)
        else {
            return false;
        };
        self.is_declaration_slot_for_binding(
            exports,
            export_index,
            export,
            DeclarationBindingContext {
                file_id,
                namespace,
                binding,
            },
        )
    }

    fn is_declaration_slot_for_binding(
        &self,
        exports: &[ExportSymbol],
        export_index: usize,
        export: &ExportSymbol,
        context: DeclarationBindingContext,
    ) -> bool {
        if context.binding.origin_file() != context.file_id {
            return true;
        }
        let Some(origin_slot) = context.binding.origin_slot() else {
            return true;
        };
        if context.namespace == ExportNamespace::Type {
            let group = self.declaration_group_slots(context.binding);
            if !group.is_empty() {
                return group.contains(&export_index);
            }
        }
        exports
            .get(origin_slot)
            .is_some_and(|origin| export.is_type_only == origin.is_type_only)
    }

    pub(super) fn declaration_slots_for_name(
        &self,
        exports: &[ExportSymbol],
        file_id: FileId,
        name: &str,
        namespace: ExportNamespace,
    ) -> Vec<usize> {
        let EffectiveExportResolution::Unique(binding) = self.resolve(file_id, name, namespace)
        else {
            return Vec::new();
        };
        if binding.origin_file() == file_id && binding.origin_slot().is_some() {
            let context = DeclarationBindingContext {
                file_id,
                namespace,
                binding,
            };
            return exports
                .iter()
                .enumerate()
                .filter_map(|(index, export)| {
                    (export.name.matches_str(name)
                        && self.is_declaration_slot_for_binding(exports, index, export, context))
                    .then_some(index)
                })
                .collect();
        }

        let mut slots = Vec::new();
        let mut found_type_only = false;
        for (index, export) in exports.iter().enumerate() {
            if !export.name.matches_str(name) {
                continue;
            }
            if namespace == ExportNamespace::Type && export.is_type_only {
                if !found_type_only {
                    slots.clear();
                    found_type_only = true;
                }
                slots.push(index);
            } else if !export.is_type_only && !found_type_only {
                slots.push(index);
            }
        }
        slots
    }

    pub(super) fn declaration_slots(
        &self,
        exports: &[ExportSymbol],
        candidates: &[usize],
        file_id: FileId,
        name: &str,
        namespace: ExportNamespace,
    ) -> Vec<usize> {
        let EffectiveExportResolution::Unique(binding) = self.resolve(file_id, name, namespace)
        else {
            return Vec::new();
        };
        if binding.origin_file() == file_id && binding.origin_slot().is_some() {
            let context = DeclarationBindingContext {
                file_id,
                namespace,
                binding,
            };
            return candidates
                .iter()
                .copied()
                .filter(|&index| {
                    exports.get(index).is_some_and(|export| {
                        export.name.matches_str(name)
                            && self.is_declaration_slot_for_binding(exports, index, export, context)
                    })
                })
                .collect();
        }

        let exact_type_only = namespace == ExportNamespace::Type
            && candidates.iter().any(|&index| exports[index].is_type_only);
        candidates
            .iter()
            .copied()
            .filter(|&index| {
                exports[index].name.matches_str(name)
                    && exports[index].is_type_only == exact_type_only
            })
            .collect()
    }
}

fn collect_declaration_merge_groups(modules: &[ResolvedModule]) -> DeclarationMergeGroups {
    let mut collected = DeclarationMergeGroups::default();
    for module in modules {
        let merge_facts: Vec<_> = module
            .semantic_facts
            .iter()
            .filter_map(|fact| match fact {
                fallow_types::extract::SemanticFact::DeclarationMerge(group) => Some(group),
                _ => None,
            })
            .collect();
        if merge_facts.is_empty() {
            continue;
        }
        let slot_by_span: FxHashMap<_, _> = module
            .exports
            .iter()
            .enumerate()
            .map(|(slot, export)| ((export.span.start, export.span.end), slot))
            .collect();
        for group in merge_facts {
            let mut slots: Vec<_> = group
                .export_spans
                .iter()
                .filter_map(|span| slot_by_span.get(span).copied())
                .collect();
            slots.sort_unstable();
            slots.dedup();
            if slots.len() < 2 {
                continue;
            }
            let group_id = collected.groups.len();
            for &slot in &slots {
                collected
                    .group_by_slot
                    .insert((module.file_id, slot), group_id);
            }
            collected.groups.push(slots.into_boxed_slice());
        }
    }
    collected
}

fn seed_direct_bindings(
    modules: &[ResolvedModule],
    fallback_capacity: usize,
    interner: &mut ExportNameInterner,
    arena: &mut DenseExportArena,
) {
    let mut value_type_fallbacks = Vec::with_capacity(fallback_capacity);
    for module in modules {
        for (slot, export) in module.exports.iter().enumerate() {
            let namespace = if export.is_type_only {
                ExportNamespace::Type
            } else {
                ExportNamespace::Value
            };
            let name = interner.intern_export_name(&export.name);
            let key = ExportLookup::new(module.file_id, name, namespace);
            let id = arena.intern(key.key);
            // Same-name declarations inside one module form one local export
            // entry. This covers legal TypeScript declaration merging (for
            // example class/function plus namespace) without weakening the
            // ambiguity rule for distinct bindings arriving through stars.
            if arena.mark_direct(id, key.namespace) {
                arena.merge_resolution(
                    id,
                    key.namespace,
                    EffectiveExportResolution::Unique(EffectiveExportBinding {
                        file_id: module.file_id,
                        kind: EffectiveExportBindingKind::Declaration(slot),
                    }),
                );
            }
            if namespace == ExportNamespace::Value {
                value_type_fallbacks.push((module.file_id, name, slot));
            }
        }
        seed_implicit_sfc_default(module, interner, arena);
    }
    seed_value_type_fallbacks(value_type_fallbacks, arena);
}

fn seed_value_type_fallbacks(
    fallbacks: Vec<(FileId, ExportNameId, usize)>,
    arena: &mut DenseExportArena,
) {
    for (file_id, name, slot) in fallbacks {
        let type_key = ExportLookup::new(file_id, name, ExportNamespace::Type);
        let id = arena.intern(type_key.key);
        if !arena.mark_direct(id, type_key.namespace) {
            continue;
        }
        arena.merge_resolution(
            id,
            type_key.namespace,
            EffectiveExportResolution::Unique(EffectiveExportBinding {
                file_id,
                kind: EffectiveExportBindingKind::Declaration(slot),
            }),
        );
    }
}

fn seed_implicit_sfc_default(
    module: &ResolvedModule,
    interner: &mut ExportNameInterner,
    arena: &mut DenseExportArena,
) {
    if !is_sfc_path(&module.path) {
        return;
    }
    let name = interner.intern("default");
    let value = ExportLookup::new(module.file_id, name, ExportNamespace::Value);
    let id = arena.intern(value.key);
    if arena.direct_namespaces(id).contains(value.namespace) {
        return;
    }
    let binding = EffectiveExportResolution::Unique(EffectiveExportBinding {
        file_id: module.file_id,
        kind: EffectiveExportBindingKind::ImplicitDefault,
    });
    for namespace in [ExportNamespace::Value, ExportNamespace::Type] {
        let key = ExportLookup::new(module.file_id, name, namespace);
        arena.mark_direct(id, key.namespace);
        arena.merge_resolution(id, key.namespace, binding);
    }
}

fn is_sfc_path(path: &std::path::Path) -> bool {
    matches!(
        path.extension().and_then(std::ffi::OsStr::to_str),
        Some("vue" | "svelte" | "astro")
    )
}

fn collect_observers(
    modules: &[ResolvedModule],
    capacity: EffectiveExportCapacity,
    interner: &mut ExportNameInterner,
    arena: &mut DenseExportArena,
) -> PropagationObservers {
    let mut observers = PropagationObservers {
        star: FxHashMap::with_capacity_and_hasher(capacity.star_re_exports, FxBuildHasher),
    };
    for module in modules {
        for re_export in &module.re_exports {
            let Some(source) = re_export.target.internal_file_id() else {
                continue;
            };
            if re_export.info.exported_name == "*" {
                observers
                    .star
                    .entry(source)
                    .or_default()
                    .push(StarObserver {
                        barrel: module.file_id,
                        type_only: re_export.info.is_type_only,
                    });
                continue;
            }
            register_named_observer(
                module.file_id,
                source,
                &re_export.info,
                ObserverBuildState { interner, arena },
            );
        }
    }
    observers
}

fn register_named_observer(
    barrel: FileId,
    source: FileId,
    info: &fallow_types::extract::ReExportInfo,
    state: ObserverBuildState<'_>,
) {
    let ObserverBuildState { interner, arena } = state;
    let exported_name = interner.intern(&info.exported_name);
    if info.imported_name == "*" {
        let destination = ExportKey::new(barrel, exported_name);
        let id = arena.intern(destination);
        let namespaces =
            ExportNamespaces::for_re_export(info.is_type_only).without(arena.direct_namespaces(id));
        arena.mark_explicit(id, namespaces);
        for namespace in [ExportNamespace::Type, ExportNamespace::Value] {
            if namespaces.contains(namespace) {
                arena.merge_resolution(
                    id,
                    namespace,
                    EffectiveExportResolution::Unique(EffectiveExportBinding {
                        file_id: barrel,
                        kind: EffectiveExportBindingKind::NamespaceObject { source },
                    }),
                );
            }
        }
        return;
    }

    let imported_name = interner.intern(&info.imported_name);
    let destination = ExportKey::new(barrel, exported_name);
    let destination_id = arena.intern(destination);
    let namespaces = ExportNamespaces::for_re_export(info.is_type_only)
        .without(arena.direct_namespaces(destination_id));
    if namespaces.is_empty() {
        return;
    }
    arena.mark_explicit(destination_id, namespaces);
    let source_id = arena.intern(ExportKey::new(source, imported_name));
    arena.add_named_observer(
        source_id,
        NamedObserver {
            namespaces,
            destination: destination_id,
        },
    );
}

fn propagate_bindings(arena: &mut DenseExportArena, observers: &PropagationObservers) {
    while let Some(source_id) = arena.pop() {
        let source_key = arena.entries[source_id.0].key;
        let source_resolutions = arena.entries[source_id.0].resolutions;
        let named_observer_count = arena.entries[source_id.0].named.len();
        for index in 0..named_observer_count {
            let observer = arena.entries[source_id.0].named[index];
            arena.merge_resolutions(
                observer.destination,
                source_resolutions,
                observer.namespaces,
            );
        }
        propagate_star_binding(arena, observers, source_key, source_resolutions);
    }
}

fn propagate_star_binding(
    arena: &mut DenseExportArena,
    observers: &PropagationObservers,
    source_key: ExportKey,
    source_resolutions: NamespaceResolutions,
) {
    if source_key.name == ExportNameId::DEFAULT {
        return;
    }
    let Some(star_observers) = observers.star.get(&source_key.file_id) else {
        return;
    };
    for observer in star_observers {
        let destination = source_key.with_file(observer.barrel);
        let destination_id = arena.intern(destination);
        let explicit = arena.entries[destination_id.0].explicit;
        let namespaces = ExportNamespaces::for_re_export(observer.type_only).without(explicit);
        if namespaces.is_empty() {
            continue;
        }
        arena.merge_resolutions(destination_id, source_resolutions, namespaces);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resolve::{ResolveResult, ResolvedReExport};
    use fallow_types::extract::{ExportInfo, ReExportInfo, VisibilityTag};

    fn value_export(name: &str) -> ExportInfo {
        ExportInfo {
            name: ExportName::Named(name.to_string()),
            local_name: Some(name.to_string()),
            is_type_only: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            span: oxc_span::Span::default(),
            members: Vec::new(),
            is_side_effect_used: false,
            super_class: None,
        }
    }

    fn re_export(source: FileId, imported: &str, exported: &str) -> ResolvedReExport {
        ResolvedReExport {
            info: ReExportInfo {
                source: format!("./{}", source.0),
                imported_name: imported.to_string(),
                exported_name: exported.to_string(),
                is_type_only: false,
                span: oxc_span::Span::default(),
                statement_span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::InternalModule(source),
        }
    }

    fn module(
        file_id: u32,
        exports: Vec<ExportInfo>,
        re_exports: Vec<ResolvedReExport>,
    ) -> ResolvedModule {
        ResolvedModule {
            file_id: FileId(file_id),
            exports: exports.into(),
            re_exports,
            ..Default::default()
        }
    }

    fn resolves_through(
        index: &EffectiveExportIndex,
        barrel: FileId,
        source: FileId,
        name: &str,
    ) -> bool {
        index.resolves_through(barrel, name, source, name, ExportNamespace::Value)
    }

    #[test]
    fn missing_export_is_explicit_in_the_resolution_contract() {
        let index = EffectiveExportIndex::build(&[module(0, Vec::new(), Vec::new())]);

        assert_eq!(
            index.resolve(FileId(0), "missing", ExportNamespace::Value),
            EffectiveExportResolution::Missing
        );
    }

    #[test]
    fn same_module_declaration_merges_keep_one_binding() {
        let index = EffectiveExportIndex::build(&[module(
            0,
            vec![value_export("Merged"), value_export("Merged")],
            Vec::new(),
        )]);

        assert!(matches!(
            index.resolve(FileId(0), "Merged", ExportNamespace::Value),
            EffectiveExportResolution::Unique(binding) if binding.origin_file() == FileId(0)
        ));
    }

    #[test]
    fn declaration_merge_groups_survive_graph_cache_roundtrip() {
        let mut interface = value_export("Merged");
        interface.is_type_only = true;
        interface.span = oxc_span::Span::new(0, 6);
        let mut namespace = value_export("Merged");
        namespace.span = oxc_span::Span::new(10, 16);
        let modules = vec![ResolvedModule {
            file_id: FileId(0),
            exports: vec![interface, namespace].into(),
            semantic_facts: vec![fallow_types::extract::SemanticFact::DeclarationMerge(
                fallow_types::extract::DeclarationMergeFact {
                    export_spans: vec![(0, 6), (10, 16)],
                },
            )]
            .into(),
            ..Default::default()
        }];
        let index = EffectiveExportIndex::build(&modules);
        let encoded = postcard::to_allocvec(&index).expect("encode effective export index");
        let decoded: EffectiveExportIndex =
            postcard::from_bytes(&encoded).expect("decode effective export index");
        let EffectiveExportResolution::Unique(binding) =
            decoded.resolve(FileId(0), "Merged", ExportNamespace::Type)
        else {
            panic!("merged type binding must remain unique");
        };

        assert_eq!(decoded.declaration_group_slots(binding), &[0, 1]);
    }

    #[test]
    fn explicit_re_export_shadows_a_star_binding() {
        let modules = vec![
            module(
                0,
                Vec::new(),
                vec![
                    re_export(FileId(1), "*", "*"),
                    re_export(FileId(2), "foo", "foo"),
                ],
            ),
            module(1, vec![value_export("foo")], Vec::new()),
            module(2, vec![value_export("foo")], Vec::new()),
        ];
        let index = EffectiveExportIndex::build(&modules);

        assert!(!resolves_through(&index, FileId(0), FileId(1), "foo"));
        assert!(resolves_through(&index, FileId(0), FileId(2), "foo"));
        assert!(!index.contributes_through(
            FileId(0),
            "foo",
            FileId(1),
            "foo",
            ExportNamespace::Value
        ));
    }

    #[test]
    fn distinct_star_bindings_are_ambiguous() {
        let modules = vec![
            module(
                0,
                Vec::new(),
                vec![
                    re_export(FileId(1), "*", "*"),
                    re_export(FileId(2), "*", "*"),
                ],
            ),
            module(1, vec![value_export("foo")], Vec::new()),
            module(2, vec![value_export("foo")], Vec::new()),
        ];
        let index = EffectiveExportIndex::build(&modules);

        assert!(!resolves_through(&index, FileId(0), FileId(1), "foo"));
        assert!(!resolves_through(&index, FileId(0), FileId(2), "foo"));
        assert!(index.contributes_through(
            FileId(0),
            "foo",
            FileId(1),
            "foo",
            ExportNamespace::Value
        ));
        assert!(index.contributes_through(
            FileId(0),
            "foo",
            FileId(2),
            "foo",
            ExportNamespace::Value
        ));
    }

    #[test]
    fn convergent_star_paths_keep_one_binding() {
        let modules = vec![
            module(
                0,
                Vec::new(),
                vec![
                    re_export(FileId(1), "*", "*"),
                    re_export(FileId(2), "*", "*"),
                ],
            ),
            module(1, Vec::new(), vec![re_export(FileId(3), "*", "*")]),
            module(2, Vec::new(), vec![re_export(FileId(3), "*", "*")]),
            module(3, vec![value_export("foo")], Vec::new()),
        ];
        let index = EffectiveExportIndex::build(&modules);

        assert!(resolves_through(&index, FileId(0), FileId(1), "foo"));
        assert!(resolves_through(&index, FileId(0), FileId(2), "foo"));
    }

    #[test]
    fn star_exports_exclude_default_bindings() {
        let mut default = value_export("local");
        default.name = ExportName::Default;
        let index = EffectiveExportIndex::build(&[
            module(0, Vec::new(), vec![re_export(FileId(1), "*", "*")]),
            module(1, vec![default], Vec::new()),
        ]);

        assert_eq!(
            index.resolve(FileId(0), "default", ExportNamespace::Value),
            EffectiveExportResolution::Missing
        );
    }

    #[test]
    fn star_cycles_converge_on_the_same_binding() {
        let index = EffectiveExportIndex::build(&[
            module(0, Vec::new(), vec![re_export(FileId(1), "*", "*")]),
            module(
                1,
                Vec::new(),
                vec![
                    re_export(FileId(0), "*", "*"),
                    re_export(FileId(2), "*", "*"),
                ],
            ),
            module(2, vec![value_export("foo")], Vec::new()),
        ]);

        assert!(resolves_through(&index, FileId(0), FileId(2), "foo"));
        assert!(resolves_through(&index, FileId(1), FileId(2), "foo"));
    }

    #[test]
    fn type_only_namespace_re_export_resolves_only_in_type_namespace() {
        let mut namespace = re_export(FileId(1), "*", "Types");
        namespace.info.is_type_only = true;
        let index = EffectiveExportIndex::build(&[
            module(0, Vec::new(), vec![namespace]),
            module(1, vec![value_export("foo")], Vec::new()),
        ]);

        assert!(matches!(
            index.resolve(FileId(0), "Types", ExportNamespace::Type),
            EffectiveExportResolution::Unique(_)
        ));
        assert_eq!(
            index.resolve(FileId(0), "Types", ExportNamespace::Value),
            EffectiveExportResolution::Missing
        );
    }

    #[test]
    fn normal_namespace_re_export_resolves_in_both_namespaces() {
        let index = EffectiveExportIndex::build(&[
            module(0, Vec::new(), vec![re_export(FileId(1), "*", "Namespace")]),
            module(1, vec![value_export("foo")], Vec::new()),
        ]);

        let type_binding = index.resolve(FileId(0), "Namespace", ExportNamespace::Type);
        let value_binding = index.resolve(FileId(0), "Namespace", ExportNamespace::Value);
        assert!(matches!(type_binding, EffectiveExportResolution::Unique(_)));
        assert_eq!(type_binding, value_binding);
    }

    #[test]
    fn persisted_index_remains_queryable_without_reconstruction() {
        let index = EffectiveExportIndex::build(&[
            module(0, Vec::new(), vec![re_export(FileId(1), "foo", "bar")]),
            module(1, vec![value_export("foo")], Vec::new()),
        ]);
        let encoded = postcard::to_allocvec(&index).expect("encode effective export index");
        let decoded: EffectiveExportIndex =
            postcard::from_bytes(&encoded).expect("decode effective export index");

        assert!(matches!(
            decoded.resolve(FileId(0), "bar", ExportNamespace::Value),
            EffectiveExportResolution::Unique(binding) if binding.origin_file() == FileId(1)
        ));
        assert_eq!(
            decoded.resolve(FileId(0), "missing", ExportNamespace::Value),
            EffectiveExportResolution::Missing
        );
    }

    #[test]
    fn sfc_file_has_an_implicit_default_value_binding() {
        let mut sfc = module(0, Vec::new(), Vec::new());
        sfc.path = std::path::PathBuf::from("/project/Widget.vue");
        let index = EffectiveExportIndex::build(&[sfc]);

        assert!(matches!(
            index.resolve(FileId(0), "default", ExportNamespace::Value),
            EffectiveExportResolution::Unique(binding) if binding.origin_file() == FileId(0)
        ));
    }
}
