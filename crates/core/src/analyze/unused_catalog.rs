//! Detection of unused pnpm catalog entries and unresolved catalog references
//! in workspace `package.json` files.
//!
//! pnpm 9+ supports two catalog forms:
//! - the top-level `catalog:` map ("default" catalog)
//! - the top-level `catalogs:` map of named catalogs
//!
//! Workspace packages reference catalog versions from their `dependencies` /
//! `devDependencies` / `peerDependencies` / `optionalDependencies` via the
//! `catalog:` protocol (`"react": "catalog:"`, `"old-react": "catalog:react17"`).
//!
//! Two findings are emitted:
//!
//! 1. **`unused-catalog-entries`**: a catalog entry no workspace `package.json`
//!    references via the `catalog:` protocol. Also tracks `hardcoded_consumers`:
//!    workspace packages declaring the same package with a non-`catalog:`
//!    version range. Helps users decide whether to delete the catalog entry or
//!    switch the consumers to `catalog:`.
//!
//! 2. **`unresolved-catalog-references`**: the inverse. A `package.json`
//!    references a `catalog:` or `catalog:<name>` that does not declare the
//!    consumed package. `pnpm install` errors with
//!    `ERR_PNPM_CATALOG_ENTRY_NOT_FOUND_FOR_CATALOG_PROTOCOL`; fallow surfaces
//!    this statically before any install runs. Each finding carries
//!    `available_in_catalogs`: other catalogs in the same workspace that DO
//!    declare the package, so consumers can flip the reference instead of
//!    adding a new catalog entry. Suppression: `ignoreCatalogReferences`
//!    config rules (per-package, optionally scoped by catalog name and/or
//!    consumer glob).
//!
//! The default catalog can be referenced as either `catalog:` (bare) or
//! `catalog:default`; both forms are treated as identical per the pnpm spec.

use std::path::{Path, PathBuf};

use fallow_config::{
    CompiledIgnoreCatalogReferenceRule, PackageJson, PnpmCatalogData, ResolvedConfig,
    WorkspaceInfo, parse_pnpm_catalog_data,
};
use fallow_types::results::{UnresolvedCatalogReference, UnusedCatalogEntry};
use rustc_hash::FxHashSet;

const PNPM_WORKSPACE_FILE: &str = "pnpm-workspace.yaml";

/// Catalog analysis state: parsed YAML data and walked consumer references.
/// Built once per analysis run so both detectors share parsing cost.
pub struct PnpmCatalogState {
    data: PnpmCatalogData,
    consumers: CatalogConsumers,
}

/// Read `pnpm-workspace.yaml` and walk workspace `package.json` files to build
/// shared catalog analysis state. Returns `None` when the YAML file is missing
/// or unreadable; callers should skip both catalog detectors in that case.
pub fn gather_pnpm_catalog_state(
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
) -> Option<PnpmCatalogState> {
    let yaml_path = config.root.join(PNPM_WORKSPACE_FILE);
    let yaml_source = std::fs::read_to_string(&yaml_path).ok()?;

    let data = parse_pnpm_catalog_data(&yaml_source);
    let consumer_pkg_paths = collect_consumer_pkg_paths(config, workspaces);
    let consumers = collect_catalog_consumers(&consumer_pkg_paths, &config.root);

    Some(PnpmCatalogState { data, consumers })
}

