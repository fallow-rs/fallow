//! Detection of unused pnpm catalog entries in `pnpm-workspace.yaml`.
//!
//! pnpm 9+ supports two catalog forms:
//! - the top-level `catalog:` map ("default" catalog)
//! - the top-level `catalogs:` map of named catalogs
//!
//! Workspace packages reference catalog versions from their `dependencies` /
//! `devDependencies` / `peerDependencies` / `optionalDependencies` via the
//! `catalog:` protocol (`"react": "catalog:"`, `"old-react": "catalog:react17"`).
//!
//! A catalog entry is "unused" when no workspace `package.json` declares the
//! same package with a `catalog:` reference to that catalog. The detector
//! also surfaces `hardcoded_consumers`: workspace packages that declare the
//! same package with a non-`catalog:` version range. Surfacing hardcoded
//! consumers helps users decide whether to delete the catalog entry or to
//! switch the consumers to `catalog:` (the more common intent).
//!
//! The default catalog can be referenced as either `catalog:` (bare) or
//! `catalog:default`; both forms are treated as identical per the pnpm spec.
//!
//! This detector runs purely off package.json declarations and `pnpm-workspace.yaml`
//! contents, not the import graph: a dep that no source file imports is irrelevant
//! here because the question is "does any package.json reference this entry via
//! the catalog: protocol", not "is the package itself used at runtime".

use std::path::{Path, PathBuf};

use fallow_config::{PackageJson, ResolvedConfig, WorkspaceInfo, parse_pnpm_catalog_data};
use fallow_types::results::UnusedCatalogEntry;
use rustc_hash::FxHashSet;

const PNPM_WORKSPACE_FILE: &str = "pnpm-workspace.yaml";

/// Walk catalog entries and report ones not referenced by any workspace
/// `package.json` via the `catalog:` protocol.
///
/// Returns an empty `Vec` when no `pnpm-workspace.yaml` exists at the project
/// root, when the file has no catalog data, or when every entry is referenced.
pub fn find_unused_catalog_entries(
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
) -> Vec<UnusedCatalogEntry> {
    let yaml_path = config.root.join(PNPM_WORKSPACE_FILE);
    let Ok(yaml_source) = std::fs::read_to_string(&yaml_path) else {
        return Vec::new();
    };

    let data = parse_pnpm_catalog_data(&yaml_source);
    if data.catalogs.is_empty() {
        return Vec::new();
    }

    let consumer_pkg_paths = collect_consumer_pkg_paths(config, workspaces);
    let consumers = collect_catalog_consumers(&consumer_pkg_paths, &config.root);

    let mut findings = Vec::new();
    for catalog in &data.catalogs {
        for entry in &catalog.entries {
            let key = ConsumerKey {
                package_name: entry.package_name.as_str(),
                catalog_name: catalog.name.as_str(),
            };
            if consumers.references.contains(&key.owned()) {
                continue;
            }

            let hardcoded_consumers = consumers
                .hardcoded
                .iter()
                .filter(|(name, _)| name == &entry.package_name)
                .map(|(_, path)| path.clone())
                .collect();

            findings.push(UnusedCatalogEntry {
                entry_name: entry.package_name.clone(),
                catalog_name: catalog.name.clone(),
                path: PathBuf::from(PNPM_WORKSPACE_FILE),
                line: entry.line,
                hardcoded_consumers,
            });
        }
    }

    findings
}

/// Collect every `package.json` path that participates in the workspace:
/// the project root plus each declared workspace package.
fn collect_consumer_pkg_paths(
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
) -> Vec<PathBuf> {
    let mut paths = Vec::with_capacity(workspaces.len() + 1);
    paths.push(config.root.join("package.json"));
    for ws in workspaces {
        paths.push(ws.root.join("package.json"));
    }
    paths
}

#[derive(Debug, Default)]
struct CatalogConsumers {
    /// `(package_name, catalog_name)` pairs referenced via `catalog:` protocol.
    /// Catalog name `"default"` covers both the bare `catalog:` and explicit
    /// `catalog:default` forms.
    references: FxHashSet<OwnedConsumerKey>,
    /// `(package_name, path)` pairs declaring the package with a hardcoded
    /// (non-`catalog:`) version range. Used to surface "this catalog entry
    /// is unreferenced, but these consumers declare a hardcoded version."
    hardcoded: Vec<(String, PathBuf)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct OwnedConsumerKey {
    package_name: String,
    catalog_name: String,
}

#[derive(Debug, Clone, Copy)]
struct ConsumerKey<'a> {
    package_name: &'a str,
    catalog_name: &'a str,
}

