//! Danger JS plugin.
//!
//! Danger evaluates a project Dangerfile from CI instead of importing it from
//! application code, so active Danger projects need those files treated as used.
//!
//! Danger is frequently run straight from CI (`npx danger ci`, an Earthfile, a
//! GitHub Action) without `danger` ever being declared in package.json, so the
//! dependency enabler alone is not enough. The plugin also activates when a
//! `dangerfile.{js,ts,mjs,cjs}` is discovered at the repo root or in a
//! workspace, mirroring the convention-only activation used by `k6`.

use std::path::{Path, PathBuf};

use super::Plugin;

const ENABLERS: &[&str] = &["danger"];

const ALWAYS_USED: &[&str] = &[
    "dangerfile.{js,ts,mjs,cjs}",
    "**/dangerfile.{js,ts,mjs,cjs}",
];

const TOOLING_DEPENDENCIES: &[&str] = &["danger"];

/// Exact dangerfile filenames Danger looks for by default. Kept symmetric with
/// `ALWAYS_USED` so any file that activates the plugin is also credited.
const DANGERFILE_NAMES: &[&str] = &[
    "dangerfile.js",
    "dangerfile.ts",
    "dangerfile.mjs",
    "dangerfile.cjs",
];

pub struct DangerPlugin;

impl Plugin for DangerPlugin {
    fn name(&self) -> &'static str {
        "danger"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn is_enabled_with_files(
        &self,
        deps: &[String],
        root: &Path,
        discovered_files: &[PathBuf],
    ) -> bool {
        self.is_enabled_with_deps(deps, root)
            || discovered_files.iter().any(|path| is_dangerfile_path(path))
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }
}

fn is_dangerfile_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| DANGERFILE_NAMES.contains(&name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_config::EntryPointRole;

    #[test]
    fn always_used_includes_dangerfile_variants() {
        let plugin = DangerPlugin;
        assert!(plugin.always_used().contains(&"dangerfile.{js,ts,mjs,cjs}"));
        assert!(
            plugin
                .always_used()
                .contains(&"**/dangerfile.{js,ts,mjs,cjs}")
        );
    }

    #[test]
    fn tooling_dependencies_include_danger() {
        let plugin = DangerPlugin;
        assert!(plugin.tooling_dependencies().contains(&"danger"));
    }

    #[test]
    fn activates_from_danger_dependency() {
        let plugin = DangerPlugin;
        let deps = vec!["danger".to_string()];

        assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
        assert!(plugin.is_enabled_with_files(&deps, Path::new("/project"), &[]));
    }

    #[test]
    fn activates_from_discovered_root_dangerfile() {
        let plugin = DangerPlugin;
        let files = vec![PathBuf::from("/project/dangerfile.js")];

        assert!(plugin.is_enabled_with_files(&[], Path::new("/project"), &files));
    }

    #[test]
    fn activates_from_discovered_workspace_dangerfile() {
        let plugin = DangerPlugin;
        let files = vec![PathBuf::from("/project/packages/foo/dangerfile.ts")];

        assert!(plugin.is_enabled_with_files(&[], Path::new("/project"), &files));
    }

    #[test]
    fn does_not_activate_from_similar_filenames() {
        let plugin = DangerPlugin;
        let files = vec![
            PathBuf::from("/project/predangerfile.js"),
            PathBuf::from("/project/dangerfile.json"),
            PathBuf::from("/project/dangerfiles.js"),
            PathBuf::from("/project/danger.js"),
        ];

        assert!(!plugin.is_enabled_with_files(&[], Path::new("/project"), &files));
    }

    #[test]
    fn entry_point_role_is_support() {
        let plugin = DangerPlugin;
        assert_eq!(plugin.entry_point_role(), EntryPointRole::Support);
    }
}
