//! Public API graph helpers owned by the engine boundary.

use std::path::{Component, Path, PathBuf};

use fallow_config::{PackageJson, ResolvedConfig, WorkspaceInfo};
use fallow_types::discover::FileId;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    discover::{EntryPoint, EntryPointSource, SOURCE_EXTENSIONS},
    module_graph::RetainedModuleGraph,
};

const OUTPUT_DIRS: &[&str] = &["dist", "build", "out", "esm", "cjs"];

/// Compute the exports-aware public API entry-point set for a project graph.
#[must_use]
pub fn public_api_package_entry_points(
    graph: &RetainedModuleGraph,
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[WorkspaceInfo],
) -> FxHashSet<FileId> {
    let graph = graph.as_graph();
    let mut public_api_entry_points = FxHashSet::default();
    let path_to_file_id = graph_path_to_file_id(graph);
    let canonical_project_root =
        dunce::canonicalize(&config.root).unwrap_or_else(|_| config.root.clone());

    add_root_public_api_entry_points(
        &mut public_api_entry_points,
        graph,
        &path_to_file_id,
        config,
        root_pkg,
        &canonical_project_root,
    );
    add_workspace_public_api_entry_points(
        &mut public_api_entry_points,
        graph,
        &path_to_file_id,
        workspaces,
        &canonical_project_root,
    );

    public_api_entry_points
}

fn graph_path_to_file_id(graph: &fallow_graph::graph::ModuleGraph) -> FxHashMap<PathBuf, FileId> {
    graph
        .modules
        .iter()
        .map(|module| (module.path.clone(), module.file_id))
        .collect()
}

fn add_root_public_api_entry_points(
    public_api_entry_points: &mut FxHashSet<FileId>,
    graph: &fallow_graph::graph::ModuleGraph,
    path_to_file_id: &FxHashMap<PathBuf, FileId>,
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    canonical_project_root: &Path,
) {
    if let Some(pkg) = root_pkg {
        add_package_public_api_entry_points(
            public_api_entry_points,
            graph,
            path_to_file_id,
            &config.root,
            pkg,
            canonical_project_root,
        );
        add_exportless_package_source_indexes(public_api_entry_points, graph, &config.root, pkg);
    }
}

fn add_workspace_public_api_entry_points(
    public_api_entry_points: &mut FxHashSet<FileId>,
    graph: &fallow_graph::graph::ModuleGraph,
    path_to_file_id: &FxHashMap<PathBuf, FileId>,
    workspaces: &[WorkspaceInfo],
    canonical_project_root: &Path,
) {
    for workspace in workspaces {
        let Ok(pkg) = PackageJson::load(&workspace.root.join("package.json")) else {
            continue;
        };
        add_package_public_api_entry_points(
            public_api_entry_points,
            graph,
            path_to_file_id,
            &workspace.root,
            &pkg,
            canonical_project_root,
        );
        add_exportless_package_source_indexes(
            public_api_entry_points,
            graph,
            &workspace.root,
            &pkg,
        );
    }
}

fn add_package_public_api_entry_points(
    public_api_entry_points: &mut FxHashSet<FileId>,
    graph: &fallow_graph::graph::ModuleGraph,
    path_to_file_id: &FxHashMap<PathBuf, FileId>,
    package_root: &Path,
    package_json: &PackageJson,
    canonical_project_root: &Path,
) {
    if package_json.private.unwrap_or(false) {
        return;
    }

    for entry in package_json.entry_points() {
        let Some(entry_point) = resolve_public_api_entry_path(
            package_root,
            &entry,
            canonical_project_root,
            EntryPointSource::PackageJsonExports,
        ) else {
            continue;
        };

        if let Some(file_id) = path_to_file_id.get(&entry_point.path).copied().or_else(|| {
            resolve_entry_via_canonical(graph, path_to_file_id, package_root, &entry_point.path)
        }) {
            public_api_entry_points.insert(file_id);
        }
    }
}

fn resolve_public_api_entry_path(
    base: &Path,
    entry: &str,
    canonical_root: &Path,
    source: EntryPointSource,
) -> Option<EntryPoint> {
    if entry.contains('*') || entry_has_parent_dir(entry) {
        return None;
    }

    if let Some(source_path) = try_output_to_source_path(base, entry) {
        return validated_entry_point(&source_path, canonical_root, source);
    }

    if is_entry_in_output_dir(entry)
        && let Some(source_path) = try_source_index_fallback(base)
    {
        return validated_entry_point(&source_path, canonical_root, source);
    }

    resolve_entry_via_filesystem_probe(base, entry, canonical_root, source)
}

