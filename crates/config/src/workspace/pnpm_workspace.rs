//! The parsed model of `pnpm-workspace.yaml`.
//!
//! One YAML parse gives the `packages` patterns, the catalogs and the
//! overrides, with the source position of each key.

use super::pnpm_catalog::{PnpmCatalog, PnpmCatalogData, PnpmCatalogEntry, PnpmCatalogGroup};
use super::pnpm_overrides::{PnpmOverrideData, PnpmOverrideEntry, parse_override_key};
use crate::yaml::{self, YamlError, YamlMapping, YamlNode};

/// Where the key of a `pnpm-workspace.yaml` entry is in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyLocation {
    /// The key is the first token of its line in a block mapping. The
    /// entry owns the lines `line` to `end_line` (1-based, inclusive).
    Block {
        /// The 1-based line of the key.
        line: u32,
        /// The 1-based last line of the value.
        end_line: u32,
    },
    /// The key is inside a flow mapping (`{ ... }`) and shares line
    /// `line` (1-based) with other keys.
    Flow {
        /// The 1-based line of the key.
        line: u32,
    },
    /// The source position is not known, or the entry comes from an
    /// alias, a merge key or a `package.json` file.
    #[default]
    Unknown,
}

/// The facts that fallow reads from one `pnpm-workspace.yaml` source.
#[derive(Debug, Clone, Default)]
pub struct PnpmWorkspaceDoc {
    packages: Vec<String>,
    catalogs: PnpmCatalogData,
    overrides: PnpmOverrideData,
}

impl PnpmWorkspaceDoc {
    /// Parse the text of a `pnpm-workspace.yaml` file.
    ///
    /// # Errors
    ///
    /// Returns an error when `source` is not valid YAML, holds more than
    /// one document, or has a duplicate mapping key.
    pub fn parse(source: &str) -> Result<Self, YamlError> {
        let document = yaml::parse_with_locations(source)?;
        let root = document.root();
        let lines = LineStarts::of(source);
        Ok(Self {
            packages: packages_of(root),
            catalogs: catalogs_of(source, &lines, root),
            overrides: overrides_of(source, &lines, root),
        })
    }

    /// The entries of the top-level `packages` sequence, as written.
    #[must_use]
    pub fn packages(&self) -> &[String] {
        &self.packages
    }

    /// The default catalog and the named catalogs.
    #[must_use]
    pub fn catalogs(&self) -> &PnpmCatalogData {
        &self.catalogs
    }

    /// The catalogs, by value.
    #[must_use]
    pub fn into_catalogs(self) -> PnpmCatalogData {
        self.catalogs
    }

    /// The entries of the top-level `overrides` mapping.
    #[must_use]
    pub fn overrides(&self) -> &PnpmOverrideData {
        &self.overrides
    }

    /// The overrides, by value.
    #[must_use]
    pub fn into_overrides(self) -> PnpmOverrideData {
        self.overrides
    }
}

/// The byte offset of the start of each line of one source text.
struct LineStarts(Vec<usize>);

impl LineStarts {
    fn of(source: &str) -> Self {
        Self(
            std::iter::once(0)
                .chain(source.match_indices('\n').map(|(index, _)| index + 1))
                .collect(),
        )
    }

    /// The 1-based line that holds the byte at `offset`.
    fn line_of(&self, offset: usize) -> u32 {
        let line = self.0.partition_point(|&start| start <= offset);
        u32::try_from(line).unwrap_or(u32::MAX)
    }

    /// The byte offset of the start of the line that holds `offset`.
    fn line_start(&self, offset: usize) -> usize {
        let line = self.0.partition_point(|&start| start <= offset);
        self.0[line.saturating_sub(1)]
    }
}

