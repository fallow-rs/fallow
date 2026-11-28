use std::path::Path;

use fallow_config::{PackageJson, ResolvedConfig};
use fallow_types::results::DependencyLocation;
use rustc_hash::FxHashSet;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SectionScanState {
    Idle,
    AwaitColon,
    AwaitOpenBrace,
    Inside,
}

fn skip_block_comment(bytes: &[u8], i: &mut usize, line: &mut u32) {
    *i += 2;
    while *i < bytes.len() {
        if bytes[*i] == b'*' && bytes.get(*i + 1) == Some(&b'/') {
            *i += 2;
            return;
        }
        if bytes[*i] == b'\n' {
            *line += 1;
        }
        *i += 1;
    }
}

/// Skip JSON/JSONC whitespace and comments (`//...`, `/*...*/`), updating `line`.
fn skip_ws_and_comments(bytes: &[u8], i: &mut usize, line: &mut u32) {
    while *i < bytes.len() {
        match bytes[*i] {
            b' ' | b'\t' | b'\r' => *i += 1,
            b'\n' => {
                *line += 1;
                *i += 1;
            }
            b'/' if bytes.get(*i + 1) == Some(&b'/') => {
                *i += 2;
                while *i < bytes.len() && bytes[*i] != b'\n' {
                    *i += 1;
                }
            }
            b'/' if bytes.get(*i + 1) == Some(&b'*') => skip_block_comment(bytes, i, line),
            _ => break,
        }
    }
}

/// Read a quoted JSON string literal starting at `bytes[*i] == b'"'`, updating
/// `line` and advancing `i` past the closing quote.
fn read_jsonc_string<'a>(
    content: &'a str,
    bytes: &[u8],
    i: &mut usize,
    line: &mut u32,
) -> Option<(&'a str, u32, bool, bool)> {
    let start_line = *line;
    *i += 1;
    let str_start = *i;
    let mut has_escape = false;
    while *i < bytes.len() {
        let b = bytes[*i];
        if b == b'\\' {
            has_escape = true;
            *i += 1;
            if *i < bytes.len() && bytes[*i] == b'\n' {
                *line += 1;
            }
            *i += 1;
            continue;
        }
        if b == b'"' {
            let raw = &content[str_start..*i];
            *i += 1;
            let mut look = *i;
            let mut look_line = *line;
            skip_ws_and_comments(bytes, &mut look, &mut look_line);
            return Some((raw, start_line, has_escape, bytes.get(look) == Some(&b':')));
        }
        if b == b'\n' {
            *line += 1;
        }
        *i += 1;
    }
    None
}

struct SectionTracker {
    brace_depth: usize,
    state: SectionScanState,
}

impl SectionTracker {
    fn step_byte(&mut self, b: u8) {
        match b {
            b'{' => {
                self.brace_depth += 1;
                let enter = self.state == SectionScanState::AwaitOpenBrace && self.brace_depth == 2;
                self.state = if enter || self.state == SectionScanState::Inside {
                    SectionScanState::Inside
                } else {
                    SectionScanState::Idle
                };
            }
            b'}' => {
                self.brace_depth = self.brace_depth.saturating_sub(1);
                if self.brace_depth < 2 || self.state != SectionScanState::Inside {
                    self.state = SectionScanState::Idle;
                }
            }
            b':' => {
                self.state = match self.state {
                    SectionScanState::AwaitColon => SectionScanState::AwaitOpenBrace,
                    SectionScanState::Inside => SectionScanState::Inside,
                    SectionScanState::Idle | SectionScanState::AwaitOpenBrace => {
                        SectionScanState::Idle
                    }
                };
            }
            _ => self.reset_if_pending(),
        }
    }

    fn reset_if_pending(&mut self) {
        if self.state != SectionScanState::Inside {
            self.state = SectionScanState::Idle;
        }
    }

    fn step_key(
        &mut self,
        raw_key: &str,
        start_line: u32,
        package_name: &str,
        target_section: Option<&str>,
    ) -> Option<u32> {
        let Some(section) = target_section else {
            self.reset_if_pending();
            return (raw_key == package_name).then_some(start_line);
        };
        if self.brace_depth == 1 && raw_key == section {
            self.state = SectionScanState::AwaitColon;
            return None;
        }
        if self.state == SectionScanState::Inside
            && self.brace_depth == 2
            && raw_key == package_name
        {
            return Some(start_line);
        }
        self.reset_if_pending();
        None
    }
}

/// Scan a `package.json` (JSON/JSONC) string for the 1-based line number of
/// `"<package_name>":`, optionally scoped to a specific top-level `section` object
/// (`dependencies`, `devDependencies`, `optionalDependencies`).
fn scan_dep_line_in_json(content: &str, package_name: &str, target_section: Option<&str>) -> u32 {
    let bytes = content.as_bytes();
    let mut i = 0;
    let mut line: u32 = 1;
    let mut tracker = SectionTracker {
        brace_depth: 0,
        state: SectionScanState::Idle,
    };

    loop {
        skip_ws_and_comments(bytes, &mut i, &mut line);
        if i >= bytes.len() {
            return 1;
        }
        if bytes[i] != b'"' {
            tracker.step_byte(bytes[i]);
            i += 1;
            continue;
        }
        let Some((raw_key, start_line, has_escape, is_key)) =
            read_jsonc_string(content, bytes, &mut i, &mut line)
        else {
            return 1;
        };
        if !is_key || has_escape {
            tracker.reset_if_pending();
            continue;
        }
        if let Some(found) = tracker.step_key(raw_key, start_line, package_name, target_section) {
            return found;
        }
    }
}

