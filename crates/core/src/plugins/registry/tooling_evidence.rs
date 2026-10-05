//! Evidence that a project uses the tool behind an active plugin.
//!
//! A declared package activates a plugin, so an active plugin alone does not
//! show that the project runs the tool. The unused devDependency check credits
//! a plugin's tooling dependencies only when the plugin found a config file of
//! its own, its config in package.json, or a script, CI or git hook reference
//! to the tool. This module records the config side of that evidence; the
//! reference side is the script-used package set, which script, CI and hook
//! analysis fill after the plugin run.

use std::path::{Path, PathBuf};

use fallow_config::ExternalPluginDef;

use super::helpers::{expand_brace_pattern, is_external_plugin_active};
use super::{AggregatedPluginResult, PluginToolingDependencies};
use crate::plugins::Plugin;

/// The inputs of one plugin run that the evidence checks read.
pub(super) struct ToolingEvidenceInput<'a> {
    /// The active built-in plugins.
    pub active: &'a [&'a dyn Plugin],
    /// Every external plugin definition; the active ones are recorded.
    pub external_plugins: &'a [ExternalPluginDef],
    /// The dependency names that activated the plugins.
    pub all_deps: &'a [String],
    /// The package root of the run first, then any other directory whose
    /// config files apply to it (the project root for a workspace run).
    pub roots: &'a [&'a Path],
    /// The files the discovery walk collected.
    pub discovered_files: &'a [PathBuf],
}

/// Record the tooling dependencies of every active plugin, built-in and
/// external, together with whether the plugin found its own config.
pub(super) fn record_plugin_tooling(
    input: &ToolingEvidenceInput<'_>,
    result: &mut AggregatedPluginResult,
) {
    let Some(&package_root) = input.roots.first() else {
        return;
    };
    let manifest = read_manifest(package_root);

    for plugin in input.active {
        let dependencies = plugin.tooling_dependencies();
        if dependencies.is_empty() {
            continue;
        }
        let mut config_keys = vec![plugin.name()];
        if let Some(key) = plugin.package_json_config_key() {
            config_keys.push(key);
        }
        let own_config =
            manifest_config(manifest.as_ref(), package_root, &config_keys).or_else(|| {
                find_own_file(
                    plugin
                        .config_patterns()
                        .iter()
                        .chain(plugin.always_used())
                        .copied(),
                    input.roots,
                    input.discovered_files,
                )
            });
        result.plugin_tooling.push(PluginToolingDependencies {
            plugin: plugin.name().to_string(),
            dependencies: dependencies.iter().map(|dep| (*dep).to_string()).collect(),
            references: reference_packages(
                dependencies.iter().copied(),
                plugin.enablers().iter().copied(),
            ),
            own_config,
        });
    }

    for ext in input.external_plugins {
        if ext.tooling_dependencies.is_empty()
            || !is_external_plugin_active(ext, input.all_deps, package_root, input.discovered_files)
        {
            continue;
        }
        let own_config = manifest_config(manifest.as_ref(), package_root, &[ext.name.as_str()])
            .or_else(|| {
                find_own_file(
                    ext.config_patterns
                        .iter()
                        .chain(&ext.always_used)
                        .map(String::as_str),
                    input.roots,
                    input.discovered_files,
                )
            });
        result.plugin_tooling.push(PluginToolingDependencies {
            plugin: ext.name.clone(),
            dependencies: ext.tooling_dependencies.clone(),
            references: reference_packages(
                ext.tooling_dependencies.iter().map(String::as_str),
                ext.enablers.iter().map(String::as_str),
            ),
            own_config,
        });
    }
}

/// The tooling dependencies plus the exact enablers. A prefix enabler such as
/// `@scope/` names no package a command can invoke.
fn reference_packages<'a>(
    dependencies: impl Iterator<Item = &'a str>,
    enablers: impl Iterator<Item = &'a str>,
) -> Vec<String> {
    let mut references: Vec<String> = Vec::new();
    for name in dependencies.chain(enablers.filter(|enabler| !enabler.ends_with('/'))) {
        if !references.iter().any(|existing| existing == name) {
            references.push(name.to_string());
        }
    }
    references
}

fn read_manifest(root: &Path) -> Option<serde_json::Value> {
    let content = std::fs::read_to_string(root.join("package.json")).ok()?;
    serde_json::from_str(&content).ok()
}

/// The package.json path when it holds the tool's config under a top-level
/// key named after it, or under `config.<key>`.
fn manifest_config(
    manifest: Option<&serde_json::Value>,
    package_root: &Path,
    keys: &[&str],
) -> Option<PathBuf> {
    let manifest = manifest?;
    keys.iter()
        .any(|key| {
            manifest.get(*key).is_some()
                || manifest
                    .get("config")
                    .and_then(|config| config.get(*key))
                    .is_some()
        })
        .then(|| package_root.join("package.json"))
}

