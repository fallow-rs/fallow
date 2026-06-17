//! Parser for package manager catalog declarations.
//!
//! pnpm supports two catalog forms:
//! - the top-level `catalog:` map (the "default" catalog)
//! - the top-level `catalogs:` map of named catalogs
//!
//! Bun supports the same shapes in root `package.json`, usually under
//! `workspaces.catalog` / `workspaces.catalogs`, with top-level `catalog` /
//! `catalogs` accepted as an alternative.
//!
//! ```yaml
//! catalog:
//!   react: ^18.2.0
//!   "@scope/lib": ^1.0.0
//!
//! catalogs:
//!   react17:
//!     react: ^17.0.2
//!     react-dom: ^17.0.2
//! ```
//!
//! Workspace packages reference catalog entries from their `dependencies`
//! (and friends) with the `catalog:` protocol:
//!
//! ```json
//! { "dependencies": { "react": "catalog:", "old-react": "catalog:react17" } }
//! ```
//!
//! For the unused-catalog-entry detector we need both the structured catalog
//! map and the 1-based line number of each entry in the source so findings
//! can point users to the exact line. `serde_yaml_ng` gives us the structural
//! parse; a second targeted scan over the raw source recovers the line
//! numbers.

/// Structured catalog data extracted from a package manager catalog source.
#[derive(Debug, Clone, Default)]
pub struct PnpmCatalogData {
    /// Catalogs found in the file. The default catalog (top-level `catalog:`)
    /// always appears first with `name = "default"` when present; named
    /// catalogs follow in source order.
    pub catalogs: Vec<PnpmCatalog>,
    /// Named catalogs under `catalogs:` that declare no package entries.
    ///
    /// The top-level `catalog:` map is intentionally not represented here:
    /// some repos keep it as a stable hook even when currently empty.
    pub empty_named_catalog_groups: Vec<PnpmCatalogGroup>,
}

/// A single catalog (the default or a named one).
#[derive(Debug, Clone)]
pub struct PnpmCatalog {
    /// Catalog name. `"default"` for the top-level `catalog:` map, or the
    /// named catalog key for entries declared under `catalogs.<name>:`.
    pub name: String,
    /// Entries declared in this catalog, in source order.
    pub entries: Vec<PnpmCatalogEntry>,
}

/// A single entry inside a catalog.
#[derive(Debug, Clone)]
pub struct PnpmCatalogEntry {
    /// Package name declared in the catalog (e.g. `"react"`, `"@scope/lib"`).
    pub package_name: String,
    /// 1-based line number of the entry within the source file.
    pub line: u32,
}

/// A named catalog group under `catalogs:` with no package entries.
#[derive(Debug, Clone)]
pub struct PnpmCatalogGroup {
    /// Catalog group name (e.g. `"react17"` for `catalogs.react17`).
    pub name: String,
    /// 1-based line number of the group header within the source file.
    pub line: u32,
}

/// Parse the catalog sections of a `pnpm-workspace.yaml` file.
///
/// Returns an empty `PnpmCatalogData` when the file has no catalog data, when
/// the YAML is malformed, or when the catalog sections are present but empty.
/// All non-catalog top-level keys (`packages`, `catalog`, `catalogs`, etc.)
/// are ignored.
#[must_use]
pub fn parse_pnpm_catalog_data(source: &str) -> PnpmCatalogData {
    let value: serde_yaml_ng::Value = match serde_yaml_ng::from_str(source) {
        Ok(v) => v,
        Err(_) => return PnpmCatalogData::default(),
    };
    let Some(mapping) = value.as_mapping() else {
        return PnpmCatalogData::default();
    };

    let line_index = build_line_index(source);
    let mut catalogs = Vec::new();
    let mut empty_named_catalog_groups = Vec::new();

    if let Some(default_value) = mapping.get("catalog")
        && let Some(default_map) = default_value.as_mapping()
    {
        let entries = collect_entries(default_map, &line_index, "default");
        if !entries.is_empty() {
            catalogs.push(PnpmCatalog {
                name: "default".to_string(),
                entries,
            });
        }
    }

    if let Some(named_value) = mapping.get("catalogs")
        && let Some(named_map) = named_value.as_mapping()
    {
        for (name_value, catalog_value) in named_map {
            let Some(name) = name_value.as_str() else {
                continue;
            };
            if let Some(catalog_map) = catalog_value.as_mapping() {
                let entries = collect_entries(catalog_map, &line_index, name);
                if entries.is_empty() {
                    if let Some(line) = line_index.group_line_for(name) {
                        empty_named_catalog_groups.push(PnpmCatalogGroup {
                            name: name.to_string(),
                            line,
                        });
                    }
                } else {
                    catalogs.push(PnpmCatalog {
                        name: name.to_string(),
                        entries,
                    });
                }
            } else if catalog_value.is_null()
                && let Some(line) = line_index.group_line_for(name)
            {
                empty_named_catalog_groups.push(PnpmCatalogGroup {
                    name: name.to_string(),
                    line,
                });
            }
        }
    }

    PnpmCatalogData {
        catalogs,
        empty_named_catalog_groups,
    }
}

