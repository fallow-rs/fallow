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
//! - `/*` matches root-level files only; `*` is the catch-all
//! - `docs/` matches everything under a `docs/` directory at any depth
//! - `/docs` matches a file `docs` or everything under the `docs/` directory
//! - `*` does not match `/`
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
//! - Repeated section names: headers with the same name, compared without
//!   case, form one section. The section keeps the name of its first header.
//!   Its default owners are the default owners of all its headers, in file
//!   order, each one time. A pattern line without inline owners inherits the
//!   default owners of the header directly above it.

use std::path::Path;

use globset::{Candidate, Glob, GlobBuilder, GlobSet, GlobSetBuilder};
use rustc_hash::{FxHashMap, FxHashSet};

/// Wildcards (`*`, `?`, `[`, `{`) allowed in one CODEOWNERS pattern.
///
/// Real CODEOWNERS patterns use a few wildcards. A pattern with thousands
/// of them makes each lookup build a large lazy DFA.
const MAX_RULE_WILDCARDS: usize = 32;

/// Wildcards allowed across all patterns of one CODEOWNERS file.
///
/// Bound the total work required to compile patterns. Large public CODEOWNERS
/// files stay below 400 wildcards.
const MAX_FILE_WILDCARDS: usize = 4096;

/// Bytes allowed in one CODEOWNERS pattern.
///
/// The `GlobSet` matches literal prefixes with an Aho-Corasick DFA, and its
/// build time grows with the square of the pattern length. Real CODEOWNERS
/// patterns are far shorter.
const MAX_RULE_BYTES: usize = 512;

/// Target regex-source bytes times glob count in one matcher batch.
///
/// Regex fallback caches multiply automaton states by capture slots for every
/// pattern. Batch this product, rather than only counting wildcards, so long
/// literals cannot multiply the cache cost of every other ownership rule.
/// A larger single glob remains valid and compiles in its own batch.
const MAX_MATCHER_COST: usize = 8 * 1024 * 1024;

/// One CODEOWNERS rule line.
#[derive(Debug)]
struct Rule {
    /// Pattern text from the file. An exclusion keeps its `!` prefix.
    pattern: String,
    /// Primary owner. `None` for a GitLab exclusion (`!path`).
    owner: Option<String>,
    /// Number of owners on the rule. Zero for an exclusion.
    owner_count: u32,
    /// Index into `CodeOwners::sections`. `None` before the first header.
    section: Option<usize>,
}

/// One GitLab section.
#[derive(Debug)]
struct Section {
    name: String,
    default_owners: Vec<String>,
}

/// The last CODEOWNERS rule that matches a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuleMatch<'a> {
    /// Primary owner. `None` when the rule is a GitLab exclusion (`!path`).
    pub owner: Option<&'a str>,
    /// Number of owners on the rule. Zero for an exclusion.
    pub owner_count: u32,
    /// Pattern text from the file. An exclusion keeps its `!` prefix.
    pub pattern: &'a str,
    /// Section of the rule. `None` for a rule before the first header.
    pub section: Option<&'a str>,
    /// Default owners of the section. Empty outside a section.
    pub section_owners: &'a [String],
}

/// Parsed CODEOWNERS file for ownership lookup.
#[derive(Debug)]
pub struct CodeOwners {
    /// Rules in file order.
    rules: Vec<Rule>,
    /// GitLab sections in the order of their first header.
    sections: Vec<Section>,
    /// Ordered batches keep regex fallback caches independent of other batches.
    globs: Vec<CodeOwnersMatcher>,
    /// Index into `rules` per translated glob, across all matcher batches.
    glob_rules: Vec<usize>,
}

#[derive(Debug)]
struct CodeOwnersMatcher {
    globs: GlobSet,
    /// Position of the batch's first glob in `CodeOwners::glob_rules`.
    glob_offset: usize,
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
    ///
    /// The parser ignores a UTF-8 byte order mark at the start of the content.
    pub fn parse(content: &str) -> Result<Self, String> {
        let content = content.strip_prefix('\u{FEFF}').unwrap_or(content);
        let mut parser = CodeOwnersParser::new();
        for line in content.lines() {
            parser.parse_line(line)?;
        }
        parser.finish()
    }

    /// Index of the last CODEOWNERS rule that matches the path, if any.
    ///
    /// CODEOWNERS resolves ownership with last-match-wins, so the highest
    /// rule index wins. Batches retain glob order, so the first matching batch
    /// in reverse order contains the winning rule.
    fn last_matching_rule(&self, relative_path: &Path) -> Option<usize> {
        let candidate = Candidate::new(relative_path);
        self.globs.iter().rev().find_map(|batch| {
            batch
                .globs
                .matches_candidate(&candidate)
                .into_iter()
                .map(|idx| self.glob_rules[batch.glob_offset + idx])
                .max()
        })
    }

