//! Dependency credit for the package of the automatic JSX runtime.
//!
//! The automatic JSX transform imports `<source>/jsx-runtime` (or
//! `jsx-dev-runtime`) into each file with JSX. No import statement shows
//! that use, so the runtime package would look unused. This pass credits the
//! package when a file with JSX and no runtime pragma uses the automatic
//! runtime, so a project without JSX files still gets the finding. These
//! settings select the automatic runtime with the `react` source:
//!
//! - a Vite or Vitest config without a JSX import source, for the files that
//!   the config transforms (a credit rule of the plugin);
//! - a tsconfig `jsx` mode of `react-jsx` or `react-jsxdev` without
//!   `jsxImportSource`, for the files below that tsconfig;
//! - a framework that compiles JSX with the automatic runtime, such as
//!   Next.js, for the files of the package where the framework is active.
//!
//! A tsconfig `jsxImportSource` credits its package in the TypeScript plugin.
//!
//! The credit goes to one manifest only, as for an import: the nearest
//! manifest in the chain of the file (its workspace, the ancestor workspaces,
//! then the root) that declares the package. Thus a JSX file in one
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

/// The package that the automatic JSX runtime imports when no setting names
/// an import source.
const DEFAULT_JSX_IMPORT_SOURCE: &str = "react";

/// The built-in plugins of frameworks that compile JSX with the automatic
/// runtime by default.
const AUTOMATIC_JSX_FRAMEWORK_PLUGINS: &[&str] = &["nextjs"];

/// The `(package.json path, package)` credits of the automatic JSX runtime
/// for the modules with `jsx_runtime_from_config` set. The result is sorted.
pub(super) fn collect_jsx_runtime_package_credits<'a>(
    plugin_result: Option<&AggregatedPluginResult>,
    modules: impl IntoIterator<Item = &'a ModuleInfo>,
    graph: &ModuleGraph,
    root: &Path,
    workspaces: &[WorkspaceInfo],
) -> Vec<(PathBuf, String)> {
    let Some(plugin_result) = plugin_result else {
        return Vec::new();
    };
    let framework_dirs = automatic_jsx_framework_dirs(plugin_result, root);
    if plugin_result.jsx_package_credits.is_empty()
        && plugin_result.tsconfig_jsx_dirs.is_empty()
        && framework_dirs.is_empty()
    {
        return Vec::new();
    }
    let rules = fallow_graph::resolve::compile_jsx_rules(&plugin_result.jsx_package_credits);
    let mut manifests = ManifestChain::new(root, workspaces);
    let mut credited: FxHashSet<(PathBuf, String)> = FxHashSet::default();
    for module in modules
        .into_iter()
        .filter(|module| module.jsx_runtime_from_config)
    {
        let Some(node) = graph.modules.get(module.file_id.0 as usize) else {
            continue;
        };
        let path = node.path.as_path();
        let mut packages: Vec<&str> = Vec::new();
        if uses_default_runtime(plugin_result, &framework_dirs, path) {
            packages.push(DEFAULT_JSX_IMPORT_SOURCE);
        }
        // A tsconfig JSX setting replaces the default runtime of a bundler
        // config for the TypeScript files below that tsconfig.
        if !(is_typescript(path) && nearest_tsconfig_depth(plugin_result, path, false).is_some()) {
            packages.extend(
                rules
                    .iter()
                    .filter(|rule| rule.matches(path))
                    .map(|rule| rule.rule().source.as_str()),
            );
        }
        for package in packages {
            let manifest = manifests.credited_manifest(path, package);
            credited.insert((manifest, package.to_string()));
        }
    }
    let mut credits: Vec<(PathBuf, String)> = credited.into_iter().collect();
    credits.sort_unstable();
    credits
}

/// The absolute roots of the packages where a framework with the automatic
/// JSX runtime is active.
fn automatic_jsx_framework_dirs(
    plugin_result: &AggregatedPluginResult,
    root: &Path,
) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = plugin_result
        .active_plugin_scopes
        .iter()
        .filter(|(plugin, _)| AUTOMATIC_JSX_FRAMEWORK_PLUGINS.contains(&plugin.as_str()))
        .map(|(_, prefix)| {
            if prefix.is_empty() {
                root.to_path_buf()
            } else {
                root.join(prefix)
            }
        })
        .collect();
    dirs.sort_unstable();
    dirs.dedup();
    dirs
}