/// Parse Bun catalog sections from a root `package.json` file.
///
/// Bun accepts `workspaces.catalog` / `workspaces.catalogs` and the same
/// `catalog` / `catalogs` keys at package.json top level. The nested
/// `workspaces` form is preferred when both forms exist for the same section.
#[must_use]
pub fn parse_package_json_catalog_data(source: &str) -> PnpmCatalogData {
    let value: serde_json::Value = match serde_json::from_str(source.trim_start_matches('\u{FEFF}'))
    {
        Ok(value) => value,
        Err(_) => return PnpmCatalogData::default(),
    };
    let Some(root) = value.as_object() else {
        return PnpmCatalogData::default();
    };

    let workspaces = root
        .get("workspaces")
        .and_then(serde_json::Value::as_object);
    let default_value = workspaces
        .and_then(|workspace| workspace.get("catalog"))
        .or_else(|| root.get("catalog"));
    let named_value = workspaces
        .and_then(|workspace| workspace.get("catalogs"))
        .or_else(|| root.get("catalogs"));
    let line_index = build_package_json_line_index(source);

    let mut catalogs = Vec::new();
    let mut empty_named_catalog_groups = Vec::new();

    if let Some(default_map) = default_value.and_then(serde_json::Value::as_object) {
        let entries = collect_json_entries(default_map, &line_index, "default");
        if !entries.is_empty() {
            catalogs.push(PnpmCatalog {
                name: "default".to_string(),
                entries,
            });
        }
    }

    if let Some(named_map) = named_value.and_then(serde_json::Value::as_object) {
        for (name, catalog_value) in named_map {
            if let Some(catalog_map) = catalog_value.as_object() {
                let entries = collect_json_entries(catalog_map, &line_index, name);
                if entries.is_empty() {
                    empty_named_catalog_groups.push(PnpmCatalogGroup {
                        name: name.clone(),
                        line: line_index.group_line_for(name).unwrap_or(1),
                    });
                } else {
                    catalogs.push(PnpmCatalog {
                        name: name.clone(),
                        entries,
                    });
                }
            } else if catalog_value.is_null() {
                empty_named_catalog_groups.push(PnpmCatalogGroup {
                    name: name.clone(),
                    line: line_index.group_line_for(name).unwrap_or(1),
                });
            }
        }
    }

    PnpmCatalogData {
        catalogs,
        empty_named_catalog_groups,
    }
}

fn collect_entries(
    mapping: &serde_yaml_ng::Mapping,
    line_index: &CatalogLineIndex,
    catalog_name: &str,
) -> Vec<PnpmCatalogEntry> {
    mapping
        .iter()
        .filter_map(|(k, _)| {
            let pkg = k.as_str()?;
            let line = line_index.line_for(catalog_name, pkg)?;
            Some(PnpmCatalogEntry {
                package_name: pkg.to_string(),
                line,
            })
        })
        .collect()
}

fn collect_json_entries(
    mapping: &serde_json::Map<String, serde_json::Value>,
    line_index: &CatalogLineIndex,
    catalog_name: &str,
) -> Vec<PnpmCatalogEntry> {
    mapping
        .keys()
        .map(|pkg| PnpmCatalogEntry {
            package_name: pkg.clone(),
            line: line_index.line_for(catalog_name, pkg).unwrap_or(1),
        })
        .collect()
}

