//! Parser for the top-level `overrides` section of a root `package.json` as
//! used by npm.
//!
//! npm supports forcing transitive dependency versions through a top-level
//! `overrides` object in the root `package.json`:
//!
//! ```json
//! {
//!   "overrides": {
//!     "sanitize-html": ">=2.17.4",
//!     "typescript": "$typescript",
//!     "lit-markdown": {
//!       ".": "^1.0.0",
//!       "sanitize-html": ">=2.17.4"
//!     }
//!   }
//! }
//! ```
//!
//! Unlike pnpm's flat `parent>child` keys, npm expresses parent scoping by
//! nesting objects: a child key inside a parent's object only applies within
//! that parent's dependency subtree. The special `"."` key pins the version of
//! the enclosing parent itself. The `"$package"` value form references the
//! version declared for `package` in the root dependencies.
//!
//! The parser flattens nested objects into the same [`PnpmOverrideEntry`]
//! shape the pnpm parser produces (joining nesting levels with `>`), so the
//! unused-override and misconfigured-override detectors can analyze npm and
//! pnpm overrides through one code path.
//!
//! bun reads the same top-level `overrides` object and, as a Yarn migration
//! aid, a top-level `resolutions` object with flat string entries whose keys
//! may use yarn's dependency-path spelling (`parent/child`, `**/child`).
//! [`parse_bun_package_json_resolutions`] maps that dialect onto the shared
//! entry shape so bun repositories get the same two findings for
//! `resolutions` (issue #2367).

use super::pnpm_overrides::{
    ParsedOverrideKey, PnpmOverrideData, PnpmOverrideEntry, parse_override_key,
    split_pkg_and_selector,
};

const OVERRIDES_KEY: &str = "overrides";
const RESOLUTIONS_KEY: &str = "resolutions";
const YARN_GLOB_SEGMENT: &str = "**";
const YARN_COMMENT_KEY_PREFIX: &str = "//";

/// Parse the top-level `overrides` section of a root `package.json`. Returns
/// an empty `PnpmOverrideData` when the file has no overrides, when the JSON
/// is malformed, or when the section is present but empty.
#[must_use]
pub fn parse_npm_package_json_overrides(source: &str) -> PnpmOverrideData {
    let value: serde_json::Value = match serde_json::from_str(source) {
        Ok(v) => v,
        Err(_) => return PnpmOverrideData::default(),
    };
    let Some(overrides) = value.get(OVERRIDES_KEY).and_then(|o| o.as_object()) else {
        return PnpmOverrideData::default();
    };

    let mut line_index = NpmOverridesLineIndex::build(source, OVERRIDES_KEY, true);
    let mut entries = Vec::new();
    let mut path: Vec<String> = Vec::new();
    flatten_overrides(overrides, &mut path, &mut line_index, &mut entries);
    PnpmOverrideData { entries }
}

/// Parse the Yarn-style top-level `resolutions` object of a root
/// `package.json` the way bun reads it (issue #2367). bun treats
/// `resolutions` as an alias of `overrides` with flat string entries: a key
/// is a bare package (`left-pad`, `@scope/pkg`, `pkg@<2`), a yarn dependency
/// path (`parent/child`, `**/child`, `parent/**/child`, where `@scope/name`
/// spans two path segments), or the pnpm `parent>child` form. Paths deeper
/// than one parent and non-string values are kept as entries without a
/// parsed key or value so the misconfigured-override detector reports them,
/// matching the warning bun prints before skipping such entries. Keys that
/// start with `//` are comments and skipped. Returns empty data when the
/// file has no `resolutions` object or the JSON is malformed. Callers decide
/// whether the repository installs with bun and whether an `overrides` key
/// shadows the section.
#[must_use]
pub fn parse_bun_package_json_resolutions(source: &str) -> PnpmOverrideData {
    let value: serde_json::Value = match serde_json::from_str(source) {
        Ok(v) => v,
        Err(_) => return PnpmOverrideData::default(),
    };
    let Some(resolutions) = value.get(RESOLUTIONS_KEY).and_then(|o| o.as_object()) else {
        return PnpmOverrideData::default();
    };

    let mut line_index = NpmOverridesLineIndex::build(source, RESOLUTIONS_KEY, false);
    let mut entries = Vec::with_capacity(resolutions.len());
    for (key, value) in resolutions {
        let line = line_index.next_line_for(key);
        if key.starts_with(YARN_COMMENT_KEY_PREFIX) {
            continue;
        }
        let raw_value = match value {
            serde_json::Value::String(s) => Some(s.clone()),
            _ => None,
        };
        let Some(line) = line else {
            continue;
        };
        entries.push(PnpmOverrideEntry {
            raw_key: key.clone(),
            parsed_key: parse_resolution_key(key),
            raw_value,
            line,
        });
    }
    PnpmOverrideData { entries }
}

