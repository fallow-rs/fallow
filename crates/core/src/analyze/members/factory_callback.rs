//! Callback result ownership joins for source helpers and canonical Vue producers.

use super::*;
use crate::extract::ImportedName;
use crate::graph::{EffectiveExportBinding, EffectiveExportResolution, ExportNamespace};
use crate::resolve::{ResolveResult, ResolvedImport};
use fallow_types::extract::FactoryPropertySource;

#[derive(Default)]
struct CallbackProducer {
    vue: bool,
    ordinary_assignment: bool,
    origin: Option<ExportKey>,
}

pub(super) fn propagate_callback_factory_accesses(
    graph: &ModuleGraph,
    modules: &[ResolvedModule],
    indexes: &MemberPassIndexes<'_>,
    sources: &FactoryPropertySources<'_>,
    accessed: &mut FxHashMap<ExportKey, FxHashSet<String>>,
) {
    if !modules.iter().any(|module| {
        SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
            .callback_factory_property_accesses()
            .next()
            .is_some()
    }) {
        return;
    }
    let imports: FxHashMap<_, _> = modules
        .iter()
        .flat_map(|module| {
            module
                .resolved_imports
                .iter()
                .filter(|import| !import.info.is_type_only)
                .map(move |import| ((module.file_id, import.info.local_name.as_str()), import))
        })
        .collect();
    let vue_bindings = vue_computed_bindings(graph, modules, &imports);
    let mut producers = FxHashMap::default();
    let mut classes = FxHashMap::default();
    for module in modules {
        for access in SemanticFactView::new(&module.semantic_facts, &module.member_accesses)
            .callback_factory_property_accesses()
        {
            let producer = producers
                .entry((
                    module.file_id,
                    access.callee_name.as_str(),
                    access.callee_member.as_deref(),
                ))
                .or_insert_with(|| {
                    imports
                        .get(&(module.file_id, access.callee_name.as_str()))
                        .map(|import| {
                            callback_producer(
                                graph,
                                import,
                                access.callee_member.as_deref(),
                                &vue_bindings,
                            )
                        })
                        .unwrap_or_default()
                });
            let class = if producer.vue {
                if access.property_path != "value" {
                    continue;
                }
                access
                    .arguments
                    .iter()
                    .find(|argument| argument.parameter_index == 0 && argument.lazy_safe)
                    .map(|argument| argument.class_local_name.as_str())
            } else if access.vue_only {
                None
            } else if access.assigned_class.is_some() && producer.ordinary_assignment {
                access.assigned_class.as_deref()
            } else if let Some(origin) = &producer.origin {
                match sources.get(&(
                    origin.file_id,
                    origin.export_name.as_str(),
                    access.property_path.as_str(),
                )) {
                    Some(FactoryPropertySource::CallbackParameterReturn { parameter_index }) => {
                        access
                            .arguments
                            .iter()
                            .find(|argument| {
                                argument.parameter_index == *parameter_index && !argument.getter
                            })
                            .map(|argument| argument.class_local_name.as_str())
                    }
                    _ => None,
                }
            } else {
                None
            };
            let Some(class) = class else {
                continue;
            };
            let origins = classes.entry((module.file_id, class)).or_insert_with(|| {
                factory_return_class_origins(graph, indexes, module.file_id, class)
            });
            for class in origins {
                accessed
                    .entry(class.clone())
                    .or_default()
                    .insert(access.member.clone());
            }
        }
    }
}

fn vue_package(source: &str, target: &ResolveResult) -> bool {
    matches!(source, "vue" | "@vue/reactivity")
        && matches!(target, ResolveResult::NpmPackage(package) if package == source)
}

fn vue_computed_bindings(
    graph: &ModuleGraph,
    modules: &[ResolvedModule],
    imports: &FxHashMap<(FileId, &str), &ResolvedImport>,
) -> FxHashSet<EffectiveExportBinding> {
    let mut bindings = FxHashSet::default();
    for module in modules {
        for export in &module.re_exports {
            if !export.info.is_type_only
                && export.info.imported_name == "computed"
                && vue_package(&export.info.source, &export.target)
                && let EffectiveExportResolution::Unique(binding) = graph.resolve_export(
                    module.file_id,
                    &export.info.exported_name,
                    ExportNamespace::Value,
                )
            {
                bindings.insert(binding);
            }
        }
        for export in module.exports.iter().filter(|export| !export.is_type_only) {
            let export_name = export.name.to_string();
            let local = export.local_name.as_deref().unwrap_or(&export_name);
            if imports.get(&(module.file_id, local)).is_some_and(|import|
                matches!(&import.info.imported_name, ImportedName::Named(name) if name == "computed") && vue_package(&import.info.source, &import.target))
                && let EffectiveExportResolution::Unique(binding) = graph.resolve_export(module.file_id, &export_name, ExportNamespace::Value)
            { bindings.insert(binding); }
        }
    }
    bindings
}

fn callback_producer(
    graph: &ModuleGraph,
    import: &ResolvedImport,
    member: Option<&str>,
    vue_bindings: &FxHashSet<EffectiveExportBinding>,
) -> CallbackProducer {
    let name = match (&import.info.imported_name, member) {
        (ImportedName::Named(name), None) => name.as_str(),
        (ImportedName::Namespace, Some(member)) => member,
        (ImportedName::Default, None) => "default",
        _ => return CallbackProducer::default(),
    };
    if name == "computed" && vue_package(&import.info.source, &import.target) {
        return CallbackProducer {
            vue: true,
            ..CallbackProducer::default()
        };
    }
    let Some(file) = import.target.internal_file_id() else {
        return CallbackProducer {
            ordinary_assignment: import.target.is_bare_package(),
            ..CallbackProducer::default()
        };
    };
    let EffectiveExportResolution::Unique(binding) =
        graph.resolve_export(file, name, ExportNamespace::Value)
    else {
        return CallbackProducer::default();
    };
    if vue_bindings.contains(&binding) {
        return CallbackProducer {
            vue: true,
            ..CallbackProducer::default()
        };
    }
    CallbackProducer {
        vue: false,
        ordinary_assignment: true,
        origin: walk_re_export_origins(graph, file, name).into_iter().next(),
    }
}
