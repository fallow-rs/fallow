use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::{PackageJson, ResolvedConfig, WorkspaceInfo};
use fallow_types::discover::{DiscoveredFile, FileId};
use oxc_span::Span;

use crate::extract::{ImportInfo, ImportedName, parse_from_content};
use crate::plugins::AggregatedPluginResult;
use crate::resolve::{
    ResolveAllImportsInput, ResolveResult, ResolvedImport, ResolvedModule,
    extract_package_name_from_node_modules_path, resolve_all_imports,
};

pub fn augment_external_style_package_usage(
    resolved_modules: &mut [ResolvedModule],
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
    plugin_result: &AggregatedPluginResult,
) {
    let mut scanner = ExternalStylePackageScanner::new(config, workspaces, plugin_result);
    let dependency_scopes = collect_declared_dependency_scopes(config, workspaces);

    for module in resolved_modules {
        let mut synthetic_packages = FxHashSet::default();
        let declared_packages = declared_packages_for_module(&module.path, &dependency_scopes);
        let existing_packages: FxHashSet<String> = module
            .all_resolved_imports()
            .filter_map(|import| match &import.target {
                ResolveResult::NpmPackage(name) => Some(name.clone()),
                _ => None,
            })
            .collect();

        for import in module.all_resolved_imports() {
            let ResolveResult::ExternalFile(path) = &import.target else {
                continue;
            };
            if !is_trackable_external_style_path(path) {
                continue;
            }
            if is_storybook_static_dir_external_style(
                &module.path,
                path,
                &plugin_result.static_dir_mappings,
            ) {
                continue;
            }

            synthetic_packages.extend(scanner.scan(path));
        }

        for package_name in synthetic_packages {
            if existing_packages.contains(package_name.as_str()) {
                continue;
            }
            let is_declared =
                declared_packages.is_some_and(|packages| packages.contains(package_name.as_str()));
            if !is_declared {
                continue;
            }
            module
                .resolved_imports
                .push(synthetic_package_import(package_name));
        }
    }
}

struct DeclaredDependencyScope {
    root: PathBuf,
    package_names: FxHashSet<String>,
}

fn collect_declared_dependency_scopes(
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
) -> Vec<DeclaredDependencyScope> {
    let mut scopes = Vec::new();
    if let Some(scope) = declared_dependency_scope(&config.root) {
        scopes.push(scope);
    }
    for workspace in workspaces {
        if let Some(scope) = declared_dependency_scope(&workspace.root) {
            scopes.push(scope);
        }
    }
    scopes.sort_by(|left, right| {
        right
            .root
            .components()
            .count()
            .cmp(&left.root.components().count())
    });
    scopes
}

fn declared_dependency_scope(root: &Path) -> Option<DeclaredDependencyScope> {
    let package_json = PackageJson::load(&root.join("package.json")).ok()?;
    let mut package_names: FxHashSet<String> =
        package_json.all_dependency_names().into_iter().collect();
    if let Some(name) = package_json.name {
        package_names.insert(name);
    }

    Some(DeclaredDependencyScope {
        root: root.to_path_buf(),
        package_names,
    })
}

fn declared_packages_for_module<'a>(
    module_path: &Path,
    scopes: &'a [DeclaredDependencyScope],
) -> Option<&'a FxHashSet<String>> {
    scopes
        .iter()
        .find(|scope| module_path.starts_with(&scope.root))
        .map(|scope| &scope.package_names)
}

fn synthetic_package_import(package_name: String) -> ResolvedImport {
    ResolvedImport {
        info: ImportInfo {
            source: package_name.clone(),
            imported_name: ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            from_style: false,
            span: Span::default(),
            source_span: Span::default(),
        },
        target: ResolveResult::NpmPackage(package_name),
    }
}

fn is_trackable_external_style_path(path: &Path) -> bool {
    extract_package_name_from_node_modules_path(path).is_some()
        && path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| matches!(ext, "css" | "scss" | "sass"))
}

fn is_storybook_preview_html(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some("preview-head.html" | "preview-body.html")
    ) && path
        .parent()
        .and_then(Path::file_name)
        .and_then(|name| name.to_str())
        == Some(".storybook")
}

