//! Edges to every condition target of a package.json `imports` entry.
//!
//! An entry such as `"#x": { "browser": "./x.browser.ts", "default": "./x.ts" }`
//! loads a different file in each environment. The resolver follows only the
//! active conditions, so `x.browser.ts` gets no edge and shows as unused. This
//! pass adds an edge from the importer to each other file target of the
//! entry, with the same imported names.

use fallow_types::discover::FileId;
use fallow_types::extract::{ImportInfo, ImportedName};
use serde_json::Value;

use super::fallbacks::{
    PackageMapTarget, nearest_package_manifest, package_map_target, resolve_package_map_target,
};
use super::react_native::retarget;
use super::types::{ResolveContext, ResolveResult, ResolvedImport, ResolvedModule};

/// Add an edge to each condition target of the `imports` entries that a
/// module imports or re-exports.
pub(super) fn synthesize_package_import_condition_edges(
    resolved: &mut [ResolvedModule],
    ctx: &ResolveContext<'_>,
) {
    if ctx
        .package_manifests
        .iter()
        .all(|manifest| manifest.package_json.imports.is_none())
    {
        return;
    }
    for module in resolved.iter_mut() {
        let mut extra = import_edges(ctx, module);
        extra.extend(re_export_edges(ctx, module));
        module.resolved_imports.extend(extra);
    }
}

/// An edge with the same imported names to each other target of an imported
/// or dynamically imported entry.
fn import_edges(ctx: &ResolveContext<'_>, module: &ResolvedModule) -> Vec<ResolvedImport> {
    let mut edges = Vec::new();
    for import in module
        .resolved_imports
        .iter()
        .chain(&module.resolved_dynamic_imports)
    {
        for other in other_condition_targets(ctx, module, &import.info.source, &import.target) {
            if let Some(target) = retarget(&import.target, other) {
                edges.push(ResolvedImport {
                    info: import.info.clone(),
                    target,
                });
            }
        }
    }
    edges
}

/// An edge to each other target of a re-exported entry. Re-export propagation
/// keeps its single resolved source. A named re-export credits its name in the
/// other targets. A star re-export gets a side-effect edge, which keeps the
/// other target reachable. The edge keeps the source span of the re-export, so
/// the duplicate-export check sees the targets as one entry.
fn re_export_edges(ctx: &ResolveContext<'_>, module: &ResolvedModule) -> Vec<ResolvedImport> {
    let mut edges = Vec::new();
    for re_export in &module.re_exports {
        let imported_name = match re_export.info.imported_name.as_str() {
            "*" => ImportedName::SideEffect,
            "default" => ImportedName::Default,
            name => ImportedName::Named(name.to_owned()),
        };
        for other in other_condition_targets(ctx, module, &re_export.info.source, &re_export.target)
        {
            edges.push(ResolvedImport {
                info: ImportInfo {
                    source: re_export.info.source.clone(),
                    imported_name: imported_name.clone(),
                    local_name: String::new(),
                    is_type_only: re_export.info.is_type_only,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::default(),
                    source_span: re_export.info.source_span,
                },
                target: ResolveResult::InternalModule(other),
            });
        }
    }
    edges
}

/// The project files that the `imports` entry for `specifier` names under any
/// condition, other than the `resolved` target and the importing module. Only
/// a target that the entry names as a path (`./...`) counts; a package target
/// is not a file of the package.
fn other_condition_targets(
    ctx: &ResolveContext<'_>,
    module: &ResolvedModule,
    specifier: &str,
    resolved: &ResolveResult,
) -> Vec<FileId> {
    if !specifier.starts_with('#') {
        return Vec::new();
    }
    let Some(resolved) = resolved.internal_file_id() else {
        return Vec::new();
    };
    let Some(manifest) = nearest_package_manifest(ctx.package_manifests, &module.path) else {
        return Vec::new();
    };
    let Some(imports) = manifest.package_json.imports.as_ref() else {
        return Vec::new();
    };
    let mut conditions = Vec::new();
    collect_condition_names(imports, &mut conditions);
    let PackageMapTarget::Targets(targets) = package_map_target(imports, specifier, &conditions)
    else {
        return Vec::new();
    };
    let mut files: Vec<FileId> = Vec::new();
    for target in targets.iter().filter(|target| target.starts_with("./")) {
        if let Some(file_id) = resolve_package_map_target(ctx, manifest, target, None)
            && file_id != resolved
            && file_id != module.file_id
            && !files.contains(&file_id)
        {
            files.push(file_id);
        }
    }
    files
}

/// Collect every condition key of a package map. A subpath key (`.`, `./x`,
/// `#x`) is not a condition.
fn collect_condition_names(value: &Value, names: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, nested) in map {
                if !key.starts_with('.') && !key.starts_with('#') && !names.contains(key) {
                    names.push(key.clone());
                }
                collect_condition_names(nested, names);
            }
        }
        Value::Array(values) => {
            for nested in values {
                collect_condition_names(nested, names);
            }
        }
        Value::String(_) | Value::Bool(_) | Value::Null | Value::Number(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_names_cover_nested_conditions_and_skip_subpath_keys() {
        let imports = serde_json::json!({
            "#a": { "browser": "./a.browser.ts", "node": { "import": "./a.mjs" }, "default": "./a.ts" },
            "#b": ["./b.ts", { "worker": "./b.worker.ts" }],
            "#c": "./c.ts"
        });
        let mut names = Vec::new();
        collect_condition_names(&imports, &mut names);
        names.sort();
        assert_eq!(names, ["browser", "default", "import", "node", "worker"]);
    }
}
