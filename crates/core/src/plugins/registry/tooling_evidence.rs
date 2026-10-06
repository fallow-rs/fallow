//! Evidence that a project uses the tool behind an active plugin.
//!
//! A declared package activates a plugin, so an active plugin alone does not
//! show that the project runs the tool. The unused devDependency check credits
//! a plugin's tooling dependencies only when the plugin found a config file of
//! its own, its config in package.json, or a script, CI or git hook reference
//! to the tool. This module records the config side of that evidence; the
//! reference side is the script-used package set, which script, CI and hook
//! analysis fill after the plugin run.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use fallow_config::ExternalPluginDef;
use rustc_hash::FxHashSet;

use super::helpers::{
    ConfigCandidateIndex, expand_brace_pattern, is_external_plugin_active, pattern_needs_filesystem,
};
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
    /// More directories to probe for a root-anchored config file when `roots`
    /// hold none. The root run passes the plugin config search roots, so a
    /// config file in a workspace package credits a tool that only the root
    /// manifest declares.
    pub extra_roots: &'a [&'a Path],
    /// The in-memory file index of the discovery walk. It is `None` in
    /// production mode, which passes no `extra_roots`.
    pub candidate_index: Option<&'a ConfigCandidateIndex>,
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
                    input,
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
                    input,
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
    find_file_in_roots(patterns, roots, &[])
}

/// The plugin's own config file: first under `roots`, then under the
/// `extra_roots` that `roots` do not already cover.
fn find_own_file<'a>(
    patterns: impl Iterator<Item = &'a str>,
    input: &ToolingEvidenceInput<'_>,
) -> Option<PathBuf> {
    let patterns: Vec<&str> = patterns.collect();
    find_file_in_roots(
        patterns.iter().copied(),
        input.roots,
        input.discovered_files,
    )
    .or_else(|| find_file_in_extra_roots(&patterns, input))
}

/// The first file under one of the `extra_roots` that one of the root-anchored
/// `patterns` names.
///
/// A monorepo can have thousands of extra roots, so a pattern resolves against
/// the in-memory candidate index of the discovery walk when the run has one.
/// The filesystem probe stays for a run without an index and for a pattern
/// under a directory that the walk does not enter. Each pattern is compiled
/// once, not once per root.
fn find_file_in_extra_roots(
    patterns: &[&str],
    input: &ToolingEvidenceInput<'_>,
) -> Option<PathBuf> {
    let probes: Vec<ExtraRootProbe> = patterns
        .iter()
        .filter(|pattern| !pattern.starts_with('!') && !pattern.starts_with("**"))
        .flat_map(|pattern| expand_brace_pattern(pattern))
        .filter_map(|pattern| ExtraRootProbe::new(pattern, input.candidate_index.is_some()))
        .collect();
    if probes.is_empty() {
        return None;
    }
    input
        .extra_roots
        .iter()
        .filter(|root| !input.roots.contains(root))
        .find_map(|root| {
            let root_names = input
                .candidate_index
                .and_then(|index| index.names_in_dir(root));
            probes
                .iter()
                .find_map(|probe| probe.find(root, root_names, input.candidate_index))
        })
}

/// The leading directory components of a glob pattern before the first glob
/// component, such as `.hooks` for `.hooks/**/*`. `None` for a pattern without
/// glob syntax, or with a glob in its first component.
fn literal_glob_dir(pattern: &str) -> Option<PathBuf> {
    if !has_glob_syntax(pattern) {
        return None;
    }
    let literal: Vec<&str> = pattern
        .split('/')
        .take_while(|component| !has_glob_syntax(component))
        .collect();
    (!literal.is_empty()).then(|| literal.iter().collect())
}

/// One expanded pattern, prepared for the probe of many extra roots.
enum ExtraRootProbe {
    /// A literal file name, read from the candidate index. `dir` is the
    /// literal directory below the root, if the pattern has one.
    IndexedName {
        dir: Option<PathBuf>,
        name: OsString,
    },
    /// A file-name glob, read from the candidate index.
    IndexedGlob {
        dir: Option<PathBuf>,
        matcher: globset::GlobMatcher,
    },
    /// A pattern that only the filesystem can answer. `dir` is the literal
    /// directory prefix of a glob pattern. One `is_dir` check on it skips a
    /// root before the glob runs.
    Filesystem {
        pattern: String,
        dir: Option<PathBuf>,
    },
}

impl ExtraRootProbe {
    /// `None` when the index cannot answer the pattern and the filesystem probe
    /// does not apply: a glob in a directory component, which the index
    /// fast path of plugin config discovery does not match either.
    fn new(pattern: String, has_index: bool) -> Option<Self> {
        if !has_index || pattern_needs_filesystem(&pattern) {
            let dir = literal_glob_dir(&pattern);
            return Some(Self::Filesystem { pattern, dir });
        }
        let (dir, name) = match pattern.rsplit_once('/') {
            Some((dir, name)) => (Some(dir), name),
            None => (None, pattern.as_str()),
        };
        if dir.is_some_and(has_glob_syntax) {
            return None;
        }
        let dir = dir.map(PathBuf::from);
        if !has_glob_syntax(name) {
            return Some(Self::IndexedName {
                dir,
                name: OsString::from(name),
            });
        }
        let matcher = globset::Glob::new(name).ok()?.compile_matcher();
        Some(Self::IndexedGlob { dir, matcher })
    }