fn is_storybook_static_dir_external_style(
    module_path: &Path,
    external_path: &Path,
    static_dir_mappings: &[(PathBuf, String)],
) -> bool {
    if static_dir_mappings.is_empty() || !is_storybook_preview_html(module_path) {
        return false;
    }
    let external =
        dunce::canonicalize(external_path).unwrap_or_else(|_| external_path.to_path_buf());
    static_dir_mappings.iter().any(|(from_dir, _)| {
        let from_dir = dunce::canonicalize(from_dir).unwrap_or_else(|_| from_dir.clone());
        external.starts_with(from_dir)
    })
}

struct ExternalStylePackageScanner<'a> {
    config: &'a ResolvedConfig,
    workspaces: &'a [WorkspaceInfo],
    plugin_result: &'a AggregatedPluginResult,
    memo: FxHashMap<PathBuf, FxHashSet<String>>,
    visiting: FxHashSet<PathBuf>,
}

impl<'a> ExternalStylePackageScanner<'a> {
    fn new(
        config: &'a ResolvedConfig,
        workspaces: &'a [WorkspaceInfo],
        plugin_result: &'a AggregatedPluginResult,
    ) -> Self {
        Self {
            config,
            workspaces,
            plugin_result,
            memo: FxHashMap::default(),
            visiting: FxHashSet::default(),
        }
    }

    fn scan(&mut self, path: &Path) -> FxHashSet<String> {
        let canonical = dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        if let Some(cached) = self.memo.get(&canonical) {
            return cached.clone();
        }
        if !self.visiting.insert(canonical.clone()) {
            return FxHashSet::default();
        }

        let mut packages = FxHashSet::default();
        if let Some(owner) = extract_package_name_from_node_modules_path(&canonical) {
            packages.insert(owner);
        }

        if !is_trackable_external_style_path(&canonical) {
            self.visiting.remove(&canonical);
            self.memo.insert(canonical.clone(), packages.clone());
            return packages;
        }

        let Ok(source) = std::fs::read_to_string(&canonical) else {
            self.visiting.remove(&canonical);
            self.memo.insert(canonical.clone(), packages.clone());
            return packages;
        };

        let file = DiscoveredFile {
            id: FileId(0),
            path: canonical.clone(),
            size_bytes: source.len() as u64,
        };
        let module = parse_from_content(FileId(0), &canonical, &source);
        let resolved = resolve_all_imports(&ResolveAllImportsInput {
            modules: &[module],
            files: &[file],
            workspaces: self.workspaces,
            active_plugins: &self.plugin_result.active_plugins,
            path_aliases: &self.plugin_result.path_aliases,
            auto_imports: &[],
            scss_include_paths: &self.plugin_result.scss_include_paths,
            static_dir_mappings: &self.plugin_result.static_dir_mappings,
            root: &self.config.root,
            extra_conditions: &self.config.resolve.conditions,
        });

        if let Some(resolved_module) = resolved.first() {
            for import in resolved_module.all_resolved_imports() {
                match &import.target {
                    ResolveResult::NpmPackage(name) => {
                        packages.insert(name.clone());
                    }
                    ResolveResult::ExternalFile(child) => {
                        if let Some(owner) = extract_package_name_from_node_modules_path(child) {
                            packages.insert(owner);
                        }
                        if is_trackable_external_style_path(child) {
                            packages.extend(self.scan(child));
                        }
                    }
                    ResolveResult::Unresolvable(_) => {
                        let child =
                            resolve_external_relative_style_import(&canonical, &import.info.source)
                                .or_else(|| {
                                    resolve_root_relative_style_import(
                                        &self.config.root,
                                        &import.info.source,
                                    )
                                });

                        if let Some(child) = child {
                            if let Some(owner) = extract_package_name_from_node_modules_path(&child)
                            {
                                packages.insert(owner);
                            }
                            if is_trackable_external_style_path(&child) {
                                packages.extend(self.scan(&child));
                            }
                        }
                    }
                    ResolveResult::InternalModule(_)
                    | ResolveResult::InternalPackageModule { .. } => {}
                }
            }
        }

        self.visiting.remove(&canonical);
        self.memo.insert(canonical.clone(), packages.clone());
        packages
    }
}

