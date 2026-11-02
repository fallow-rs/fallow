//! pnpm package manager plugin.
//!
//! Marks pnpm workspace and configuration files as always used so they are
//! not flagged as unused. pnpm loads `.pnpmfile.mjs` or `.pnpmfile.cjs` next
//! to the lockfile, and the files that the `pnpmfile` setting names in
//! `pnpm-workspace.yaml` or `.npmrc`. No import reaches these hook files.
//!
//! Activates when `pnpm-workspace.yaml`, `pnpm-lock.yaml`, or a pnpmfile
//! exists, or when `packageManager` names pnpm, since pnpm is rarely a
//! package.json dependency.

use std::ffi::OsStr;
use std::path::Path;

use fallow_config::PackageJson;
use fallow_config::yaml::{self, YamlNode};

use super::{Plugin, PluginResult, config_parser};

const ALWAYS_USED: &[&str] = &[
    "pnpm-workspace.yaml",
    "pnpm-lock.yaml",
    ".pnpmfile.cjs",
    ".pnpmfile.mjs",
    ".npmrc",
];

const ACTIVATION_FILES: &[&str] = &[
    "pnpm-workspace.yaml",
    "pnpm-lock.yaml",
    ".pnpmfile.cjs",
    ".pnpmfile.mjs",
];

const WORKSPACE_FILE: &str = "pnpm-workspace.yaml";
const NPMRC_FILE: &str = ".npmrc";

const CONFIG_PATTERNS: &[&str] = &[WORKSPACE_FILE, NPMRC_FILE];

/// The setting that moves the pnpmfile away from its default path.
const PNPMFILE_SETTING: &str = "pnpmfile";

const TOOLING_DEPENDENCIES: &[&str] = &["pnpm"];

pub struct PnpmPlugin;

impl Plugin for PnpmPlugin {
    fn name(&self) -> &'static str {
        "pnpm"
    }

    fn enablers(&self) -> &'static [&'static str] {
        &["pnpm"]
    }

    // The registry calls this method and not `is_enabled`, so the file probe
    // must live here.
    fn is_enabled_with_deps(&self, deps: &[String], root: &Path) -> bool {
        deps.iter().any(|d| d == "pnpm")
            || ACTIVATION_FILES
                .iter()
                .any(|file| root.join(file).is_file())
    }

    fn is_enabled_with_package_json(&self, pkg: &PackageJson, _root: &Path) -> bool {
        pkg.package_manager
            .as_deref()
            .is_some_and(|manager| manager.starts_with("pnpm@"))
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        ALWAYS_USED
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let pnpmfiles = match config_path.file_name().and_then(OsStr::to_str) {
            Some(WORKSPACE_FILE) => workspace_pnpmfiles(source),
            Some(NPMRC_FILE) => npmrc_pnpmfiles(source),
            _ => Vec::new(),
        };
        let mut result = PluginResult::default();
        result
            .always_used_files
            .extend(pnpmfiles.iter().filter_map(|raw| {
                config_parser::normalize_filesystem_config_path(raw, config_path, root)
            }));
        result
    }
}

/// The `pnpmfile` setting of `pnpm-workspace.yaml`: one path or a list.
fn workspace_pnpmfiles(source: &str) -> Vec<String> {
    let Ok(document) = yaml::parse(source) else {
        return Vec::new();
    };
    let Some(value) = document.root().get(PNPMFILE_SETTING) else {
        return Vec::new();
    };
    match value.as_sequence() {
        Some(items) => items
            .filter_map(YamlNode::as_str)
            .map(str::to_string)
            .collect(),
        None => value.as_str().map(str::to_string).into_iter().collect(),
    }
}

