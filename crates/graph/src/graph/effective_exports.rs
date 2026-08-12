//! Canonical effective export bindings for direct and transitive re-exports.

use std::collections::VecDeque;

use fallow_types::discover::FileId;
use fallow_types::extract::ExportName;
use rustc_hash::{FxHashMap, FxHashSet};

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

    const fn with_file(self, file_id: FileId) -> Self {
        Self {
            key: self.key.with_file(file_id),
            ..self
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
}

struct ExportNameInterner {
    ids: FxHashMap<Box<str>, ExportNameId>,
    next_id: usize,
}

impl ExportNameInterner {
    fn new() -> Self {
        let ids = FxHashMap::from_iter([(Box::<str>::from("default"), ExportNameId::DEFAULT)]);
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

struct PropagationObservers {
    explicit_keys: FxHashSet<ExportLookup>,
    named: FxHashMap<ExportLookup, Vec<ExportLookup>>,
    star: FxHashMap<FileId, Vec<StarObserver>>,
}

struct ObserverBuildState<'a> {
    direct_keys: &'a FxHashSet<ExportLookup>,
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
    declaration_merge_groups: DeclarationMergeGroups,
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct DeclarationMergeGroups {
    groups: Vec<Box<[usize]>>,
    group_by_slot: FxHashMap<(FileId, usize), usize>,
}

impl EffectiveExportIndex {
    pub(super) fn build(modules: &[ResolvedModule]) -> Self {
        let mut interner = ExportNameInterner::new();
        let mut resolutions = FxHashMap::default();
        let mut queue = VecDeque::new();
        let direct_keys =
            seed_direct_bindings(modules, &mut interner, &mut resolutions, &mut queue);
        let observers = collect_observers(
            modules,
            &direct_keys,
            &mut interner,
            &mut resolutions,
            &mut queue,
        );
        propagate_bindings(&mut resolutions, &mut queue, &observers);

        Self {
            name_ids: interner.ids,
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
) -> FxHashSet<ExportLookup> {
    let mut direct_keys = FxHashSet::default();
    let mut value_type_fallbacks = Vec::new();
    for module in modules {
        for (slot, export) in module.exports.iter().enumerate() {
            let namespace = if export.is_type_only {
                ExportNamespace::Type
            } else {
                ExportNamespace::Value
            };
            let name = interner.intern_export_name(&export.name);
            let key = ExportLookup::new(module.file_id, name, namespace);
            // Same-name declarations inside one module form one local export
            // entry. This covers legal TypeScript declaration merging (for
            // example class/function plus namespace) without weakening the
            // ambiguity rule for distinct bindings arriving through stars.
            if direct_keys.insert(key) {
                merge_resolution(
                    resolutions,
                    queue,
                    key,
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
        seed_implicit_sfc_default(module, interner, &mut direct_keys, resolutions, queue);
    }
    seed_value_type_fallbacks(value_type_fallbacks, &mut direct_keys, resolutions, queue);
    direct_keys
}

fn seed_value_type_fallbacks(
    fallbacks: Vec<(FileId, ExportNameId, usize)>,
    direct_keys: &mut FxHashSet<ExportLookup>,
    resolutions: &mut FxHashMap<ExportKey, NamespaceResolutions>,
    queue: &mut VecDeque<ExportLookup>,
) {
    for (file_id, name, slot) in fallbacks {
        let type_key = ExportLookup::new(file_id, name, ExportNamespace::Type);
        if direct_keys.contains(&type_key) {
            continue;
        }
        direct_keys.insert(type_key);
        merge_resolution(
            resolutions,
            queue,
            type_key,
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
    direct_keys: &mut FxHashSet<ExportLookup>,
    resolutions: &mut FxHashMap<ExportKey, NamespaceResolutions>,
    queue: &mut VecDeque<ExportLookup>,
) {
    if !is_sfc_path(&module.path) {
        return;
    }
    let name = interner.intern("default");
    let value = ExportLookup::new(module.file_id, name, ExportNamespace::Value);
    if direct_keys.contains(&value) {
        return;
    }
    let binding = EffectiveExportResolution::Unique(EffectiveExportBinding {
        file_id: module.file_id,
        kind: EffectiveExportBindingKind::ImplicitDefault,
    });
    for namespace in [ExportNamespace::Value, ExportNamespace::Type] {
        let key = ExportLookup::new(module.file_id, name, namespace);
        direct_keys.insert(key);
        merge_resolution(resolutions, queue, key, binding);
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
    direct_keys: &FxHashSet<ExportLookup>,
    interner: &mut ExportNameInterner,
    resolutions: &mut FxHashMap<ExportKey, NamespaceResolutions>,
    queue: &mut VecDeque<ExportLookup>,
) -> PropagationObservers {
    let mut observers = PropagationObservers {
        explicit_keys: direct_keys.clone(),
        named: FxHashMap::default(),
        star: FxHashMap::default(),
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
                ObserverBuildState {
                    direct_keys,
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

fn register_named_observer(
    barrel: FileId,
    source: FileId,
    info: &fallow_types::extract::ReExportInfo,
    state: ObserverBuildState<'_>,
) {
    let ObserverBuildState {
        direct_keys,
        interner,
        resolutions,
        queue,
        observers,
    } = state;
    let exported_name = interner.intern(&info.exported_name);
    if info.imported_name == "*" {
        let namespaces: &[ExportNamespace] = if info.is_type_only {
            &[ExportNamespace::Type]
        } else {
            &[ExportNamespace::Type, ExportNamespace::Value]
        };
        for &namespace in namespaces {
            let destination = ExportLookup::new(barrel, exported_name, namespace);
            if !direct_keys.contains(&destination) {
                observers.explicit_keys.insert(destination);
                merge_resolution(
                    resolutions,
                    queue,
                    destination,
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
    let namespaces: &[ExportNamespace] = if info.is_type_only {
        &[ExportNamespace::Type]
    } else {
        &[ExportNamespace::Type, ExportNamespace::Value]
    };
    for &namespace in namespaces {
        let destination = ExportLookup::new(barrel, exported_name, namespace);
        if direct_keys.contains(&destination) {
            continue;
        }
        observers.explicit_keys.insert(destination);
        observers
            .named
            .entry(ExportLookup::new(source, imported_name, namespace))
            .or_default()
            .push(destination);
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
            .map(|resolutions| resolutions.get(source_key.namespace))
        else {
            continue;
        };
        if let Some(destinations) = observers.named.get(&source_key) {
            for destination in destinations {
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
        if observer.type_only && source_key.namespace == ExportNamespace::Value {
            continue;
        }
        let mut destination = source_key.with_file(observer.barrel);
        if observer.type_only {
            destination.namespace = ExportNamespace::Type;
        }
        if observers.explicit_keys.contains(&destination) {
            continue;
        }
        merge_resolution(resolutions, queue, destination, source_resolution);
    }
}

fn merge_resolution(
    resolutions: &mut FxHashMap<ExportKey, NamespaceResolutions>,
    queue: &mut VecDeque<ExportLookup>,
    key: ExportLookup,
    incoming: EffectiveExportResolution,
) {
    let namespace_resolutions = resolutions.entry(key.key).or_default();
    let current = namespace_resolutions.get(key.namespace);
    let next = current.merged_with(incoming);
    if current == next {
        return;
    }
    namespace_resolutions.set(key.namespace, next);
    queue.push_back(key);
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