/// Parse a `resolutions` key into the shared override key shape. The pnpm
/// `parent>child` spelling is delegated to the pnpm key parser; everything
/// else is a yarn dependency path where `**` segments match any depth,
/// `@scope/name` spans two segments, and at most one parent precedes the
/// target. Returns `None` for the shapes bun rejects: an empty segment, a
/// bare scope, a trailing `**`, or more than one parent level.
fn parse_resolution_key(key: &str) -> Option<ParsedOverrideKey> {
    let trimmed = key.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(delimiter) = pnpm_delimiter(trimmed) {
        if pnpm_delimiter(&trimmed[delimiter + 1..]).is_some() {
            return None;
        }
        return parse_override_key(trimmed);
    }

    let mut segments: Vec<String> = Vec::with_capacity(2);
    let mut tokens = trimmed.split('/').peekable();
    while let Some(token) = tokens.next() {
        if token.is_empty() {
            return None;
        }
        if token == YARN_GLOB_SEGMENT {
            tokens.peek()?;
            continue;
        }
        let segment = if token.starts_with('@') {
            let name = tokens.next().filter(|name| !name.is_empty())?;
            format!("{token}/{name}")
        } else {
            token.to_string()
        };
        if segments.len() == 2 {
            return None;
        }
        segments.push(segment);
    }

    let (parent, target) = match segments.as_slice() {
        [target] => (None, target),
        [parent, target] => (Some(parent), target),
        _ => return None,
    };
    let (target_package, target_version_selector) = split_pkg_and_selector(target)?;
    let (parent_package, parent_version_selector) = match parent {
        Some(parent) => {
            let (package, selector) = split_pkg_and_selector(parent)?;
            (Some(package), selector)
        }
        None => (None, None),
    };
    Some(ParsedOverrideKey {
        parent_package,
        parent_version_selector,
        target_package,
        target_version_selector,
    })
}

/// Byte offset of the pnpm `parent>child` delimiter as bun recognises it:
/// the first `>` past the first byte that is not preceded by a space, `|`,
/// or `@`, so `pkg@>=1` keeps its version selector intact.
fn pnpm_delimiter(key: &str) -> Option<usize> {
    let bytes = key.as_bytes();
    bytes.iter().enumerate().skip(1).find_map(|(index, byte)| {
        (*byte == b'>' && !matches!(bytes[index - 1], b' ' | b'|' | b'@')).then_some(index)
    })
}

/// Depth-first flattening of the (possibly nested) overrides object into flat
/// entries. `serde_json` is built with `preserve_order`, so map iteration
/// follows source order and stays aligned with the line index cursor.
fn flatten_overrides(
    object: &serde_json::Map<String, serde_json::Value>,
    path: &mut Vec<String>,
    line_index: &mut NpmOverridesLineIndex,
    entries: &mut Vec<PnpmOverrideEntry>,
) {
    for (key, value) in object {
        let line = line_index.next_line_for(key);
        if let serde_json::Value::Object(child) = value {
            path.push(key.clone());
            flatten_overrides(child, path, line_index, entries);
            path.pop();
            continue;
        }

        let raw_value = match value {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Null => None,
            other => Some(other.to_string()),
        };
        let Some(line) = line else {
            continue;
        };
        let (raw_key, parsed_key) = build_npm_key(path, key);
        entries.push(PnpmOverrideEntry {
            raw_key,
            parsed_key,
            raw_value,
            line,
        });
    }
}