/// The reported line and the location of `key` in the mapping `parent`.
fn locate_key(
    source: &str,
    lines: &LineStarts,
    parent: YamlNode<'_>,
    key: YamlNode<'_>,
    value: YamlNode<'_>,
) -> (u32, KeyLocation) {
    let Some(key_range) = key.byte_range() else {
        return (1, KeyLocation::Unknown);
    };
    let line = lines.line_of(key_range.start);
    let Some(parent_range) = parent.byte_range() else {
        return (line, KeyLocation::Unknown);
    };
    if key_range.start < parent_range.start {
        return (line, KeyLocation::Unknown);
    }
    let Some(content) = source.get(parent_range.clone()).map(skip_node_properties) else {
        return (line, KeyLocation::Unknown);
    };
    if content.starts_with('{') {
        return (line, KeyLocation::Flow { line });
    }
    let before_key = source
        .get(lines.line_start(key_range.start)..key_range.start)
        .unwrap_or("x");
    if !before_key.bytes().all(|byte| byte == b' ') {
        return (line, KeyLocation::Unknown);
    }
    // Every key of a block mapping starts in the column of its first key. A
    // merge key (`<<: *x`) adds keys of a deeper anchored mapping, which
    // start in another column and do not own their lines in `parent`.
    let first_key = parent_range.end - content.len();
    if first_key - lines.line_start(first_key) != before_key.len() {
        return (line, KeyLocation::Unknown);
    }
    let end_line = if value.is_null() {
        line
    } else {
        match value.byte_range() {
            Some(range) if range.start >= key_range.end => lines
                .line_of(range.end.saturating_sub(1).max(range.start))
                .max(line),
            // An alias value has the position of the anchored node.
            Some(_) if value_on_key_line(source, key_range.end) => line,
            _ => return (line, KeyLocation::Unknown),
        }
    };
    (line, KeyLocation::Block { line, end_line })
}

/// The text of a node after its anchor, its tag and the comments before its
/// content. A flow mapping with an anchor (`&a { x: 1 }`) starts at `&`.
fn skip_node_properties(node_text: &str) -> &str {
    let mut rest = node_text;
    loop {
        rest = rest.trim_start();
        if let Some(comment) = rest.strip_prefix('#') {
            rest = comment.find('\n').map_or("", |end| &comment[end..]);
        } else if rest.starts_with(['&', '!']) {
            let end = rest
                .find(|c: char| c.is_whitespace() || matches!(c, '{' | '}' | '[' | ']' | ','))
                .unwrap_or(rest.len());
            rest = &rest[end..];
        } else {
            return rest;
        }
    }
}

/// Whether the value of the key that ends at `key_end` starts on the line
/// of the key.
fn value_on_key_line(source: &str, key_end: usize) -> bool {
    let rest_of_line = source
        .get(key_end..)
        .and_then(|rest| rest.split('\n').next())
        .unwrap_or("");
    rest_of_line
        .trim_start()
        .strip_prefix(':')
        .map(str::trim)
        .is_some_and(|value| !value.is_empty() && !value.starts_with('#'))
}

/// The key node and the value node of the top-level key `name`.
fn top_level_entry<'a>(root: YamlNode<'a>, name: &str) -> Option<(YamlNode<'a>, YamlNode<'a>)> {
    root.as_mapping()?
        .iter()
        .find(|(key, _)| key.as_str() == Some(name))
}

/// Whether `value` is an alias. An alias has the position of the
/// anchored node, which is before `key`.
fn is_alias(key: YamlNode<'_>, value: YamlNode<'_>) -> bool {
    match (key.byte_range(), value.byte_range()) {
        (Some(key_range), Some(value_range)) => value_range.start < key_range.end,
        _ => false,
    }
}

/// The entries of one catalog mapping. `owner_key` is the key that holds
/// the mapping (`catalog`, or the name of a group).
fn catalog_entries(
    source: &str,
    lines: &LineStarts,
    owner_key: YamlNode<'_>,
    mapping_node: YamlNode<'_>,
) -> Vec<PnpmCatalogEntry> {
    let Some(mapping) = mapping_node.as_mapping() else {
        return Vec::new();
    };
    // The lines of an aliased catalog belong to the anchored catalog.
    let aliased = is_alias(owner_key, mapping_node);
    mapping
        .iter()
        .filter_map(|(key, value)| {
            let package_name = key.as_str()?.to_string();
            let (line, location) = locate_key(source, lines, mapping_node, key, value);
            Some(PnpmCatalogEntry {
                package_name,
                specifier: value.scalar_text().map(str::to_string),
                line,
                location: if aliased {
                    KeyLocation::Unknown
                } else {
                    location
                },
            })
        })
        .collect()
}

