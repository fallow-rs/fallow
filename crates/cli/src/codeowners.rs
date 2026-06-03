//! CODEOWNERS file parser and ownership lookup.
//!
//! Parses GitHub/GitLab-style CODEOWNERS files and matches file paths
//! to their owners. Used by `--group-by owner` to group analysis output
//! by team ownership.
//!
//! # Pattern semantics
//!
//! CODEOWNERS patterns follow gitignore-like rules:
//! - `*.js` matches any `.js` file in any directory
//! - `/docs/*` matches files directly in `docs/` (root-anchored)
//! - `docs/` matches everything under `docs/`
//! - Last matching rule wins
//! - First owner on a multi-owner line is the primary owner
//!
//! # GitLab extensions
//!
//! GitLab's CODEOWNERS format is a superset of GitHub's. The following
//! GitLab-only syntax is accepted (though it doesn't affect ownership
//! lookup beyond propagating the default owners within a section):
//!
//! - Section headers: `[Section name]`, `^[Section name]` (optional section),
//!   `[Section name][N]` (N required approvals)
//! - Section default owners: `[Section] @owner1 @owner2`. Pattern lines
//!   inside the section that omit inline owners inherit the section's defaults
//! - Exclusion patterns: `!path` clears ownership for matching files
//!   (GitLab 17.10+). A negation that is the last matching rule for a
//!   file makes it unowned.

use std::path::Path;

use globset::{Glob, GlobSet, GlobSetBuilder};

/// Parsed CODEOWNERS file for ownership lookup.
#[derive(Debug)]
pub struct CodeOwners {
    /// Primary owner per rule, indexed by glob position in the `GlobSet`.
    /// Empty string for negation rules (see `is_negation`).
    owners: Vec<String>,
    /// Number of owners matched by each rule, indexed by glob position.
    /// Zero for negation rules.
    owner_counts: Vec<u32>,
    /// Original CODEOWNERS pattern per rule (e.g. `/src/` or `*.ts`).
    /// For negations, the raw pattern is prefixed with `!`.
    patterns: Vec<String>,
    /// Whether each rule is a GitLab-style negation (`!path`). A matching
    /// negation as the last-matching rule clears ownership for that file.
    is_negation: Vec<bool>,
    /// GitLab section name per rule, or `None` for rules that appear before
    /// the first section header. Used by `--group-by section`.
    sections: Vec<Option<String>>,
    /// Section default owners per rule (cloned from the active section
    /// header). Empty for rules outside any section, used as metadata in
    /// JSON output for `--group-by section`.
    section_owners: Vec<Vec<String>>,
    /// Whether the file contains at least one GitLab section header.
    has_sections: bool,
    /// Compiled glob patterns for matching.
    globs: GlobSet,
}

/// Standard locations to probe for a CODEOWNERS file, in priority order.
///
/// Order: root catch-all → GitHub → GitLab → GitHub legacy (`docs/`).
const PROBE_PATHS: &[&str] = &[
    "CODEOWNERS",
    ".github/CODEOWNERS",
    ".gitlab/CODEOWNERS",
    "docs/CODEOWNERS",
];

/// Label for files that match no CODEOWNERS rule.
pub const UNOWNED_LABEL: &str = "(unowned)";

/// Label for files owned by a rule declared before any GitLab section header.
///
/// Used as the group key for `--group-by section` when the last matching rule
/// isn't inside any `[Section]` block.
pub const NO_SECTION_LABEL: &str = "(no section)";

