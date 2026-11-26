//! The runtime entry points of an app that an end-to-end runner starts as
//! its web server move to the support role. The app exists for the tests, so
//! the files that only the app reaches are not production code. A file that
//! a production entry point also reaches stays runtime code (issue #5270).

use std::path::{Path, PathBuf};

use fallow_types::discover::{EntryPoint, EntryPointSource};

use fallow_config::{ManifestPatternMatcher, ResolvedConfig};

use super::CategorizedEntryPoints;
use crate::plugins::AggregatedPluginResult;

impl CategorizedEntryPoints {
    /// Move each runtime entry point below one of `app_dirs` to the support
    /// role. An entry stays runtime when the user lists it in `entry`, or
    /// when `is_declared_workspace` holds for a folder between the app
    /// folder and the entry, the app folder included: a workspace package is
    /// production code, also when the tests start it.
    fn move_web_server_app_entries_to_support(
        &mut self,
        app_dirs: &[PathBuf],
        is_declared_workspace: impl Fn(&Path) -> bool,
    ) {
        if app_dirs.is_empty() {
            return;
        }
        // An entry path can be canonical (a resolved symlink, such as
        // `/private/var` for `/var` on macOS) while a config path is not.
        let dirs: Vec<PathBuf> = app_dirs
            .iter()
            .flat_map(|dir| [dir.clone(), dunce::canonicalize(dir).unwrap_or_default()])
            .filter(|dir| !dir.as_os_str().is_empty())
            .collect();
        let is_app_entry = |entry: &EntryPoint| {
            !matches!(entry.source, EntryPointSource::ManualEntry)
                && dirs.iter().any(|dir| {
                    entry.path.starts_with(dir)
                        && !entry
                            .path
                            .ancestors()
                            .skip(1)
                            .take_while(|folder| folder.starts_with(dir))
                            .any(&is_declared_workspace)
                })
        };
        let (moved, kept) = std::mem::take(&mut self.runtime)
            .into_iter()
            .partition(is_app_entry);
        self.runtime = kept;
        self.support.extend(moved);
    }
}

/// Move the runtime entry points of each app that an end-to-end runner
/// starts as its web server to the support role. A folder that a workspace
/// pattern of the root manifest declares stays production code, also when
/// the tests start it (issue #5270).
pub fn move_web_server_app_entries(
    entry_points: &mut CategorizedEntryPoints,
    config: &ResolvedConfig,
    plugin_result: &AggregatedPluginResult,
) {
    if plugin_result.web_server_app_dirs.is_empty() {
        return;
    }
    let declared = ManifestPatternMatcher::load(&config.root);
    let canonical_root = dunce::canonicalize(&config.root).unwrap_or_else(|_| config.root.clone());
    let is_declared_workspace = |dir: &Path| {
        let root = if dir.starts_with(&config.root) {
            &config.root
        } else {
            &canonical_root
        };
        dir.starts_with(root) && declared.declares(root, dir)
    };
    entry_points.move_web_server_app_entries_to_support(
        &plugin_result.web_server_app_dirs,
        is_declared_workspace,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> EntryPoint {
        EntryPoint {
            path: PathBuf::from(path),
            source: EntryPointSource::DefaultIndex,
        }
    }

    fn paths(entries: &[EntryPoint]) -> Vec<&Path> {
        entries.iter().map(|entry| entry.path.as_path()).collect()
    }

    #[test]
    fn moves_only_runtime_entries_below_an_app_directory() {
        let mut entries = CategorizedEntryPoints::default();
        entries.extend_runtime([
            entry("/project/app/index.html"),
            entry("/project/app-shell/main.ts"),
            entry("/project/src/index.ts"),
        ]);
        entries.move_web_server_app_entries_to_support(&[PathBuf::from("/project/app")], |_| false);

        assert_eq!(
            paths(&entries.runtime),
            [
                Path::new("/project/app-shell/main.ts"),
                Path::new("/project/src/index.ts")
            ]
        );
        assert_eq!(
            paths(&entries.support),
            [Path::new("/project/app/index.html")]
        );
        assert_eq!(entries.all.len(), 3, "every entry point stays an entry");
    }

    #[test]
    fn keeps_manual_entries_and_declared_workspace_entries() {
        let mut entries = CategorizedEntryPoints::default();
        entries.extend_runtime([
            EntryPoint {
                path: PathBuf::from("/project/app/src/main.ts"),
                source: EntryPointSource::ManualEntry,
            },
            entry("/project/app/packages/web/src/index.ts"),
            entry("/project/app/index.html"),
        ]);
        entries.move_web_server_app_entries_to_support(&[PathBuf::from("/project/app")], |dir| {
            dir == Path::new("/project/app/packages/web")
        });

        assert_eq!(
            paths(&entries.runtime),
            [
                Path::new("/project/app/src/main.ts"),
                Path::new("/project/app/packages/web/src/index.ts")
            ]
        );
        assert_eq!(
            paths(&entries.support),
            [Path::new("/project/app/index.html")]
        );
    }
}