fn catalogs_of(source: &str, lines: &LineStarts, root: YamlNode<'_>) -> PnpmCatalogData {
    let mut data = PnpmCatalogData::default();
    if let Some((key, value)) = top_level_entry(root, "catalog") {
        let entries = catalog_entries(source, lines, key, value);
        if !entries.is_empty() {
            data.catalogs.push(PnpmCatalog {
                name: "default".to_string(),
                entries,
            });
        }
    }
    let Some((_, named)) = top_level_entry(root, "catalogs") else {
        return data;
    };
    let Some(named_map) = named.as_mapping() else {
        return data;
    };
    for (name_key, value) in named_map.iter() {
        let Some(name) = name_key.as_str() else {
            continue;
        };
        if value.is_null() || value.as_mapping().is_some_and(YamlMapping::is_empty) {
            let (line, location) = locate_key(source, lines, named, name_key, value);
            data.empty_named_catalog_groups.push(PnpmCatalogGroup {
                name: name.to_string(),
                line,
                location,
            });
            continue;
        }
        let entries = catalog_entries(source, lines, name_key, value);
        if !entries.is_empty() {
            data.catalogs.push(PnpmCatalog {
                name: name.to_string(),
                entries,
            });
        }
    }
    data
}

fn overrides_of(source: &str, lines: &LineStarts, root: YamlNode<'_>) -> PnpmOverrideData {
    let Some((owner_key, node)) = top_level_entry(root, "overrides") else {
        return PnpmOverrideData::default();
    };
    let Some(mapping) = node.as_mapping() else {
        return PnpmOverrideData::default();
    };
    let aliased = is_alias(owner_key, node);
    let entries = mapping
        .iter()
        .filter_map(|(key, value)| {
            let raw_key = key.as_str()?.to_string();
            let (line, location) = locate_key(source, lines, node, key, value);
            Some(PnpmOverrideEntry {
                parsed_key: parse_override_key(&raw_key),
                raw_key,
                raw_value: value.scalar_text().map(str::to_string),
                line,
                location: if aliased {
                    KeyLocation::Unknown
                } else {
                    location
                },
            })
        })
        .collect();
    PnpmOverrideData { entries }
}

fn packages_of(root: YamlNode<'_>) -> Vec<String> {
    let Some(items) = root.get("packages").and_then(YamlNode::as_sequence) else {
        return Vec::new();
    };
    items
        .filter_map(YamlNode::scalar_text)
        .map(str::trim)
        .filter(|pattern| !pattern.is_empty())
        .map(str::to_string)
        .collect()
}

/// The `packages` patterns of a `pnpm-workspace.yaml` source.
///
/// pnpm accepts some files that the YAML parser of fallow rejects. For
/// such a file a line scan reads the block list under `packages:`, so
/// workspace discovery does not lose every package.
#[must_use]
pub fn pnpm_workspace_packages(source: &str) -> Vec<String> {
    match PnpmWorkspaceDoc::parse(source) {
        Ok(doc) => doc.packages,
        Err(_) => packages_from_lines(source),
    }
}

