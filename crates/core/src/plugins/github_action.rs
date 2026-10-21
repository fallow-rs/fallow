//! GitHub Action manifest plugin.
//!
//! A JavaScript action declares its runtime files in `action.yml` or
//! `action.yaml`: `runs.pre`, `runs.main` and `runs.post`. The runner loads
//! these files, so no module imports them. The paths usually name built
//! output in `dist/`. The plugin maps them back to source files with the
//! resolver of the `package.json` `main` field, so a tsconfig `outDir` and
//! `rootDir`, the `dist/` to `src/` convention and the source index fallback
//! all apply.
//!
//! Only a manifest whose `runs.using` names a `node` runtime activates the
//! plugin. A composite or Docker action runs no JavaScript entry file.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use fallow_config::{EntryPointRole, yaml};
use rustc_hash::FxHashMap;

use super::{Plugin, PluginResult};
use crate::discover::{EntryPointSource, resolve_entry_path_with_tracking};

const PLUGIN_NAME: &str = "github-action";
const CONFIG_PATTERNS: &[&str] = &["action.yml", "action.yaml"];
const NODE_RUNTIME_PREFIX: &str = "node";
const ENTRY_KEYS: &[&str] = &["pre", "main", "post"];

/// Built-in plugin for JavaScript GitHub Action manifests.
pub struct GithubActionPlugin;

impl Plugin for GithubActionPlugin {
    fn name(&self) -> &'static str {
        PLUGIN_NAME
    }

    fn entry_point_role(&self) -> EntryPointRole {
        EntryPointRole::Runtime
    }

    fn is_enabled_with_files(
        &self,
        _deps: &[String],
        root: &Path,
        _discovered_files: &[PathBuf],
        candidate_index: Option<&super::registry::ConfigCandidateIndex>,
    ) -> bool {
        CONFIG_PATTERNS.iter().any(|name| {
            if candidate_index.is_some_and(|index| !index.dir_contains(root, OsStr::new(name))) {
                return false;
            }
            std::fs::read_to_string(root.join(name))
                .is_ok_and(|source| !node_action_entries(&source).is_empty())
        })
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    /// Resolve the manifest entries inside `root`. The filesystem config
    /// path of the registry calls `resolve_config` without the analysis
    /// root, so the package root is the bound for every entry.
    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let Some(base) = config_path.parent() else {
            return result;
        };
        let Ok(canonical_root) = dunce::canonicalize(root) else {
            return result;
        };
        // A missing target is a broken manifest, not a broken package entry:
        // collect the skips so that the resolver writes no warning.
        let mut skipped = FxHashMap::default();
        let mut entries: Vec<String> = node_action_entries(source)
            .into_iter()
            .filter_map(|entry| {
                let resolved = resolve_entry_path_with_tracking(
                    base,
                    &entry,
                    &canonical_root,
                    EntryPointSource::Plugin {
                        name: PLUGIN_NAME.to_string(),
                    },
                    Some(&mut skipped),
                    None,
                )?;
                root_relative_pattern(&resolved.path, root)
            })
            .collect();
        entries.sort();
        entries.dedup();
        result.extend_entry_patterns(entries);
        result
    }
}

/// The trimmed `runs.pre`, `runs.main` and `runs.post` values of a manifest
/// whose `runs.using` names a `node` runtime. Any other manifest gives none.
fn node_action_entries(source: &str) -> Vec<String> {
    let Ok(document) = yaml::parse(source) else {
        return Vec::new();
    };
    let Some(runs) = document.root().get("runs") else {
        return Vec::new();
    };
    let is_node = runs
        .get("using")
        .and_then(|using| using.as_str())
        .is_some_and(|using| using.trim().starts_with(NODE_RUNTIME_PREFIX));
    if !is_node {
        return Vec::new();
    }
    ENTRY_KEYS
        .iter()
        .filter_map(|key| runs.get(key).and_then(|value| value.as_str()))
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(str::to_owned)
        .collect()
}