/// Build the flattened key and parsed structure for one npm override entry.
///
/// `path` holds the enclosing parent keys (outermost first); `key` is the
/// entry's own key. The npm `"."` key targets the enclosing parent itself, so
/// it is dropped from the effective segment list. The flattened `raw_key`
/// joins the effective segments with `>` to mirror pnpm's spelling in
/// reports and suppression rules.
fn build_npm_key(path: &[String], key: &str) -> (String, Option<ParsedOverrideKey>) {
    let mut segments: Vec<&str> = path.iter().map(String::as_str).collect();
    if key == "." {
        if segments.is_empty() {
            return (key.to_string(), None);
        }
    } else {
        segments.push(key);
    }

    let raw_key = segments.join(">");
    let Some(target_segment) = segments.last() else {
        return (raw_key, None);
    };
    let Some((target_package, target_version_selector)) = split_pkg_and_selector(target_segment)
    else {
        return (raw_key, None);
    };

    let (parent_package, parent_version_selector) = if segments.len() >= 2 {
        // Credit against the outermost parent: that is the direct dependency
        // the user declares in package.json.
        match split_pkg_and_selector(segments[0]) {
            Some((pkg, selector)) => (Some(pkg), selector),
            None => return (raw_key, None),
        }
    } else {
        (None, None)
    };

    (
        raw_key,
        Some(ParsedOverrideKey {
            parent_package,
            parent_version_selector,
            target_package,
            target_version_selector,
        }),
    )
}

/// Ordered `(key, line)` pairs for every key inside one top-level section
/// object (`overrides` or `resolutions`), consumed by a forward-only cursor
/// during flattening.
struct NpmOverridesLineIndex {
    entries: Vec<(String, u32)>,
    cursor: usize,
}

impl NpmOverridesLineIndex {
    /// Return the line of the next recorded key matching `key`, scanning
    /// forward from the cursor so repeated child names under different
    /// parents resolve to their own occurrence.
    fn next_line_for(&mut self, key: &str) -> Option<u32> {
        let found = self.entries[self.cursor..]
            .iter()
            .position(|(k, _)| k == key)?;
        let index = self.cursor + found;
        self.cursor = index + 1;
        Some(self.entries[index].1)
    }

    /// Walk the raw source with a brace-depth scanner and record every key
    /// inside the top-level `section` object, paired with its 1-based line
    /// number. With `nested` set, keys at any depth inside the section are
    /// recorded (npm scopes overrides by nesting); otherwise only the
    /// section's direct keys are, so a nested object value bun rejects cannot
    /// shift the cursor onto a later key of the same name.
    fn build(source: &str, section: &'static str, nested: bool) -> Self {
        let mut scan = NpmOverridesJsonScan::new(section, nested);
        let mut current_line = 1u32;

        for ch in source.chars() {
            if ch == '\n' {
                current_line += 1;
            }

            if scan.in_string {
                scan.consume_in_string_char(ch);
            } else {
                scan.consume_structural_char(ch, current_line);
            }
        }

        Self {
            entries: scan.entries,
            cursor: 0,
        }
    }
}

/// Char-by-char brace-depth scanner state for a top-level section line
/// index. Mirrors the pnpm `pnpm.overrides` scanner, but can record keys at
/// every nesting depth inside the section object because npm scopes
/// overrides by nesting rather than by `parent>child` keys.
struct NpmOverridesJsonScan {
    section: &'static str,
    nested: bool,
    entries: Vec<(String, u32)>,
    depth: i32,
    section_depth: Option<i32>,
    in_string: bool,
    escape: bool,
    last_key: Option<String>,
    key_buf: String,
    collecting_key: bool,
}

impl NpmOverridesJsonScan {
    fn new(section: &'static str, nested: bool) -> Self {
        Self {
            section,
            nested,
            entries: Vec::new(),
            depth: 0,
            section_depth: None,
            in_string: false,
            escape: false,
            last_key: None,
            key_buf: String::new(),
            collecting_key: false,
        }
    }

    fn consume_in_string_char(&mut self, ch: char) {
        if self.escape {
            if self.collecting_key {
                self.key_buf.push(ch);
            }
            self.escape = false;
            return;
        }
        if ch == '\\' {
            self.escape = true;
            if self.collecting_key {
                self.key_buf.push(ch);
            }
            return;
        }
        if ch == '"' {
            self.in_string = false;
            if self.collecting_key {
                let raw_key = std::mem::take(&mut self.key_buf);
                let quoted = format!("\"{raw_key}\"");
                self.last_key = serde_json::from_str(&quoted).ok().or(Some(raw_key));
                self.collecting_key = false;
            }
            return;
        }
        if self.collecting_key {
            self.key_buf.push(ch);
        }
    }