/// The `pnpmfile=<path>` lines of an `.npmrc` file.
fn npmrc_pnpmfiles(source: &str) -> Vec<String> {
    source
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|inner| inner.strip_suffix('"'))
                .unwrap_or(value);
            (key.trim() == PNPMFILE_SETTING).then(|| value.to_string())
        })
        .filter(|value| !value.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn is_enabled_with_pnpm_dep() {
        let plugin = PnpmPlugin;
        let deps = vec!["pnpm".to_string()];
        assert!(plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn is_not_enabled_without_pnpm_dep() {
        let plugin = PnpmPlugin;
        let deps = vec!["npm".to_string()];
        assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn is_not_enabled_with_empty_deps() {
        let plugin = PnpmPlugin;
        let deps: Vec<String> = vec![];
        assert!(!plugin.is_enabled_with_deps(&deps, Path::new("/project")));
    }

    #[test]
    fn is_enabled_when_pnpm_lock_yaml_exists() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pnpm-lock.yaml"), "lockfileVersion: 9\n").unwrap();

        let pkg = PackageJson::default();
        let plugin = PnpmPlugin;
        assert!(plugin.is_enabled(&pkg, dir.path()));
    }

    #[test]
    fn is_enabled_when_pnpm_workspace_yaml_exists() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - packages/*\n",
        )
        .unwrap();

        let pkg = PackageJson::default();
        let plugin = PnpmPlugin;
        assert!(plugin.is_enabled(&pkg, dir.path()));
    }

    #[test]
    fn is_not_enabled_in_empty_directory() {
        let dir = tempfile::tempdir().unwrap();

        let pkg = PackageJson::default();
        let plugin = PnpmPlugin;
        assert!(!plugin.is_enabled(&pkg, dir.path()));
    }

    #[test]
    fn is_not_enabled_for_nonexistent_root() {
        let pkg = PackageJson::default();
        let plugin = PnpmPlugin;
        assert!(!plugin.is_enabled(&pkg, Path::new("/nonexistent/path/that/does/not/exist")));
    }

    #[test]
    fn always_used_includes_lockfile() {
        let plugin = PnpmPlugin;
        assert!(plugin.always_used().contains(&"pnpm-lock.yaml"));
    }

    #[test]
    fn always_used_includes_workspace_yaml() {
        let plugin = PnpmPlugin;
        assert!(plugin.always_used().contains(&"pnpm-workspace.yaml"));
    }

    #[test]
    fn always_used_includes_pnpmfile_variants() {
        let plugin = PnpmPlugin;
        assert!(plugin.always_used().contains(&".pnpmfile.cjs"));
        assert!(plugin.always_used().contains(&".pnpmfile.mjs"));
    }

    #[test]
    fn always_used_includes_npmrc() {
        let plugin = PnpmPlugin;
        assert!(plugin.always_used().contains(&".npmrc"));
    }

    #[test]
    fn tooling_dependencies_include_pnpm() {
        let plugin = PnpmPlugin;
        assert!(plugin.tooling_dependencies().contains(&"pnpm"));
    }

    #[test]
    fn registry_activation_path_sees_workspace_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pnpm-workspace.yaml"), "packages: []\n").unwrap();

        assert!(PnpmPlugin.is_enabled_with_files(&[], dir.path(), &[], None));
    }

    #[test]
    fn registry_activation_path_sees_pnpmfile() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join(".pnpmfile.mjs"),
            "export const hooks = {};\n",
        )
        .unwrap();

        assert!(PnpmPlugin.is_enabled_with_files(&[], dir.path(), &[], None));
    }

    #[test]
    fn package_manager_field_activates() {
        let pkg: PackageJson = serde_json::from_str(r#"{ "packageManager": "pnpm@10.12.1" }"#)
            .expect("valid package.json");
        assert!(PnpmPlugin.is_enabled_with_package_json(&pkg, Path::new("/project")));

        let pkg: PackageJson = serde_json::from_str(r#"{ "packageManager": "yarn@4.0.0" }"#)
            .expect("valid package.json");
        assert!(!PnpmPlugin.is_enabled_with_package_json(&pkg, Path::new("/project")));
    }

    #[test]
    fn workspace_pnpmfile_setting_is_always_used() {
        let result = PnpmPlugin.resolve_config(
            Path::new("/project/pnpm-workspace.yaml"),
            "packages:\n  - packages/*\npnpmfile:\n  - .pnpm/hooks.mjs\n  - ./tools/more.cjs\n",
            Path::new("/project"),
        );
        assert_eq!(
            result.always_used_files,
            vec![".pnpm/hooks.mjs", "tools/more.cjs"]
        );

        let result = PnpmPlugin.resolve_config(
            Path::new("/project/pnpm-workspace.yaml"),
            "pnpmfile: hooks/pnpmfile.cjs\n",
            Path::new("/project"),
        );
        assert_eq!(result.always_used_files, vec!["hooks/pnpmfile.cjs"]);
    }

    #[test]
    fn npmrc_pnpmfile_setting_is_always_used() {
        let result = PnpmPlugin.resolve_config(
            Path::new("/project/.npmrc"),
            "# hooks\nauto-install-peers=true\npnpmfile = \"tools/hooks.cjs\"\npnpmfile=/opt/hooks.cjs\n",
            Path::new("/project"),
        );
        assert_eq!(result.always_used_files, vec!["tools/hooks.cjs"]);
    }

    #[test]
    fn pnpmfile_setting_outside_the_root_is_dropped() {
        let result = PnpmPlugin.resolve_config(
            Path::new("/project/.npmrc"),
            "pnpmfile=../shared/hooks.cjs\n",
            Path::new("/project"),
        );
        assert!(result.always_used_files.is_empty());
    }
}
