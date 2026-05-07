use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::{ResolvedConfig, WorkspaceInfo};
use fallow_types::discover::{DiscoveredFile, FileId};
use oxc_span::Span;

use crate::extract::{ImportInfo, ImportedName, parse_from_content};
use crate::plugins::AggregatedPluginResult;
use crate::resolve::{
    ResolveResult, ResolvedImport, ResolvedModule, extract_package_name_from_node_modules_path,
    resolve_all_imports,
};

pub fn augment_external_style_package_usage(
    resolved_modules: &mut [ResolvedModule],
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
    plugin_result: &AggregatedPluginResult,
) {
    let mut scanner = ExternalStylePackageScanner::new(config, workspaces, plugin_result);

    for module in resolved_modules {
        let mut synthetic_packages = FxHashSet::default();
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

            synthetic_packages.extend(scanner.scan(path));
        }

        for package_name in synthetic_packages {
            if existing_packages.contains(package_name.as_str()) {
                continue;
            }
            module
                .resolved_imports
                .push(synthetic_package_import(package_name));
        }
    }
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
        let resolved = resolve_all_imports(
            &[module],
            &[file],
            self.workspaces,
            &self.plugin_result.active_plugins,
            &self.plugin_result.path_aliases,
            &self.plugin_result.scss_include_paths,
            &self.config.root,
            &self.config.resolve.conditions,
        );

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
                        if let Some(child) = resolve_root_relative_style_import(
                            &self.config.root,
                            &import.info.source,
                        ) {
                            if let Some(owner) = extract_package_name_from_node_modules_path(&child)
                            {
                                packages.insert(owner);
                            }
                            if is_trackable_external_style_path(&child) {
                                packages.extend(self.scan(&child));
                            }
                        }
                    }
                    ResolveResult::InternalModule(_) => {}
                }
            }
        }

        self.visiting.remove(&canonical);
        self.memo.insert(canonical.clone(), packages.clone());
        packages
    }
}

fn resolve_root_relative_style_import(root: &Path, specifier: &str) -> Option<PathBuf> {
    let relative = specifier.strip_prefix('/')?;
    let candidate = root.join(relative);
    if candidate.is_file() {
        return Some(dunce::canonicalize(&candidate).unwrap_or(candidate));
    }

    if candidate.extension().is_some() {
        return None;
    }

    for ext in ["css", "scss", "sass"] {
        let candidate = candidate.with_extension(ext);
        if candidate.is_file() {
            return Some(dunce::canonicalize(&candidate).unwrap_or(candidate));
        }
    }

    None
}