    /// Look up the last CODEOWNERS rule that matches a path (relative to the
    /// project root).
    ///
    /// Returns `None` when no rule matches. For a GitLab exclusion (`!path`)
    /// the result has `owner: None` and `owner_count: 0`. For a rule before the
    /// first section header the result has `section: None` and no section
    /// owners.
    pub fn match_path(&self, relative_path: &Path) -> Option<RuleMatch<'_>> {
        let rule = &self.rules[self.last_matching_rule(relative_path)?];
        let section = rule.section.map(|idx| &self.sections[idx]);
        Some(RuleMatch {
            owner: rule.owner.as_deref(),
            owner_count: rule.owner_count,
            pattern: &rule.pattern,
            section: section.map(|s| s.name.as_str()),
            section_owners: section.map_or(&[][..], |s| s.default_owners.as_slice()),
        })
    }

    /// Look up the primary owner of a file path (relative to project root).
    ///
    /// Returns the first owner from the last matching CODEOWNERS rule,
    /// or `None` if no rule matches or the last matching rule is a
    /// GitLab-style exclusion (`!path`).
    pub fn owner_of(&self, relative_path: &Path) -> Option<&str> {
        self.match_path(relative_path)?.owner
    }

    /// Look up the number of owners matched by the last matching CODEOWNERS rule.
    ///
    /// Returns `Some(0)` when the path is explicitly unowned by a GitLab
    /// negation, and `None` when no CODEOWNERS rule matches.
    pub fn owner_count_of(&self, relative_path: &Path) -> Option<u32> {
        self.match_path(relative_path).map(|m| m.owner_count)
    }

    /// Whether the parsed file contains at least one GitLab section header.
    ///
    /// `--group-by section` errors out when this is false, since every file
    /// would collapse into the `(no section)` bucket.
    pub fn has_sections(&self) -> bool {
        !self.sections.is_empty()
    }
}

struct CodeOwnersParser {
    globs: Vec<Glob>,
    glob_rules: Vec<usize>,
    rules: Vec<Rule>,
    sections: Vec<Section>,
    /// Index into `sections` by lowercase section name.
    section_index: FxHashMap<String, usize>,
    /// `(section index, owner)` pairs in the default owners of a section.
    /// A hash set keeps the merge of a header linear in its owner count.
    section_owners_seen: FxHashSet<(usize, String)>,
    current_section: Option<usize>,
    /// Default owners of the header directly above the current line.
    current_section_owners: Vec<String>,
    wildcards: usize,
}

impl CodeOwnersParser {
    fn new() -> Self {
        Self {
            globs: Vec::new(),
            glob_rules: Vec::new(),
            rules: Vec::new(),
            sections: Vec::new(),
            section_index: FxHashMap::default(),
            section_owners_seen: FxHashSet::default(),
            current_section: None,
            current_section_owners: Vec::new(),
            wildcards: 0,
        }
    }

    fn parse_line(&mut self, line: &str) -> Result<(), String> {
        match parse_codeowners_line(line, &self.current_section_owners) {
            ParsedCodeOwnersLine::Skip => Ok(()),
            ParsedCodeOwnersLine::Section { name, defaults } => {
                self.enter_section(name, defaults);
                Ok(())
            }
            ParsedCodeOwnersLine::Rule {
                pattern,
                owner,
                owner_count,
                negate,
            } => self.add_rule(pattern, owner, owner_count, negate),
        }
    }

    /// Make the section of a header the current section.
    ///
    /// GitLab combines sections with the same name and compares the names
    /// without case. The section keeps the name of its first header.
    fn enter_section(&mut self, name: String, defaults: Vec<String>) {
        let key = name.to_lowercase();
        let idx = if let Some(&idx) = self.section_index.get(&key) {
            idx
        } else {
            let idx = self.sections.len();
            self.sections.push(Section {
                name,
                default_owners: Vec::new(),
            });
            self.section_index.insert(key, idx);
            idx
        };
        for owner in &defaults {
            if self.section_owners_seen.insert((idx, owner.clone())) {
                self.sections[idx].default_owners.push(owner.clone());
            }
        }
        self.current_section = Some(idx);
        self.current_section_owners = defaults;
    }

