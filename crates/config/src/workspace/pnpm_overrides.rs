//! Parser for the `overrides:` section of `pnpm-workspace.yaml` and the
//! `pnpm.overrides` section of a root `package.json`.
//!
//! pnpm supports forcing transitive dependency versions through two equivalent
//! locations:
//!
//! ```yaml
//! # pnpm-workspace.yaml (pnpm 9+, canonical)
//! overrides:
//!   axios: ^1.6.0
//!   "@types/react@<18": "18.0.0"
//!   "react>react-dom": ^17
//! ```
//!
//! ```json
//! // package.json (legacy form, still supported)
//! { "pnpm": { "overrides": { "axios": "^1.6.0" } } }
//! ```
//!
//! For the unused-dependency-override and misconfigured-dependency-override
//! detectors we need both the structured map of entries and the 1-based line
//! number of each entry in the source so findings can point users to the exact
//! line. `serde_yaml_ng` and `serde_json` give us the structural parse; a second
//! targeted scan over the raw source recovers the line numbers.
//!
//! The detector treats the following key shapes as valid pnpm syntax:
//! - `axios` (bare package)
//! - `@scope/pkg` (scoped package)
//! - `axios@>=1.0.0` (version selector on the overridden package)
//! - `react>react-dom` (parent matcher; override `react-dom` only inside `react`'s subtree)
//! - `react@1>zoo` (parent matcher with version selector on the parent)
//! - `@scope/parent>@scope/child` (scoped packages on both sides)
//!
//! Special values that are valid pnpm syntax and must NOT be flagged as
//! misconfigured: `-` (removal), `$ref` (self-reference to a workspace dep),
//! `npm:alias@^1` (npm-protocol alias).

use std::path::Path;

use super::pnpm_catalog::{parse_key, strip_inline_comment};

/// Where an override entry was declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverrideSource {
    /// Top-level `overrides:` in `pnpm-workspace.yaml`.
    PnpmWorkspaceYaml,
    /// `pnpm.overrides` in a root `package.json`.
    PnpmPackageJson,
}

/// Structured override data extracted from one source.
#[derive(Debug, Clone, Default)]
pub struct PnpmOverrideData {
    /// Entries declared in source order.
    pub entries: Vec<PnpmOverrideEntry>,
}

/// A single override entry.
#[derive(Debug, Clone)]
pub struct PnpmOverrideEntry {
    /// The full original key as written in the source (e.g.
    /// `"react>react-dom"`, `"@types/react@<18"`). Preserved for round-trip
    /// reporting so agents see the unmodified spelling.
    pub raw_key: String,
    /// Parsed structure of the key. `None` when the key cannot be parsed into
    /// a pnpm-recognised shape; in that case the entry is reported as
    /// misconfigured rather than checked for usage.
    pub parsed_key: Option<ParsedOverrideKey>,
    /// The right-hand side of the entry (the version pnpm should force).
    /// `None` when the value is missing or unparsable.
    pub raw_value: Option<String>,
    /// 1-based line number of the entry within the source file.
    pub line: u32,
}

/// Parsed structure of an override key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedOverrideKey {
    /// Optional parent package (left side of `>`). `None` for bare-target keys.
    pub parent_package: Option<String>,
    /// Optional version selector on the parent (e.g. `react@1>zoo` has
    /// `parent_version_selector = Some("1")`).
    pub parent_version_selector: Option<String>,
    /// The target package name (the entry pnpm rewrites).
    pub target_package: String,
    /// Optional version selector on the target (e.g. `@types/react@<18` has
    /// `target_version_selector = Some("<18")`).
    pub target_version_selector: Option<String>,
}

