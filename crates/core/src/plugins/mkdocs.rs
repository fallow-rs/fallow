//! `MkDocs` and Zensical plugin.
//!
//! `MkDocs` adds each file that `mkdocs.yml` lists in `extra_javascript` and
//! `extra_css` to every page of the site, so no module imports these files.
//! Zensical reads the same keys from the `[project]` table of
//! `zensical.toml`. The paths are relative to `docs_dir`, which is `docs`
//! next to the config by default. A URL entry names a remote file and adds
//! no entry point.
//!
//! Both tools are Python tools, so a config file activates the plugin, not a
//! dependency. A JavaScript project often keeps its site in a subdirectory
//! such as `docs/` that is not a workspace package, so a config anywhere
//! below the root activates the plugin. Config discovery then reads each
//! nested config that is an ancestor of a source file.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use fallow_config::yaml;

use super::{PathRule, Plugin, PluginResult};

const PLUGIN_NAME: &str = "mkdocs";
const CONFIG_PATTERNS: &[&str] = &["mkdocs.yml", "mkdocs.yaml", "zensical.toml"];
const ZENSICAL_PROJECT_TABLE: &str = "project";
const DEFAULT_DOCS_DIR: &str = "docs";
const DOCS_DIR_KEY: &str = "docs_dir";
const ASSET_KEYS: &[&str] = &["extra_javascript", "extra_css"];
const ASSET_PATH_KEY: &str = "path";

/// Built-in plugin for `MkDocs` and Zensical sites.
pub struct MkdocsPlugin;

impl Plugin for MkdocsPlugin {
    fn name(&self) -> &'static str {
        PLUGIN_NAME
    }

    fn is_enabled_with_files(
        &self,
        _deps: &[String],
        root: &Path,
        _discovered_files: &[PathBuf],
        candidate_index: Option<&super::registry::ConfigCandidateIndex>,
    ) -> bool {
        // In production mode (`candidate_index` is `None`) only a config in
        // the root activates the plugin, as for the other sentinel plugins.
        CONFIG_PATTERNS.iter().any(|name| match candidate_index {
            Some(index) => index.any_descendant_contains(root, OsStr::new(name)),
            None => root.join(name).is_file(),
        })
    }

    fn config_patterns(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn always_used(&self) -> &'static [&'static str] {
        CONFIG_PATTERNS
    }

    fn resolve_config(&self, config_path: &Path, source: &str, root: &Path) -> PluginResult {
        let mut result = PluginResult::default();
        let Some(site) = SiteAssets::parse(config_path, source) else {
            return result;
        };
        let Some(base) = config_path
            .parent()
            .and_then(|dir| root_relative_dir(dir, root))
        else {
            return result;
        };
        let docs_root = base.join(site.docs_dir.as_deref().unwrap_or(DEFAULT_DOCS_DIR));
        let mut entries: Vec<String> = site
            .assets
            .iter()
            .filter(|asset| is_local(asset))
            .filter_map(|asset| relative_pattern(&docs_root.join(asset)))
            .collect();
        entries.sort();
        entries.dedup();
        result
            .entry_patterns
            .extend(entries.into_iter().map(|pattern| PathRule {
                package_relative: true,
                ..PathRule::new(pattern)
            }));
        result
    }
}

/// The asset settings of one site config, in either format.
struct SiteAssets {
    docs_dir: Option<String>,
    assets: Vec<String>,
}

impl SiteAssets {
    fn parse(config_path: &Path, source: &str) -> Option<Self> {
        let is_toml = config_path
            .extension()
            .is_some_and(|extension| extension == "toml");
        if is_toml {
            Self::from_toml(source)
        } else {
            Self::from_yaml(source)
        }
    }

    fn from_yaml(source: &str) -> Option<Self> {
        let document = yaml::parse(source).ok()?;
        let config = document.root();
        let docs_dir = config
            .get(DOCS_DIR_KEY)
            .and_then(|value| value.as_str())
            .and_then(non_empty);
        let assets = ASSET_KEYS
            .iter()
            .filter_map(|key| config.get(key))
            .filter_map(|list| list.as_sequence())
            .flatten()
            .filter_map(|item| {
                item.as_str()
                    .or_else(|| item.get(ASSET_PATH_KEY).and_then(|value| value.as_str()))
            })
            .filter_map(non_empty)
            .collect();
        Some(Self { docs_dir, assets })
    }