    fn add_rule(
        &mut self,
        pattern: String,
        owner: String,
        owner_count: u32,
        negate: bool,
    ) -> Result<(), String> {
        if pattern.len() > MAX_RULE_BYTES {
            let start: String = pattern.chars().take(40).collect();
            return Err(format!(
                "CODEOWNERS pattern '{start}...' is longer than {MAX_RULE_BYTES} bytes"
            ));
        }
        let wildcards = pattern
            .bytes()
            .filter(|b| matches!(b, b'*' | b'?' | b'[' | b'{'))
            .count();
        if wildcards > MAX_RULE_WILDCARDS {
            return Err(format!(
                "CODEOWNERS pattern '{pattern}' has more than {MAX_RULE_WILDCARDS} wildcards"
            ));
        }
        self.wildcards += wildcards;
        if self.wildcards > MAX_FILE_WILDCARDS {
            return Err(format!(
                "CODEOWNERS file has more than {MAX_FILE_WILDCARDS} wildcards in its patterns"
            ));
        }

        let rule_idx = self.rules.len();
        for glob_pattern in translate_pattern(&pattern) {
            // A backslash escapes the next character on every platform, as in
            // gitignore. The globset default turns escapes off on Windows.
            let glob = GlobBuilder::new(&glob_pattern)
                .literal_separator(true)
                .backslash_escape(true)
                .build()
                .map_err(|e| format!("invalid CODEOWNERS pattern '{pattern}': {e}"))?;
            self.globs.push(glob);
            self.glob_rules.push(rule_idx);
        }

        self.rules.push(Rule {
            pattern: if negate {
                format!("!{pattern}")
            } else {
                pattern
            },
            owner: (!negate).then_some(owner),
            owner_count,
            section: self.current_section,
        });
        Ok(())
    }

    fn finish(self) -> Result<CodeOwners, String> {
        let mut globs = Vec::new();
        let mut offset = 0;
        let mut remaining = self.globs.into_iter().peekable();
        while remaining.peek().is_some() {
            let mut builder = GlobSetBuilder::new();
            let mut regex_bytes: usize = 0;
            let mut count = 0;
            while let Some(glob) = remaining.next_if(|glob| {
                let next_bytes = regex_bytes.saturating_add(glob.regex().len());
                if count > 0 && next_bytes.saturating_mul(count + 1) > MAX_MATCHER_COST {
                    return false;
                }
                regex_bytes = next_bytes;
                true
            }) {
                builder.add(glob);
                count += 1;
            }
            globs.push(CodeOwnersMatcher {
                globs: builder
                    .build()
                    .map_err(|e| format!("failed to compile CODEOWNERS patterns: {e}"))?,
                glob_offset: offset,
            });
            offset += count;
        }

        Ok(CodeOwners {
            rules: self.rules,
            sections: self.sections,
            globs,
            glob_rules: self.glob_rules,
        })
    }
}

enum ParsedCodeOwnersLine {
    Skip,
    Section {
        name: String,
        defaults: Vec<String>,
    },
    Rule {
        pattern: String,
        owner: String,
        owner_count: u32,
        negate: bool,
    },
}

fn parse_codeowners_line(line: &str, current_section_owners: &[String]) -> ParsedCodeOwnersLine {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return ParsedCodeOwnersLine::Skip;
    }

    if let Some((name, defaults)) = parse_section_header(line) {
        return ParsedCodeOwnersLine::Section { name, defaults };
    }

    let (negate, rest) = if let Some(after) = line.strip_prefix('!') {
        (true, after.trim_start())
    } else {
        (false, line)
    };

    let (pattern, owners_text) = split_pattern(rest);
    if pattern.is_empty() {
        return ParsedCodeOwnersLine::Skip;
    }
    let inline_owners = owners_text.split_whitespace().collect::<Vec<_>>();

    let (owner, owner_count) = if negate {
        (String::new(), 0)
    } else if let Some(owner) = inline_owners.first() {
        (
            (*owner).to_string(),
            u32::try_from(inline_owners.len()).unwrap_or(u32::MAX),
        )
    } else if let Some(owner) = current_section_owners.first() {
        (
            owner.clone(),
            u32::try_from(current_section_owners.len()).unwrap_or(u32::MAX),
        )
    } else {
        return ParsedCodeOwnersLine::Skip;
    };

    ParsedCodeOwnersLine::Rule {
        pattern: pattern.to_string(),
        owner,
        owner_count,
        negate,
    }
}

/// Split a rule line at the first whitespace that a backslash does not escape.
///
/// GitLab writes a space in a path as `\ `, for example
/// `/docs/Security\ Release.md @owner`.
fn split_pattern(line: &str) -> (&str, &str) {
    let mut escaped = false;
    for (idx, ch) in line.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch.is_whitespace() {
            return (&line[..idx], &line[idx..]);
        }
    }
    (line, "")
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

