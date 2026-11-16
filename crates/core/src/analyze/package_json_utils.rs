use std::path::Path;

use fallow_config::{PackageJson, ResolvedConfig};
use rustc_hash::FxHashSet;

/// Find the 1-based line number of a dependency key in a package.json file.
///
/// Searches the raw file content for `"<package_name>"` followed by `:` on the
/// same line. Skips JSONC comment lines. Returns 1 if not found (safe fallback).
#[expect(
    clippy::cast_possible_truncation,
    reason = "line count in package.json is bounded by file size"
)]
pub fn find_dep_line_in_json(content: &str, package_name: &str) -> u32 {
    let needle = format!("\"{package_name}\"");
    let mut in_block_comment = false;
    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim_start();
        if in_block_comment {
            if let Some(end) = trimmed.find("*/") {
                let rest = &trimmed[end + 2..];
                in_block_comment = false;
                if let Some(pos) = rest.find(&*needle) {
                    let after = &rest[pos + needle.len()..];
                    if after.trim_start().starts_with(':') {
                        return (i + 1) as u32;
                    }
                }
            }
            continue;
        }
        if trimmed.starts_with("//") {
            continue;
        }
        if let Some(after_open) = trimmed.strip_prefix("/*") {
            if let Some(end) = after_open.find("*/") {
                let rest = &after_open[end + 2..];
                if let Some(pos) = rest.find(&*needle) {
                    let after = &rest[pos + needle.len()..];
                    if after.trim_start().starts_with(':') {
                        return (i + 1) as u32;
                    }
                }
            } else {
                in_block_comment = true;
            }
            continue;
        }
        if let Some(pos) = line.find(&needle) {
            let after = &line[pos + needle.len()..];
            if after.trim_start().starts_with(':') {
                return (i + 1) as u32;
            }
        }
    }
    1
}

/// Read a package.json file's raw text for line-number scanning.
pub fn read_pkg_json_content(pkg_path: &Path) -> Option<String> {
    std::fs::read_to_string(pkg_path).ok()
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
}