fn resolve_entry_via_filesystem_probe(
    base: &Path,
    entry: &str,
    canonical_root: &Path,
    source: EntryPointSource,
) -> Option<EntryPoint> {
    let resolved = base.join(entry);

    if resolved.is_file() {
        return validated_entry_point(&resolved, canonical_root, source);
    }

    for ext in SOURCE_EXTENSIONS {
        let with_ext = resolved.with_extension(ext);
        if with_ext.is_file() {
            return validated_entry_point(&with_ext, canonical_root, source);
        }
    }

    if let Some(index_entry) = try_directory_index_entry(&resolved) {
        return validated_entry_point(&index_entry, canonical_root, source);
    }

    if is_package_root_index_entry(entry)
        && let Some(source_path) = try_source_index_fallback(base)
    {
        return validated_entry_point(&source_path, canonical_root, source);
    }

    None
}

fn entry_has_parent_dir(entry: &str) -> bool {
    Path::new(entry)
        .components()
        .any(|component| matches!(component, Component::ParentDir))
}

fn validated_entry_point(
    candidate: &Path,
    canonical_root: &Path,
    source: EntryPointSource,
) -> Option<EntryPoint> {
    let canonical_candidate = dunce::canonicalize(candidate).ok()?;
    canonical_candidate
        .starts_with(canonical_root)
        .then(|| EntryPoint {
            path: candidate.to_path_buf(),
            source,
        })
}

fn try_directory_index_entry(resolved: &Path) -> Option<PathBuf> {
    for ext in SOURCE_EXTENSIONS {
        let candidate = resolved.join(format!("index.{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn is_package_root_index_entry(entry: &str) -> bool {
    let mut components = Path::new(entry)
        .components()
        .filter(|component| !matches!(component, Component::CurDir));

    let Some(Component::Normal(file_name)) = components.next() else {
        return false;
    };
    if components.next().is_some() {
        return false;
    }

    file_name
        .to_str()
        .is_some_and(|name| name == "index" || name.starts_with("index."))
}

fn try_output_to_source_path(base: &Path, entry: &str) -> Option<PathBuf> {
    let entry_path = Path::new(entry);
    let components: Vec<_> = entry_path.components().collect();

    let output_pos = components.iter().rposition(|component| {
        if let Component::Normal(name) = component
            && let Some(name) = name.to_str()
        {
            return OUTPUT_DIRS.contains(&name);
        }
        false
    })?;

    let prefix: PathBuf = components[..output_pos]
        .iter()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect();
    let suffix: PathBuf = components[output_pos + 1..].iter().collect();

    for ext in SOURCE_EXTENSIONS {
        let source_candidate = base
            .join(&prefix)
            .join("src")
            .join(suffix.with_extension(ext));
        if source_candidate.exists() {
            return Some(source_candidate);
        }
    }

    None
}

fn is_entry_in_output_dir(entry: &str) -> bool {
    Path::new(entry).components().any(|component| {
        if let Component::Normal(name) = component
            && let Some(name) = name.to_str()
        {
            return OUTPUT_DIRS.contains(&name);
        }
        false
    })
}

fn try_source_index_fallback(base: &Path) -> Option<PathBuf> {
    for ext in SOURCE_EXTENSIONS {
        let candidate = base.join("src").join(format!("index.{ext}"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn resolve_entry_via_canonical(
    graph: &fallow_graph::graph::ModuleGraph,
    path_to_file_id: &FxHashMap<PathBuf, FileId>,
    package_root: &Path,
    entry_path: &Path,
) -> Option<FileId> {
    dunce::canonicalize(entry_path).ok().and_then(|canonical| {
        path_to_file_id
            .get(&canonical)
            .copied()
            .or_else(|| resolve_entry_via_scoped_canonical(graph, package_root, &canonical))
    })
}

fn resolve_entry_via_scoped_canonical(
    graph: &fallow_graph::graph::ModuleGraph,
    package_root: &Path,
    canonical_entry: &Path,
) -> Option<FileId> {
    graph
        .modules
        .iter()
        .filter(|module| module.path.starts_with(package_root))
        .find_map(|module| {
            (dunce::canonicalize(&module.path).ok().as_deref() == Some(canonical_entry))
                .then_some(module.file_id)
        })
}

fn add_exportless_package_source_indexes(
    public_api_entry_points: &mut FxHashSet<FileId>,
    graph: &fallow_graph::graph::ModuleGraph,
    package_root: &Path,
    package_json: &PackageJson,
) {
    if package_json.private.unwrap_or(false) || package_json.exports.is_some() {
        return;
    }

    let mut roots = vec![package_root.to_path_buf()];
    if let Ok(canonical) = dunce::canonicalize(package_root) {
        roots.push(canonical);
    }

    for module in &graph.modules {
        if roots
            .iter()
            .any(|root| is_source_index_under_package(&module.path, root))
        {
            public_api_entry_points.insert(module.file_id);
        }
    }
}

fn is_source_index_under_package(path: &Path, package_root: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(package_root) else {
        return false;
    };

    if !matches!(
        relative.components().next(),
        Some(std::path::Component::Normal(segment)) if segment == "src"
    ) {
        return false;
    }

    path.file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem == "index")
}