/// Translate a CODEOWNERS pattern to one or more `globset` glob patterns.
///
/// CODEOWNERS uses gitignore semantics. The caller compiles each glob with
/// `literal_separator`, so `*` and `?` do not match `/`.
/// - A leading `/` anchors the pattern to the root (stripped for globset).
/// - A trailing `/` matches the directory contents only (`dir/` → `dir/**`).
/// - A pattern without a trailing `/` matches a file or a directory. A
///   directory match covers all paths below it (`/docs` → `docs` and
///   `docs/**`).
/// - A last segment of only `*` or `**` gets no extra glob. Thus `docs/*`
///   matches the direct children of `docs` only, as GitHub documents.
/// - A pattern without an inner `/` matches at any depth (`*.js` →
///   `**/*.js`, `apps/` → `**/apps/**`).
fn translate_pattern(pattern: &str) -> Vec<String> {
    let (anchored, rest) = match pattern.strip_prefix('/') {
        Some(p) => (true, p),
        None => (false, pattern),
    };
    let (directory_only, body) = match rest.strip_suffix('/') {
        Some(p) => (true, p),
        None => (false, rest),
    };
    if body.is_empty() {
        return vec!["**".to_string()];
    }

    let base = if !anchored && !body.contains('/') {
        format!("**/{body}")
    } else {
        body.to_string()
    };
    let contents = format!("{base}/**");
    if directory_only {
        return vec![contents];
    }

    let last_segment = base.rsplit('/').next().unwrap_or(&base);
    if matches!(last_segment, "*" | "**") {
        vec![base]
    } else {
        vec![base, contents]
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

    #[test]
    fn translate_bare_glob() {
        assert_eq!(translate_pattern("*.js"), ["**/*.js", "**/*.js/**"]);
    }

    #[test]
    fn translate_rooted_pattern() {
        assert_eq!(translate_pattern("/docs/*"), ["docs/*"]);
    }

    #[test]
    fn translate_directory_pattern() {
        assert_eq!(translate_pattern("docs/"), ["**/docs/**"]);
    }

    #[test]
    fn translate_rooted_directory() {
        assert_eq!(translate_pattern("/src/app/"), ["src/app/**"]);
    }

    #[test]
    fn translate_rooted_path_without_trailing_slash() {
        assert_eq!(translate_pattern("/docs"), ["docs", "docs/**"]);
    }

    #[test]
    fn translate_path_with_slash() {
        assert_eq!(
            translate_pattern("src/utils/*.ts"),
            ["src/utils/*.ts", "src/utils/*.ts/**"]
        );
    }

    #[test]
    fn translate_double_star() {
        assert_eq!(
            translate_pattern("**/test_*.py"),
            ["**/test_*.py", "**/test_*.py/**"]
        );
        assert_eq!(translate_pattern("/build/**"), ["build/**"]);
    }

    #[test]
    fn translate_single_file() {
        assert_eq!(
            translate_pattern("Makefile"),
            ["**/Makefile", "**/Makefile/**"]
        );
    }

    #[test]
    fn translate_catch_all() {
        assert_eq!(translate_pattern("*"), ["**/*"]);
    }

    #[test]
    fn parse_simple_codeowners() {
        let content = "* @global-owner\n/src/ @frontend\n*.rs @rust-team\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.rules.len(), 3);
    }

    #[test]
    fn parse_skips_comments_and_blanks() {
        let content = "# Comment\n\n* @owner\n  # Indented comment\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.rules.len(), 1);
    }

    #[test]
    fn parse_multi_owner_takes_first() {
        let content = "*.ts @team-a @team-b @team-c\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.rules[0].owner.as_deref(), Some("@team-a"));
    }

    #[test]
    fn parse_skips_pattern_without_owner() {
        let content = "*.ts\n*.js @owner\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.rules.len(), 1);
        assert_eq!(co.rules[0].owner.as_deref(), Some("@owner"));
    }

    #[test]
    fn long_patterns_do_not_multiply_lookup_resources_by_rule_count() {
        use std::fmt::Write as _;

        let mut content = String::from("[Files] @files\n/docs/escaped\\ file.md @escaped\n");
        for index in 0..256 {
            writeln!(content, "file{index}?").expect("writing to a String cannot fail");
        }
        let literal = "a".repeat(MAX_RULE_BYTES - 2);
        writeln!(
            content,
            "[Long] @long\n{literal}*\"\n[Overrides] @override\n!file0x\nfile1x"
        )
        .expect("writing to a String cannot fail");
        let owners = CodeOwners::parse(&content).unwrap();
        let start = std::time::Instant::now();
        for _ in 0..5 {
            for index in 2..256 {
                assert_eq!(
                    owners.owner_of(Path::new(&format!("file{index}x"))),
                    Some("@files")
                );
            }
            assert_eq!(
                owners.owner_of(Path::new(&format!("{literal}suffix\""))),
                Some("@long")
            );
            assert_eq!(owners.owner_of(Path::new("file0x")), None);
            assert_eq!(owners.owner_of(Path::new("file1x")), Some("@override"));
            assert_eq!(
                owners.owner_of(Path::new("docs/escaped file.md")),
                Some("@escaped")
            );
            assert_eq!(
                owners.match_path(Path::new("file1x")).map(|m| m.section),
                Some(Some("Overrides"))
            );
        }
        assert!(
            start.elapsed() < std::time::Duration::from_secs(25),
            "ownership lookup exceeded its resource regression budget"
        );
    }

    /// The matcher build time grows with the square of the pattern length.
    /// OSS-Fuzz issue 579300911 had a 96 KB pattern.
    #[test]
    fn parse_rejects_rule_that_is_too_long() {
        let content = format!(
            "/{}/ @owner\n/x/{}/ @owner\n",
            "0".repeat(512),
            "0".repeat(4_096)
        );
        let err = CodeOwners::parse(&content).err().unwrap_or_default();
        assert!(err.contains("longer than 512 bytes"), "{err}");

        let content = format!("/{}/ @owner\n", "0".repeat(509));
        assert!(CodeOwners::parse(&content).is_ok());
    }

    #[test]
    fn parse_rejects_rule_with_too_many_wildcards() {
        let content = format!("{} @owner\n", "*a".repeat(33));
        let err = CodeOwners::parse(&content).unwrap_err();
        assert!(err.contains("more than 32 wildcards"), "{err}");
    }

    #[test]
    fn parse_rejects_file_with_too_many_wildcards() {
        let rule = format!("{} @owner\n", "*a".repeat(16));
        let err = CodeOwners::parse(&rule.repeat(257)).unwrap_err();
        assert!(err.contains("more than 4096 wildcards"), "{err}");
    }

    #[test]
    fn parse_counts_every_wildcard_kind() {
        let content = format!("!{} @owner\n", "{a,b}?[x]".repeat(11));
        let err = CodeOwners::parse(&content).unwrap_err();
        assert!(err.contains("more than 32 wildcards"), "{err}");
    }

    #[test]
    fn parse_accepts_wildcards_at_the_limits() {
        let rule = format!("{} @owner\n", "*a".repeat(32));
        let co = CodeOwners::parse(&rule.repeat(128)).unwrap();
        assert_eq!(co.rules.len(), 128);
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
    fn match_path_returns_owner_and_pattern() {
        let content = "* @default\n/src/ @frontend\n*.rs @rust-team\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.match_path(Path::new("src/app.ts"))
                .map(|m| (m.owner, m.pattern)),
            Some((Some("@frontend"), "/src/"))
        );
        assert_eq!(
            co.match_path(Path::new("src/lib.rs"))
                .map(|m| (m.owner, m.pattern)),
            Some((Some("@rust-team"), "*.rs"))
        );
        assert_eq!(
            co.match_path(Path::new("README.md"))
                .map(|m| (m.owner, m.pattern)),
            Some((Some("@default"), "*"))
        );
    }

    #[test]
    fn match_path_no_match() {
        let content = "/src/ @frontend\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.match_path(Path::new("README.md")), None);
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
        let root = std::path::PathBuf::from(
            std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
        )
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
        assert_eq!(co.rules.len(), 1);
        assert_eq!(co.owner_of(Path::new("app.ts")), Some("@owner"));
    }

    #[test]
    fn gitlab_optional_section_header_skipped() {
        let content = "^[Optional Section]\n*.ts @owner\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.rules.len(), 1);
    }

    #[test]
    fn gitlab_section_header_with_approval_count_skipped() {
        let content = "[Section Name][2]\n*.ts @owner\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.rules.len(), 1);
    }

    #[test]
    fn gitlab_optional_section_with_approval_count_skipped() {
        let content = "^[Section Name][3] @fallback-team\nfoo/\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.rules.len(), 1);
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
        assert_eq!(co.rules.len(), 2);
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
        assert_eq!(co.rules.len(), 1);
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
            co.match_path(Path::new("README.md"))
                .map(|m| (m.owner, m.pattern)),
            Some((Some("@default"), "*"))
        );
        let excluded = co.match_path(Path::new("src/vendor/lib.js"));
        assert!(excluded.is_some());
        assert_eq!(excluded.and_then(|m| m.owner), None);
    }

    #[test]
    fn escaped_space_stays_in_pattern() {
        let content = "* @default\n/docs/Security\\ Release.md @security @release\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.match_path(Path::new("docs/Security Release.md"))
                .map(|m| (m.owner, m.pattern)),
            Some((Some("@security"), "/docs/Security\\ Release.md"))
        );
        assert_eq!(
            co.owner_count_of(Path::new("docs/Security Release.md")),
            Some(2)
        );
        assert_eq!(co.owner_of(Path::new("docs/Security")), Some("@default"));
    }

    #[test]
    fn split_pattern_edge_cases() {
        assert_eq!(split_pattern("foo\\\\ @o"), ("foo\\\\", " @o"));
        assert_eq!(split_pattern("foo\\\tbar @o"), ("foo\\\tbar", " @o"));
        assert_eq!(
            split_pattern("caf\u{e9}\\ menu @o"),
            ("caf\u{e9}\\ menu", " @o")
        );
        assert_eq!(split_pattern("foo\\"), ("foo\\", ""));
        assert_eq!(split_pattern("foo\t@o"), ("foo", "\t@o"));
    }

    #[test]
    fn negated_rule_with_escaped_space() {
        let content = "* @default\n!docs/Security\\ Release.md\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("docs/Security Release.md")), None);
        assert_eq!(co.owner_of(Path::new("docs/other.md")), Some("@default"));
    }

    // GitHub documents that `\#` does not escape a comment. Gitignore
    // accepts it, so Fallow reads it as a pattern.
    #[test]
    fn escaped_hash_is_a_pattern() {
        let content = "\\#notes.md @owner\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("#notes.md")), Some("@owner"));
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
    fn match_path_returns_named_section() {
        let content = "\
            [Billing] @billing-team\n\
            src/billing/\n\
            [Search] @search-team\n\
            src/search/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.match_path(Path::new("src/billing/invoice.ts"))
                .map(|m| m.section),
            Some(Some("Billing"))
        );
        assert_eq!(
            co.match_path(Path::new("src/search/indexer.ts"))
                .map(|m| m.section),
            Some(Some("Search"))
        );
    }

    #[test]
    fn match_path_returns_no_section_for_pre_section_rule() {
        let content = "\
            * @default\n\
            [Billing] @billing-team\n\
            src/billing/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.match_path(Path::new("README.md")).map(|m| m.section),
            Some(None)
        );
        assert_eq!(
            co.match_path(Path::new("src/billing/invoice.ts"))
                .map(|m| m.section),
            Some(Some("Billing"))
        );
    }

    #[test]
    fn match_path_returns_none_for_unmatched_path() {
        let content = "[Billing] @billing-team\nsrc/billing/\n";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.match_path(Path::new("src/other/x.ts")), None);
    }

    #[test]
    fn match_path_returns_no_owner_for_negation_last_match() {
        let content = "\
            [Billing] @billing-team\n\
            src/billing/\n\
            !src/billing/vendor/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.match_path(Path::new("src/billing/invoice.ts"))
                .map(|m| m.section),
            Some(Some("Billing"))
        );
        let excluded = co.match_path(Path::new("src/billing/vendor/lib.js"));
        assert!(excluded.is_some());
        assert_eq!(excluded.and_then(|m| m.owner), None);
    }

    #[test]
    fn match_path_returns_section_defaults() {
        let content = "\
            [Billing] @core-reviewers @alice\n\
            src/billing/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        let (section, owners) = co
            .match_path(Path::new("src/billing/invoice.ts"))
            .map(|m| (m.section, m.section_owners))
            .unwrap();
        assert_eq!(section, Some("Billing"));
        assert_eq!(
            owners,
            &["@core-reviewers".to_string(), "@alice".to_string()]
        );
    }

    #[test]
    fn match_path_same_owners_distinct_sections() {
        let content = "\
            [billing] @core-reviewers @alice @bob\n\
            src/billing/\n\
            [notifications] @core-reviewers @alice @bob\n\
            src/notifications/\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        let (billing_sec, _) = co
            .match_path(Path::new("src/billing/invoice.ts"))
            .map(|m| (m.section, m.section_owners))
            .unwrap();
        let (notifications_sec, _) = co
            .match_path(Path::new("src/notifications/email.ts"))
            .map(|m| (m.section, m.section_owners))
            .unwrap();
        assert_eq!(billing_sec, Some("billing"));
        assert_eq!(notifications_sec, Some("notifications"));
    }

    #[test]
    fn match_path_empty_owners_for_pre_section_rule() {
        let content = "* @default\n[Billing]\nsrc/billing/ @billing\n";
        let co = CodeOwners::parse(content).unwrap();
        let (section, owners) = co
            .match_path(Path::new("README.md"))
            .map(|m| (m.section, m.section_owners))
            .unwrap();
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
        assert_eq!(co.rules.len(), 1);
        assert_eq!(co.owner_of(Path::new("adef")), Some("@owner"));
    }

    #[test]
    fn pattern_without_trailing_slash_owns_directory_contents() {
        let content = "\
            src/platform/packages/shared/kbn-try-in-console @elastic/search\n\
            /docs @docs-team\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.owner_of(Path::new(
                "src/platform/packages/shared/kbn-try-in-console/index.ts"
            )),
            Some("@elastic/search")
        );
        assert_eq!(
            co.owner_of(Path::new(
                "src/platform/packages/shared/kbn-try-in-console/src/components/a.tsx"
            )),
            Some("@elastic/search")
        );
        assert_eq!(
            co.owner_of(Path::new("docs/guide/intro.md")),
            Some("@docs-team")
        );
        assert_eq!(
            co.match_path(Path::new("docs/guide/intro.md"))
                .map(|m| (m.owner, m.pattern)),
            Some((Some("@docs-team"), "/docs"))
        );
        assert_eq!(co.owner_count_of(Path::new("docs/a.md")), Some(1));
    }

    #[test]
    fn pattern_without_trailing_slash_still_matches_a_file() {
        let co = CodeOwners::parse("/package.json @root\nMakefile @build\n").unwrap();
        assert_eq!(co.owner_of(Path::new("package.json")), Some("@root"));
        assert_eq!(co.owner_of(Path::new("tools/Makefile")), Some("@build"));
    }

    #[test]
    fn pattern_without_trailing_slash_does_not_match_sibling_prefix() {
        let co = CodeOwners::parse("/docs @docs-team\n").unwrap();
        assert_eq!(co.owner_of(Path::new("docs-old/a.md")), None);
        assert_eq!(co.owner_of(Path::new("docs.md")), None);
    }

    #[test]
    fn directory_pattern_keeps_last_match_wins_order() {
        let content = "\
            /src @general\n\
            /src/lib @lib\n\
            /src/lib/legacy.ts @legacy\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("src/main.ts")), Some("@general"));
        assert_eq!(co.owner_of(Path::new("src/lib/a/b.ts")), Some("@lib"));
        assert_eq!(co.owner_of(Path::new("src/lib/legacy.ts")), Some("@legacy"));

        let reversed = "\
            /src/lib @lib\n\
            /src @general\n\
        ";
        let co = CodeOwners::parse(reversed).unwrap();
        assert_eq!(co.owner_of(Path::new("src/lib/a/b.ts")), Some("@general"));
    }

    #[test]
    fn trailing_star_matches_direct_children_only() {
        let co = CodeOwners::parse("* @default\ndocs/* @docs-team\n").unwrap();
        assert_eq!(
            co.owner_of(Path::new("docs/getting-started.md")),
            Some("@docs-team")
        );
        assert_eq!(
            co.owner_of(Path::new("docs/build-app/troubleshooting.md")),
            Some("@default")
        );
    }

    #[test]
    fn root_star_owns_root_level_files_only() {
        // GitHub matches `/*` like `docs/*`: direct children only. A
        // catch-all for the whole repository needs `*`.
        let co = CodeOwners::parse("/* @root\n").unwrap();
        assert_eq!(co.owner_of(Path::new("package.json")), Some("@root"));
        assert_eq!(co.owner_of(Path::new("src/index.ts")), None);

        let co = CodeOwners::parse("* @all\n").unwrap();
        assert_eq!(co.owner_of(Path::new("src/deep/index.ts")), Some("@all"));
    }

    #[test]
    fn single_star_does_not_cross_directories() {
        let co = CodeOwners::parse("* @default\n/src/*.ts @ts\n").unwrap();
        assert_eq!(co.owner_of(Path::new("src/index.ts")), Some("@ts"));
        assert_eq!(co.owner_of(Path::new("src/nested/a.ts")), Some("@default"));
    }

    #[test]
    fn wildcard_name_owns_matching_directory_contents() {
        let co = CodeOwners::parse("/fixtures/date_* @data\n").unwrap();
        assert_eq!(
            co.owner_of(Path::new("fixtures/date_nanos/data.json")),
            Some("@data")
        );
    }

    #[test]
    fn trailing_double_star_keeps_its_meaning() {
        let co = CodeOwners::parse("/build/logs/** @ops\n").unwrap();
        assert_eq!(co.owner_of(Path::new("build/logs/a/b.log")), Some("@ops"));
        assert_eq!(co.owner_of(Path::new("build/other.log")), None);
    }

    #[test]
    fn unanchored_directory_pattern_matches_at_any_depth() {
        let co = CodeOwners::parse("apps/ @apps-team\n").unwrap();
        assert_eq!(co.owner_of(Path::new("apps/web/a.ts")), Some("@apps-team"));
        assert_eq!(
            co.owner_of(Path::new("packages/x/apps/web/a.ts")),
            Some("@apps-team")
        );
    }

    #[test]
    fn negation_without_trailing_slash_clears_directory_contents() {
        let content = "\
            * @default\n\
            !/src/generated\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(co.owner_of(Path::new("src/generated/types.ts")), None);
        assert_eq!(
            co.owner_count_of(Path::new("src/generated/a/b.ts")),
            Some(0)
        );
        assert_eq!(co.owner_of(Path::new("src/main.ts")), Some("@default"));
    }

    #[test]
    fn gitlab_section_pattern_without_trailing_slash_owns_directory_contents() {
        let content = "\
            [Frontend] @ui\n\
            /src/ui\n\
            [Backend] @api\n\
            /src/api @api-lead\n\
        ";
        let co = CodeOwners::parse(content).unwrap();
        assert_eq!(
            co.match_path(Path::new("src/ui/button/a.tsx")).map(|m| (
                m.section,
                m.section_owners,
                m.pattern
            )),
            Some((Some("Frontend"), ["@ui".to_string()].as_slice(), "/src/ui"))
        );
        assert_eq!(
            co.match_path(Path::new("src/api/users.ts"))
                .map(|m| m.section),
            Some(Some("Backend"))
        );
        assert_eq!(
            co.owner_of(Path::new("src/api/users.ts")),
            Some("@api-lead")
        );
    }

    #[test]
    fn parse_ignores_a_leading_bom() {
        for content in ["\u{FEFF}* @team\n", "* @team\n"] {
            let co = CodeOwners::parse(content).unwrap();
            assert_eq!(co.owner_of(Path::new("src/a.ts")), Some("@team"));
            assert_eq!(co.owner_count_of(Path::new("src/a.ts")), Some(1));
        }
        let co = CodeOwners::parse("\u{FEFF}[Docs] @docs\ndocs/\n").unwrap();
        assert!(co.has_sections());
        assert_eq!(co.owner_of(Path::new("docs/a.md")), Some("@docs"));
    }

    fn section_and_owners<'a>(co: &'a CodeOwners, path: &str) -> (Option<&'a str>, Vec<String>) {
        let m = co.match_path(Path::new(path)).unwrap();
        (m.section, m.section_owners.to_vec())
    }

    fn owner_list(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_string()).collect()
    }

    #[test]
    fn repeated_section_headers_merge_their_default_owners() {
        let co = CodeOwners::parse(
            "[Team] @alice\nsrc/a/\n!src/a/vendor/\n[Other] @carol\nsrc/c/\n[Team] @bob\nsrc/b/\n",
        )
        .unwrap();
        let team = (Some("Team"), owner_list(&["@alice", "@bob"]));
        assert_eq!(section_and_owners(&co, "src/a/x.ts"), team);
        assert_eq!(section_and_owners(&co, "src/b/x.ts"), team);
        assert_eq!(co.owner_of(Path::new("src/a/x.ts")), Some("@alice"));
        assert_eq!(co.owner_of(Path::new("src/b/x.ts")), Some("@bob"));
        assert_eq!(co.owner_count_of(Path::new("src/b/x.ts")), Some(1));

        let excluded = co.match_path(Path::new("src/a/vendor/x.js")).unwrap();
        assert_eq!(excluded.owner, None);
        assert_eq!(excluded.owner_count, 0);
        assert_eq!(excluded.pattern, "!src/a/vendor/");
        assert_eq!(co.owner_count_of(Path::new("src/a/vendor/x.js")), Some(0));
        assert_eq!(co.owner_count_of(Path::new("other/x.ts")), None);
    }

    #[test]
    fn repeated_section_headers_merge_without_case() {
        let co = CodeOwners::parse("[Docs] @a\ndocs/\n[DOCS] @b\nREADME.md\n").unwrap();
        let docs = (Some("Docs"), owner_list(&["@a", "@b"]));
        assert_eq!(section_and_owners(&co, "docs/guide.md"), docs);
        assert_eq!(section_and_owners(&co, "README.md"), docs);
    }

    #[test]
    fn repeated_section_headers_keep_each_owner_one_time() {
        let co = CodeOwners::parse(
            "[Team] @alice @bob\nsrc/a/\n[Team]\nsrc/b/ @x\n[Team] @bob @carol\nsrc/c/\n",
        )
        .unwrap();
        let team = (Some("Team"), owner_list(&["@alice", "@bob", "@carol"]));
        for path in ["src/a/x.ts", "src/b/x.ts", "src/c/x.ts"] {
            assert_eq!(section_and_owners(&co, path), team, "{path}");
        }
    }

    #[test]
    fn a_section_header_keeps_each_default_owner_one_time() {
        let co = CodeOwners::parse("[Team] @a @b @a\nsrc/\n").unwrap();
        assert_eq!(
            section_and_owners(&co, "src/x.ts"),
            (Some("Team"), owner_list(&["@a", "@b"]))
        );
        assert_eq!(co.owner_count_of(Path::new("src/x.ts")), Some(3));
    }
}