impl CodeOwners {
    /// Load and parse a CODEOWNERS file from the given path.
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("failed to read {}: {e}", path.display()))?;
        Self::parse(&content)
    }

    /// Auto-probe standard CODEOWNERS locations relative to the project root.
    ///
    /// Tries `CODEOWNERS`, `.github/CODEOWNERS`, `.gitlab/CODEOWNERS`, `docs/CODEOWNERS`.
    pub fn discover(root: &Path) -> Result<Self, String> {
        for probe in PROBE_PATHS {
            let path = root.join(probe);
            if path.is_file() {
                return Self::from_file(&path);
            }
        }
        Err(format!(
            "no CODEOWNERS file found (looked for: {}). \
             Create one of these files or use --group-by directory instead",
            PROBE_PATHS.join(", ")
        ))
    }

    /// Load from a config-specified path, or auto-discover.
    pub fn load(root: &Path, config_path: Option<&str>) -> Result<Self, String> {
        if let Some(p) = config_path {
            let path = root.join(p);
            Self::from_file(&path)
        } else {
            Self::discover(root)
        }
    }

    /// Parse CODEOWNERS content into a lookup structure.
    pub(crate) fn parse(content: &str) -> Result<Self, String> {
        let mut builder = GlobSetBuilder::new();
        let mut owners = Vec::new();
        let mut owner_counts = Vec::new();
        let mut patterns = Vec::new();
        let mut is_negation = Vec::new();
        let mut sections: Vec<Option<String>> = Vec::new();
        let mut section_owners: Vec<Vec<String>> = Vec::new();
        let mut current_section: Option<String> = None;
        let mut current_section_owners: Vec<String> = Vec::new();
        let mut has_sections = false;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            if let Some((name, defaults)) = parse_section_header(line) {
                current_section = Some(name);
                current_section_owners = defaults;
                has_sections = true;
                continue;
            }

            let (negate, rest) = if let Some(after) = line.strip_prefix('!') {
                (true, after.trim_start())
            } else {
                (false, line)
            };

            let mut parts = rest.split_whitespace();
            let Some(pattern) = parts.next() else {
                continue;
            };
            let inline_owners = parts.collect::<Vec<_>>();

            let (effective_owner, owner_count): (&str, u32) = if negate {
                ("", 0)
            } else if let Some(owner) = inline_owners.first() {
                (
                    owner,
                    u32::try_from(inline_owners.len()).unwrap_or(u32::MAX),
                )
            } else if let Some(owner) = current_section_owners.first() {
                (
                    owner.as_str(),
                    u32::try_from(current_section_owners.len()).unwrap_or(u32::MAX),
                )
            } else {
                continue;
            };

            let glob_pattern = translate_pattern(pattern);
            let glob = Glob::new(&glob_pattern)
                .map_err(|e| format!("invalid CODEOWNERS pattern '{pattern}': {e}"))?;

            builder.add(glob);
            owners.push(effective_owner.to_string());
            owner_counts.push(owner_count);
            patterns.push(if negate {
                format!("!{pattern}")
            } else {
                pattern.to_string()
            });
            is_negation.push(negate);
            sections.push(current_section.clone());
            section_owners.push(current_section_owners.clone());
        }

        let globs = builder
            .build()
            .map_err(|e| format!("failed to compile CODEOWNERS patterns: {e}"))?;

        Ok(Self {
            owners,
            owner_counts,
            patterns,
            is_negation,
            sections,
            section_owners,
            has_sections,
            globs,
        })
    }

    /// Look up the primary owner of a file path (relative to project root).
    ///
    /// Returns the first owner from the last matching CODEOWNERS rule,
    /// or `None` if no rule matches or the last matching rule is a
    /// GitLab-style exclusion (`!path`).
    pub fn owner_of(&self, relative_path: &Path) -> Option<&str> {
        let matches = self.globs.matches(relative_path);
        matches.iter().max().and_then(|&idx| {
            if self.is_negation[idx] {
                None
            } else {
                Some(self.owners[idx].as_str())
            }
        })
    }

    /// Look up the number of owners matched by the last matching CODEOWNERS rule.
    ///
    /// Returns `Some(0)` when the path is explicitly unowned by a GitLab
    /// negation, and `None` when no CODEOWNERS rule matches.
    pub fn owner_count_of(&self, relative_path: &Path) -> Option<u32> {
        let matches = self.globs.matches(relative_path);
        matches.iter().max().map(|&idx| {
            if self.is_negation[idx] {
                0
            } else {
                self.owner_counts[idx]
            }
        })
    }

    /// Look up the primary owner and the original CODEOWNERS pattern for a path.
    ///
    /// Returns `(owner, pattern)` from the last matching rule, or `None` if
    /// no rule matches or the last matching rule is a GitLab-style exclusion.
    /// The pattern is the raw string from the CODEOWNERS file (e.g. `/src/`
    /// or `*.ts`).
    pub fn owner_and_rule_of(&self, relative_path: &Path) -> Option<(&str, &str)> {
        let matches = self.globs.matches(relative_path);
        matches.iter().max().and_then(|&idx| {
            if self.is_negation[idx] {
                None
            } else {
                Some((self.owners[idx].as_str(), self.patterns[idx].as_str()))
            }
        })
    }

    /// Look up the GitLab CODEOWNERS section that owns a file.
    ///
    /// Returns `Some(Some(name))` when the last matching rule is inside a
    /// named section, `Some(None)` when the rule appears before any section
    /// header, or `None` when no rule matches or the last match is a
    /// GitLab-style exclusion.
    #[allow(
        clippy::option_option,
        reason = "three distinct states: no match, matched pre-section, matched in named section"
    )]
    pub fn section_of(&self, relative_path: &Path) -> Option<Option<&str>> {
        let matches = self.globs.matches(relative_path);
        matches.iter().max().and_then(|&idx| {
            if self.is_negation[idx] {
                None
            } else {
                Some(self.sections[idx].as_deref())
            }
        })
    }

    /// Look up the section name plus the section's default owners for a file.
    ///
    /// Used by `--group-by section` to attach owner metadata to each group.
    /// Returns `None` when no rule matches or the last match is a negation.
    /// The returned owner slice is empty for rules declared outside any
    /// section or for sections that declare no default owners.
    pub fn section_and_owners_of(&self, relative_path: &Path) -> Option<(Option<&str>, &[String])> {
        let matches = self.globs.matches(relative_path);
        matches.iter().max().and_then(|&idx| {
            if self.is_negation[idx] {
                None
            } else {
                Some((
                    self.sections[idx].as_deref(),
                    self.section_owners[idx].as_slice(),
                ))
            }
        })
    }

    /// Look up section, section owners, and the raw CODEOWNERS pattern in one
    /// glob pass.
    ///
    /// Used by `--group-by section` display paths that need both the section
    /// key and the matching rule text without walking the `GlobSet` twice.
    pub fn section_owners_and_rule_of(
        &self,
        relative_path: &Path,
    ) -> Option<(Option<&str>, &[String], &str)> {
        let matches = self.globs.matches(relative_path);
        matches.iter().max().and_then(|&idx| {
            if self.is_negation[idx] {
                None
            } else {
                Some((
                    self.sections[idx].as_deref(),
                    self.section_owners[idx].as_slice(),
                    self.patterns[idx].as_str(),
                ))
            }
        })
    }

    /// Whether the parsed file contains at least one GitLab section header.
    ///
    /// `--group-by section` errors out when this is false, since every file
    /// would collapse into the `(no section)` bucket.
    pub fn has_sections(&self) -> bool {
        self.has_sections
    }
}