/// Parse the `overrides:` section of `pnpm-workspace.yaml`. Returns an empty
/// `PnpmOverrideData` when the file has no overrides, when the YAML is
/// malformed, or when the section is present but empty.
#[must_use]
pub fn parse_pnpm_workspace_overrides(source: &str) -> PnpmOverrideData {
    let value: serde_yaml_ng::Value = match serde_yaml_ng::from_str(source) {
        Ok(v) => v,
        Err(_) => return PnpmOverrideData::default(),
    };
    let Some(mapping) = value.as_mapping() else {
        return PnpmOverrideData::default();
    };
    let Some(overrides_value) = mapping.get("overrides") else {
        return PnpmOverrideData::default();
    };
    let Some(overrides_map) = overrides_value.as_mapping() else {
        return PnpmOverrideData::default();
    };

    let line_index = build_yaml_line_index(source);
    let entries = overrides_map
        .iter()
        .filter_map(|(k, v)| {
            let raw_key = k.as_str()?.to_string();
            let raw_value = match v {
                serde_yaml_ng::Value::String(s) => Some(s.clone()),
                serde_yaml_ng::Value::Null => None,
                other => Some(yaml_value_to_string(other)),
            };
            let line = line_index.line_for(&raw_key)?;
            let parsed_key = parse_override_key(&raw_key);
            Some(PnpmOverrideEntry {
                raw_key,
                parsed_key,
                raw_value,
                line,
            })
        })
        .collect();

    PnpmOverrideData { entries }
}

/// Parse the `pnpm.overrides` section of a root `package.json`. Returns an
/// empty `PnpmOverrideData` when the file has no overrides, when the JSON is
/// malformed, or when the section is present but empty.
#[must_use]
pub fn parse_pnpm_package_json_overrides(source: &str) -> PnpmOverrideData {
    let value: serde_json::Value = match serde_json::from_str(source) {
        Ok(v) => v,
        Err(_) => return PnpmOverrideData::default(),
    };
    let Some(overrides) = value.get("pnpm").and_then(|p| p.get("overrides")) else {
        return PnpmOverrideData::default();
    };
    let Some(overrides_obj) = overrides.as_object() else {
        return PnpmOverrideData::default();
    };

    let line_index = build_package_json_line_index(source);
    let entries = overrides_obj
        .iter()
        .filter_map(|(raw_key, v)| {
            let raw_value = match v {
                serde_json::Value::String(s) => Some(s.clone()),
                serde_json::Value::Null => None,
                other => Some(other.to_string()),
            };
            let line = line_index.line_for(raw_key)?;
            let parsed_key = parse_override_key(raw_key);
            Some(PnpmOverrideEntry {
                raw_key: raw_key.clone(),
                parsed_key,
                raw_value,
                line,
            })
        })
        .collect();

    PnpmOverrideData { entries }
}

/// Parse an override key into `parent`, `target`, and optional version
/// selectors. Returns `None` when the key cannot be split into a recognised
/// shape (empty key, parent or target missing).
#[must_use]
pub fn parse_override_key(key: &str) -> Option<ParsedOverrideKey> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return None;
    }

    // Split on the last `>`. pnpm parses single-depth parent matchers; multi-hop
    // `a>b>c` is not officially documented but the resolver treats the rightmost
    // segment as the target and everything left as the parent chain. We split
    // on the LAST `>` so the parent side keeps any earlier `>` for future
    // multi-hop support.
    let (parent_part, target_part) = if let Some(idx) = trimmed.rfind('>') {
        (Some(trimmed[..idx].trim()), trimmed[idx + 1..].trim())
    } else {
        (None, trimmed)
    };

    let (target_package, target_version_selector) = split_pkg_and_selector(target_part)?;

    let (parent_package, parent_version_selector) = match parent_part {
        Some(parent) if !parent.is_empty() => {
            let (pkg, selector) = split_pkg_and_selector(parent)?;
            (Some(pkg), selector)
        }
        // `>target` (leading separator with empty parent) is malformed: the
        // user clearly intended a parent chain but left the parent slot blank.
        Some(_) => return None,
        None => (None, None),
    };

    Some(ParsedOverrideKey {
        parent_package,
        parent_version_selector,
        target_package,
        target_version_selector,
    })
}

/// Split a `pkg@selector` segment into `(package_name, Option<selector>)`.
/// Handles scoped packages (`@scope/name@<2`) by skipping the leading `@`.
/// Returns `None` when the package name is empty.
fn split_pkg_and_selector(segment: &str) -> Option<(String, Option<String>)> {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return None;
    }

    let bytes = trimmed.as_bytes();
    let scoped = bytes.first().copied() == Some(b'@');
    let start = usize::from(scoped);
    let at_pos = trimmed[start..].find('@').map(|i| i + start);

    let (pkg, selector) = match at_pos {
        Some(pos) => (
            trimmed[..pos].to_string(),
            Some(trimmed[pos + 1..].to_string()),
        ),
        None => (trimmed.to_string(), None),
    };

    if pkg.is_empty() {
        return None;
    }
    Some((pkg, selector))
}