fn resolve_external_relative_style_import(from_file: &Path, specifier: &str) -> Option<PathBuf> {
    if !specifier.starts_with('.') {
        return None;
    }

    let candidate = from_file.parent()?.join(specifier);
    resolve_sass_style_candidate(&candidate)
}

fn resolve_root_relative_style_import(root: &Path, specifier: &str) -> Option<PathBuf> {
    let relative = specifier.strip_prefix('/')?;
    let candidate = root.join(relative);
    resolve_sass_style_candidate(&candidate)
}

fn resolve_sass_style_candidate(candidate: &Path) -> Option<PathBuf> {
    if candidate.extension().is_some() {
        return canonical_file(candidate).or_else(|| resolve_sass_partial_candidate(candidate));
    }

    for ext in ["css", "scss", "sass"] {
        let candidate = candidate.with_extension(ext);
        if let Some(path) = canonical_file(&candidate) {
            return Some(path);
        }
    }

    if let Some(path) = resolve_sass_partial_candidate(candidate) {
        return Some(path);
    }

    for ext in ["scss", "sass", "css"] {
        for index_name in ["_index", "index"] {
            let candidate = candidate.join(index_name).with_extension(ext);
            if let Some(path) = canonical_file(&candidate) {
                return Some(path);
            }
        }
    }

    canonical_file(candidate)
}

fn resolve_sass_partial_candidate(candidate: &Path) -> Option<PathBuf> {
    let file_name = candidate.file_name()?.to_str()?;
    if file_name.starts_with('_') {
        return None;
    }

    let partial = candidate.with_file_name(format!("_{file_name}"));
    if let Some(path) = canonical_file(&partial) {
        return Some(path);
    }

    if partial.extension().is_some() {
        return None;
    }

    for ext in ["scss", "sass", "css"] {
        let partial = partial.with_extension(ext);
        if let Some(path) = canonical_file(&partial) {
            return Some(path);
        }
    }

    None
}

fn canonical_file(candidate: &Path) -> Option<PathBuf> {
    if candidate.is_file() {
        return Some(dunce::canonicalize(candidate).unwrap_or_else(|_| candidate.to_path_buf()));
    }

    None
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use super::resolve_external_relative_style_import;

    #[test]
    fn external_relative_style_import_resolves_partial() {
        let dir = tempdir().expect("temp dir");
        let entry = dir.path().join("node_modules/@pkg/theme/_index.scss");
        let partial = dir.path().join("node_modules/@pkg/theme/core/_tokens.scss");
        fs::create_dir_all(partial.parent().expect("partial parent")).expect("create dirs");
        fs::write(&entry, "").expect("write entry");
        fs::write(&partial, "").expect("write partial");

        let resolved = resolve_external_relative_style_import(&entry, "./core/tokens")
            .expect("partial should resolve");

        assert_eq!(
            resolved,
            dunce::canonicalize(partial).expect("canonical partial")
        );
    }

    #[test]
    fn external_relative_style_import_resolves_index() {
        let dir = tempdir().expect("temp dir");
        let entry = dir.path().join("node_modules/@pkg/theme/_index.scss");
        let index = dir.path().join("node_modules/@pkg/theme/core/_index.scss");
        fs::create_dir_all(index.parent().expect("index parent")).expect("create dirs");
        fs::write(&entry, "").expect("write entry");
        fs::write(&index, "").expect("write index");

        let resolved =
            resolve_external_relative_style_import(&entry, "./core").expect("index should resolve");

        assert_eq!(
            resolved,
            dunce::canonicalize(index).expect("canonical index")
        );
    }

    #[test]
    fn external_relative_style_import_skips_non_relative_specifier() {
        let dir = tempdir().expect("temp dir");
        let entry = dir.path().join("node_modules/@pkg/theme/_index.scss");
        fs::create_dir_all(entry.parent().expect("entry parent")).expect("create dirs");
        fs::write(&entry, "").expect("write entry");

        assert!(resolve_external_relative_style_import(&entry, "@angular/material").is_none());
    }
}