/// Whether a framework or a tsconfig selects the automatic runtime with the
/// default `react` source for the file at `path`.
///
/// For a TypeScript file, the nearest tsconfig with a JSX setting decides: a
/// deeper tsconfig with `jsxImportSource` or a mode without the automatic
/// runtime replaces an automatic mode of an ancestor tsconfig.
fn uses_default_runtime(
    plugin_result: &AggregatedPluginResult,
    framework_dirs: &[PathBuf],
    path: &Path,
) -> bool {
    if framework_dirs.iter().any(|dir| path.starts_with(dir)) {
        return true;
    }
    let Some(automatic_depth) = nearest_tsconfig_depth(plugin_result, path, true) else {
        return false;
    };
    !is_typescript(path)
        || nearest_tsconfig_depth(plugin_result, path, false)
            .is_none_or(|depth| depth < automatic_depth)
}

/// The path depth of the deepest tsconfig directory that contains `path` and
/// selects the `react` automatic runtime (`automatic_react`) or not.
fn nearest_tsconfig_depth(
    plugin_result: &AggregatedPluginResult,
    path: &Path,
    automatic_react: bool,
) -> Option<usize> {
    plugin_result
        .tsconfig_jsx_dirs
        .iter()
        .filter(|entry| entry.automatic_react == automatic_react && path.starts_with(&entry.dir))
        .map(|entry| entry.dir.components().count())
        .max()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::registry::TsconfigJsxDir;

    fn plugin_result(automatic: &[&str], other: &[&str]) -> AggregatedPluginResult {
        let entries = |dirs: &[&str], automatic_react: bool| -> Vec<TsconfigJsxDir> {
            dirs.iter()
                .map(|dir| TsconfigJsxDir {
                    dir: PathBuf::from(dir),
                    automatic_react,
                })
                .collect()
        };
        let mut tsconfig_jsx_dirs = entries(automatic, true);
        tsconfig_jsx_dirs.extend(entries(other, false));
        AggregatedPluginResult {
            tsconfig_jsx_dirs,
            ..AggregatedPluginResult::default()
        }
    }

    #[test]
    fn automatic_tsconfig_covers_the_files_below_it() {
        let result = plugin_result(&["/p"], &[]);
        assert!(uses_default_runtime(
            &result,
            &[],
            Path::new("/p/src/a.tsx")
        ));
        assert!(uses_default_runtime(
            &result,
            &[],
            Path::new("/p/src/a.jsx")
        ));
        assert!(!uses_default_runtime(&result, &[], Path::new("/q/a.tsx")));
    }

    #[test]
    fn deeper_tsconfig_with_other_jsx_setting_wins_for_typescript() {
        let result = plugin_result(&["/p"], &["/p/packages/x"]);
        assert!(!uses_default_runtime(
            &result,
            &[],
            Path::new("/p/packages/x/a.tsx")
        ));
        assert!(uses_default_runtime(
            &result,
            &[],
            Path::new("/p/packages/x/a.jsx")
        ));
    }

    #[test]
    fn deeper_automatic_tsconfig_wins_over_an_ancestor_setting() {
        let result = plugin_result(&["/p/packages/x"], &["/p"]);
        assert!(uses_default_runtime(
            &result,
            &[],
            Path::new("/p/packages/x/a.tsx")
        ));
        assert!(!uses_default_runtime(&result, &[], Path::new("/p/a.tsx")));
    }

    #[test]
    fn framework_scope_covers_its_package_only() {
        let result = AggregatedPluginResult {
            active_plugin_scopes: vec![
                ("nextjs".to_string(), "apps/web".to_string()),
                ("vitest".to_string(), String::new()),
            ],
            ..AggregatedPluginResult::default()
        };
        let dirs = automatic_jsx_framework_dirs(&result, Path::new("/p"));
        assert_eq!(dirs, vec![PathBuf::from("/p/apps/web")]);
        assert!(uses_default_runtime(
            &result,
            &dirs,
            Path::new("/p/apps/web/app/page.tsx")
        ));
        assert!(!uses_default_runtime(
            &result,
            &dirs,
            Path::new("/p/packages/lib/a.tsx")
        ));
    }
}