/// Check whether `value` is a valid pnpm override right-hand side, even if it
/// is not a semver range. Returns `false` when the value is empty, contains a
/// raw newline, or is otherwise garbage.
#[must_use]
pub fn is_valid_override_value(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.contains('\n') {
        return false;
    }
    // pnpm accepts: semver ranges, `-` (removal), `$ref` (self-ref),
    // `npm:alias@^1` (alias), `workspace:*`. We do not validate semver ranges
    // here, only screen for obviously broken inputs.
    true
}

/// Convenience: is this entry effectively a misconfiguration the user should
/// see as an error?
#[must_use]
pub fn override_misconfig_reason(entry: &PnpmOverrideEntry) -> Option<MisconfigReason> {
    if entry.parsed_key.is_none() {
        return Some(MisconfigReason::UnparsableKey);
    }
    match &entry.raw_value {
        None => Some(MisconfigReason::EmptyValue),
        Some(v) if !is_valid_override_value(v) => Some(MisconfigReason::EmptyValue),
        _ => None,
    }
}

/// Why an override entry is misconfigured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MisconfigReason {
    /// The override key cannot be parsed into a recognised pnpm shape.
    UnparsableKey,
    /// The override value is missing or empty.
    EmptyValue,
}

impl MisconfigReason {
    /// Human-readable description.
    #[must_use]
    pub const fn describe(self) -> &'static str {
        match self {
            Self::UnparsableKey => "override key cannot be parsed",
            Self::EmptyValue => "override value is missing or empty",
        }
    }
}

struct YamlLineIndex {
    entries: Vec<(String, u32)>,
}

impl YamlLineIndex {
    fn line_for(&self, key: &str) -> Option<u32> {
        self.entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, line)| *line)
    }
}

/// Walk the raw YAML source to map each `overrides:` entry key to its 1-based
/// line number. Mirrors the catalog parser's section-aware scanner.
fn build_yaml_line_index(source: &str) -> YamlLineIndex {
    let mut entries = Vec::new();
    let mut in_overrides = false;

    for (idx, raw_line) in source.lines().enumerate() {
        let line_no = u32::try_from(idx).unwrap_or(u32::MAX).saturating_add(1);
        let trimmed = strip_inline_comment(raw_line);
        let trimmed_left = trimmed.trim_start();
        let indent = trimmed.len() - trimmed_left.len();

        if trimmed_left.is_empty() {
            continue;
        }

        if indent == 0 {
            in_overrides = trimmed_left.starts_with("overrides:");
            continue;
        }

        if in_overrides && let Some(key) = parse_key(trimmed_left) {
            entries.push((key, line_no));
        }
    }

    YamlLineIndex { entries }
}

