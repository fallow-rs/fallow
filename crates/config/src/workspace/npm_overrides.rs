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

use super::pnpm_overrides::{
    ParsedOverrideKey, PnpmOverrideData, PnpmOverrideEntry, split_pkg_and_selector,
};

/// Parse the top-level `overrides` section of a root `package.json`. Returns
/// an empty `PnpmOverrideData` when the file has no overrides, when the JSON
/// is malformed, or when the section is present but empty.
#[must_use]
pub fn parse_npm_package_json_overrides(source: &str) -> PnpmOverrideData {
    let value: serde_json::Value = match serde_json::from_str(source) {
        Ok(v) => v,
        Err(_) => return PnpmOverrideData::default(),
    };
    let Some(overrides) = value.get("overrides").and_then(|o| o.as_object()) else {
        return PnpmOverrideData::default();
    };

    let mut line_index = NpmOverridesLineIndex::build(source);
    let mut entries = Vec::new();
    let mut path: Vec<String> = Vec::new();
    flatten_overrides(overrides, &mut path, &mut line_index, &mut entries);
    PnpmOverrideData { entries }
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

/// Ordered `(key, line)` pairs for every key inside the top-level `overrides`
/// object, consumed by a forward-only cursor during flattening.
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
    /// (at any nesting depth) inside the top-level `overrides` object,
    /// paired with its 1-based line number.
    fn build(source: &str) -> Self {
        let mut scan = NpmOverridesJsonScan::default();
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

/// Char-by-char brace-depth scanner state for the top-level `overrides` line
/// index. Mirrors the pnpm `pnpm.overrides` scanner, but records keys at every
/// nesting depth inside the overrides object because npm scopes overrides by
/// nesting rather than by `parent>child` keys.
#[derive(Default)]
struct NpmOverridesJsonScan {
    entries: Vec<(String, u32)>,
    depth: i32,
    overrides_depth: Option<i32>,
    in_string: bool,
    escape: bool,
    last_key: Option<String>,
    key_buf: String,
    collecting_key: bool,
}

impl NpmOverridesJsonScan {
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
                self.last_key = Some(std::mem::take(&mut self.key_buf));
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
                // The overrides object closes when depth returns to the level
                // where the `overrides` key was seen.
                if self.overrides_depth == Some(self.depth - 1) {
                    self.overrides_depth = None;
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
        if self.overrides_depth.is_none() && self.depth == 1 && key == "overrides" {
            self.overrides_depth = Some(self.depth);
        } else if let Some(d) = self.overrides_depth
            && self.depth > d
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
}
