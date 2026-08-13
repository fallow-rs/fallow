//! Canonical effective export bindings for direct and transitive re-exports.

use std::collections::VecDeque;

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
    NamespaceObject {
        source: FileId,
    },
    ImplicitDefault,
    /// A known export surface whose declaration lives outside this graph.
    /// The re-export slot keeps opaque bindings distinct without pretending
    /// that Fallow can inspect or canonicalize the external declaration.
    ExternalReExport(usize),
}

impl EffectiveExportBinding {
    /// Module that owns the resolved declaration or opaque external surface.
    #[must_use]
    pub const fn origin_file(&self) -> FileId {
        self.file_id
    }

    pub(in crate::graph) const fn origin_slot(self) -> Option<usize> {
        match self.kind {
            EffectiveExportBindingKind::Declaration(slot) => Some(slot),
            EffectiveExportBindingKind::NamespaceObject { .. }
            | EffectiveExportBindingKind::ImplicitDefault
            | EffectiveExportBindingKind::ExternalReExport(_) => None,
        }
    }

    /// Source module represented by a namespace-object export.
    #[must_use]
    pub const fn namespace_source(self) -> Option<FileId> {
        match self.kind {
            EffectiveExportBindingKind::NamespaceObject { source } => Some(source),
            EffectiveExportBindingKind::Declaration(_)
            | EffectiveExportBindingKind::ImplicitDefault
            | EffectiveExportBindingKind::ExternalReExport(_) => None,
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

/// Bitset over the two export namespaces, for consumer-side namespace demand.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
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

    pub(super) const fn is_empty(self) -> bool {
        !self.r#type && !self.value
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

/// Propagation lane for one exported name.
///
/// The type namespace is resolved in two lanes. `Type` holds declarations that
/// really occupy TypeScript's type space (`interface`, `type`, `export type`).
/// `TypeFallback` holds the type meaning synthesized for plain value exports so
/// dual-space declarations (`class`, `enum`) keep resolving as types. A real
/// type declaration therefore wins over a synthesized one instead of colliding
/// with it, while two real declarations still collide into `Ambiguous`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ResolutionLane {
    Type,
    TypeFallback,
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct ExportLookup {
    key: ExportKey,
    lane: ResolutionLane,
}

impl ExportLookup {
    const fn new(file_id: FileId, name: ExportNameId, lane: ResolutionLane) -> Self {
        Self {
            key: ExportKey::new(file_id, name),
            lane,
        }
    }

    const fn with_file(self, file_id: FileId) -> Self {
        Self {
            key: self.key.with_file(file_id),
            ..self
        }
    }
}

/// Which lanes of one exported name are claimed by the module itself.
///
/// Seeding used to carry this in two `(file, name, lane)` hash sets alongside
/// the resolution table. The bits live on the resolution entry instead, so a
/// seed or observer registration hashes the name once instead of three times.
/// Build-time only: the flags are rebuilt with the index and stay out of the
/// serialized cache.
#[derive(Debug, Clone, Copy, Default)]
struct SeededLanes(u8);

impl SeededLanes {
    const fn bit(lane: ResolutionLane) -> u8 {
        match lane {
            ResolutionLane::Type => 1,
            ResolutionLane::TypeFallback => 2,
            ResolutionLane::Value => 4,
        }
    }

    const fn is_direct(self, lane: ResolutionLane) -> bool {
        self.0 & Self::bit(lane) != 0
    }

    /// Claim `lane` as locally declared, reporting whether it was unclaimed.
    const fn claim_direct(&mut self, lane: ResolutionLane) -> bool {
        let bit = Self::bit(lane);
        let claimed = self.0 & bit == 0;
        self.0 |= bit;
        claimed
    }

    const fn is_explicit(self, lane: ResolutionLane) -> bool {
        self.0 & (Self::bit(lane) | (Self::bit(lane) << 3)) != 0
    }

    const fn claim_explicit(&mut self, lane: ResolutionLane) {
        self.0 |= Self::bit(lane) << 3;
    }
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
struct NamespaceResolutions {
    r#type: EffectiveExportResolution,
    type_fallback: EffectiveExportResolution,
    value: EffectiveExportResolution,
    #[serde(skip)]
    seeded: SeededLanes,
}

impl NamespaceResolutions {
    /// Merge `incoming` into `lane`, reporting whether the lane advanced.
    fn merge(&mut self, lane: ResolutionLane, incoming: EffectiveExportResolution) -> bool {
        let current = self.get(lane);
        let next = current.merged_with(incoming);
        if current == next {
            return false;
        }
        self.set(lane, next);
        true
    }

    const fn get(self, lane: ResolutionLane) -> EffectiveExportResolution {
        match lane {
            ResolutionLane::Type => self.r#type,
            ResolutionLane::TypeFallback => self.type_fallback,
            ResolutionLane::Value => self.value,
        }
    }

    const fn set(&mut self, lane: ResolutionLane, resolution: EffectiveExportResolution) {
        match lane {
            ResolutionLane::Type => self.r#type = resolution,
            ResolutionLane::TypeFallback => self.type_fallback = resolution,
            ResolutionLane::Value => self.value = resolution,
        }
    }

    const fn effective(self, namespace: ExportNamespace) -> EffectiveExportResolution {
        match namespace {
            ExportNamespace::Value => self.value,
            ExportNamespace::Type => match self.r#type {
                EffectiveExportResolution::Missing => self.type_fallback,
                resolution => resolution,
            },
        }
    }
}

/// Upfront element counts for the propagation tables.
///
/// The tables are keyed by interned name, so they grow into the thousands on
/// real projects and re-hash repeatedly when they start empty. Counting is a
/// linear scan over already-resident slices and buys exact capacity.
#[derive(Clone, Copy)]
struct ProjectExportSizes {
    /// Distinct `(file, name)` pairs, an upper bound on the resolution table.
    keys: usize,
    /// Distinct `(file, name, lane)` triples, an upper bound on seeded lanes.
    lane_keys: usize,
    /// Interned name upper bound: every export plus every re-export alias.
    names: usize,
}

impl ProjectExportSizes {
    fn measure(modules: &[ResolvedModule]) -> Self {
        let mut keys = 0;
        let mut lane_keys = 0;
        let mut names = 0;
        for module in modules {
            let value_exports = module
                .exports
                .iter()
                .filter(|export| !export.is_type_only)
                .count();
            keys += module.exports.len() + module.re_exports.len();
            // Value exports also reserve the type lane and seed the fallback lane.
            lane_keys += module.exports.len() + value_exports * 2 + module.re_exports.len() * 3;
            names += module.exports.len() + module.re_exports.len() * 2;
        }
        Self {
            keys,
            lane_keys,
            names,
        }
    }
}

struct ExportNameInterner {
    ids: FxHashMap<Box<str>, ExportNameId>,
    next_id: usize,
}

impl ExportNameInterner {
    fn with_capacity(capacity: usize) -> Self {
        let mut ids = FxHashMap::with_capacity_and_hasher(capacity + 1, FxBuildHasher);
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

/// Destinations fed by one source lane.
///
/// A re-exported name almost always has a single barrel destination, so the
/// common case stays inline instead of allocating a one-element vector per
/// observed lane.
enum NamedDestinations {
    One(ExportLookup),
    Many(Vec<ExportLookup>),
}

impl NamedDestinations {
    fn push(&mut self, destination: ExportLookup) {
        match self {
            Self::One(first) => *self = Self::Many(vec![*first, destination]),
            Self::Many(destinations) => destinations.push(destination),
        }
    }

    fn iter(&self) -> std::slice::Iter<'_, ExportLookup> {
        match self {
            Self::One(destination) => std::slice::from_ref(destination).iter(),
            Self::Many(destinations) => destinations.iter(),
        }
    }
}

struct PropagationObservers {
    named: FxHashMap<ExportLookup, NamedDestinations>,
    star: FxHashMap<FileId, Vec<StarObserver>>,
}

struct ObserverBuildState<'a> {
    interner: &'a mut ExportNameInterner,
    resolutions: &'a mut FxHashMap<ExportKey, NamespaceResolutions>,
    queue: &'a mut VecDeque<ExportLookup>,
    observers: &'a mut PropagationObservers,
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
    /// Names resolved on each module, so [`EffectiveExportIndex::unique_bindings`]
    /// reads one module instead of scanning every key in the project.
    names_by_file: FxHashMap<FileId, Box<[ExportNameId]>>,
    declaration_merge_groups: DeclarationMergeGroups,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct DeclarationMergeGroups {
    groups: Vec<Box<[usize]>>,
    group_by_slot: FxHashMap<(FileId, usize), usize>,
}

impl EffectiveExportIndex {
    pub(super) fn build(modules: &[ResolvedModule]) -> Self {
        let sizes = ProjectExportSizes::measure(modules);
        let mut interner = ExportNameInterner::with_capacity(sizes.names);
        let mut resolutions = FxHashMap::with_capacity_and_hasher(sizes.keys, FxBuildHasher);
        let mut queue = VecDeque::with_capacity(sizes.lane_keys);
        seed_direct_bindings(modules, &mut interner, &mut resolutions, &mut queue);
        let observers = collect_observers(modules, &mut interner, &mut resolutions, &mut queue);
        propagate_bindings(&mut resolutions, &mut queue, &observers);

        Self {
            name_ids: interner.ids,
            names_by_file: index_names_by_file(&resolutions),
            resolutions,
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
                resolutions.effective(namespace)
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
        let Some(names) = self.names_by_file.get(&file_id) else {
            return FxHashSet::default();
        };
        names
            .iter()
            .filter_map(|name| {
                match self
                    .resolutions
                    .get(&ExportKey::new(file_id, *name))?
                    .effective(namespace)
                {
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
        if binding.origin_file() != file_id {
            return true;
        }
        let Some(origin_slot) = binding.origin_slot() else {
            return true;
        };
        if namespace == ExportNamespace::Type {
            let group = self.declaration_group_slots(binding);
            if !group.is_empty() {
                return group.contains(&export_index);
            }
        }
        exports
            .get(origin_slot)
            .is_some_and(|origin| export.is_type_only == origin.is_type_only)
    }

    pub(super) fn declaration_slots(
        &self,
        exports: &[ExportSymbol],
        candidates: &[usize],
        file_id: FileId,
        name: &str,
        namespace: ExportNamespace,
    ) -> Vec<usize> {
        let mut slots = Vec::new();
        self.extend_declaration_slots(
            DeclarationSlotQuery {
                exports,
                candidates,
                file_id,
                name,
                namespace,
            },
            &mut slots,
        );
        slots
    }

    /// Append the declaration slots for one name into `slots`.
    pub(super) fn extend_declaration_slots(
        &self,
        query: DeclarationSlotQuery<'_>,
        slots: &mut Vec<usize>,
    ) {
        let DeclarationSlotQuery {
            exports,
            candidates,
            file_id,
            name,
            namespace,
        } = query;
        let EffectiveExportResolution::Unique(binding) = self.resolve(file_id, name, namespace)
        else {
            return;
        };
        if binding.origin_file() == file_id && binding.origin_slot().is_some() {
            slots.extend(candidates.iter().copied().filter(|&index| {
                self.is_declaration_slot(exports, file_id, name, namespace, index)
            }));
            return;
        }

        let exact_type_only = namespace == ExportNamespace::Type
            && candidates.iter().any(|&index| exports[index].is_type_only);
        slots.extend(candidates.iter().copied().filter(|&index| {
            exports[index].name.matches_str(name) && exports[index].is_type_only == exact_type_only
        }));
    }
}

/// One name's candidate export slots on a module.
#[derive(Clone, Copy)]
pub(in crate::graph) struct DeclarationSlotQuery<'a> {
    pub(in crate::graph) exports: &'a [ExportSymbol],
    pub(in crate::graph) candidates: &'a [usize],
    pub(in crate::graph) file_id: FileId,
    pub(in crate::graph) name: &'a str,
    pub(in crate::graph) namespace: ExportNamespace,
}

fn index_names_by_file(
    resolutions: &FxHashMap<ExportKey, NamespaceResolutions>,
) -> FxHashMap<FileId, Box<[ExportNameId]>> {
    let mut names_by_file: FxHashMap<FileId, Vec<ExportNameId>> = FxHashMap::default();
    for key in resolutions.keys() {
        names_by_file.entry(key.file_id).or_default().push(key.name);
    }
    names_by_file
        .into_iter()
        .map(|(file_id, mut names)| {
            names.sort_unstable_by_key(|name| name.0);
            (file_id, names.into_boxed_slice())
        })
        .collect()
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
    interner: &mut ExportNameInterner,
    resolutions: &mut FxHashMap<ExportKey, NamespaceResolutions>,
    queue: &mut VecDeque<ExportLookup>,
) {
    let mut value_type_fallbacks = Vec::new();
    for module in modules {
        for (slot, export) in module.exports.iter().enumerate() {
            let lane = if export.is_type_only {
                ResolutionLane::Type
            } else {
                ResolutionLane::Value
            };
            let name = interner.intern_export_name(&export.name);
            let key = ExportLookup::new(module.file_id, name, lane);
            let binding = EffectiveExportResolution::Unique(EffectiveExportBinding {
                file_id: module.file_id,
                kind: EffectiveExportBindingKind::Declaration(slot),
            });
            let entry = resolutions.entry(key.key).or_default();
            // Same-name declarations inside one module form one local export
            // entry. This covers legal TypeScript declaration merging (for
            // example class/function plus namespace) without weakening the
            // ambiguity rule for distinct bindings arriving through stars.
            if entry.seeded.claim_direct(lane) && entry.merge(lane, binding) {
                queue.push_back(key);
            }
            if lane == ResolutionLane::Value {
                value_type_fallbacks.push((module.file_id, name, slot));
            }
        }
        seed_implicit_sfc_default(module, interner, resolutions, queue);
    }
    seed_value_type_fallbacks(value_type_fallbacks, resolutions, queue);
}

fn seed_value_type_fallbacks(
    fallbacks: Vec<(FileId, ExportNameId, usize)>,
    resolutions: &mut FxHashMap<ExportKey, NamespaceResolutions>,
    queue: &mut VecDeque<ExportLookup>,
) {
    for (file_id, name, slot) in fallbacks {
        let key = ExportKey::new(file_id, name);
        let binding = EffectiveExportResolution::Unique(EffectiveExportBinding {
            file_id,
            kind: EffectiveExportBindingKind::Declaration(slot),
        });
        let entry = resolutions.entry(key).or_default();
        // A locally declared name owns its module's type surface even when the
        // declaration only occupies value space, so the real type lane stays
        // reserved here and star sources cannot propagate into it.
        entry.seeded.claim_direct(ResolutionLane::Type);
        if entry.seeded.claim_direct(ResolutionLane::TypeFallback)
            && entry.merge(ResolutionLane::TypeFallback, binding)
        {
            queue.push_back(ExportLookup::new(
                file_id,
                name,
                ResolutionLane::TypeFallback,
            ));
        }
    }
}

fn seed_implicit_sfc_default(
    module: &ResolvedModule,
    interner: &mut ExportNameInterner,
    resolutions: &mut FxHashMap<ExportKey, NamespaceResolutions>,
    queue: &mut VecDeque<ExportLookup>,
) {
    if !is_sfc_path(&module.path) {
        return;
    }
    let name = interner.intern("default");
    let key = ExportKey::new(module.file_id, name);
    let entry = resolutions.entry(key).or_default();
    if entry.seeded.is_direct(ResolutionLane::Value) {
        return;
    }
    let binding = EffectiveExportResolution::Unique(EffectiveExportBinding {
        file_id: module.file_id,
        kind: EffectiveExportBindingKind::ImplicitDefault,
    });
    for lane in [ResolutionLane::Value, ResolutionLane::Type] {
        entry.seeded.claim_direct(lane);
        if entry.merge(lane, binding) {
            queue.push_back(ExportLookup::new(module.file_id, name, lane));
        }
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
    interner: &mut ExportNameInterner,
    resolutions: &mut FxHashMap<ExportKey, NamespaceResolutions>,
    queue: &mut VecDeque<ExportLookup>,
) -> PropagationObservers {
    let mut observers = PropagationObservers {
        named: FxHashMap::default(),
        star: FxHashMap::default(),
    };
    for module in modules {
        for (re_export_index, re_export) in module.re_exports.iter().enumerate() {
            let Some(source) = re_export.target.internal_file_id() else {
                if re_export.info.exported_name != "*"
                    && is_external_re_export_target(&re_export.target)
                {
                    register_external_re_export(
                        module.file_id,
                        re_export_index,
                        &re_export.info,
                        interner,
                        resolutions,
                        queue,
                    );
                }
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
                ObserverBuildState {
                    interner,
                    resolutions,
                    queue,
                    observers: &mut observers,
                },
            );
        }
    }
    observers
}

fn is_external_re_export_target(target: &crate::resolve::ResolveResult) -> bool {
    matches!(
        target,
        crate::resolve::ResolveResult::ExternalFile(_)
            | crate::resolve::ResolveResult::NpmPackage(_)
            | crate::resolve::ResolveResult::CommonJsNpmPackage(_)
    )
}

/// Seed an opaque barrel-owned binding for a named re-export of an external
/// declaration, so consumers importing through the barrel keep crediting the
/// export instead of losing it to a missing resolution. Unresolvable targets
/// stay `Missing`: an unknown surface must not manufacture credit.
fn register_external_re_export(
    barrel: FileId,
    re_export_index: usize,
    info: &fallow_types::extract::ReExportInfo,
    interner: &mut ExportNameInterner,
    resolutions: &mut FxHashMap<ExportKey, NamespaceResolutions>,
    queue: &mut VecDeque<ExportLookup>,
) {
    let exported_name = interner.intern(&info.exported_name);
    let binding = EffectiveExportResolution::Unique(EffectiveExportBinding {
        file_id: barrel,
        kind: EffectiveExportBindingKind::ExternalReExport(re_export_index),
    });
    let lanes: &[ResolutionLane] = if info.is_type_only {
        &[ResolutionLane::Type]
    } else {
        &[ResolutionLane::Type, ResolutionLane::Value]
    };
    for &lane in lanes {
        let destination = ExportLookup::new(barrel, exported_name, lane);
        let entry = resolutions.entry(destination.key).or_default();
        if entry.seeded.is_direct(lane) {
            continue;
        }
        entry.seeded.claim_explicit(lane);
        if entry.merge(lane, binding) {
            queue.push_back(destination);
        }
    }
}

fn register_named_observer(
    barrel: FileId,
    source: FileId,
    info: &fallow_types::extract::ReExportInfo,
    state: ObserverBuildState<'_>,
) {
    let ObserverBuildState {
        interner,
        resolutions,
        queue,
        observers,
    } = state;
    let exported_name = interner.intern(&info.exported_name);
    if info.imported_name == "*" {
        let lanes: &[ResolutionLane] = if info.is_type_only {
            &[ResolutionLane::Type]
        } else {
            &[ResolutionLane::Type, ResolutionLane::Value]
        };
        let binding = EffectiveExportResolution::Unique(EffectiveExportBinding {
            file_id: barrel,
            kind: EffectiveExportBindingKind::NamespaceObject { source },
        });
        for &lane in lanes {
            let destination = ExportLookup::new(barrel, exported_name, lane);
            let entry = resolutions.entry(destination.key).or_default();
            if entry.seeded.is_direct(lane) {
                continue;
            }
            entry.seeded.claim_explicit(lane);
            if entry.merge(lane, binding) {
                queue.push_back(destination);
            }
        }
        return;
    }

    let imported_name = interner.intern(&info.imported_name);
    let lanes: &[ResolutionLane] = if info.is_type_only {
        &[ResolutionLane::Type, ResolutionLane::TypeFallback]
    } else {
        &[
            ResolutionLane::Type,
            ResolutionLane::TypeFallback,
            ResolutionLane::Value,
        ]
    };
    for &lane in lanes {
        let destination = ExportLookup::new(barrel, exported_name, lane);
        let entry = resolutions.entry(destination.key).or_default();
        if entry.seeded.is_direct(lane) {
            continue;
        }
        entry.seeded.claim_explicit(lane);
        observers
            .named
            .entry(ExportLookup::new(source, imported_name, lane))
            .and_modify(|destinations| destinations.push(destination))
            .or_insert(NamedDestinations::One(destination));
    }
}

fn propagate_bindings(
    resolutions: &mut FxHashMap<ExportKey, NamespaceResolutions>,
    queue: &mut VecDeque<ExportLookup>,
    observers: &PropagationObservers,
) {
    while let Some(source_key) = queue.pop_front() {
        let Some(source_resolution) = resolutions
            .get(&source_key.key)
            .map(|resolutions| resolutions.get(source_key.lane))
        else {
            continue;
        };
        if let Some(destinations) = observers.named.get(&source_key) {
            for destination in destinations.iter() {
                merge_resolution(resolutions, queue, *destination, source_resolution);
            }
        }
        propagate_star_binding(
            resolutions,
            queue,
            observers,
            &source_key,
            source_resolution,
        );
    }
}

fn propagate_star_binding(
    resolutions: &mut FxHashMap<ExportKey, NamespaceResolutions>,
    queue: &mut VecDeque<ExportLookup>,
    observers: &PropagationObservers,
    source_key: &ExportLookup,
    source_resolution: EffectiveExportResolution,
) {
    if source_key.key.name == ExportNameId::DEFAULT {
        return;
    }
    let Some(star_observers) = observers.star.get(&source_key.key.file_id) else {
        return;
    };
    for observer in star_observers {
        if observer.type_only && source_key.lane == ResolutionLane::Value {
            continue;
        }
        let destination = source_key.with_file(observer.barrel);
        let entry = resolutions.entry(destination.key).or_default();
        if entry.seeded.is_explicit(destination.lane) {
            continue;
        }
        if entry.merge(destination.lane, source_resolution) {
            queue.push_back(destination);
        }
    }
}

fn merge_resolution(
    resolutions: &mut FxHashMap<ExportKey, NamespaceResolutions>,
    queue: &mut VecDeque<ExportLookup>,
    key: ExportLookup,
    incoming: EffectiveExportResolution,
) {
    if resolutions
        .entry(key.key)
        .or_default()
        .merge(key.lane, incoming)
    {
        queue.push_back(key);
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

    fn external_re_export(imported: &str, exported: &str, type_only: bool) -> ResolvedReExport {
        ResolvedReExport {
            info: ReExportInfo {
                source: "node:path".to_string(),
                imported_name: imported.to_string(),
                exported_name: exported.to_string(),
                is_type_only: type_only,
                span: oxc_span::Span::default(),
                statement_span: oxc_span::Span::default(),
                source_span: oxc_span::Span::default(),
            },
            target: ResolveResult::NpmPackage("node:path".to_string()),
        }
    }

    #[test]
    fn external_named_re_exports_keep_an_opaque_local_binding() {
        let index = EffectiveExportIndex::build(&[module(
            0,
            Vec::new(),
            vec![
                external_re_export("join", "join", false),
                external_re_export("Stats", "Stats", true),
                external_re_export("*", "path", false),
            ],
        )]);
        let encoded = postcard::to_allocvec(&index).expect("encode effective export index");
        let decoded: EffectiveExportIndex =
            postcard::from_bytes(&encoded).expect("decode effective export index");

        let value = decoded.resolve(FileId(0), "join", ExportNamespace::Value);
        assert!(matches!(
            value,
            EffectiveExportResolution::Unique(binding)
                if binding.origin_file() == FileId(0) && binding.origin_slot().is_none()
        ));
        assert_eq!(
            value,
            decoded.resolve(FileId(0), "join", ExportNamespace::Type)
        );
        assert!(matches!(
            decoded.resolve(FileId(0), "Stats", ExportNamespace::Type),
            EffectiveExportResolution::Unique(_)
        ));
        assert_eq!(
            decoded.resolve(FileId(0), "Stats", ExportNamespace::Value),
            EffectiveExportResolution::Missing
        );
        assert_eq!(
            decoded.resolve(FileId(0), "path", ExportNamespace::Type),
            decoded.resolve(FileId(0), "path", ExportNamespace::Value)
        );
    }

    #[test]
    fn unresolved_named_re_exports_do_not_gain_an_external_binding() {
        let mut unresolved = external_re_export("missing", "missing", false);
        unresolved.target = ResolveResult::Unresolvable("./missing".to_string());
        let index = EffectiveExportIndex::build(&[module(0, Vec::new(), vec![unresolved])]);

        assert_eq!(
            index.resolve(FileId(0), "missing", ExportNamespace::Type),
            EffectiveExportResolution::Missing
        );
        assert_eq!(
            index.resolve(FileId(0), "missing", ExportNamespace::Value),
            EffectiveExportResolution::Missing
        );
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
    fn a_real_type_declaration_wins_over_a_value_type_fallback() {
        let mut interface = value_export("User");
        interface.is_type_only = true;
        let modules = vec![
            module(
                0,
                Vec::new(),
                vec![
                    re_export(FileId(1), "*", "*"),
                    re_export(FileId(2), "*", "*"),
                ],
            ),
            module(1, vec![value_export("User")], Vec::new()),
            module(2, vec![interface], Vec::new()),
        ];
        let index = EffectiveExportIndex::build(&modules);

        assert!(matches!(
            index.resolve(FileId(0), "User", ExportNamespace::Type),
            EffectiveExportResolution::Unique(binding) if binding.origin_file() == FileId(2)
        ));
        assert!(matches!(
            index.resolve(FileId(0), "User", ExportNamespace::Value),
            EffectiveExportResolution::Unique(binding) if binding.origin_file() == FileId(1)
        ));
        assert!(index.resolves_through(
            FileId(0),
            "User",
            FileId(2),
            "User",
            ExportNamespace::Type
        ));
        assert!(index.resolves_through(
            FileId(0),
            "User",
            FileId(1),
            "User",
            ExportNamespace::Value
        ));
    }

    #[test]
    fn colliding_real_type_declarations_stay_ambiguous() {
        let type_export = |name: &str| {
            let mut export = value_export(name);
            export.is_type_only = true;
            export
        };
        let modules = vec![
            module(
                0,
                Vec::new(),
                vec![
                    re_export(FileId(1), "*", "*"),
                    re_export(FileId(2), "*", "*"),
                ],
            ),
            module(1, vec![type_export("User")], Vec::new()),
            module(2, vec![type_export("User")], Vec::new()),
        ];
        let index = EffectiveExportIndex::build(&modules);

        assert_eq!(
            index.resolve(FileId(0), "User", ExportNamespace::Type),
            EffectiveExportResolution::Ambiguous
        );
    }

    #[test]
    fn a_value_export_still_carries_its_type_meaning_through_a_barrel() {
        let modules = vec![
            module(0, Vec::new(), vec![re_export(FileId(1), "*", "*")]),
            module(1, vec![value_export("Widget")], Vec::new()),
        ];
        let index = EffectiveExportIndex::build(&modules);

        assert!(index.resolves_through(
            FileId(0),
            "Widget",
            FileId(1),
            "Widget",
            ExportNamespace::Type
        ));
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