/// Walk a raw `package.json` source string to map each `pnpm.overrides` entry
/// key to its 1-based line number. The scan tracks brace depth so nested
/// objects under unrelated keys (e.g., `dependenciesMeta`) cannot be misread
/// as override entries.
fn build_package_json_line_index(source: &str) -> YamlLineIndex {
    let mut entries = Vec::new();
    let mut depth: i32 = 0;
    let mut pnpm_depth: Option<i32> = None;
    let mut in_overrides_depth: Option<i32> = None;
    let mut in_string = false;
    let mut escape = false;
    let mut current_line = 1u32;
    let mut last_key: Option<String> = None;
    let mut key_buf = String::new();
    let mut collecting_key = false;

    for ch in source.chars() {
        if ch == '\n' {
            current_line += 1;
        }

        if in_string {
            if escape {
                if collecting_key {
                    key_buf.push(ch);
                }
                escape = false;
                continue;
            }
            if ch == '\\' {
                escape = true;
                if collecting_key {
                    key_buf.push(ch);
                }
                continue;
            }
            if ch == '"' {
                in_string = false;
                if collecting_key {
                    last_key = Some(std::mem::take(&mut key_buf));
                    collecting_key = false;
                }
                continue;
            }
            if collecting_key {
                key_buf.push(ch);
            }
            continue;
        }

        match ch {
            '"' => {
                in_string = true;
                // Start collecting a new key candidate. We commit it only when
                // followed by `:` at the appropriate depth.
                collecting_key = true;
                key_buf.clear();
            }
            '{' => depth += 1,
            '}' => {
                if Some(depth) == in_overrides_depth {
                    in_overrides_depth = None;
                }
                if Some(depth) == pnpm_depth {
                    pnpm_depth = None;
                }
                depth -= 1;
            }
            ':' => {
                if let Some(key) = last_key.take() {
                    // Promote into a section if the key opens an object
                    // immediately. We track section transitions by matching
                    // the key name + current depth.
                    if pnpm_depth.is_none() && depth == 1 && key == "pnpm" {
                        pnpm_depth = Some(depth);
                    } else if in_overrides_depth.is_none()
                        && pnpm_depth.is_some()
                        && depth == pnpm_depth.unwrap_or(0) + 1
                        && key == "overrides"
                    {
                        in_overrides_depth = Some(depth);
                    } else if let Some(d) = in_overrides_depth
                        && depth == d + 1
                    {
                        // This is an override entry key at the right depth.
                        entries.push((key, current_line));
                    }
                }
            }
            ',' => {
                last_key = None;
            }
            _ => {}
        }
    }

    YamlLineIndex { entries }
}

fn yaml_value_to_string(value: &serde_yaml_ng::Value) -> String {
    match value {
        serde_yaml_ng::Value::String(s) => s.clone(),
        serde_yaml_ng::Value::Number(n) => n.to_string(),
        serde_yaml_ng::Value::Bool(b) => b.to_string(),
        serde_yaml_ng::Value::Null => String::new(),
        _ => serde_yaml_ng::to_string(value).unwrap_or_default(),
    }
}