/// Parse a GitLab CODEOWNERS section header.
///
/// Recognized forms (all optionally prefixed with `^` for optional sections):
/// - `[Section name]`
/// - `[Section name][N]` (N required approvals)
/// - `[Section name] @owner1 @owner2` (section default owners)
/// - `^[Section name][N] @owner` (any combination of the above)
///
/// Returns `Some((name, default_owners))` if the line is a well-formed section
/// header. The returned owner vec is empty when the header declares no default
/// owners. Returns `None` when the line is not a section header and should be
/// parsed as a rule instead. Detection is strict: a line like `[abc]def @owner`
/// that has non-whitespace content directly after the closing `]` is not
/// treated as a section header, so legacy GitHub CODEOWNERS patterns continue
/// to parse.
fn parse_section_header(line: &str) -> Option<(String, Vec<String>)> {
    let rest = line.strip_prefix('^').unwrap_or(line);
    let rest = rest.strip_prefix('[')?;
    let close = rest.find(']')?;
    let name = &rest[..close];
    if name.is_empty() {
        return None;
    }
    let mut after = &rest[close + 1..];

    if let Some(inner) = after.strip_prefix('[') {
        let n_close = inner.find(']')?;
        let count = &inner[..n_close];
        if count.is_empty() || !count.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        after = &inner[n_close + 1..];
    }

    if !after.is_empty() && !after.starts_with(char::is_whitespace) {
        return None;
    }

    Some((
        name.to_string(),
        after.split_whitespace().map(String::from).collect(),
    ))
}