/// Maps `(catalog_name, package_name)` to its 1-based source line.
///
/// `catalog_name` is `"default"` for entries under the top-level `catalog:`
/// key, or the named catalog key for entries under `catalogs.<name>:`.
struct CatalogLineIndex {
    entries: Vec<((String, String), u32)>,
    groups: Vec<(String, u32)>,
}

impl CatalogLineIndex {
    fn line_for(&self, catalog_name: &str, package_name: &str) -> Option<u32> {
        self.entries
            .iter()
            .find(|((cat, pkg), _)| cat == catalog_name && pkg == package_name)
            .map(|(_, line)| *line)
    }

    fn group_line_for(&self, catalog_name: &str) -> Option<u32> {
        self.groups
            .iter()
            .find(|(name, _)| name == catalog_name)
            .map(|(_, line)| *line)
    }
}

/// Walk the raw YAML source to map each catalog entry to its 1-based line
/// number. This is a small section-aware scanner: it tracks whether the
/// current line falls inside `catalog:` (the default catalog) or inside
/// `catalogs.<name>:` (a named catalog), and records each key at the
/// expected indentation level.
fn build_line_index(source: &str) -> CatalogLineIndex {
    let mut entries = Vec::new();
    let mut groups = Vec::new();
    let mut section: Section = Section::None;
    let mut named_catalog: Option<(String, usize)> = None;

    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = u32::try_from(idx).unwrap_or(u32::MAX).saturating_add(1);
        let trimmed = strip_inline_comment(raw_line);
        let trimmed_left = trimmed.trim_start();
        let indent = trimmed.len() - trimmed_left.len();

        if trimmed_left.is_empty() {
            continue;
        }

        if indent == 0 {
            section = if trimmed_left.starts_with("catalogs:") {
                Section::NamedCatalogs
            } else if trimmed_left.starts_with("catalog:") {
                Section::DefaultCatalog
            } else {
                Section::None
            };
            named_catalog = None;
            continue;
        }

        match section {
            Section::None => {}
            Section::DefaultCatalog => {
                if let Some(name) = parse_key(trimmed_left) {
                    entries.push((("default".to_string(), name), line_no));
                }
            }
            Section::NamedCatalogs => {
                if let Some(name) = parse_key(trimmed_left) {
                    match &named_catalog {
                        Some((_, existing_indent)) if indent > *existing_indent => {
                            entries.push((
                                (
                                    named_catalog
                                        .as_ref()
                                        .map_or_else(String::new, |(n, _)| n.clone()),
                                    name,
                                ),
                                line_no,
                            ));
                        }
                        _ => {
                            groups.push((name.clone(), line_no));
                            named_catalog = Some((name, indent));
                        }
                    }
                }
            }
        }
    }

    CatalogLineIndex { entries, groups }
}

fn build_package_json_line_index(source: &str) -> CatalogLineIndex {
    let mut entries = Vec::new();
    let mut groups = Vec::new();
    let mut current_depth: u32 = 0;
    let mut workspaces_depth: Option<u32> = None;
    let mut section: Section = Section::None;
    let mut section_depth: u32 = 0;
    let mut named_catalog: Option<(String, u32)> = None;

    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = u32::try_from(idx).unwrap_or(u32::MAX).saturating_add(1);
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let key = parse_json_key(trimmed);
        let parent_depth = current_depth;
        let in_supported_parent =
            parent_depth == 1 || workspaces_depth.is_some_and(|depth| parent_depth == depth);

        if let Some(name) = key {
            match section {
                Section::DefaultCatalog if parent_depth == section_depth => {
                    entries.push((("default".to_string(), name.to_string()), line_no));
                }
                Section::NamedCatalogs if parent_depth == section_depth => {
                    groups.push((name.to_string(), line_no));
                    named_catalog = Some((name.to_string(), parent_depth));
                }
                Section::NamedCatalogs => {
                    if let Some((catalog_name, group_depth)) = &named_catalog
                        && parent_depth == group_depth.saturating_add(1)
                    {
                        entries.push(((catalog_name.clone(), name.to_string()), line_no));
                    }
                }
                Section::DefaultCatalog | Section::None => {}
            }
        }

        let (opens, closes) = count_json_braces(raw_line);
        let depth_after_opens = current_depth.saturating_add(opens);

        if let Some(name) = key {
            if parent_depth == 1 && name == "workspaces" && opens > 0 {
                workspaces_depth = Some(parent_depth.saturating_add(1));
            }
            if in_supported_parent && name == "catalog" && opens > 0 {
                section = Section::DefaultCatalog;
                section_depth = parent_depth.saturating_add(1);
                named_catalog = None;
            } else if in_supported_parent && name == "catalogs" && opens > 0 {
                section = Section::NamedCatalogs;
                section_depth = parent_depth.saturating_add(1);
                named_catalog = None;
            }
        }

        current_depth = depth_after_opens.saturating_sub(closes);

        if matches!(section, Section::DefaultCatalog | Section::NamedCatalogs)
            && current_depth < section_depth
        {
            section = Section::None;
            named_catalog = None;
        }
        if let Some(depth) = workspaces_depth
            && current_depth < depth
        {
            workspaces_depth = None;
        }
    }

    CatalogLineIndex { entries, groups }
}