/// Source-name string for diagnostics.
#[must_use]
pub fn override_source_label(source: OverrideSource, path: &Path) -> String {
    match source {
        OverrideSource::PnpmWorkspaceYaml => "pnpm-workspace.yaml".to_string(),
        OverrideSource::PnpmPackageJson => path.display().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_bare_target() {
        let parsed = parse_override_key("axios").unwrap();
        assert_eq!(parsed.target_package, "axios");
        assert!(parsed.parent_package.is_none());
        assert!(parsed.target_version_selector.is_none());
    }

    #[test]
    fn parse_scoped_target() {
        let parsed = parse_override_key("@types/react").unwrap();
        assert_eq!(parsed.target_package, "@types/react");
        assert!(parsed.target_version_selector.is_none());
    }

    #[test]
    fn parse_target_with_version_selector() {
        let parsed = parse_override_key("@types/react@<18").unwrap();
        assert_eq!(parsed.target_package, "@types/react");
        assert_eq!(parsed.target_version_selector.as_deref(), Some("<18"));
    }

    #[test]
    fn parse_parent_chain() {
        let parsed = parse_override_key("react>react-dom").unwrap();
        assert_eq!(parsed.parent_package.as_deref(), Some("react"));
        assert_eq!(parsed.target_package, "react-dom");
    }

    #[test]
    fn parse_parent_chain_with_selectors() {
        let parsed = parse_override_key("react@1>zoo").unwrap();
        assert_eq!(parsed.parent_package.as_deref(), Some("react"));
        assert_eq!(parsed.parent_version_selector.as_deref(), Some("1"));
        assert_eq!(parsed.target_package, "zoo");
    }

    #[test]
    fn parse_scoped_parent_and_target() {
        let parsed = parse_override_key("@react-spring/web>@react-spring/core").unwrap();
        assert_eq!(parsed.parent_package.as_deref(), Some("@react-spring/web"));
        assert_eq!(parsed.target_package, "@react-spring/core");
    }

    #[test]
    fn parse_empty_returns_none() {
        assert!(parse_override_key("").is_none());
        assert!(parse_override_key("   ").is_none());
    }

    #[test]
    fn parse_dangling_separator_returns_none() {
        assert!(parse_override_key("react>").is_none());
        assert!(parse_override_key(">react-dom").is_none());
    }

    #[test]
    fn is_valid_override_value_accepts_pnpm_idioms() {
        assert!(is_valid_override_value("^1.6.0"));
        assert!(is_valid_override_value("-"));
        assert!(is_valid_override_value("$foo"));
        assert!(is_valid_override_value("npm:@scope/alias@^1.0.0"));
        assert!(is_valid_override_value("workspace:*"));
    }

    #[test]
    fn is_valid_override_value_rejects_empty_and_newline() {
        assert!(!is_valid_override_value(""));
        assert!(!is_valid_override_value("   "));
        assert!(!is_valid_override_value("^1\n^2"));
    }

    #[test]
    fn parses_workspace_yaml_overrides() {
        let yaml = "packages:\n  - 'packages/*'\n\noverrides:\n  axios: ^1.6.0\n  \"@types/react@<18\": '18.0.0'\n  \"react>react-dom\": ^17\n";
        let data = parse_pnpm_workspace_overrides(yaml);
        assert_eq!(data.entries.len(), 3);
        assert_eq!(data.entries[0].raw_key, "axios");
        assert_eq!(data.entries[0].line, 5);
        assert_eq!(data.entries[0].raw_value.as_deref(), Some("^1.6.0"));

        assert_eq!(data.entries[1].raw_key, "@types/react@<18");
        assert_eq!(data.entries[1].line, 6);
        assert_eq!(data.entries[1].raw_value.as_deref(), Some("18.0.0"));
        assert_eq!(
            data.entries[1]
                .parsed_key
                .as_ref()
                .and_then(|p| p.target_version_selector.as_deref()),
            Some("<18")
        );

        assert_eq!(data.entries[2].raw_key, "react>react-dom");
        assert_eq!(data.entries[2].line, 7);
        assert_eq!(
            data.entries[2]
                .parsed_key
                .as_ref()
                .map(|p| p.target_package.as_str()),
            Some("react-dom")
        );
    }

    #[test]
    fn parses_package_json_overrides() {
        let json = r#"{
  "name": "root",
  "pnpm": {
    "overrides": {
      "axios": "^1.6.0",
      "react>react-dom": "^17"
    }
  },
  "dependenciesMeta": {
    "shouldNotMatch": { "injected": true }
  }
}"#;
        let data = parse_pnpm_package_json_overrides(json);
        assert_eq!(data.entries.len(), 2);
        assert_eq!(data.entries[0].raw_key, "axios");
        assert_eq!(data.entries[0].raw_value.as_deref(), Some("^1.6.0"));
        assert_eq!(data.entries[0].line, 5);
        assert_eq!(data.entries[1].raw_key, "react>react-dom");
        assert_eq!(data.entries[1].line, 6);
    }

    #[test]
    fn empty_workspace_overrides_returns_no_entries() {
        let data = parse_pnpm_workspace_overrides("overrides: {}\n");
        assert!(data.entries.is_empty());
    }

    #[test]
    fn malformed_yaml_returns_no_entries() {
        let data = parse_pnpm_workspace_overrides("{this is\nnot: valid: yaml");
        assert!(data.entries.is_empty());
    }

    #[test]
    fn package_json_without_pnpm_overrides_returns_no_entries() {
        let data = parse_pnpm_package_json_overrides(r#"{"dependencies": {"axios": "^1"}}"#);
        assert!(data.entries.is_empty());
    }

    #[test]
    fn malformed_json_returns_no_entries() {
        let data = parse_pnpm_package_json_overrides("{not valid json");
        assert!(data.entries.is_empty());
    }

    #[test]
    fn unparsable_key_carries_misconfig_signal() {
        let yaml = "overrides:\n  \">@bad-key>\": ^1.0.0\n";
        let data = parse_pnpm_workspace_overrides(yaml);
        assert_eq!(data.entries.len(), 1);
        assert!(data.entries[0].parsed_key.is_none());
        assert_eq!(
            override_misconfig_reason(&data.entries[0]),
            Some(MisconfigReason::UnparsableKey)
        );
    }
}