/// Emit one `UnusedCatalogEntry` for every catalog entry not referenced by any
/// workspace `package.json` via the `catalog:` protocol.
pub fn find_unused_catalog_entries(state: &PnpmCatalogState) -> Vec<UnusedCatalogEntry> {
    if state.data.catalogs.is_empty() {
        return Vec::new();
    }

    let mut findings = Vec::new();
    for catalog in &state.data.catalogs {
        for entry in &catalog.entries {
            let key = ConsumerKey {
                package_name: entry.package_name.as_str(),
                catalog_name: catalog.name.as_str(),
            };
            if state.consumers.references.contains(&key.owned()) {
                continue;
            }

            let hardcoded_consumers = state
                .consumers
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

/// Emit one `UnresolvedCatalogReference` for every `catalog:` / `catalog:<name>`
/// reference whose target catalog does not declare the consumed package.
///
/// `available_in_catalogs` lists OTHER catalogs in the same workspace that DO
/// declare the package, sorted lexicographically. When non-empty, the suggested
/// fix is to flip the reference to one of those catalogs; when empty, the fix
/// is to add the missing entry to the named catalog or to remove the reference.
///
/// Findings matching any rule in `ignore_rules` are suppressed.
pub fn find_unresolved_catalog_references(
    state: &PnpmCatalogState,
    ignore_rules: &[CompiledIgnoreCatalogReferenceRule],
    root: &Path,
) -> Vec<UnresolvedCatalogReference> {
    let mut findings = Vec::new();

    for reference in &state.consumers.referenced_with_locations {
        // Is the (catalog, package) pair already valid?
        if catalog_has_entry(
            &state.data,
            &reference.catalog_name,
            &reference.package_name,
        ) {
            continue;
        }

        // User-written consumer globs are project-root-relative with forward
        // slashes, but `consumer_path` is stored as an absolute filesystem
        // path. Strip the root before matching so `packages/**/package.json`
        // matches the consumer path correctly on every platform.
        let consumer_path_str = reference
            .consumer_path
            .strip_prefix(root)
            .unwrap_or(&reference.consumer_path)
            .to_string_lossy()
            .replace('\\', "/");
        if ignore_rules.iter().any(|rule| {
            rule.matches(
                &reference.package_name,
                &reference.catalog_name,
                &consumer_path_str,
            )
        }) {
            continue;
        }

        let available_in_catalogs = collect_available_in_catalogs(
            &state.data,
            &reference.package_name,
            &reference.catalog_name,
        );

        findings.push(UnresolvedCatalogReference {
            entry_name: reference.package_name.clone(),
            catalog_name: reference.catalog_name.clone(),
            path: reference.consumer_path.clone(),
            line: reference.line,
            available_in_catalogs,
        });
    }

    findings
}

fn catalog_has_entry(data: &PnpmCatalogData, catalog_name: &str, package_name: &str) -> bool {
    data.catalogs
        .iter()
        .filter(|catalog| catalog.name == catalog_name)
        .flat_map(|catalog| catalog.entries.iter())
        .any(|entry| entry.package_name == package_name)
}

fn collect_available_in_catalogs(
    data: &PnpmCatalogData,
    package_name: &str,
    excluded_catalog: &str,
) -> Vec<String> {
    let mut catalogs: Vec<String> = data
        .catalogs
        .iter()
        .filter(|catalog| catalog.name != excluded_catalog)
        .filter(|catalog| {
            catalog
                .entries
                .iter()
                .any(|entry| entry.package_name == package_name)
        })
        .map(|catalog| catalog.name.clone())
        .collect();
    catalogs.sort();
    catalogs.dedup();
    catalogs
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
    /// One entry per consumer-side `catalog:` reference, paired with the
    /// consumer `package.json` location for `unresolved-catalog-references`
    /// findings. Same package name appearing in `dependencies` AND
    /// `devDependencies` produces two entries.
    referenced_with_locations: Vec<ConsumerReference>,
    /// `(package_name, path)` pairs declaring the package with a hardcoded
    /// (non-`catalog:`) version range. Used to surface "this catalog entry
    /// is unreferenced, but these consumers declare a hardcoded version."
    hardcoded: Vec<(String, PathBuf)>,
}

#[derive(Debug, Clone)]
struct ConsumerReference {
    package_name: String,
    catalog_name: String,
    consumer_path: PathBuf,
    line: u32,
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
        let Ok(raw_source) = std::fs::read_to_string(pkg_path) else {
            continue;
        };
        let Ok(pkg) = serde_json::from_str::<PackageJson>(&raw_source) else {
            continue;
        };
        // For `hardcoded_consumers` keep the relative-path storage that
        // shipped with #329 (the JSON consumer contract uses relative paths
        // and the #329 integration test asserts that shape).
        let relative_path = pkg_path
            .strip_prefix(root)
            .map_or_else(|_| pkg_path.clone(), Path::to_path_buf);
        // For `ConsumerReference.consumer_path` keep the absolute path so the
        // path-anchored filters that other finding types use
        // (`filter_results_by_changed_files`, `filter_to_workspaces`'s
        // `starts_with` check) work without a separate root-join pass. JSON
        // output strips the root via `serde_path::serialize`.
        let absolute_path = pkg_path.clone();

        let line_map = scan_dep_lines(&raw_source);

        for (section, deps) in [
            (DepSection::Dependencies, pkg.dependencies.as_ref()),
            (DepSection::DevDependencies, pkg.dev_dependencies.as_ref()),
            (DepSection::PeerDependencies, pkg.peer_dependencies.as_ref()),
            (
                DepSection::OptionalDependencies,
                pkg.optional_dependencies.as_ref(),
            ),
        ] {
            let Some(deps) = deps else {
                continue;
            };
            for (name, version) in deps {
                if let Some(catalog) = parse_catalog_reference(version) {
                    consumers.references.insert(OwnedConsumerKey {
                        package_name: name.clone(),
                        catalog_name: catalog.to_string(),
                    });
                    // Fall back to line 1 (file top) on the rare minified
                    // form where `"dependencies": {"react": "..."}` shares a
                    // line so the LSP diagnostic still lands inside the file
                    // instead of off-screen at line 0.
                    let line = line_map.line_for(section, name).unwrap_or(1);
                    consumers.referenced_with_locations.push(ConsumerReference {
                        package_name: name.clone(),
                        catalog_name: catalog.to_string(),
                        consumer_path: absolute_path.clone(),
                        line,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum DepSection {
    Dependencies,
    DevDependencies,
    PeerDependencies,
    OptionalDependencies,
}

impl DepSection {
    const fn json_key(self) -> &'static str {
        match self {
            Self::Dependencies => "dependencies",
            Self::DevDependencies => "devDependencies",
            Self::PeerDependencies => "peerDependencies",
            Self::OptionalDependencies => "optionalDependencies",
        }
    }
}

/// Maps `(section, package_name)` to its 1-based line number in the
/// `package.json` source. Built via a section-aware line scanner so we can
/// surface the consumer-side reference location for
/// `unresolved-catalog-references` findings.
#[derive(Debug, Default)]
struct DepLineMap {
    entries: Vec<((DepSection, String), u32)>,
}

impl DepLineMap {
    fn line_for(&self, section: DepSection, package_name: &str) -> Option<u32> {
        self.entries
            .iter()
            .find(|((sec, pkg), _)| *sec == section && pkg == package_name)
            .map(|(_, line)| *line)
    }
}

/// Walk the raw package.json source and record the 1-based line for each
/// `"name":` key under the four dep sections. Tracks brace depth so nested
/// objects under unrelated keys (e.g., `bin`, `peerDependenciesMeta`) do not
/// pollute the map.
///
/// Plain JSON: no comments, no trailing commas. A package.json with comments
/// would fail `serde_json::from_str` upstream and never reach this scanner.
fn scan_dep_lines(source: &str) -> DepLineMap {
    let mut entries = Vec::new();
    let mut current_section: Option<DepSection> = None;
    let mut section_depth_at_open: u32 = 0;
    let mut current_depth: u32 = 0;

    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = u32::try_from(idx).unwrap_or(u32::MAX).saturating_add(1);
        let trimmed = raw_line.trim();

        // Detect entering a known dep section by `"<section>":` followed by `{`.
        if current_section.is_none() {
            for section in [
                DepSection::Dependencies,
                DepSection::DevDependencies,
                DepSection::PeerDependencies,
                DepSection::OptionalDependencies,
            ] {
                let needle = format!("\"{}\"", section.json_key());
                if trimmed.starts_with(&needle) && raw_line.contains('{') {
                    current_section = Some(section);
                    section_depth_at_open = current_depth.saturating_add(1);
                    // Fall through to the brace-counting pass below so this
                    // line's own `{` increments depth.
                    break;
                }
            }
        }

        // Count braces for depth tracking. Quoted strings are not skipped
        // because package.json is plain JSON: any unbalanced brace inside a
        // quoted value would fail upstream serde_json parse, so the scanner
        // never sees such inputs.
        let mut opens: u32 = 0;
        let mut closes: u32 = 0;
        let mut in_string = false;
        let mut escaped = false;
        for ch in raw_line.chars() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                continue;
            }
            if ch == '"' {
                in_string = !in_string;
                continue;
            }
            if in_string {
                continue;
            }
            match ch {
                '{' => opens = opens.saturating_add(1),
                '}' => closes = closes.saturating_add(1),
                _ => {}
            }
        }

        let depth_before = current_depth;
        let depth_after_opens = depth_before.saturating_add(opens);
        // Inside an active dep section, record keys at exactly section_depth_at_open.
        if let Some(section) = current_section
            && depth_before == section_depth_at_open
            && let Some(name) = parse_json_key(trimmed)
        {
            entries.push(((section, name), line_no));
        }

        current_depth = depth_after_opens.saturating_sub(closes);

        // Section ends when depth drops below where we entered.
        if current_section.is_some() && current_depth < section_depth_at_open {
            current_section = None;
        }
    }

    DepLineMap { entries }
}

/// Parse a JSON object key from a trimmed line of the form `"key": value,?`.
/// Returns `None` for lines that do not start with a quoted key followed by
/// `:`.
fn parse_json_key(trimmed: &str) -> Option<String> {
    let rest = trimmed.strip_prefix('"')?;
    let end = rest.find('"')?;
    let key = &rest[..end];
    let after = rest[end.saturating_add(1)..].trim_start();
    if after.starts_with(':') {
        Some(key.to_string())
    } else {
        None
    }
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

    #[test]
    fn scan_dep_lines_captures_each_section() {
        let source = r#"{
  "name": "demo",
  "dependencies": {
    "react": "catalog:react17",
    "lodash": "^4.0.0"
  },
  "devDependencies": {
    "vitest": "catalog:"
  }
}
"#;
        let map = scan_dep_lines(source);
        assert_eq!(map.line_for(DepSection::Dependencies, "react"), Some(4));
        assert_eq!(map.line_for(DepSection::Dependencies, "lodash"), Some(5));
        assert_eq!(map.line_for(DepSection::DevDependencies, "vitest"), Some(8));
        // Not present in either section
        assert_eq!(map.line_for(DepSection::Dependencies, "vitest"), None);
    }

    #[test]
    fn scan_dep_lines_ignores_nested_object_keys() {
        // `peerDependenciesMeta.react.optional` must not be misread as a
        // peerDependencies entry. The outer key sits at the section-open
        // depth; the `react` nested key sits one deeper and must be skipped.
        let source = r#"{
  "peerDependencies": {
    "react": "*"
  },
  "peerDependenciesMeta": {
    "react": {
      "optional": true
    }
  }
}
"#;
        let map = scan_dep_lines(source);
        assert_eq!(map.line_for(DepSection::PeerDependencies, "react"), Some(3));
        // `react` from peerDependenciesMeta is not in our tracked sections.
        // Make sure we did not stash it under PeerDependencies a second time.
        let peer_react_hits = map
            .entries
            .iter()
            .filter(|((sec, name), _)| *sec == DepSection::PeerDependencies && name == "react")
            .count();
        assert_eq!(peer_react_hits, 1);
    }

    #[test]
    fn collect_available_in_catalogs_excludes_the_unresolved_one() {
        let data = parse_pnpm_catalog_data(
            r"
catalogs:
  react17:
    react: ^17.0.2
  react18:
    react: ^18.2.0
",
        );
        let available = collect_available_in_catalogs(&data, "react", "react17");
        assert_eq!(available, vec!["react18".to_string()]);
    }

    #[test]
    fn catalog_has_entry_default_form() {
        let data = parse_pnpm_catalog_data(
            r"
catalog:
  react: ^18.2.0
",
        );
        assert!(catalog_has_entry(&data, "default", "react"));
        assert!(!catalog_has_entry(&data, "default", "vue"));
        assert!(!catalog_has_entry(&data, "react17", "react"));
    }
}