fn packages_from_lines(content: &str) -> Vec<String> {
    let mut patterns = Vec::new();
    let mut in_packages = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "packages:" {
            in_packages = true;
            continue;
        }
        if in_packages {
            if let Some(value) = trimmed.strip_prefix("- ") {
                let value = value.trim().trim_matches('\'').trim_matches('"');
                if !value.is_empty() {
                    patterns.push(value.to_string());
                }
            } else if !trimmed.is_empty() && !trimmed.starts_with('#') {
                break;
            }
        }
    }
    patterns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packages_basic() {
        let yaml = "packages:\n  - 'packages/*'\n  - 'apps/*'\n";
        assert_eq!(pnpm_workspace_packages(yaml), vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn packages_double_quotes() {
        let yaml = "packages:\n  - \"packages/*\"\n  - \"apps/*\"\n";
        assert_eq!(pnpm_workspace_packages(yaml), vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn packages_no_quotes() {
        let yaml = "packages:\n  - packages/*\n  - apps/*\n";
        assert_eq!(pnpm_workspace_packages(yaml), vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn packages_empty() {
        assert!(pnpm_workspace_packages("").is_empty());
    }

    #[test]
    fn packages_no_packages_key() {
        assert!(pnpm_workspace_packages("other:\n  - something\n").is_empty());
    }

    #[test]
    fn packages_with_comments() {
        let yaml = "packages:\n  # Comment\n  - 'packages/*'\n";
        assert_eq!(pnpm_workspace_packages(yaml), vec!["packages/*"]);
    }

    #[test]
    fn packages_stops_at_next_key() {
        let yaml = "packages:\n  - 'packages/*'\ncatalog:\n  react: ^18\n";
        assert_eq!(pnpm_workspace_packages(yaml), vec!["packages/*"]);
    }

    #[test]
    fn packages_malformed() {
        assert!(pnpm_workspace_packages(":::not yaml at all:::").is_empty());
    }

    #[test]
    fn packages_packages_key_empty_list() {
        let yaml = "packages:\nother:\n  - something\n";
        assert!(pnpm_workspace_packages(yaml).is_empty());
    }

    #[test]
    fn packages_with_empty_lines_between_entries() {
        let yaml = "packages:\n  - 'packages/*'\n\n  - 'apps/*'\n";
        assert_eq!(pnpm_workspace_packages(yaml), vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn packages_mixed_quotes() {
        let yaml = "packages:\n  - 'single/*'\n  - \"double/*\"\n  - bare/*\n";
        assert_eq!(
            pnpm_workspace_packages(yaml),
            vec!["single/*", "double/*", "bare/*"]
        );
    }

    #[test]
    fn packages_with_negation() {
        let yaml = "packages:\n  - 'packages/*'\n  - '!packages/test-*'\n";
        assert_eq!(
            pnpm_workspace_packages(yaml),
            vec!["packages/*", "!packages/test-*"]
        );
    }

    #[test]
    fn packages_reads_a_flow_sequence() {
        assert_eq!(
            pnpm_workspace_packages("packages: [packages/*, apps/*]\n"),
            vec!["packages/*", "apps/*"]
        );
    }

    #[test]
    fn packages_drops_a_trailing_comment() {
        assert_eq!(
            pnpm_workspace_packages("packages:\n  - packages/* # the libraries\n"),
            vec!["packages/*"]
        );
    }

    #[test]
    fn packages_keeps_a_quoted_hash() {
        assert_eq!(
            pnpm_workspace_packages("packages:\n  - 'pkgs/#internal/*'\n"),
            vec!["pkgs/#internal/*"]
        );
    }

    #[test]
    fn packages_accepts_a_comment_after_the_key() {
        assert_eq!(
            pnpm_workspace_packages("packages: # members\n  - 'a/*'\n"),
            vec!["a/*"]
        );
    }

    #[test]
    fn packages_ignores_a_nested_packages_key() {
        let yaml = "settings:\n  packages:\n    - nested/*\npackages:\n  - top/*\n";
        assert_eq!(pnpm_workspace_packages(yaml), vec!["top/*"]);
    }

    #[test]
    fn packages_falls_back_to_the_line_scan_for_rejected_yaml() {
        let yaml = "packages:\n  - packages/*\n\ncatalog:\n  is-odd: 3.0.1\n  unused-entry: 1.0.0\n\nonlyBuiltDependencies: [\nesbuild\n]\n";
        assert!(PnpmWorkspaceDoc::parse(yaml).is_err());
        assert_eq!(pnpm_workspace_packages(yaml), vec!["packages/*"]);
    }

    fn patterns_of_workspace_file(text: &str) -> Vec<String> {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(dir.path().join("pnpm-workspace.yaml"), text).expect("write file");
        super::super::collect_workspace_patterns(dir.path(), &[]).expect("no root package.json")
    }

    #[test]
    fn discovery_reads_a_flow_sequence() {
        assert_eq!(
            patterns_of_workspace_file("packages: [packages/*, libs/*]\n"),
            ["packages/*", "libs/*"]
        );
    }

    #[test]
    fn discovery_drops_a_trailing_comment() {
        assert_eq!(
            patterns_of_workspace_file("packages:\n  - packages/* # the libraries\n"),
            ["packages/*"]
        );
    }

    fn doc(source: &str) -> PnpmWorkspaceDoc {
        PnpmWorkspaceDoc::parse(source).expect("valid yaml")
    }

    fn catalog<'d>(doc: &'d PnpmWorkspaceDoc, name: &str) -> &'d PnpmCatalog {
        doc.catalogs()
            .catalogs
            .iter()
            .find(|catalog| catalog.name == name)
            .unwrap_or_else(|| panic!("no catalog {name}"))
    }

    fn empty_groups(doc: &PnpmWorkspaceDoc) -> Vec<(&str, u32, KeyLocation)> {
        doc.catalogs()
            .empty_named_catalog_groups
            .iter()
            .map(|group| (group.name.as_str(), group.line, group.location))
            .collect()
    }

    #[test]
    fn flow_style_group_is_not_empty() {
        let doc = doc("catalogs:\n  ui: { react: \"18\" }\n");
        let ui = catalog(&doc, "ui");
        assert_eq!(ui.entries.len(), 1);
        let entry = &ui.entries[0];
        assert_eq!(entry.package_name, "react");
        assert_eq!(entry.specifier.as_deref(), Some("18"));
        assert_eq!(entry.line, 2);
        assert_eq!(entry.location, KeyLocation::Flow { line: 2 });
        assert!(doc.catalogs().empty_named_catalog_groups.is_empty());
    }

    #[test]
    fn empty_groups_come_from_the_mapping() {
        let doc = doc(
            "catalogs:\n  react17: {}\n  legacy:\n    # note\n  vue3:\n    vue: ^3.4.0\n  last:\n",
        );
        assert_eq!(
            empty_groups(&doc),
            [
                (
                    "react17",
                    2,
                    KeyLocation::Block {
                        line: 2,
                        end_line: 2
                    }
                ),
                (
                    "legacy",
                    3,
                    KeyLocation::Block {
                        line: 3,
                        end_line: 3
                    }
                ),
                (
                    "last",
                    7,
                    KeyLocation::Block {
                        line: 7,
                        end_line: 7
                    }
                ),
            ]
        );
        assert_eq!(doc.catalogs().catalogs.len(), 1);
        assert_eq!(catalog(&doc, "vue3").entries[0].package_name, "vue");
    }

    #[test]
    fn flow_style_catalogs_mapping_keeps_groups() {
        let doc = doc("catalogs: { old: {}, ui: { react: \"18\" } }\n");
        assert_eq!(
            empty_groups(&doc),
            [("old", 1, KeyLocation::Flow { line: 1 })]
        );
        assert_eq!(catalog(&doc, "ui").entries[0].package_name, "react");
    }

    #[test]
    fn block_entries_own_their_lines() {
        let doc = doc("catalog:\n  react:\n    specifier: ^18.2.0\n  vue: ^3.4.0\n");
        let entries = &catalog(&doc, "default").entries;
        assert_eq!(
            entries[0].location,
            KeyLocation::Block {
                line: 2,
                end_line: 3
            }
        );
        assert_eq!(
            entries[1].location,
            KeyLocation::Block {
                line: 4,
                end_line: 4
            }
        );
    }

    #[test]
    fn quoted_top_level_key_is_read() {
        let doc = doc("\"catalog\":\n  react: ^18\n");
        let entries = &catalog(&doc, "default").entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].package_name, "react");
        assert_eq!(entries[0].line, 2);
        assert!(matches!(entries[0].location, KeyLocation::Block { .. }));
    }

    #[test]
    fn default_flow_catalog_is_read() {
        let doc = doc("catalog: { react: ^18 }\n");
        let entries = &catalog(&doc, "default").entries;
        assert_eq!(entries[0].package_name, "react");
        assert_eq!(entries[0].location, KeyLocation::Flow { line: 1 });
    }

    #[test]
    fn aliased_catalog_has_no_editable_location() {
        let doc = doc("catalogs:\n  base: &base\n    react: ^18\n  copy: *base\n");
        let copy = &catalog(&doc, "copy").entries;
        assert_eq!(copy[0].package_name, "react");
        assert_eq!(copy[0].location, KeyLocation::Unknown);
        let base = &catalog(&doc, "base").entries;
        assert_eq!(base[0].package_name, "react");
        assert_eq!(
            base[0].location,
            KeyLocation::Block {
                line: 3,
                end_line: 3
            }
        );
        assert!(doc.catalogs().empty_named_catalog_groups.is_empty());
    }

    #[test]
    fn many_keys_get_their_own_lines() {
        use std::fmt::Write as _;
        const COUNT: usize = 5_000;
        let mut source = String::from("catalog:\n");
        for index in 0..COUNT {
            writeln!(source, "  pkg-{index}: 1.0.0").expect("writing to a String cannot fail");
        }
        let doc = doc(&source);
        let entries = &catalog(&doc, "default").entries;
        assert_eq!(entries.len(), COUNT);
        for (index, entry) in entries.iter().enumerate() {
            assert_eq!(entry.line as usize, index + 2);
        }
    }

    #[test]
    fn flow_style_overrides_are_kept() {
        let doc = doc("overrides: {react: \"catalog:\"}\n");
        let entries = &doc.overrides().entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].raw_key, "react");
        assert_eq!(entries[0].raw_value.as_deref(), Some("catalog:"));
        assert_eq!(entries[0].line, 1);
        assert_eq!(entries[0].location, KeyLocation::Flow { line: 1 });
    }

    #[test]
    fn block_overrides_own_their_lines() {
        let doc = doc(
            "packages:\n  - 'packages/*'\n\noverrides:\n  axios: ^1.6.0\n  \"@types/react@<18\": '18.0.0'\n  \"react>react-dom\": ^17\n",
        );
        let locations: Vec<_> = doc
            .overrides()
            .entries
            .iter()
            .map(|entry| (entry.line, entry.location))
            .collect();
        assert_eq!(
            locations,
            [5, 6, 7].map(|line| (
                line,
                KeyLocation::Block {
                    line,
                    end_line: line
                }
            ))
        );
    }

    #[test]
    fn one_parse_gives_all_three_sections() {
        let doc = doc("packages:\n  - 'a/*'\ncatalog:\n  react: ^18\noverrides:\n  axios: ^1\n");
        assert_eq!(doc.packages(), ["a/*"]);
        assert_eq!(catalog(&doc, "default").entries.len(), 1);
        assert_eq!(doc.overrides().entries.len(), 1);
    }

    #[test]
    fn anchored_or_tagged_flow_mapping_keys_are_flow() {
        let doc = doc(
            "catalogs:\n  ui: &ui {\n    react: \"18\", vue: \"3\"\n    }\n  web: !!map {\n    next: \"15\"\n    }\n",
        );
        let locations: Vec<_> = ["ui", "web"]
            .iter()
            .flat_map(|name| catalog(&doc, name).entries.iter())
            .map(|entry| (entry.package_name.as_str(), entry.location))
            .collect();
        assert_eq!(
            locations,
            [
                ("react", KeyLocation::Flow { line: 3 }),
                ("vue", KeyLocation::Flow { line: 3 }),
                ("next", KeyLocation::Flow { line: 6 }),
            ]
        );
    }

    #[test]
    fn empty_flow_group_over_several_lines_has_its_extent() {
        let doc = doc("catalogs:\n  old: !!map {\n    }\n");
        assert_eq!(
            empty_groups(&doc),
            [(
                "old",
                2,
                KeyLocation::Block {
                    line: 2,
                    end_line: 3
                }
            )]
        );
    }

    #[test]
    fn merged_keys_have_no_editable_location() {
        let doc = doc("catalogs:\n  ui:\n    a: &x\n      b: 1\n    <<: *x\n");
        let locations: Vec<_> = catalog(&doc, "ui")
            .entries
            .iter()
            .map(|entry| (entry.package_name.as_str(), entry.location))
            .collect();
        assert!(
            locations.contains(&(
                "a",
                KeyLocation::Block {
                    line: 3,
                    end_line: 4
                }
            )),
            "{locations:?}"
        );
        assert!(
            locations.contains(&("b", KeyLocation::Unknown)),
            "{locations:?}"
        );
    }

    #[test]
    fn alias_value_on_the_key_line_owns_its_line() {
        let doc = doc("catalog:\n  react: &v ^18\n  vue: *v\n");
        let entries = &catalog(&doc, "default").entries;
        assert_eq!(entries[1].package_name, "vue");
        assert_eq!(entries[1].specifier.as_deref(), Some("^18"));
        assert_eq!(
            entries[1].location,
            KeyLocation::Block {
                line: 3,
                end_line: 3
            }
        );
    }

    #[test]
    fn packages_drops_an_unquoted_tag_item() {
        let yaml = "packages:\n  - packages/*\n  - !packages/test-*\n";
        assert_eq!(pnpm_workspace_packages(yaml), vec!["packages/*"]);
    }
}