    fn consume_structural_char(&mut self, ch: char, current_line: u32) {
        match ch {
            '"' => {
                self.in_string = true;
                self.collecting_key = true;
                self.key_buf.clear();
            }
            '{' => self.depth += 1,
            '}' => {
                // The section object closes when depth returns to the level
                // where its key was seen.
                if self.section_depth == Some(self.depth - 1) {
                    self.section_depth = None;
                }
                self.depth -= 1;
            }
            ':' => self.record_key_after_colon(current_line),
            ',' => {
                self.last_key = None;
            }
            _ => {}
        }
    }

    fn record_key_after_colon(&mut self, current_line: u32) {
        let Some(key) = self.last_key.take() else {
            return;
        };
        if self.section_depth.is_none() && self.depth == 1 && key == self.section {
            self.section_depth = Some(self.depth);
        } else if let Some(d) = self.section_depth
            && self.depth > d
            && (self.nested || self.depth == d + 1)
        {
            self.entries.push((key, current_line));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_level_simple_override() {
        let json = r#"{
  "name": "root",
  "overrides": {
    "sanitize-html": ">=2.17.4"
  }
}"#;
        let data = parse_npm_package_json_overrides(json);
        assert_eq!(data.entries.len(), 1);
        assert_eq!(data.entries[0].raw_key, "sanitize-html");
        assert_eq!(data.entries[0].raw_value.as_deref(), Some(">=2.17.4"));
        assert_eq!(data.entries[0].line, 4);
        let parsed = data.entries[0].parsed_key.as_ref().unwrap();
        assert_eq!(parsed.target_package, "sanitize-html");
        assert!(parsed.parent_package.is_none());
    }

    #[test]
    fn flattens_nested_overrides_with_parent() {
        let json = r#"{
  "overrides": {
    "lit-markdown": {
      ".": "^1.0.0",
      "sanitize-html": ">=2.17.4"
    }
  }
}"#;
        let data = parse_npm_package_json_overrides(json);
        assert_eq!(data.entries.len(), 2);

        assert_eq!(data.entries[0].raw_key, "lit-markdown");
        assert_eq!(data.entries[0].line, 4);
        let dot = data.entries[0].parsed_key.as_ref().unwrap();
        assert_eq!(dot.target_package, "lit-markdown");
        assert!(dot.parent_package.is_none());

        assert_eq!(data.entries[1].raw_key, "lit-markdown>sanitize-html");
        assert_eq!(data.entries[1].line, 5);
        let nested = data.entries[1].parsed_key.as_ref().unwrap();
        assert_eq!(nested.target_package, "sanitize-html");
        assert_eq!(nested.parent_package.as_deref(), Some("lit-markdown"));
    }