fn root_relative_pattern(path: &Path, root: &Path) -> Option<String> {
    let relative = if let Ok(relative) = path.strip_prefix(root) {
        relative.to_path_buf()
    } else {
        let canonical_root = dunce::canonicalize(root).ok()?;
        dunce::canonicalize(path)
            .ok()?
            .strip_prefix(&canonical_root)
            .ok()?
            .to_path_buf()
    };
    let pattern = relative.to_string_lossy().replace('\\', "/");
    (!pattern.is_empty()).then_some(pattern)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Project {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    fn project(files: &[(&str, &str)]) -> Project {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dunce::canonicalize(dir.path()).expect("canonical root");
        for (path, content) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
            std::fs::write(path, content).expect("write");
        }
        Project { _dir: dir, root }
    }

    fn entries(project: &Project, manifest: &str) -> Vec<String> {
        let source = std::fs::read_to_string(project.root.join(manifest)).expect("manifest");
        GithubActionPlugin
            .resolve_config(&project.root.join(manifest), &source, &project.root)
            .entry_patterns
            .iter()
            .map(|rule| rule.pattern.clone())
            .collect()
    }

    const NODE_ACTION: &str = "runs:\n  using: node24\n  pre: dist/setup.js\n  main: dist/main.js\n  post: dist/cleanup.js\n";

    #[test]
    fn node_action_maps_pre_main_and_post_to_source_files() {
        let project = project(&[
            ("action.yml", NODE_ACTION),
            ("src/setup.ts", ""),
            ("src/main.ts", ""),
            ("src/cleanup.ts", ""),
        ]);
        assert_eq!(
            entries(&project, "action.yml"),
            vec!["src/cleanup.ts", "src/main.ts", "src/setup.ts"]
        );
        assert!(GithubActionPlugin.is_enabled_with_files(&[], &project.root, &[], None));
    }

    #[test]
    fn action_yaml_extension_is_read() {
        let project = project(&[("action.yaml", NODE_ACTION), ("src/main.ts", "")]);
        assert!(entries(&project, "action.yaml").contains(&"src/main.ts".to_string()));
        assert!(GithubActionPlugin.is_enabled_with_files(&[], &project.root, &[], None));
    }

    #[test]
    fn composite_and_docker_actions_add_no_entries_and_do_not_activate() {
        for using in ["composite", "docker"] {
            let manifest = format!("runs:\n  using: '{using}'\n  main: index.js\n");
            let project = project(&[("action.yml", &manifest), ("index.js", "")]);
            assert!(entries(&project, "action.yml").is_empty(), "{using}");
            assert!(
                !GithubActionPlugin.is_enabled_with_files(&[], &project.root, &[], None),
                "{using}"
            );
        }
    }

    #[test]
    fn plain_javascript_main_is_an_entry() {
        let project = project(&[
            ("action.yml", "runs:\n  using: node20\n  main: index.js\n"),
            ("index.js", ""),
        ]);
        assert_eq!(entries(&project, "action.yml"), vec!["index.js"]);
    }

    #[test]
    fn bundled_dist_index_falls_back_to_the_source_main_file() {
        let project = project(&[
            (
                "action.yml",
                "runs:\n  using: node20\n  main: dist/index.js\n",
            ),
            ("src/main.ts", ""),
        ]);
        assert_eq!(entries(&project, "action.yml"), vec!["src/main.ts"]);
    }

    #[test]
    fn tsconfig_out_dir_maps_to_a_custom_root_dir() {
        let project = project(&[
            (
                "action.yml",
                "runs:\n  using: node20\n  main: build/run.js\n",
            ),
            (
                "tsconfig.json",
                r#"{"compilerOptions":{"outDir":"./build","rootDir":"./lib-src"}}"#,
            ),
            ("lib-src/run.ts", ""),
        ]);
        assert_eq!(entries(&project, "action.yml"), vec!["lib-src/run.ts"]);
    }

    #[test]
    fn a_path_outside_the_project_is_not_an_entry() {
        let project = project(&[
            (
                "action/action.yml",
                "runs:\n  using: node20\n  main: ../../outside.js\n",
            ),
            ("action/index.js", ""),
        ]);
        assert!(entries(&project, "action/action.yml").is_empty());
    }

    #[test]
    fn invalid_yaml_gives_no_entries() {
        let project = project(&[("action.yml", "runs: [unclosed\n"), ("index.js", "")]);
        assert!(entries(&project, "action.yml").is_empty());
        assert!(!GithubActionPlugin.is_enabled_with_files(&[], &project.root, &[], None));
    }

    #[test]
    fn main_and_post_on_the_same_file_give_one_entry() {
        let project = project(&[
            (
                "action.yml",
                "runs:\n  using: node24\n  main: dist/index.mjs\n  post: dist/index.mjs\n",
            ),
            ("src/index.ts", ""),
        ]);
        assert_eq!(entries(&project, "action.yml"), vec!["src/index.ts"]);
    }
}
