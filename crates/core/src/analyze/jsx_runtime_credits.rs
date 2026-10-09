//! Dependency credit for the JSX runtime that a config uses by default.
//!
//! When a Vite or Vitest config sets no JSX import source, the automatic
//! transform imports `react/jsx-dev-runtime` into each file with JSX. No
//! import statement shows that use, so `react` would look unused. The plugin
//! records the default runtime as a credit rule with the files that the
//! config transforms. This pass credits the package only when one of those
//! files has JSX and no runtime pragma, so a project without JSX test files
//! still gets the finding.
//!
//! The credit goes to one manifest only, as for an import: the nearest
//! manifest in the chain of the file (its workspace, the ancestor workspaces,
//! then the root) that declares the package. Thus a JSX test file in one
//! workspace does not hide an unused `react` in a different workspace.

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::{PackageJson, WorkspaceInfo};

use crate::extract::ModuleInfo;
use crate::graph::ModuleGraph;
use crate::plugins::AggregatedPluginResult;

/// Extensions of the files that Vite transforms with the tsconfig JSX
/// settings.
const TYPESCRIPT_EXTENSIONS: &[&str] = &["ts", "tsx", "mts", "cts"];

const PACKAGE_JSON: &str = "package.json";

/// The `(package.json path, package)` credits of the default-runtime rules
/// that match at least one module with `jsx_runtime_from_config` set. The
/// result is sorted.
pub(super) fn collect_jsx_runtime_package_credits(
    plugin_result: Option<&AggregatedPluginResult>,
    modules: &[ModuleInfo],
    graph: &ModuleGraph,
    root: &Path,
    workspaces: &[WorkspaceInfo],
) -> Vec<(PathBuf, String)> {
    let Some(plugin_result) = plugin_result else {
        return Vec::new();
    };
    if plugin_result.jsx_package_credits.is_empty() {
        return Vec::new();
    }
    let rules = fallow_graph::resolve::compile_jsx_rules(&plugin_result.jsx_package_credits);
    let mut manifests = ManifestChain::new(root, workspaces);
    let mut credited: FxHashSet<(PathBuf, String)> = FxHashSet::default();
    for module in modules
        .iter()
        .filter(|module| module.jsx_runtime_from_config)
    {
        let Some(node) = graph.modules.get(module.file_id.0 as usize) else {
            continue;
        };
        // A tsconfig JSX setting replaces the default runtime for the
        // TypeScript files below that tsconfig.
        if is_typescript(&node.path)
            && plugin_result
                .tsconfig_jsx_dirs
                .iter()
                .any(|dir| node.path.starts_with(dir))
        {
            continue;
        }
        for rule in &rules {
            if !rule.matches(&node.path) {
                continue;
            }
            let package = rule.rule().source.as_str();
            let manifest = manifests.credited_manifest(&node.path, package);
            credited.insert((manifest, package.to_string()));
        }
    }
    let mut credits: Vec<(PathBuf, String)> = credited.into_iter().collect();
    credits.sort_unstable();
    credits
}

/// The manifests that can own an import of a file, with the declared
/// dependency names of each manifest read once.
struct ManifestChain<'a> {
    root: &'a Path,
    workspaces: &'a [WorkspaceInfo],
    declared: FxHashMap<PathBuf, FxHashSet<String>>,
}

impl<'a> ManifestChain<'a> {
    fn new(root: &'a Path, workspaces: &'a [WorkspaceInfo]) -> Self {
        Self {
            root,
            workspaces,
            declared: FxHashMap::default(),
        }
    }

    /// The nearest manifest of `path` that declares `package`. When no
    /// manifest of the chain declares it, the nearest manifest of the chain.
    fn credited_manifest(&mut self, path: &Path, package: &str) -> PathBuf {
        let mut chain: Vec<&Path> = self
            .workspaces
            .iter()
            .map(|workspace| workspace.root.as_path())
            .filter(|workspace_root| path.starts_with(workspace_root))
            .collect();
        // The nearest declaration wins, so deeper workspaces come first.
        chain.sort_by_key(|workspace_root| std::cmp::Reverse(workspace_root.components().count()));
        chain.push(self.root);
        let manifests: Vec<PathBuf> = chain.iter().map(|dir| dir.join(PACKAGE_JSON)).collect();
        let declaring = manifests
            .iter()
            .position(|manifest| self.declares(manifest, package))
            .unwrap_or(0);
        manifests[declaring].clone()
    }

    fn declares(&mut self, manifest: &Path, package: &str) -> bool {
        self.declared
            .entry(manifest.to_path_buf())
            .or_insert_with(|| {
                PackageJson::load(manifest)
                    .map(|pkg| pkg.all_dependency_names().into_iter().collect())
                    .unwrap_or_default()
            })
            .contains(package)
    }
}

fn is_typescript(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| TYPESCRIPT_EXTENSIONS.contains(&extension))
}