    #[test]
    fn deep_nesting_credits_outermost_parent() {
        let json = r#"{
  "overrides": {
    "a": {
      "b": {
        "c": "^1.0.0"
      }
    }
  }
}"#;
        let data = parse_npm_package_json_overrides(json);
        assert_eq!(data.entries.len(), 1);
        assert_eq!(data.entries[0].raw_key, "a>b>c");
        assert_eq!(data.entries[0].line, 5);
        let parsed = data.entries[0].parsed_key.as_ref().unwrap();
        assert_eq!(parsed.target_package, "c");
        assert_eq!(parsed.parent_package.as_deref(), Some("a"));
    }

    #[test]
    fn version_selector_on_key_is_parsed() {
        let json = r#"{"overrides": {"@types/react@<18": "18.0.0"}}"#;
        let data = parse_npm_package_json_overrides(json);
        assert_eq!(data.entries.len(), 1);
        let parsed = data.entries[0].parsed_key.as_ref().unwrap();
        assert_eq!(parsed.target_package, "@types/react");
        assert_eq!(parsed.target_version_selector.as_deref(), Some("<18"));
    }

    #[test]
    fn escaped_json_override_key_keeps_its_source_line() {
        let json = r#"{
  "overrides": {
    "left\u002dpad": "^1.3.0"
  }
}"#;
        let data = parse_npm_package_json_overrides(json);
        assert_eq!(data.entries.len(), 1);
        assert_eq!(data.entries[0].raw_key, "left-pad");
        assert_eq!(data.entries[0].line, 3);
    }

    #[test]
    fn dollar_reference_value_is_preserved() {
        let json = r#"{"overrides": {"typescript": "$typescript"}}"#;
        let data = parse_npm_package_json_overrides(json);
        assert_eq!(data.entries.len(), 1);
        assert_eq!(data.entries[0].raw_value.as_deref(), Some("$typescript"));
    }

    #[test]
    fn repeated_child_names_get_distinct_lines() {
        let json = r#"{
  "overrides": {
    "parent-one": {
      "shared-child": "^1.0.0"
    },
    "parent-two": {
      "shared-child": "^2.0.0"
    }
  }
}"#;
        let data = parse_npm_package_json_overrides(json);
        assert_eq!(data.entries.len(), 2);
        assert_eq!(data.entries[0].raw_key, "parent-one>shared-child");
        assert_eq!(data.entries[0].line, 4);
        assert_eq!(data.entries[1].raw_key, "parent-two>shared-child");
        assert_eq!(data.entries[1].line, 7);
    }

    #[test]
    fn nested_overrides_key_deeper_in_document_is_ignored() {
        let json = r#"{
  "config": {
    "overrides": {
      "not-an-override": "^1.0.0"
    }
  }
}"#;
        let data = parse_npm_package_json_overrides(json);
        assert!(data.entries.is_empty());
    }

    #[test]
    fn top_level_dot_key_is_unparsable() {
        let json = r#"{"overrides": {".": "^1.0.0"}}"#;
        let data = parse_npm_package_json_overrides(json);
        assert_eq!(data.entries.len(), 1);
        assert!(data.entries[0].parsed_key.is_none());
    }

    #[test]
    fn package_json_without_overrides_returns_no_entries() {
        let data = parse_npm_package_json_overrides(r#"{"dependencies": {"axios": "^1"}}"#);
        assert!(data.entries.is_empty());
    }

    #[test]
    fn pnpm_overrides_section_is_not_picked_up() {
        let data =
            parse_npm_package_json_overrides(r#"{"pnpm": {"overrides": {"axios": "^1.6.0"}}}"#);
        assert!(data.entries.is_empty());
    }

    #[test]
    fn malformed_json_returns_no_entries() {
        let data = parse_npm_package_json_overrides("{not valid json");
        assert!(data.entries.is_empty());
    }

    // Issue #2367: bun reads Yarn-style `resolutions` as an `overrides` alias.

    fn parsed(entry: &PnpmOverrideEntry) -> &ParsedOverrideKey {
        entry
            .parsed_key
            .as_ref()
            .unwrap_or_else(|| panic!("{} should parse", entry.raw_key))
    }

    #[test]
    fn resolutions_flat_entries_parse_with_lines() {
        let json = r#"{
  "name": "root",
  "resolutions": {
    "ws": "^8.21.0",
    "left-pad": "^1.3.0"
  }
}"#;
        let data = parse_bun_package_json_resolutions(json);
        assert_eq!(data.entries.len(), 2);
        assert_eq!(data.entries[0].raw_key, "ws");
        assert_eq!(data.entries[0].line, 4);
        assert_eq!(data.entries[1].raw_key, "left-pad");
        assert_eq!(data.entries[1].raw_value.as_deref(), Some("^1.3.0"));
        assert_eq!(data.entries[1].line, 5);
        let left_pad = parsed(&data.entries[1]);
        assert_eq!(left_pad.target_package, "left-pad");
        assert!(left_pad.parent_package.is_none());
    }

    #[test]
    fn resolutions_yarn_path_keys_map_to_parent_and_target() {
        let json = r#"{
  "resolutions": {
    "**/left-pad": "^1.3.0",
    "react/left-pad": "^1.3.0",
    "react/**/left-pad": "^1.3.0",
    "@scope/parent/left-pad": "^1.3.0",
    "@scope/parent/@scope/child": "^1.3.0",
    "react@^18/left-pad": "^1.3.0",
    "@types/react@<18": "18.0.0"
  }
}"#;
        let data = parse_bun_package_json_resolutions(json);
        assert_eq!(data.entries.len(), 7);

        let glob = parsed(&data.entries[0]);
        assert_eq!(data.entries[0].raw_key, "**/left-pad");
        assert_eq!(glob.target_package, "left-pad");
        assert!(glob.parent_package.is_none());
        assert_eq!(data.entries[0].line, 3);

        let scoped_parent = parsed(&data.entries[1]);
        assert_eq!(scoped_parent.parent_package.as_deref(), Some("react"));
        assert_eq!(scoped_parent.target_package, "left-pad");

        let glob_between = parsed(&data.entries[2]);
        assert_eq!(glob_between.parent_package.as_deref(), Some("react"));
        assert_eq!(glob_between.target_package, "left-pad");

        let scoped = parsed(&data.entries[3]);
        assert_eq!(scoped.parent_package.as_deref(), Some("@scope/parent"));
        assert_eq!(scoped.target_package, "left-pad");

        let both_scoped = parsed(&data.entries[4]);
        assert_eq!(both_scoped.parent_package.as_deref(), Some("@scope/parent"));
        assert_eq!(both_scoped.target_package, "@scope/child");

        let ranged_parent = parsed(&data.entries[5]);
        assert_eq!(ranged_parent.parent_package.as_deref(), Some("react"));
        assert_eq!(
            ranged_parent.parent_version_selector.as_deref(),
            Some("^18")
        );
        assert_eq!(ranged_parent.target_package, "left-pad");

        let selector = parsed(&data.entries[6]);
        assert_eq!(selector.target_package, "@types/react");
        assert_eq!(selector.target_version_selector.as_deref(), Some("<18"));
        assert_eq!(data.entries[6].line, 9);
    }

    #[test]
    fn resolutions_pnpm_delimiter_keys_parse_like_overrides() {
        let json = r#"{"resolutions": {"react>left-pad": "^1.3.0", "pkg@>=1": "^2.0.0"}}"#;
        let data = parse_bun_package_json_resolutions(json);
        assert_eq!(data.entries.len(), 2);
        let chained = parsed(&data.entries[0]);
        assert_eq!(chained.parent_package.as_deref(), Some("react"));
        assert_eq!(chained.target_package, "left-pad");
        // `@>` is a version selector, not the pnpm delimiter.
        let selector = parsed(&data.entries[1]);
        assert!(selector.parent_package.is_none());
        assert_eq!(selector.target_package, "pkg");
        assert_eq!(selector.target_version_selector.as_deref(), Some(">=1"));
    }

    #[test]
    fn resolutions_shapes_bun_rejects_are_unparsable_or_valueless() {
        let json = r#"{
  "resolutions": {
    "a/b/c": "^1.0.0",
    "a>b>c": "^1.0.0",
    "@scope": "^1.0.0",
    "left-pad/**": "^1.0.0",
    "nested": { "left-pad": "^1.3.0" },
    "left-pad": "^1.3.0"
  }
}"#;
        let data = parse_bun_package_json_resolutions(json);
        assert_eq!(data.entries.len(), 6);
        for entry in &data.entries[..4] {
            assert!(
                entry.parsed_key.is_none(),
                "{} is deeper than one parent or malformed and must not parse",
                entry.raw_key
            );
        }
        assert_eq!(data.entries[4].raw_key, "nested");
        assert!(data.entries[4].parsed_key.is_some());
        assert!(
            data.entries[4].raw_value.is_none(),
            "bun only honours string resolution values"
        );
        // The nested object's inner key must not steal the line of the later
        // top-level entry with the same name.
        assert_eq!(data.entries[5].raw_key, "left-pad");
        assert_eq!(data.entries[5].line, 8);
    }

    #[test]
    fn resolutions_comment_keys_are_skipped_without_shifting_lines() {
        let json = r#"{
  "resolutions": {
    "//": "pin for CVE-2024-0001",
    "left-pad": "^1.3.0"
  }
}"#;
        let data = parse_bun_package_json_resolutions(json);
        assert_eq!(data.entries.len(), 1);
        assert_eq!(data.entries[0].raw_key, "left-pad");
        assert_eq!(data.entries[0].line, 4);
    }

    #[test]
    fn resolutions_parser_ignores_overrides_and_nested_sections() {
        assert!(
            parse_bun_package_json_resolutions(r#"{"overrides": {"left-pad": "^1.3.0"}}"#)
                .entries
                .is_empty()
        );
        assert!(
            parse_bun_package_json_resolutions(
                r#"{"config": {"resolutions": {"left-pad": "^1.3.0"}}}"#
            )
            .entries
            .is_empty()
        );
        assert!(
            parse_npm_package_json_overrides(r#"{"resolutions": {"left-pad": "^1.3.0"}}"#)
                .entries
                .is_empty()
        );
        assert!(
            parse_bun_package_json_resolutions("{not valid json")
                .entries
                .is_empty()
        );
    }
}