    fn find(
        &self,
        root: &Path,
        root_names: Option<&FxHashSet<OsString>>,
        index: Option<&ConfigCandidateIndex>,
    ) -> Option<PathBuf> {
        match (self, index) {
            (Self::IndexedName { dir: None, name }, _) => {
                root_names?.contains(name).then(|| root.join(name))
            }
            (
                Self::IndexedName {
                    dir: Some(dir),
                    name,
                },
                Some(index),
            ) => {
                let path = root.join(dir).join(name);
                index.contains_file(&path).then_some(path)
            }
            (Self::IndexedGlob { dir: None, matcher }, _) => root_names?
                .iter()
                .filter(|name| matcher.is_match(Path::new(name)))
                .min()
                .map(|name| root.join(name)),
            (
                Self::IndexedGlob {
                    dir: Some(dir),
                    matcher,
                },
                Some(index),
            ) => index
                .glob_matches_in_dir(&root.join(dir), matcher)
                .into_iter()
                .min(),
            (Self::Filesystem { pattern, dir }, _) => {
                if dir.as_ref().is_some_and(|dir| !root.join(dir).is_dir()) {
                    return None;
                }
                first_file_match(root, pattern)
            }
            (Self::IndexedName { .. } | Self::IndexedGlob { .. }, None) => None,
        }
    }
}

/// The first existing file under one of `roots` that one of `patterns` names.
///
/// A root-anchored pattern is probed on the filesystem and stops at the first
/// hit, so a hidden config file or a hook directory the discovery walk does
/// not collect still counts. A pattern that starts with `**` would walk the
/// whole tree, `node_modules` included, so it is matched against the
/// discovered files only.
fn find_file_in_roots<'a>(
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
            find_file_in_roots([".toolrc.{yaml,json}"].into_iter(), &[root], &[]),
            Some(root.join(".toolrc.json"))
        );
        assert_eq!(
            find_file_in_roots([".hooks/**/*"].into_iter(), &[root], &[]),
            Some(root.join(".hooks/inner/pre-commit"))
        );
        assert_eq!(
            find_file_in_roots(
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
            find_file_in_roots(["**/tool.config.ts"].into_iter(), &[root], &discovered),
            Some(root.join("packages/app/tool.config.ts"))
        );
        assert_eq!(
            find_file_in_roots(["**/other.config.ts"].into_iter(), &[root], &discovered),
            None
        );
    }

    fn extra_roots_input<'a>(
        roots: &'a [&'a Path],
        extra_roots: &'a [&'a Path],
        candidate_index: Option<&'a ConfigCandidateIndex>,
    ) -> ToolingEvidenceInput<'a> {
        ToolingEvidenceInput {
            active: &[],
            external_plugins: &[],
            all_deps: &[],
            roots,
            extra_roots,
            candidate_index,
            discovered_files: &[],
        }
    }

    #[test]
    fn extra_roots_resolve_against_the_index_when_one_exists() {
        let root = Path::new("/project");
        let package = root.join("packages/app");
        let indexed = package.join(".toolrc.json");
        let index = ConfigCandidateIndex::build([indexed.as_path()]);
        let roots = [root];
        let extra_roots = [root, package.as_path()];

        // The index holds the file, so the probe does not read the disk.
        let input = extra_roots_input(&roots, &extra_roots, Some(&index));
        assert_eq!(
            find_own_file([".toolrc.{yaml,json}"].into_iter(), &input),
            Some(indexed)
        );
        assert_eq!(find_own_file(["tool.config.js"].into_iter(), &input), None);
    }

    #[test]
    fn extra_roots_probe_the_disk_without_an_index() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let package = root.join("packages/app");
        std::fs::create_dir_all(&package).expect("create package");
        std::fs::write(package.join(".toolrc.json"), "{}").expect("write");
        let roots = [root];
        let extra_roots = [root, package.as_path()];

        let input = extra_roots_input(&roots, &extra_roots, None);
        assert_eq!(
            find_own_file([".toolrc.{yaml,json}"].into_iter(), &input),
            Some(package.join(".toolrc.json"))
        );

        // An empty index does not hold the file, so the fast path misses it.
        let empty = ConfigCandidateIndex::build(std::iter::empty::<&Path>());
        let input = extra_roots_input(&roots, &extra_roots, Some(&empty));
        assert_eq!(
            find_own_file([".toolrc.{yaml,json}"].into_iter(), &input),
            None
        );
    }

    #[test]
    fn extra_roots_probe_the_disk_for_a_directory_the_walk_skips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let package = root.join("packages/app");
        std::fs::create_dir_all(package.join(".hooks/inner")).expect("create hooks");
        std::fs::write(package.join(".hooks/inner/pre-commit"), "").expect("write");
        let roots = [root];
        let extra_roots = [root, package.as_path()];
        let empty = ConfigCandidateIndex::build(std::iter::empty::<&Path>());

        // `.hooks` is a hidden directory that the walk does not enter, so the
        // probe reads the disk even when an index exists.
        let input = extra_roots_input(&roots, &extra_roots, Some(&empty));
        assert_eq!(
            find_own_file([".hooks/**/*"].into_iter(), &input),
            Some(package.join(".hooks/inner/pre-commit"))
        );
        assert_eq!(find_own_file([".other/**/*"].into_iter(), &input), None);
    }

    #[test]
    fn literal_glob_dir_keeps_the_components_before_the_first_glob() {
        assert_eq!(
            literal_glob_dir(".hooks/**/*"),
            Some(PathBuf::from(".hooks"))
        );
        assert_eq!(
            literal_glob_dir(".config/tool/*.json"),
            Some(PathBuf::from(".config/tool"))
        );
        assert_eq!(literal_glob_dir("tool.config.*"), None);
        assert_eq!(literal_glob_dir(".toolrc.json"), None);
    }
}