fn parse_json_key(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix('"')?;
    let end = rest.find('"')?;
    let after = rest[end.saturating_add(1)..].trim_start();
    after.starts_with(':').then_some(&rest[..end])
}

fn count_json_braces(line: &str) -> (u32, u32) {
    let mut opens: u32 = 0;
    let mut closes: u32 = 0;
    let mut in_string = false;
    let mut escaped = false;
    for ch in line.chars() {
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
    (opens, closes)
}

#[derive(Debug, Clone, Copy)]
enum Section {
    None,
    DefaultCatalog,
    NamedCatalogs,
}

/// Strip an unquoted trailing `# ...` comment from a single line. Preserves
/// `#` characters inside quoted strings so `"# in quotes": "value"` is left
/// alone.
pub(super) fn strip_inline_comment(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut in_single = false;
    let mut in_double = false;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'\'' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'#' if !in_single && !in_double => {
                let head = &line[..i];
                return head.trim_end();
            }
            _ => {}
        }
    }
    line.trim_end()
}

/// Parse a key declaration of the form `key:` or `key: value`, returning just
/// the (unquoted) key. Returns `None` when the line is not a key declaration
/// (e.g., a list item `- foo`, a block scalar marker, or malformed).
pub(super) fn parse_key(line: &str) -> Option<String> {
    let bytes = line.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let first = bytes[0];
    if first == b'-' || first == b'#' {
        return None;
    }

    if first == b'"' || first == b'\'' {
        let quote = first;
        let mut i = 1;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if b == quote {
                let key = &line[1..i];
                let rest = &line[i + 1..];
                let trimmed = rest.trim_start();
                if trimmed.starts_with(':') {
                    return Some(unescape_key(key));
                }
                return None;
            }
            i += 1;
        }
        return None;
    }

    let colon_pos = bytes.iter().position(|&b| b == b':')?;
    let key = line[..colon_pos].trim();
    if key.is_empty() {
        return None;
    }
    if key.contains(['{', '[', '&', '*', '!']) {
        return None;
    }
    Some(key.to_string())
}