/// Translate a CODEOWNERS pattern to a `globset`-compatible glob pattern.
///
/// CODEOWNERS uses gitignore-like semantics:
/// - Leading `/` anchors to root (stripped for globset)
/// - Trailing `/` means directory contents (`dir/` → `dir/**`)
/// - No `/` in pattern: matches in any directory (`*.js` → `**/*.js`)
/// - Contains `/` (non-trailing): root-relative as-is
fn translate_pattern(pattern: &str) -> String {
    let (anchored, rest) = if let Some(p) = pattern.strip_prefix('/') {
        (true, p)
    } else {
        (false, pattern)
    };

    let expanded = if let Some(p) = rest.strip_suffix('/') {
        format!("{p}/**")
    } else {
        rest.to_string()
    };

    if !anchored && !expanded.contains('/') {
        format!("**/{expanded}")
    } else {
        expanded
    }
}

/// Extract the first path component for `--group-by directory` grouping.
///
/// Returns the first directory segment of a relative path.
/// For monorepo structures (`packages/auth/...`), returns `packages`.
pub fn directory_group(relative_path: &Path) -> &str {
    let s = relative_path.to_str().unwrap_or("");
    let s = if s.contains('\\') {
        return s.split(['/', '\\']).next().unwrap_or(s);
    } else {
        s
    };

    match s.find('/') {
        Some(pos) => &s[..pos],
        None => s, // Root-level file
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn translate_bare_glob() {
        assert_eq!(translate_pattern("*.js"), "**/*.js");
    }

    #[test]
    fn translate_rooted_pattern() {
        assert_eq!(translate_pattern("/docs/*"), "docs/*");
    }

    #[test]
    fn translate_directory_pattern() {
        assert_eq!(translate_pattern("docs/"), "docs/**");
    }

    #[test]
    fn translate_rooted_directory() {
        assert_eq!(translate_pattern("/src/app/"), "src/app/**");
    }

    #[test]
    fn translate_path_with_slash() {
        assert_eq!(translate_pattern("src/utils/*.ts"), "src/utils/*.ts");
    }

    #[test]
    fn translate_double_star() {
        assert_eq!(translate_pattern("**/test_*.py"), "**/test_*.py");
    }

    #[test]
    fn translate_single_file() {
        assert_eq!(translate_pattern("Makefile"), "**/Makefile");
    }

    #[test]
    fn parse_simple_codeowners() {
        let content = "* @global-owner\n/src/ @frontend\n*.rs @rust-team\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owners.len(), 3);
    }

    #[test]
    fn parse_skips_comments_and_blanks() {
        let content = "# Comment\n\n* @owner\n  # Indented comment\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owners.len(), 1);
    }

    #[test]
    fn parse_multi_owner_takes_first() {
        let content = "*.ts @team-a @team-b @team-c\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owners[0], "@team-a");
    }

    #[test]
    fn parse_skips_pattern_without_owner() {
        let content = "*.ts\n*.js @owner\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owners.len(), 1);
        assert_eq!(co.owners[0], "@owner");
    }

    #[test]
    fn parse_empty_content() {
        let co = CodeOwners::parse("").unwrap();
        assert_eq!(co.owner_of(Path::new("anything.ts")), None);
    }

    #[test]
    fn owner_of_last_match_wins() {
        let content = "* @default\n/src/ @frontend\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("src/app.ts")), Some("@frontend"));
    }

    #[test]
    fn owner_of_falls_back_to_catch_all() {
        let content = "* @default\n/src/ @frontend\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("README.md")), Some("@default"));
    }

    #[test]
    fn owner_of_no_match_returns_none() {
        let content = "/src/ @frontend\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("README.md")), None);
    }

    #[test]
    fn owner_of_extension_glob() {
        let content = "*.rs @rust-team\n*.ts @ts-team\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("src/lib.rs")), Some("@rust-team"));
        assert_eq!(
            co.owner_of(Path::new("packages/ui/Button.ts")),
            Some("@ts-team")
        );
    }

    #[test]
    fn owner_of_nested_directory() {
        let content = "* @default\n/packages/auth/ @auth-team\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.owner_of(Path::new("packages/auth/src/login.ts")),
            Some("@auth-team")
        );
        assert_eq!(
            co.owner_of(Path::new("packages/ui/Button.ts")),
            Some("@default")
        );
    }

    #[test]
    fn owner_of_specific_overrides_general() {
        let content = "\
            * @default\n\
            /src/ @frontend\n\
            /src/api/ @backend\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.owner_of(Path::new("src/api/routes.ts")),
            Some("@backend")
        );
        assert_eq!(co.owner_of(Path::new("src/app.ts")), Some("@frontend"));
    }

    #[test]
    fn owner_and_rule_of_returns_owner_and_pattern() {
        let content = "* @default\n/src/ @frontend\n*.rs @rust-team\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.owner_and_rule_of(Path::new("src/app.ts")),
            Some(("@frontend", "/src/"))
        );
        assert_eq!(
            co.owner_and_rule_of(Path::new("src/lib.rs")),
            Some(("@rust-team", "*.rs"))
        );
        assert_eq!(
            co.owner_and_rule_of(Path::new("README.md")),
            Some(("@default", "*"))
        );
    }

    #[test]
    fn owner_and_rule_of_no_match() {
        let content = "/src/ @frontend\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_and_rule_of(Path::new("README.md")), None);
    }

    #[test]
    fn directory_group_simple() {
        assert_eq!(directory_group(Path::new("src/utils/index.ts")), "src");
    }

    #[test]
    fn directory_group_root_file() {
        assert_eq!(directory_group(Path::new("index.ts")), "index.ts");
    }

    #[test]
    fn directory_group_monorepo() {
        assert_eq!(
            directory_group(Path::new("packages/auth/src/login.ts")),
            "packages"
        );
    }

    #[test]
    fn discover_nonexistent_root() {
        let result = CodeOwners::discover(Path::new("/nonexistent/path"));
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("no CODEOWNERS file found"));
        assert!(err.contains("--group-by directory"));
    }

    #[test]
    fn from_file_nonexistent() {
        let result = CodeOwners::from_file(Path::new("/nonexistent/CODEOWNERS"));
        assert!(result.is_err());
    }

    #[test]
    fn from_file_real_codeowners() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let path = root.join(".github/CODEOWNERS");
        if path.exists() {
            let co = CodeOwners::from_file(&path).unwrap();
            assert_eq!(
                co.owner_of(Path::new(".claude/rules/detection.md")),
                Some("@bartwaardenburg")
            );
        }
    }

    #[test]
    fn email_owner() {
        let content = "*.js user@example.com\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("index.js")), Some("user@example.com"));
    }

    #[test]
    fn team_owner() {
        let content = "*.ts @org/frontend-team\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("app.ts")), Some("@org/frontend-team"));
    }

    #[test]
    fn gitlab_section_header_skipped_as_rule() {
        let content = "[Section Name]\n*.ts @owner\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owners.len(), 1);
        assert_eq!(co.owner_of(Path::new("app.ts")), Some("@owner"));
    }

    #[test]
    fn gitlab_optional_section_header_skipped() {
        let content = "^[Optional Section]\n*.ts @owner\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owners.len(), 1);
    }

    #[test]
    fn gitlab_section_header_with_approval_count_skipped() {
        let content = "[Section Name][2]\n*.ts @owner\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owners.len(), 1);
    }

    #[test]
    fn gitlab_optional_section_with_approval_count_skipped() {
        let content = "^[Section Name][3] @fallback-team\nfoo/\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owners.len(), 1);
        assert_eq!(co.owner_of(Path::new("foo/bar.ts")), Some("@fallback-team"));
    }

    #[test]
    fn gitlab_section_default_owners_inherited() {
        let content = "\
            [Utilities] @utils-team\n\
            src/utils/\n\
            [UI Components] @ui-team\n\
            src/components/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owners.len(), 2);
        assert_eq!(
            co.owner_of(Path::new("src/utils/greet.ts")),
            Some("@utils-team")
        );
        assert_eq!(
            co.owner_of(Path::new("src/components/button.ts")),
            Some("@ui-team")
        );
    }

    #[test]
    fn gitlab_inline_owner_overrides_section_default() {
        let content = "\
            [Section] @section-owner\n\
            src/generic/\n\
            src/special/ @special-owner\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.owner_of(Path::new("src/generic/a.ts")),
            Some("@section-owner")
        );
        assert_eq!(
            co.owner_of(Path::new("src/special/a.ts")),
            Some("@special-owner")
        );
    }

    #[test]
    fn gitlab_section_defaults_reset_between_sections() {
        let content = "\
            [Section1] @team-a\n\
            foo/\n\
            [Section2]\n\
            bar/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owners.len(), 1);
        assert_eq!(co.owner_of(Path::new("foo/x.ts")), Some("@team-a"));
        assert_eq!(co.owner_of(Path::new("bar/x.ts")), None);
    }

    #[test]
    fn gitlab_section_header_multiple_default_owners_uses_first() {
        let content = "[Section] @first @second\nfoo/\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("foo/a.ts")), Some("@first"));
    }

    #[test]
    fn gitlab_rules_before_first_section_retain_inline_owners() {
        let content = "\
            * @default-owner\n\
            [Utilities] @utils-team\n\
            src/utils/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("README.md")), Some("@default-owner"));
        assert_eq!(
            co.owner_of(Path::new("src/utils/greet.ts")),
            Some("@utils-team")
        );
    }

    #[test]
    fn gitlab_issue_127_reproduction() {
        let content = "\
# Default section (no header, rules before first section)
* @default-owner

[Utilities] @utils-team
src/utils/

[UI Components] @ui-team
src/components/
";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("README.md")), Some("@default-owner"));
        assert_eq!(
            co.owner_of(Path::new("src/utils/greet.ts")),
            Some("@utils-team")
        );
        assert_eq!(
            co.owner_of(Path::new("src/components/button.ts")),
            Some("@ui-team")
        );
    }

    #[test]
    fn gitlab_negation_last_match_clears_ownership() {
        let content = "\
            * @default\n\
            !src/generated/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("README.md")), Some("@default"));
        assert_eq!(co.owner_of(Path::new("src/generated/bundle.js")), None);
    }

    #[test]
    fn gitlab_negation_only_clears_when_last_match() {
        let content = "\
            * @default\n\
            !src/\n\
            /src/special/ @special\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("src/foo.ts")), None);
        assert_eq!(co.owner_of(Path::new("src/special/a.ts")), Some("@special"));
    }

    #[test]
    fn gitlab_negation_owner_and_rule_returns_none() {
        let content = "* @default\n!src/vendor/\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.owner_and_rule_of(Path::new("README.md")),
            Some(("@default", "*"))
        );
        assert_eq!(co.owner_and_rule_of(Path::new("src/vendor/lib.js")), None);
    }

    #[test]
    fn parse_section_header_variants() {
        assert_eq!(
            parse_section_header("[Section]"),
            Some(("Section".into(), vec![]))
        );
        assert_eq!(
            parse_section_header("^[Section]"),
            Some(("Section".into(), vec![]))
        );
        assert_eq!(
            parse_section_header("[Section][2]"),
            Some(("Section".into(), vec![]))
        );
        assert_eq!(
            parse_section_header("^[Section][2]"),
            Some(("Section".into(), vec![]))
        );
        assert_eq!(
            parse_section_header("[Section] @a @b"),
            Some(("Section".into(), vec!["@a".into(), "@b".into()]))
        );
        assert_eq!(
            parse_section_header("[Section][2] @a"),
            Some(("Section".into(), vec!["@a".into()]))
        );
    }

    #[test]
    fn parse_section_header_rejects_malformed() {
        assert_eq!(parse_section_header("[unclosed"), None);
        assert_eq!(parse_section_header("[]"), None);
        assert_eq!(parse_section_header("[abc]def @owner"), None);
        assert_eq!(parse_section_header("[Section][] @owner"), None);
        assert_eq!(parse_section_header("[Section][abc] @owner"), None);
    }

    #[test]
    fn has_sections_false_without_headers() {
        let co = CodeOwners::parse("* @default\n/src/ @frontend\n").unwrap();
        assert!(!co.has_sections());
    }

    #[test]
    fn has_sections_true_with_headers() {
        let co = CodeOwners::parse("[Utilities] @utils\nsrc/utils/\n").unwrap();
        assert!(co.has_sections());
    }

    #[test]
    fn section_of_returns_named_section() {
        let content = "\
            [Billing] @billing-team\n\
            src/billing/\n\
            [Search] @search-team\n\
            src/search/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.section_of(Path::new("src/billing/invoice.ts")),
            Some(Some("Billing"))
        );
        assert_eq!(
            co.section_of(Path::new("src/search/indexer.ts")),
            Some(Some("Search"))
        );
    }

    #[test]
    fn section_of_returns_some_none_for_pre_section_rule() {
        let content = "\
            * @default\n\
            [Billing] @billing-team\n\
            src/billing/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.section_of(Path::new("README.md")), Some(None));
        assert_eq!(
            co.section_of(Path::new("src/billing/invoice.ts")),
            Some(Some("Billing"))
        );
    }

    #[test]
    fn section_of_returns_none_for_unmatched_path() {
        let content = "[Billing] @billing-team\nsrc/billing/\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.section_of(Path::new("src/other/x.ts")), None);
    }

    #[test]
    fn section_of_returns_none_for_negation_last_match() {
        let content = "\
            [Billing] @billing-team\n\
            src/billing/\n\
            !src/billing/vendor/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.section_of(Path::new("src/billing/invoice.ts")),
            Some(Some("Billing"))
        );
        assert_eq!(co.section_of(Path::new("src/billing/vendor/lib.js")), None);
    }

    #[test]
    fn section_and_owners_of_returns_section_defaults() {
        let content = "\
            [Billing] @core-reviewers @alice\n\
            src/billing/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        let (section, owners) = co
            .section_and_owners_of(Path::new("src/billing/invoice.ts"))
            .unwrap();
        assert_eq!(section, Some("Billing"));
        assert_eq!(
            owners,
            &["@core-reviewers".to_string(), "@alice".to_string()]
        );
    }

    #[test]
    fn section_and_owners_of_same_owners_distinct_sections() {
        let content = "\
            [billing] @core-reviewers @alice @bob\n\
            src/billing/\n\
            [notifications] @core-reviewers @alice @bob\n\
            src/notifications/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        let (billing_sec, _) = co
            .section_and_owners_of(Path::new("src/billing/invoice.ts"))
            .unwrap();
        let (notifications_sec, _) = co
            .section_and_owners_of(Path::new("src/notifications/email.ts"))
            .unwrap();
        assert_eq!(billing_sec, Some("billing"));
        assert_eq!(notifications_sec, Some("notifications"));
    }

    #[test]
    fn section_and_owners_of_empty_owners_for_pre_section_rule() {
        let content = "* @default\n[Billing]\nsrc/billing/ @billing\n";
        let co = CodeOwners::parse(content).unwrap();
        let (section, owners) = co.section_and_owners_of(Path::new("README.md")).unwrap();
        assert_eq!(section, None);
        assert!(owners.is_empty());
    }

    #[test]
    fn owner_count_of_counts_all_matched_owners() {
        let content = "\
            * @default\n\
            src/api/ @backend @payments @security\n\
            [Frontend] @ui @design\n\
            src/ui/\n\
            !src/generated/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_count_of(Path::new("src/api/payments.ts")), Some(3));
        assert_eq!(co.owner_count_of(Path::new("src/ui/button.tsx")), Some(2));
        assert_eq!(co.owner_count_of(Path::new("README.md")), Some(1));
        assert_eq!(
            co.owner_count_of(Path::new("src/generated/types.ts")),
            Some(0)
        );
        assert_eq!(
            co.owner_count_of(Path::new("other/generated/types.ts")),
            Some(1)
        );
    }

    #[test]
    fn non_section_bracket_pattern_parses_as_rule() {
        let content = "[abc]def @owner\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owners.len(), 1);
        assert_eq!(co.owner_of(Path::new("adef")), Some("@owner"));
    }
}