fn has_glob_syntax(pattern: &str) -> bool {
    pattern.contains(['*', '?', '['])
}

/// The first existing file under one of `roots` that one of the root-anchored
/// `patterns` names.
pub fn find_config_file<'a>(
    patterns: impl Iterator<Item = &'a str>,
    roots: &[&Path],
) -> Option<PathBuf> {
    find_own_file(patterns, roots, &[])
}

/// The first existing file under one of `roots` that one of `patterns` names.
///
/// A root-anchored pattern is probed on the filesystem and stops at the first
/// hit, so a hidden config file or a hook directory the discovery walk does
/// not collect still counts. A pattern that starts with `**` would walk the
/// whole tree, `node_modules` included, so it is matched against the
/// discovered files only.
fn find_own_file<'a>(
    patterns: impl Iterator<Item = &'a str>,
    roots: &[&Path],
    discovered_files: &[PathBuf],
) -> Option<PathBuf> {
    let mut deep = globset::GlobSetBuilder::new();
    let mut has_deep = false;
    for pattern in patterns {
        if pattern.starts_with('!') {
            continue;
        }
        if pattern.starts_with("**") {
            if let Ok(glob) = globset::Glob::new(pattern) {
                deep.add(glob);
                has_deep = true;
            }
            continue;
        }
        if let Some(found) = roots
            .iter()
            .find_map(|root| first_file_match(root, pattern))
        {
            return Some(found);
        }
    }
    if !has_deep {
        return None;
    }
    let deep = deep.build().ok()?;
    discovered_files
        .iter()
        .find(|file| {
            roots.iter().any(|root| {
                file.strip_prefix(root)
                    .is_ok_and(|relative| deep.is_match(relative))
            })
        })
        .cloned()
}

fn first_file_match(root: &Path, pattern: &str) -> Option<PathBuf> {
    expand_brace_pattern(pattern)
        .into_iter()
        .find_map(|expanded| {
            if !has_glob_syntax(&expanded) {
                let path = root.join(&expanded);
                return path.is_file().then_some(path);
            }
            let full = format!(
                "{}/{expanded}",
                glob::Pattern::escape(&root.to_string_lossy())
            );
            glob::glob(&full)
                .ok()?
                .filter_map(Result::ok)
                .find(|path| path.is_file())
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_config_key_counts_top_level_and_config_section() {
        let manifest = serde_json::json!({
            "simple-git-hooks": { "pre-commit": "npx tool" },
            "config": { "commitizen": { "path": "./adapter" } }
        });
        let root = Path::new("/project");
        let package_json = Some(root.join("package.json"));
        assert_eq!(
            manifest_config(Some(&manifest), root, &["simple-git-hooks"]),
            package_json
        );
        assert_eq!(
            manifest_config(Some(&manifest), root, &["commitizen"]),
            package_json
        );
        assert_eq!(manifest_config(Some(&manifest), root, &["karma"]), None);
        assert_eq!(manifest_config(None, root, &["karma"]), None);
    }

    #[test]
    fn reference_packages_skip_prefix_enablers_and_duplicates() {
        let references = reference_packages(
            ["tool", "tool-cli"].into_iter(),
            ["tool", "@tool/"].into_iter(),
        );
        assert_eq!(references, vec!["tool".to_string(), "tool-cli".to_string()]);
    }

    #[test]
    fn own_file_found_on_disk_for_hidden_and_nested_patterns() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::create_dir_all(root.join(".hooks/inner")).expect("mkdir");
        std::fs::write(root.join(".hooks/inner/pre-commit"), "run\n").expect("write");
        std::fs::write(root.join(".toolrc.json"), "{}").expect("write");

        assert_eq!(
            find_own_file([".toolrc.{yaml,json}"].into_iter(), &[root], &[]),
            Some(root.join(".toolrc.json"))
        );
        assert_eq!(
            find_own_file([".hooks/**/*"].into_iter(), &[root], &[]),
            Some(root.join(".hooks/inner/pre-commit"))
        );
        assert_eq!(
            find_own_file(
                ["tool.config.{js,ts}", ".other/**/*"].into_iter(),
                &[root],
                &[]
            ),
            None
        );
    }

    #[test]
    fn deep_patterns_match_discovered_files_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let discovered = vec![root.join("packages/app/tool.config.ts")];
        assert_eq!(
            find_own_file(["**/tool.config.ts"].into_iter(), &[root], &discovered),
            Some(root.join("packages/app/tool.config.ts"))
        );
        assert_eq!(
            find_own_file(["**/other.config.ts"].into_iter(), &[root], &discovered),
            None
        );
    }
}
