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
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct EffectiveExportBinding {
    file_id: FileId,
    /// Stable declaration slot within the resolved module. Direct exports use
    /// their export index; namespace re-exports use the following re-export
    /// range. Named re-exports retain their source binding identity.
    slot: usize,
}

impl EffectiveExportBinding {
    /// Module that owns the resolved declaration.
    #[must_use]
    pub const fn origin_file(&self) -> FileId {
        self.file_id
    }
}

/// Effective resolution for one module/name/namespace tuple.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EffectiveExportResolution {
    /// The module does not export this name in the requested namespace.
    Missing,
    /// Exactly one canonical declaration supplies the exported name.
    Unique(EffectiveExportBinding),
    /// Multiple distinct star-exported declarations supply the same name.
    Ambiguous,
}

impl EffectiveExportResolution {
    fn merged_with(&self, incoming: &Self) -> Self {
        match (self, incoming) {
            (Self::Missing, resolution) | (resolution, Self::Missing) => resolution.clone(),
            (Self::Unique(left), Self::Unique(right)) if left == right => self.clone(),
            (Self::Ambiguous, _) | (_, Self::Ambiguous) | (Self::Unique(_), Self::Unique(_)) => {
                Self::Ambiguous
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
struct ExportKey {
    file_id: FileId,
    name: ExportName,
    namespace: ExportNamespace,
}

impl ExportKey {
    fn new(file_id: FileId, name: ExportName, namespace: ExportNamespace) -> Self {
        Self {
            file_id,
            name,
            namespace,
        }
    }

    fn with_file(&self, file_id: FileId) -> Self {
        Self {
            file_id,
            name: self.name.clone(),
            namespace: self.namespace,
        }
    }
}

#[derive(Clone, Copy)]
struct StarObserver {
    barrel: FileId,
    type_only: bool,
}

struct PropagationObservers {
    explicit_keys: FxHashSet<ExportKey>,
    named: FxHashMap<ExportKey, Vec<ExportKey>>,
    star: FxHashMap<FileId, Vec<StarObserver>>,
}

struct ObserverBuildState<'a> {
    direct_keys: &'a FxHashSet<ExportKey>,
    resolutions: &'a mut FxHashMap<ExportKey, EffectiveExportResolution>,
    queue: &'a mut VecDeque<ExportKey>,
    observers: &'a mut PropagationObservers,
}

/// Immutable effective export bindings for one resolved project.
///
/// Resolution is a finite monotone propagation: each key advances at most from
/// missing to one binding and then to ambiguous. Cycles therefore terminate,
/// while multiple paths to the same binding remain unique.
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub(super) struct EffectiveExportIndex {
    resolutions: FxHashMap<ExportKey, EffectiveExportResolution>,
}

impl EffectiveExportIndex {
    pub(super) fn build(modules: &[ResolvedModule]) -> Self {
        let mut resolutions = FxHashMap::default();
        let mut queue = VecDeque::new();
        let direct_keys = seed_direct_bindings(modules, &mut resolutions, &mut queue);
        let observers = collect_observers(modules, &direct_keys, &mut resolutions, &mut queue);
        propagate_bindings(&mut resolutions, &mut queue, &observers);

        Self { resolutions }
    }

    pub(super) fn resolve(
        &self,
        file_id: FileId,
        name: &str,
        namespace: ExportNamespace,
    ) -> EffectiveExportResolution {
        self.resolutions
            .get(&ExportKey::new(file_id, parse_export_name(name), namespace))
            .cloned()
            .unwrap_or(EffectiveExportResolution::Missing)
    }

    pub(super) fn resolves_through(
        &self,
        barrel: FileId,
        source: FileId,
        name: &str,
        namespace: ExportNamespace,
    ) -> bool {
        matches!(
            (
                self.resolve(barrel, name, namespace),
                self.resolve(source, name, namespace),
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
}

fn seed_direct_bindings(
    modules: &[ResolvedModule],
    resolutions: &mut FxHashMap<ExportKey, EffectiveExportResolution>,
    queue: &mut VecDeque<ExportKey>,
) -> FxHashSet<ExportKey> {
    let mut direct_keys = FxHashSet::default();
    let mut value_type_fallbacks = Vec::new();
    for module in modules {
        for (slot, export) in module.exports.iter().enumerate() {
            let namespace = if export.is_type_only {
                ExportNamespace::Type
            } else {
                ExportNamespace::Value
            };
            let key = ExportKey::new(module.file_id, export.name.clone(), namespace);
            direct_keys.insert(key.clone());
            merge_resolution(
                resolutions,
                queue,
                key,
                &EffectiveExportResolution::Unique(EffectiveExportBinding {
                    file_id: module.file_id,
                    slot,
                }),
            );
            if namespace == ExportNamespace::Value {
                value_type_fallbacks.push((module.file_id, export.name.clone(), slot));
            }
        }
    }
    seed_value_type_fallbacks(value_type_fallbacks, &mut direct_keys, resolutions, queue);
    direct_keys
}

fn seed_value_type_fallbacks(
    fallbacks: Vec<(FileId, ExportName, usize)>,
    direct_keys: &mut FxHashSet<ExportKey>,
    resolutions: &mut FxHashMap<ExportKey, EffectiveExportResolution>,
    queue: &mut VecDeque<ExportKey>,
) {
    for (file_id, name, slot) in fallbacks {
        let type_key = ExportKey::new(file_id, name.clone(), ExportNamespace::Type);
        if direct_keys.contains(&type_key) {
            continue;
        }
        direct_keys.insert(type_key.clone());
        merge_resolution(
            resolutions,
            queue,
            type_key,
            &EffectiveExportResolution::Unique(EffectiveExportBinding { file_id, slot }),
        );
    }
}

fn collect_observers(
    modules: &[ResolvedModule],
    direct_keys: &FxHashSet<ExportKey>,
    resolutions: &mut FxHashMap<ExportKey, EffectiveExportResolution>,
    queue: &mut VecDeque<ExportKey>,
) -> PropagationObservers {
    let mut observers = PropagationObservers {
        explicit_keys: direct_keys.clone(),
        named: FxHashMap::default(),
        star: FxHashMap::default(),
    };
    for module in modules {
        for (re_export_index, re_export) in module.re_exports.iter().enumerate() {
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
                module.exports.len() + re_export_index,
                ObserverBuildState {
                    direct_keys,
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
    re_export_slot: usize,
    state: ObserverBuildState<'_>,
) {
    let ObserverBuildState {
        direct_keys,
        resolutions,
        queue,
        observers,
    } = state;
    let exported_name = parse_export_name(&info.exported_name);
    if info.imported_name == "*" {
        let namespace = if info.is_type_only {
            ExportNamespace::Type
        } else {
            ExportNamespace::Value
        };
        let destination = ExportKey::new(barrel, exported_name, namespace);
        if !direct_keys.contains(&destination) {
            observers.explicit_keys.insert(destination.clone());
            merge_resolution(
                resolutions,
                queue,
                destination,
                &EffectiveExportResolution::Unique(EffectiveExportBinding {
                    file_id: barrel,
                    slot: re_export_slot,
                }),
            );
        }
        return;
    }

    let imported_name = parse_export_name(&info.imported_name);
    let namespaces: &[ExportNamespace] = if info.is_type_only {
        &[ExportNamespace::Type]
    } else {
        &[ExportNamespace::Type, ExportNamespace::Value]
    };
    for &namespace in namespaces {
        let destination = ExportKey::new(barrel, exported_name.clone(), namespace);
        if direct_keys.contains(&destination) {
            continue;
        }
        observers.explicit_keys.insert(destination.clone());
        observers
            .named
            .entry(ExportKey::new(source, imported_name.clone(), namespace))
            .or_default()
            .push(destination);
    }
}

fn propagate_bindings(
    resolutions: &mut FxHashMap<ExportKey, EffectiveExportResolution>,
    queue: &mut VecDeque<ExportKey>,
    observers: &PropagationObservers,
) {
    while let Some(source_key) = queue.pop_front() {
        let Some(source_resolution) = resolutions.get(&source_key).cloned() else {
            continue;
        };
        if let Some(destinations) = observers.named.get(&source_key) {
            for destination in destinations {
                merge_resolution(resolutions, queue, destination.clone(), &source_resolution);
            }
        }
        propagate_star_binding(
            resolutions,
            queue,
            observers,
            &source_key,
            &source_resolution,
        );
    }
}

fn propagate_star_binding(
    resolutions: &mut FxHashMap<ExportKey, EffectiveExportResolution>,
    queue: &mut VecDeque<ExportKey>,
    observers: &PropagationObservers,
    source_key: &ExportKey,
    source_resolution: &EffectiveExportResolution,
) {
    if matches!(source_key.name, ExportName::Default) {
        return;
    }
    let Some(star_observers) = observers.star.get(&source_key.file_id) else {
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
    resolutions: &mut FxHashMap<ExportKey, EffectiveExportResolution>,
    queue: &mut VecDeque<ExportKey>,
    key: ExportKey,
    incoming: &EffectiveExportResolution,
) {
    let next = resolutions
        .get(&key)
        .map_or_else(|| incoming.clone(), |current| current.merged_with(incoming));
    if resolutions.get(&key) == Some(&next) {
        return;
    }
    resolutions.insert(key.clone(), next);
    queue.push_back(key);
}

fn parse_export_name(name: &str) -> ExportName {
    if name == "default" {
        ExportName::Default
    } else {
        ExportName::Named(name.to_string())
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
        index.resolves_through(barrel, source, name, ExportNamespace::Value)
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
}