/// Map a [`DependencyLocation`] to its top-level `package.json` section key.
pub(super) const fn section_name(location: &DependencyLocation) -> &'static str {
    match location {
        DependencyLocation::Dependencies => "dependencies",
        DependencyLocation::DevDependencies => "devDependencies",
        DependencyLocation::OptionalDependencies => "optionalDependencies",
    }
}

/// Find the 1-based line number of a dependency key inside a specific top-level
/// `package.json` section (`dependencies`, `devDependencies`, `optionalDependencies`).
///
/// Returns `1` if the section or key is missing (safe fallback).
pub fn find_dep_line_in_section(content: &str, package_name: &str, section: &str) -> u32 {
    scan_dep_line_in_json(content, package_name, Some(section))
}

#[cfg(test)]
fn find_dep_line_in_json(content: &str, package_name: &str) -> u32 {
    scan_dep_line_in_json(content, package_name, None)
}

/// Read a package.json file's raw text for line-number scanning.
pub fn read_pkg_json_content(pkg_path: &Path) -> Option<String> {
    PackageJson::read_source(pkg_path).ok()
}

/// Collect every package name declared across the root `package.json` and each
/// workspace `package.json`. This is the dependency universe the plugin system
/// activates on, reused by the framework-scoped security catalogue rows (#861) to
/// gate a row on the active framework. Missing or malformed manifests contribute
/// nothing (a framework row simply stays inert), matching the conservative
/// false-negatives-over-false-positives posture.
pub(super) fn collect_declared_dependency_names(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> FxHashSet<String> {
    let mut deps: FxHashSet<String> = FxHashSet::default();
    if let Some(pkg) = root_pkg {
        deps.extend(pkg.all_dependency_names());
    }
    for ws in workspaces {
        if ws.root == config.root {
            continue; // already covered by root_pkg
        }
        if let Some(pkg) = fallow_config::load_dir_package_json(&ws.root) {
            deps.extend(pkg.all_dependency_names());
        }
    }
    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_dep_line_finds_dependency_key() {
        let content = r#"{
  "name": "my-app",
  "dependencies": {
    "react": "^18.0.0",
    "lodash": "^4.17.21"
  }
}"#;
        assert_eq!(find_dep_line_in_json(content, "lodash"), 5);
        assert_eq!(find_dep_line_in_json(content, "react"), 4);
    }

    #[test]
    fn find_dep_line_returns_1_when_not_found() {
        let content = r#"{ "dependencies": {} }"#;
        assert_eq!(find_dep_line_in_json(content, "missing"), 1);
    }

    #[test]
    fn find_dep_line_handles_scoped_packages() {
        let content = r#"{
  "devDependencies": {
    "@typescript-eslint/parser": "^6.0.0"
  }
}"#;
        assert_eq!(
            find_dep_line_in_json(content, "@typescript-eslint/parser"),
            3
        );
    }

    #[test]
    fn find_dep_line_skips_line_comments() {
        let content = r#"{
  // "lodash": "old version",
  "dependencies": {
    "lodash": "^4.17.21"
  }
}"#;
        assert_eq!(find_dep_line_in_json(content, "lodash"), 4);
    }

    #[test]
    fn find_dep_line_skips_block_comments() {
        let content = r#"{
  /* "lodash": "old" */
  "dependencies": {
    "lodash": "^4.17.21"
  }
}"#;
        assert_eq!(find_dep_line_in_json(content, "lodash"), 4);
    }

    #[test]
    fn find_dep_line_skips_multiline_block_comment() {
        let content = r#"{
  /*
    "lodash": "commented out",
    "react": "also commented"
  */
  "dependencies": {
    "lodash": "^4.17.21"
  }
}"#;
        assert_eq!(find_dep_line_in_json(content, "lodash"), 7);
    }

    #[test]
    fn find_dep_line_after_block_comment_end_on_same_line() {
        let content = r#"{
  /* comment */ "lodash": "^4.17.21"
}"#;
        assert_eq!(find_dep_line_in_json(content, "lodash"), 2);
    }

    #[test]
    fn find_dep_line_dep_inside_and_after_block_comment() {
        let content = "{\n  /* \"lodash\": \"old\" */ \"lodash\": \"^4.17.21\"\n}";
        assert_eq!(find_dep_line_in_json(content, "lodash"), 2);
    }

    #[test]
    fn find_dep_line_minimal_block_comment() {
        let content = "{\n  /**/ \"lodash\": \"^4.17.21\"\n}";
        assert_eq!(find_dep_line_in_json(content, "lodash"), 2);
    }

    #[test]
    fn find_dep_line_multiline_block_comment_end_with_dep_on_remainder() {
        let content = "{\n  /* start of comment\n  end */ \"lodash\": \"^4.17.21\"\n}";
        assert_eq!(find_dep_line_in_json(content, "lodash"), 3);
    }

    #[test]
    fn find_dep_line_block_comment_end_without_dep_on_remainder() {
        let content =
            "{\n  /* start\n  end */ \"other\": \"1.0.0\",\n  \"lodash\": \"^4.17.21\"\n}";
        assert_eq!(find_dep_line_in_json(content, "lodash"), 4);
    }

    #[test]
    fn find_dep_line_value_not_key_is_not_matched() {
        let content = r#"{
  "dependencies": {
    "my-lodash-wrapper": "lodash"
  }
}"#;
        assert_eq!(find_dep_line_in_json(content, "lodash"), 1);
        assert_eq!(find_dep_line_in_json(content, "my-lodash-wrapper"), 3);
    }

    #[test]
    fn find_dep_line_empty_content() {
        assert_eq!(find_dep_line_in_json("", "lodash"), 1);
    }

    #[test]
    fn find_dep_line_multiple_dep_sections() {
        let content = r#"{
  "dependencies": {
    "react": "^18.0.0"
  },
  "devDependencies": {
    "react": "^18.0.0"
  }
}"#;
        assert_eq!(find_dep_line_in_json(content, "react"), 3);
    }

    #[test]
    fn find_dep_line_malformed_content() {
        assert_eq!(
            find_dep_line_in_json("this is not json at all", "lodash"),
            1
        );
        assert_eq!(find_dep_line_in_json("{{{", "lodash"), 1);
        assert_eq!(find_dep_line_in_json("null", "lodash"), 1);
    }

    #[test]
    fn read_pkg_json_content_nonexistent_path() {
        let result = read_pkg_json_content(Path::new("/nonexistent/path/package.json"));
        assert!(result.is_none(), "nonexistent path should return None");
    }

    #[test]
    fn read_pkg_json_content_valid_path() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let pkg_path = dir.path().join("package.json");
        std::fs::write(&pkg_path, r#"{"name": "test"}"#).expect("write temp file");

        let result = read_pkg_json_content(&pkg_path);
        assert!(result.is_some(), "valid path should return Some");
        assert_eq!(result.unwrap(), r#"{"name": "test"}"#);
    }

    #[test]
    fn find_dep_line_deeply_nested_scoped_package() {
        let content = r#"{
  "dependencies": {
    "@babel/plugin-transform-runtime": "^7.0.0",
    "@babel/core": "^7.0.0"
  }
}"#;
        assert_eq!(
            find_dep_line_in_json(content, "@babel/plugin-transform-runtime"),
            3
        );
        assert_eq!(find_dep_line_in_json(content, "@babel/core"), 4);
    }

    #[test]
    fn find_dep_line_with_trailing_comma_jsonc() {
        let content = "{\n  \"dependencies\": {\n    \"lodash\": \"^4.17.21\",\n  }\n}";
        assert_eq!(find_dep_line_in_json(content, "lodash"), 3);
    }

    #[test]
    fn find_dep_line_in_section_skips_peer_dependencies_before_dev_dependencies() {
        let content = r#"{
  "peerDependencies": {
    "react": ">=18"
  },
  "devDependencies": {
    "react": "^19.0.0"
  }
}"#;
        assert_eq!(
            find_dep_line_in_section(content, "react", "devDependencies"),
            6
        );
        assert_eq!(
            find_dep_line_in_section(content, "react", "dependencies"),
            1
        );
    }

    #[test]
    fn find_dep_line_in_section_skips_overrides_and_resolutions_before_target_section() {
        let content = r#"{
  "overrides": {
    "dependencies": {
      "react": "^17.0.0"
    },
    "lodash": "^4.17.21"
  },
  "resolutions": {
    "chalk": "^5.0.0"
  },
  "pnpm": {
    "overrides": {
      "chalk": "^5.0.0"
    }
  },
  "dependencies": {
    "react": "^19.0.0"
  },
  "devDependencies": {
    "lodash": "^4.17.21"
  },
  "optionalDependencies": {
    "chalk": "^5.3.0"
  }
}"#;
        assert_eq!(
            find_dep_line_in_section(content, "react", "dependencies"),
            17
        );
        assert_eq!(
            find_dep_line_in_section(content, "lodash", "devDependencies"),
            20
        );
        assert_eq!(
            find_dep_line_in_section(content, "chalk", "optionalDependencies"),
            23
        );
    }

    #[test]
    fn find_dep_line_in_section_handles_same_package_across_sections_and_missing_fallback() {
        let content = r#"{
  "dependencies": {
    "react": "^18.0.0"
  },
  "devDependencies": {
    "react": "^19.0.0"
  }
}"#;
        assert_eq!(
            find_dep_line_in_section(content, "react", "dependencies"),
            3
        );
        assert_eq!(
            find_dep_line_in_section(content, "react", "devDependencies"),
            6
        );
        assert_eq!(
            find_dep_line_in_section(content, "react", "optionalDependencies"),
            1
        );
    }
}