impl ConsumerKey<'_> {
    fn owned(self) -> OwnedConsumerKey {
        OwnedConsumerKey {
            package_name: self.package_name.to_string(),
            catalog_name: self.catalog_name.to_string(),
        }
    }
}

fn collect_catalog_consumers(pkg_paths: &[PathBuf], root: &Path) -> CatalogConsumers {
    let mut consumers = CatalogConsumers::default();
    for pkg_path in pkg_paths {
        let Ok(pkg) = PackageJson::load(pkg_path) else {
            continue;
        };
        let relative_path = pkg_path
            .strip_prefix(root)
            .map_or_else(|_| pkg_path.clone(), Path::to_path_buf);

        for deps in [
            pkg.dependencies.as_ref(),
            pkg.dev_dependencies.as_ref(),
            pkg.peer_dependencies.as_ref(),
            pkg.optional_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            for (name, version) in deps {
                if let Some(catalog) = parse_catalog_reference(version) {
                    consumers.references.insert(OwnedConsumerKey {
                        package_name: name.clone(),
                        catalog_name: catalog.to_string(),
                    });
                } else if is_hardcoded_version(version) {
                    consumers
                        .hardcoded
                        .push((name.clone(), relative_path.clone()));
                }
            }
        }
    }
    consumers
}

/// Parse a `catalog:` protocol value. Returns the catalog name (`"default"`
/// for bare `catalog:` and explicit `catalog:default`, or the named catalog).
/// Returns `None` for any non-catalog version string.
fn parse_catalog_reference(value: &str) -> Option<&str> {
    let rest = value.strip_prefix("catalog:")?;
    if rest.is_empty() || rest == "default" {
        Some("default")
    } else {
        Some(rest)
    }
}

/// Identify version strings that represent a hardcoded version range, as
/// opposed to a workspace cross-reference (`workspace:*`, `workspace:^`),
/// a filesystem path (`file:..`), or a symlinked dependency (`link:..`).
/// Catalog references are handled by the caller and never reach this
/// function. Surfacing only true hardcoded ranges keeps
/// `hardcoded_consumers` actionable: the user can decide whether to switch
/// the consumer to `catalog:` rather than chase an internal workspace
/// reference.
fn is_hardcoded_version(value: &str) -> bool {
    !(value.starts_with("workspace:")
        || value.starts_with("file:")
        || value.starts_with("link:")
        || value.starts_with("portal:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bare_catalog_as_default() {
        assert_eq!(parse_catalog_reference("catalog:"), Some("default"));
        assert_eq!(parse_catalog_reference("catalog:default"), Some("default"));
    }

    #[test]
    fn parses_named_catalog() {
        assert_eq!(parse_catalog_reference("catalog:react17"), Some("react17"));
    }

    #[test]
    fn non_catalog_versions_return_none() {
        assert_eq!(parse_catalog_reference("^18.2.0"), None);
        assert_eq!(parse_catalog_reference("workspace:*"), None);
        assert_eq!(parse_catalog_reference("npm:other-pkg@^1.0.0"), None);
        assert_eq!(parse_catalog_reference(""), None);
    }

    #[test]
    fn workspace_and_link_protocols_are_not_hardcoded() {
        // `workspace:*`, `file:..`, `link:..`, and `portal:..` are internal
        // workspace references, not hardcoded version ranges. They must not
        // appear in `hardcoded_consumers` because the user can't "switch
        // them to catalog:" - they're a different kind of relationship.
        assert!(!is_hardcoded_version("workspace:*"));
        assert!(!is_hardcoded_version("workspace:^"));
        assert!(!is_hardcoded_version("workspace:~"));
        assert!(!is_hardcoded_version("file:../other-pkg"));
        assert!(!is_hardcoded_version("link:../symlinked"));
        assert!(!is_hardcoded_version("portal:../portal"));
    }

    #[test]
    fn semver_ranges_and_npm_specs_are_hardcoded() {
        assert!(is_hardcoded_version("^1.0.0"));
        assert!(is_hardcoded_version("~2.5.0"));
        assert!(is_hardcoded_version("1.2.3"));
        assert!(is_hardcoded_version(">=1.0.0 <2.0.0"));
        assert!(is_hardcoded_version("npm:other-pkg@^1.0.0"));
        assert!(is_hardcoded_version("github:user/repo#commit"));
        assert!(is_hardcoded_version("https://example.com/pkg.tgz"));
    }
}
