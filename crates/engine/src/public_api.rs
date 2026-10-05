//! Public API graph helpers owned by the engine boundary.

use std::path::{Component, Path, PathBuf};

use fallow_config::{
    PackageJson, ResolvedConfig, TsconfigOutputMap, TsconfigOutputResolution, WorkspaceInfo,
};
use fallow_types::discover::FileId;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_graph::resolve::OUTPUT_DIRS;

use crate::{
    discover::{EntryPoint, EntryPointSource, SOURCE_EXTENSIONS},
    module_graph::RetainedModuleGraph,
};

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
        &config.public_packages,
        &canonical_project_root,
    );

    public_api_entry_points
}

/// Compute public export keys for a retained project graph.
#[must_use]
pub fn public_export_keys_for_graph(
    graph: &RetainedModuleGraph,
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
    root: &Path,
) -> FxHashSet<String> {
    let root_pkg = fallow_config::load_dir_package_json(&config.root);
    let public_entries =
        public_api_package_entry_points(graph, config, root_pkg.as_ref(), workspaces);
    graph.public_export_keys(&public_entries, root)
}

/// Resolve exports-aware package entry points to their source paths for
/// semantic API-surface queries.
#[must_use]
pub fn public_api_entry_paths_for_graph(
    graph: &RetainedModuleGraph,
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
) -> Vec<PathBuf> {
    let root_pkg = fallow_config::load_dir_package_json(&config.root);
    let public_entries =
        public_api_package_entry_points(graph, config, root_pkg.as_ref(), workspaces);
    let mut paths = public_entries
        .into_iter()
        .filter_map(|file_id| {
            graph
                .as_graph()
                .modules
                .get(file_id.0 as usize)
                .map(|module| module.path.clone())
        })
        .collect::<Vec<_>>();
    paths.sort();
    paths.dedup();
    paths
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
    public_packages: &[String],
    canonical_project_root: &Path,
) {
    for workspace in workspaces
        .iter()
        .filter(|workspace| fallow_config::workspace_is_public(&workspace.name, public_packages))
    {
        let Some(pkg) = fallow_config::load_dir_package_json(&workspace.root) else {
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

    let output_map = TsconfigOutputMap::from_project(package_root);
    for entry in package_json.entry_points() {
        let Some(entry_point) = resolve_public_api_entry_path(
            package_root,
            &entry,
            canonical_project_root,
            EntryPointSource::PackageJsonExports,
            &output_map,
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
    output_map: &TsconfigOutputMap,
) -> Option<EntryPoint> {
    if entry.contains('*') || entry_has_parent_dir(entry) {
        return None;
    }

    match output_map.resolve_source_for_entry(entry, SOURCE_EXTENSIONS) {
        TsconfigOutputResolution::Resolved(source_path) => {
            return validated_entry_point(&source_path, canonical_root, source);
        }
        TsconfigOutputResolution::ConfiguredButUnresolved => {}
        TsconfigOutputResolution::Unconfigured => {
            if let Some(source_path) = try_legacy_output_to_source_path(base, entry) {
                return validated_entry_point(&source_path, canonical_root, source);
            }

            if is_entry_in_output_dir(entry)
                && let Some(source_path) = try_source_index_fallback(base)
            {
                return validated_entry_point(&source_path, canonical_root, source);
            }
        }
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

fn try_legacy_output_to_source_path(base: &Path, entry: &str) -> Option<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::AnalysisSession;

    fn fixture_root() -> PathBuf {
        std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
        )
        .join("../../tests/fixtures/public-package-members")
    }

    fn public_entry_paths(session: &AnalysisSession) -> Vec<PathBuf> {
        let artifacts = session
            .analyze_dead_code_with_artifacts(false, true)
            .expect("analysis succeeds");
        let graph = artifacts.graph.expect("retained graph");
        public_api_entry_paths_for_graph(&graph, session.config(), session.workspaces())
    }

    #[test]
    fn workspace_public_entries_require_public_packages_selection() {
        let root = fixture_root();
        let unselected = AnalysisSession::load_with_config(&root, None, |config| {
            config.public_packages.clear();
        })
        .expect("unselected session loads");

        assert!(public_entry_paths(&unselected).is_empty());

        let selected = AnalysisSession::load_with_config(&root, None, |config| {
            config.public_packages = vec!["@workspace/public-lib".to_string()];
        })
        .expect("selected session loads");
        let selected_paths = public_entry_paths(&selected);

        assert_eq!(selected_paths.len(), 1);
        assert!(selected_paths[0].ends_with("packages/public-lib/src/index.ts"));
    }

    #[test]
    fn package_output_entries_resolve_through_tsconfig_with_outputs_present_or_absent() {
        let directory = tempfile::tempdir().expect("temporary project directory");
        let root = directory.path();
        std::fs::create_dir_all(root.join("source")).expect("source directory");
        std::fs::create_dir_all(root.join("distribution")).expect("output directory");
        std::fs::write(
            root.join("package.json"),
            r#"{
                "name": "configured-output-package",
                "exports": {
                    ".": {
                        "types": "./distribution/index.d.ts",
                        "import": "./distribution/index.js"
                    }
                }
            }"#,
        )
        .expect("package manifest");
        std::fs::write(
            root.join("tsconfig.build.json"),
            r#"{
                "compilerOptions": {
                    "rootDir": "./source",
                    "outDir": "./distribution"
                },
                "include": ["source"]
            }"#,
        )
        .expect("TypeScript config");
        std::fs::write(
            root.join("source/index.ts"),
            "export const publicValue = 1;\n",
        )
        .expect("source entry");
        std::fs::write(
            root.join("source/internal.ts"),
            "export const internalValue = 1;\n",
        )
        .expect("unrelated source file");
        std::fs::write(
            root.join("distribution/index.js"),
            "export const publicValue = 1;\n",
        )
        .expect("generated JavaScript entry");
        std::fs::write(
            root.join("distribution/index.d.ts"),
            "export declare const publicValue: 1;\n",
        )
        .expect("generated declaration entry");

        let with_outputs = AnalysisSession::load_with_config(root, None, |_| {})
            .expect("project with outputs loads");
        let expected = root.join("source/index.ts");
        assert_eq!(
            public_entry_paths(&with_outputs),
            vec![expected.clone()],
            "configured output entries should resolve to their source entry, without exposing unrelated source files"
        );

        std::fs::remove_dir_all(root.join("distribution")).expect("remove generated outputs");
        let without_outputs = AnalysisSession::load_with_config(root, None, |_| {})
            .expect("project without outputs loads");
        assert_eq!(
            public_entry_paths(&without_outputs),
            vec![expected],
            "configured output entries should still resolve when build artifacts are absent"
        );
    }

    #[test]
    fn out_dir_without_root_dir_resolves_public_subpaths_and_bin() {
        let directory = tempfile::tempdir().expect("temporary project directory");
        let root = directory.path();
        std::fs::create_dir_all(root.join("src")).expect("source directory");
        std::fs::write(
            root.join("package.json"),
            r#"{
                "name": "out-dir-only-package",
                "main": "./dist/index.js",
                "bin": "./dist/cli.js",
                "exports": {
                    ".": "./dist/index.js",
                    "./browser": "./dist/browser.js"
                }
            }"#,
        )
        .expect("package manifest");
        std::fs::write(
            root.join("tsconfig.json"),
            r#"{"compilerOptions":{"outDir":"./dist"},"include":["src"]}"#,
        )
        .expect("TypeScript config");
        for name in ["index.ts", "browser.ts", "cli.ts", "internal.ts"] {
            std::fs::write(
                root.join("src").join(name),
                format!("export const {} = 1;\n", name.trim_end_matches(".ts")),
            )
            .expect("source module");
        }

        let session = AnalysisSession::load_with_config(root, None, |_| {})
            .expect("outDir-only package loads");
        let entries = public_entry_paths(&session);
        for entry in ["src/index.ts", "src/browser.ts", "src/cli.ts"] {
            assert!(
                entries.iter().any(|path| path.ends_with(entry)),
                "public output entry {entry} should map to its source, public entries: {entries:?}"
            );
        }
        assert!(
            !entries.iter().any(|path| path.ends_with("src/internal.ts")),
            "mapping public outputs must not expose unrelated source files, public entries: {entries:?}"
        );
    }

    #[test]
    fn declaration_dir_without_out_dir_resolves_package_types_entries() {
        for with_output in [true, false] {
            let directory = tempfile::tempdir().expect("temporary project directory");
            let root = directory.path();
            std::fs::create_dir_all(root.join("source")).expect("source directory");
            std::fs::write(
                root.join("package.json"),
                r#"{
                    "name":"declaration-only-package",
                    "types":"./types/index.d.ts",
                    "exports":{".":{"types":"./types/index.d.ts"}}
                }"#,
            )
            .expect("package manifest");
            std::fs::write(
                root.join("tsconfig.build.json"),
                r#"{
                    "compilerOptions": {
                        "rootDir":"./source",
                        "declarationDir":"./types"
                    }
                }"#,
            )
            .expect("TypeScript config");
            std::fs::write(
                root.join("source/index.ts"),
                "export const publicValue = 1;\n",
            )
            .expect("source entry");
            std::fs::write(
                root.join("source/internal.ts"),
                "export const internalValue = 1;\n",
            )
            .expect("unrelated source file");
            if with_output {
                std::fs::create_dir_all(root.join("types")).expect("types directory");
                std::fs::write(
                    root.join("types/index.d.ts"),
                    "export declare const publicValue: 1;\n",
                )
                .expect("generated declaration entry");
            }

            let session = AnalysisSession::load_with_config(root, None, |_| {})
                .expect("declaration-only project loads");
            assert_eq!(
                public_entry_paths(&session),
                vec![root.join("source/index.ts")],
                "declarationDir should resolve with output present={with_output}"
            );
        }
    }

    #[test]
    fn ambiguous_configured_output_does_not_use_legacy_public_entry_guess() {
        for with_output in [true, false] {
            let directory = tempfile::tempdir().expect("temporary project directory");
            let root = directory.path();
            for source_dir in ["source", "src"] {
                std::fs::create_dir_all(root.join(source_dir)).expect("source directory");
                std::fs::write(
                    root.join(source_dir).join("index.ts"),
                    "export const value = 1;\n",
                )
                .expect("source entry");
            }
            std::fs::write(
                root.join("package.json"),
                r#"{"name":"ambiguous-output-package","exports":{".":"./dist/index.js","./runtime":"./runtime.ts"}}"#,
            )
            .expect("package manifest");
            std::fs::write(root.join("runtime.ts"), "export const runtime = 1;\n")
                .expect("independent runtime export");
            for (config_name, source_dir) in [
                ("tsconfig.source.json", "source"),
                ("tsconfig.src.json", "src"),
            ] {
                std::fs::write(
                    root.join(config_name),
                    format!(
                        r#"{{"compilerOptions":{{"rootDir":"./{source_dir}","outDir":"./dist"}}}}"#
                    ),
                )
                .expect("TypeScript config");
            }
            if with_output {
                std::fs::create_dir_all(root.join("dist")).expect("output directory");
                std::fs::write(root.join("dist/index.js"), "export const value = 1;\n")
                    .expect("generated output");
            }

            let session = AnalysisSession::load_with_config(root, None, |_| {})
                .expect("ambiguous project loads");
            let entries = public_entry_paths(&session);
            assert!(
                !entries.iter().any(|path| path.ends_with("src/index.ts")),
                "an ambiguous configured map must not expose the legacy src guess with output present={with_output}, entries: {entries:?}"
            );
            assert!(
                !entries.iter().any(|path| path.ends_with("source/index.ts")),
                "an ambiguous configured map must not expose either source candidate, entries: {entries:?}"
            );
        }

        let legacy = tempfile::tempdir().expect("legacy project directory");
        std::fs::create_dir_all(legacy.path().join("src")).expect("legacy source directory");
        std::fs::write(
            legacy.path().join("package.json"),
            r#"{"name":"legacy-output-package","main":"./dist/index.js"}"#,
        )
        .expect("package manifest");
        std::fs::write(
            legacy.path().join("src/index.ts"),
            "export const value = 1;\n",
        )
        .expect("legacy source entry");
        let session = AnalysisSession::load_with_config(legacy.path(), None, |_| {})
            .expect("legacy project loads");
        assert!(
            public_entry_paths(&session)
                .iter()
                .any(|path| path.ends_with("src/index.ts")),
            "the legacy dist/src convention should still resolve without a matching config"
        );
    }
}