    fn from_toml(source: &str) -> Option<Self> {
        let config = fallow_config::toml_input::from_str::<toml::Table>(source).ok()?;
        let project = config
            .get(ZENSICAL_PROJECT_TABLE)
            .and_then(toml::Value::as_table)?;
        let docs_dir = project
            .get(DOCS_DIR_KEY)
            .and_then(toml::Value::as_str)
            .and_then(non_empty);
        let assets = ASSET_KEYS
            .iter()
            .filter_map(|key| project.get(*key))
            .filter_map(toml::Value::as_array)
            .flatten()
            .filter_map(|item| {
                item.as_str().or_else(|| {
                    item.as_table()
                        .and_then(|table| table.get(ASSET_PATH_KEY))
                        .and_then(toml::Value::as_str)
                })
            })
            .filter_map(non_empty)
            .collect();
        Some(Self { docs_dir, assets })
    }
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Whether an asset entry names a file of the site and not a URL.
fn is_local(asset: &str) -> bool {
    !asset.starts_with("//") && !asset.contains("://")
}

/// The directory `dir` relative to `root`. The registry can pass a config
/// path in canonical form while `root` is not, so a canonical compare is the
/// fallback.
fn root_relative_dir(dir: &Path, root: &Path) -> Option<PathBuf> {
    if let Ok(relative) = dir.strip_prefix(root) {
        return Some(relative.to_path_buf());
    }
    let canonical_root = dunce::canonicalize(root).ok()?;
    let canonical_dir = dunce::canonicalize(dir).ok()?;
    Some(
        canonical_dir
            .strip_prefix(&canonical_root)
            .ok()?
            .to_path_buf(),
    )
}

/// The escaped glob pattern of a root-relative `path`, or `None` when the
/// path leaves the root. The file need not exist.
fn relative_pattern(path: &Path) -> Option<String> {
    let normalized = lexical_normalize(path)?;
    let pattern = normalized.to_string_lossy().replace('\\', "/");
    (!pattern.is_empty()).then(|| globset::escape(&pattern))
}

/// Resolve `.` and `..` components without the filesystem. A `..` above the
/// start of the path, or an absolute path, gives `None`.
fn lexical_normalize(path: &Path) -> Option<PathBuf> {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(part) => normalized.push(part),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(config_name: &str, source: &str) -> Vec<String> {
        let root = Path::new("/project");
        let mut patterns = MkdocsPlugin
            .resolve_config(&root.join(config_name), source, root)
            .entry_patterns
            .into_iter()
            .map(|rule| rule.pattern)
            .collect::<Vec<_>>();
        patterns.sort();
        patterns
    }

    #[test]
    fn a_nested_site_config_activates_only_through_the_index() {
        let config = PathBuf::from("/repo/docs/zensical.toml");
        let index = crate::plugins::registry::ConfigCandidateIndex::build(std::iter::once(
            config.as_path(),
        ));
        let enabled = |root: &str, index| {
            MkdocsPlugin.is_enabled_with_files(&[], Path::new(root), &[], index)
        };

        assert!(enabled("/repo", Some(&index)));
        assert!(!enabled("/other", Some(&index)));
        assert!(!enabled("/repo", None));
    }

    #[test]
    fn extra_assets_resolve_against_the_default_docs_dir() {
        let source = "site_name: Demo\nextra_javascript:\n  - js/extra.js\n  - path: js/module.mjs\n    type: module\nextra_css:\n  - css/extra.css\n";
        assert_eq!(
            entries("mkdocs.yml", source),
            [
                "docs/css/extra.css",
                "docs/js/extra.js",
                "docs/js/module.mjs"
            ]
        );
    }

    #[test]
    fn a_custom_docs_dir_is_the_base_of_each_asset() {
        let source = "docs_dir: site/src\nextra_css:\n  - ./styles/main.css\n";
        assert_eq!(entries("mkdocs.yaml", source), ["site/src/styles/main.css"]);
    }

    #[test]
    fn urls_and_paths_outside_the_root_add_no_entry() {
        let source = "extra_javascript:\n  - https://unpkg.com/x.js\n  - //cdn.example.com/y.js\n  - ../../outside.js\nextra_css:\n  - path: https://example.com/z.css\n";
        assert!(entries("mkdocs.yml", source).is_empty());
    }

    #[test]
    fn python_tags_of_a_material_config_do_not_stop_the_read() {
        let source = "site_name: !ENV [SITE_NAME, Demo]\nmarkdown_extensions:\n  - pymdownx.emoji:\n      emoji_index: !!python/name:material.extensions.emoji.twemoji\n      emoji_generator: !!python/name:material.extensions.emoji.to_svg\nextra_javascript:\n  - js/extra.js\n";
        assert_eq!(entries("mkdocs.yml", source), ["docs/js/extra.js"]);
    }

    #[test]
    fn glob_characters_in_a_path_match_only_that_file() {
        let source = "extra_css:\n  - css/[theme].css\n";
        assert_eq!(
            entries("mkdocs.yml", source),
            [globset::escape("docs/css/[theme].css")]
        );
    }

    #[test]
    fn zensical_reads_the_project_table() {
        let source = "[project]\nsite_name = \"Demo\"\ndocs_dir = \"chapters\"\nextra_css = [\"stylesheets/extra.css\"]\nextra_javascript = [\n  \"javascripts/nav.mjs\",\n  { path = \"javascripts/module.mjs\", type = \"module\" },\n  \"https://unpkg.com/x.js\",\n]\n";
        assert_eq!(
            entries("zensical.toml", source),
            [
                "chapters/javascripts/module.mjs",
                "chapters/javascripts/nav.mjs",
                "chapters/stylesheets/extra.css",
            ]
        );
    }

    #[test]
    fn zensical_keys_outside_the_project_table_add_no_entry() {
        let source = "extra_css = [\"stylesheets/extra.css\"]\n";
        assert!(entries("zensical.toml", source).is_empty());
    }
}
