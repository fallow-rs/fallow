//! The parsed model of `pnpm-workspace.yaml`.
//!
//! One YAML parse gives the `packages` patterns, the catalogs and the
//! overrides, with the source position of each key.

use crate::yaml::{self, YamlError, YamlNode};

/// The facts that fallow reads from one `pnpm-workspace.yaml` source.
#[derive(Debug, Clone, Default)]
pub struct PnpmWorkspaceDoc {
    packages: Vec<String>,
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
        Ok(Self {
            packages: packages_of(root),
        })
    }

    /// The entries of the top-level `packages` sequence, as written.
    #[must_use]
    pub fn packages(&self) -> &[String] {
        &self.packages
    }
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
}