fn unescape_key(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(next) = chars.next()
        {
            match next {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_catalog() {
        let yaml = "packages:\n  - 'packages/*'\n\ncatalog:\n  react: ^18.2.0\n  is-even: ^1.0.0\n";
        let data = parse_pnpm_catalog_data(yaml);
        assert_eq!(data.catalogs.len(), 1);
        let default = &data.catalogs[0];
        assert_eq!(default.name, "default");
        assert_eq!(default.entries.len(), 2);
        assert_eq!(default.entries[0].package_name, "react");
        assert_eq!(default.entries[0].line, 5);
        assert_eq!(default.entries[1].package_name, "is-even");
        assert_eq!(default.entries[1].line, 6);
    }

    #[test]
    fn parses_bun_workspaces_catalog() {
        let json = r#"{
  "name": "demo",
  "workspaces": {
    "packages": ["packages/*"],
    "catalog": {
      "react": "^19.0.0",
      "react-dom": "^19.0.0"
    },
    "catalogs": {
      "testing": {
        "vitest": "^3.0.0"
      },
      "empty": {}
    }
  }
}
"#;
        let data = parse_package_json_catalog_data(json);
        assert_eq!(data.catalogs.len(), 2);
        assert_eq!(data.catalogs[0].name, "default");
        assert_eq!(data.catalogs[0].entries[0].package_name, "react");
        assert_eq!(data.catalogs[0].entries[0].line, 6);
        assert_eq!(data.catalogs[1].name, "testing");
        assert_eq!(data.catalogs[1].entries[0].package_name, "vitest");
        assert_eq!(data.catalogs[1].entries[0].line, 11);
        let empty: Vec<_> = data
            .empty_named_catalog_groups
            .iter()
            .map(|group| (group.name.as_str(), group.line))
            .collect();
        assert_eq!(empty, vec![("empty", 13)]);
    }

    #[test]
    fn parses_bun_top_level_catalog_fallback() {
        let json = r#"{
  "name": "demo",
  "workspaces": ["packages/*"],
  "catalog": {
    "bun-types": "^1.3.0"
  },
  "catalogs": {
    "testing": {
      "vitest": "^3.0.0"
    }
  }
}
"#;
        let data = parse_package_json_catalog_data(json);
        assert_eq!(data.catalogs.len(), 2);
        assert_eq!(data.catalogs[0].name, "default");
        assert_eq!(data.catalogs[0].entries[0].package_name, "bun-types");
        assert_eq!(data.catalogs[0].entries[0].line, 5);
        assert_eq!(data.catalogs[1].name, "testing");
        assert_eq!(data.catalogs[1].entries[0].line, 9);
    }

    #[test]
    fn workspaces_catalog_takes_precedence_over_top_level_catalog() {
        let json = r#"{
  "workspaces": {
    "packages": ["packages/*"],
    "catalog": {
      "react": "^19.0.0"
    }
  },
  "catalog": {
    "react": "^18.0.0",
    "vue": "^3.0.0"
  }
}
"#;
        let data = parse_package_json_catalog_data(json);
        assert_eq!(data.catalogs.len(), 1);
        let entries: Vec<_> = data.catalogs[0]
            .entries
            .iter()
            .map(|entry| entry.package_name.as_str())
            .collect();
        assert_eq!(entries, vec!["react"]);
        assert_eq!(data.catalogs[0].entries[0].line, 5);
    }

    #[test]
    fn parses_named_catalogs() {
        let yaml = "catalogs:\n  react17:\n    react: ^17.0.2\n    react-dom: ^17.0.2\n  ui:\n    headlessui: ^2.0.0\n";
        let data = parse_pnpm_catalog_data(yaml);
        assert_eq!(data.catalogs.len(), 2);
        assert_eq!(data.catalogs[0].name, "react17");
        assert_eq!(data.catalogs[0].entries.len(), 2);
        assert_eq!(data.catalogs[0].entries[0].package_name, "react");
        assert_eq!(data.catalogs[0].entries[0].line, 3);
        assert_eq!(data.catalogs[1].name, "ui");
        assert_eq!(data.catalogs[1].entries[0].package_name, "headlessui");
        assert_eq!(data.catalogs[1].entries[0].line, 6);
        assert!(data.empty_named_catalog_groups.is_empty());
    }

    #[test]
    fn handles_default_and_named_together() {
        let yaml = "catalog:\n  react: ^18\n\ncatalogs:\n  legacy:\n    react: ^17\n";
        let data = parse_pnpm_catalog_data(yaml);
        assert_eq!(data.catalogs.len(), 2);
        assert_eq!(data.catalogs[0].name, "default");
        assert_eq!(data.catalogs[0].entries[0].line, 2);
        assert_eq!(data.catalogs[1].name, "legacy");
        assert_eq!(data.catalogs[1].entries[0].line, 6);
    }

    #[test]
    fn handles_quoted_keys() {
        let yaml = "catalog:\n  \"@scope/lib\": ^1.0.0\n  'my-pkg': ^2.0.0\n";
        let data = parse_pnpm_catalog_data(yaml);
        let default = &data.catalogs[0];
        assert_eq!(default.entries[0].package_name, "@scope/lib");
        assert_eq!(default.entries[0].line, 2);
        assert_eq!(default.entries[1].package_name, "my-pkg");
        assert_eq!(default.entries[1].line, 3);
    }

    #[test]
    fn handles_inline_comments() {
        let yaml = "catalog:\n  react: ^18  # pin until #1234\n  is-even: ^1.0\n";
        let data = parse_pnpm_catalog_data(yaml);
        assert_eq!(data.catalogs[0].entries.len(), 2);
        assert_eq!(data.catalogs[0].entries[0].package_name, "react");
        assert_eq!(data.catalogs[0].entries[1].package_name, "is-even");
        assert_eq!(data.catalogs[0].entries[1].line, 3);
    }

    #[test]
    fn handles_four_space_indentation() {
        let yaml = "catalog:\n    react: ^18.2.0\n    vue: ^3.4.0\n";
        let data = parse_pnpm_catalog_data(yaml);
        assert_eq!(data.catalogs[0].entries.len(), 2);
        assert_eq!(data.catalogs[0].entries[0].line, 2);
        assert_eq!(data.catalogs[0].entries[1].line, 3);
    }

    #[test]
    fn empty_catalog_returns_no_catalogs() {
        let yaml = "catalog: {}\n";
        let data = parse_pnpm_catalog_data(yaml);
        assert!(data.catalogs.is_empty());
        assert!(data.empty_named_catalog_groups.is_empty());
    }

    #[test]
    fn tracks_empty_named_catalog_groups() {
        let yaml = "catalog:\n  react: ^18\n\ncatalogs:\n  react17: {}\n  legacy:\n    # retained note\n  vue3:\n    vue: ^3.4.0\n";
        let data = parse_pnpm_catalog_data(yaml);
        assert_eq!(data.catalogs.len(), 2);
        let empty: Vec<_> = data
            .empty_named_catalog_groups
            .iter()
            .map(|group| (group.name.as_str(), group.line))
            .collect();
        assert_eq!(empty, vec![("react17", 5), ("legacy", 6)]);
    }

    #[test]
    fn no_catalog_keys_returns_no_catalogs() {
        let yaml = "packages:\n  - 'packages/*'\n";
        let data = parse_pnpm_catalog_data(yaml);
        assert!(data.catalogs.is_empty());
    }

    #[test]
    fn malformed_yaml_returns_no_catalogs() {
        let yaml = "{this is\nnot: valid: yaml: at: all";
        let data = parse_pnpm_catalog_data(yaml);
        assert!(data.catalogs.is_empty());
    }

    #[test]
    fn empty_input_returns_no_catalogs() {
        let data = parse_pnpm_catalog_data("");
        assert!(data.catalogs.is_empty());
    }

    #[test]
    fn handles_object_form_entries() {
        let yaml = "catalog:\n  react:\n    specifier: ^18.2.0\n  vue: ^3.4.0\n";
        let data = parse_pnpm_catalog_data(yaml);
        assert_eq!(data.catalogs[0].entries.len(), 2);
        let names: Vec<_> = data.catalogs[0]
            .entries
            .iter()
            .map(|e| e.package_name.as_str())
            .collect();
        assert!(names.contains(&"react"));
        assert!(names.contains(&"vue"));
    }

    #[test]
    fn skips_packages_section() {
        let yaml = "packages:\n  - 'apps/*'\n  - 'libs/*'\ncatalog:\n  react: ^18\n";
        let data = parse_pnpm_catalog_data(yaml);
        assert_eq!(data.catalogs.len(), 1);
        assert_eq!(data.catalogs[0].entries[0].line, 5);
    }

    #[test]
    fn strip_inline_comment_preserves_quoted_hash() {
        assert_eq!(strip_inline_comment("foo: \"a#b\" # tail"), "foo: \"a#b\"");
        assert_eq!(strip_inline_comment("# top-level"), "");
        assert_eq!(strip_inline_comment("plain: value"), "plain: value");
    }

    #[test]
    fn parse_key_handles_simple_and_quoted() {
        assert_eq!(parse_key("react: ^18"), Some("react".to_string()));
        assert_eq!(
            parse_key("\"@scope/lib\": ^1"),
            Some("@scope/lib".to_string())
        );
        assert_eq!(parse_key("'pkg': ^2"), Some("pkg".to_string()));
        assert_eq!(parse_key("- item"), None);
        assert_eq!(parse_key(""), None);
    }
}
