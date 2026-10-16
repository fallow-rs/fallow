//! Extglob groups for `globset` patterns.
//!
//! Vite, Vitest, Storybook, Rstest, Jasmine and Playwright match globs with
//! libraries that support the extglob groups `@(a|b)`, `?(a|b)`, `+(a|b)`,
//! `*(a|b)` and `!(a|b)`. The `globset` crate reads these groups as literal
//! text, so a pattern such as `**/!(*.spec).js` matches no file. This module
//! rewrites each group into `globset` syntax.
//!
//! An include pattern must never match fewer files than the tool does, and an
//! exclude pattern must never match more files than the tool does. Thus each
//! group has two rewrites:
//!
//! | Group | Include rewrite | Exclude rewrite |
//! | --- | --- | --- |
//! | `@(a\|b)` | `{a,b}`, exact | `{a,b}`, exact |
//! | `?(a\|b)` | `{,a,b}`, exact | `{,a,b}`, exact |
//! | `+(a\|b)` | `{a,b}*`, wider | `{a,b}`, narrower |
//! | `*(a\|b)` | `*`, wider | `{,a,b}`, narrower |
//! | `!(a\|b)` | `*` with an exclude glob | no rewrite |
//!
//! `globset` cannot repeat a group, so the wider and narrower forms are the
//! closest safe forms.
//!
//! In an include pattern, `!(a|b)` becomes `*`. One exclude glob per negated
//! group puts `{a,b}` at the position of that group. A path matches when the
//! include glob matches it and no exclude glob matches it. Compile each exclude
//! glob with `literal_separator(true)`, so that its `*` stays in one path
//! segment. Then an exclude glob never removes a path that the tool matches
//! through a different path segment. A negated group inside another group
//! becomes `*` and excludes nothing.
//!
//! Compile every rewritten glob with `empty_alternates(true)`, because the
//! rewrite of `?(a|b)` has an empty alternative.

use std::path::Path;

use globset::{Candidate, GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};

/// The `globset` form of an include pattern with extglob groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtglobInclude {
    /// The include glob. It matches each path that the pattern matches.
    pub include: String,
    /// One exclude glob per negated group. A path that an exclude glob
    /// matches does not match the pattern.
    pub excludes: Vec<String>,
}

/// Rewrite the extglob groups of an include pattern. A pattern without
/// extglob groups stays as it is and gives no exclude glob.
#[must_use]
pub fn rewrite_include(pattern: &str) -> ExtglobInclude {
    let (include, negated_groups) =
        rewrite(pattern, Mode::Include { focus: None }).unwrap_or_else(|| (pattern.to_string(), 0));
    let excludes = (0..negated_groups)
        .filter_map(|group| rewrite(pattern, Mode::Include { focus: Some(group) }))
        .map(|(exclude, _)| exclude)
        .collect();
    ExtglobInclude { include, excludes }
}

/// Rewrite the extglob groups of an exclude pattern into a glob that never
/// matches more paths than the pattern does. Returns `None` when the pattern
/// has a negated group, which has no such rewrite. Then the exclude pattern
/// must remove no path.
#[must_use]
pub fn rewrite_exclude(pattern: &str) -> Option<String> {
    rewrite(pattern, Mode::Exclude).map(|(exclude, _)| exclude)
}

/// Whether `pattern` contains a closed extglob group, such as `@(a|b)`.
#[must_use]
pub fn has_extglob_group(pattern: &str) -> bool {
    let mut rest = pattern;
    while let Some((start, _)) = next_group(rest) {
        let body_start = start + 2;
        if group_body_len(&rest[body_start..]).is_some() {
            return true;
        }
        rest = &rest[body_start..];
    }
    false
}

/// A compiled pattern with extglob groups and exclude patterns: the paths
/// that the include glob matches, minus the paths that an exclude glob
/// matches.
#[derive(Debug, Clone)]
pub struct ExtglobMatcher {
    include: GlobMatcher,
    excludes: GlobSet,
}

impl ExtglobMatcher {
    /// Compile `pattern` with the exclude patterns `excludes`. The `*` of the
    /// include glob also matches `/`. Returns `None` when `pattern` is not a
    /// valid glob. An exclude pattern that is not valid, or that has no safe
    /// rewrite, removes no path.
    #[must_use]
    pub fn new(pattern: &str, excludes: &[&str]) -> Option<Self> {
        let rewritten = rewrite_include(pattern);
        let include = build_glob(&rewritten.include, false)?.compile_matcher();
        let mut exclude_set = GlobSetBuilder::new();
        for exclude in &rewritten.excludes {
            exclude_set.add(build_glob(exclude, true)?);
        }
        for exclude in excludes {
            if let Some(glob) = rewrite_exclude(exclude).and_then(|glob| build_glob(&glob, true)) {
                exclude_set.add(glob);
            }
        }
        Some(Self {
            include,
            excludes: exclude_set.build().ok()?,
        })
    }

    /// Whether `path` matches the pattern.
    #[must_use]
    pub fn is_match(&self, path: &Path) -> bool {
        let candidate = Candidate::new(path);
        self.include.is_match_candidate(&candidate) && !self.excludes.is_match_candidate(&candidate)
    }
}

fn build_glob(glob: &str, literal_separator: bool) -> Option<globset::Glob> {
    GlobBuilder::new(glob)
        .empty_alternates(true)
        .literal_separator(literal_separator)
        .build()
        .ok()
}

/// How [`rewrite`] treats the groups that have no exact rewrite.
#[derive(Debug, Clone, Copy)]
enum Mode {
    /// Match at least the paths of the pattern. Each top-level negated group
    /// becomes `*`, except the group with the index `focus`, which becomes
    /// its alternatives.
    Include { focus: Option<usize> },
    /// Match at most the paths of the pattern. A negated group gives no
    /// rewrite.
    Exclude,
}

/// Rewrite the extglob groups of `pattern` into `globset` syntax. Returns the
/// rewritten glob and the number of top-level negated groups, or `None` in
/// [`Mode::Exclude`] for a pattern with a negated group.
fn rewrite(pattern: &str, mode: Mode) -> Option<(String, usize)> {
    let nested = match mode {
        Mode::Include { .. } => Mode::Include { focus: None },
        Mode::Exclude => Mode::Exclude,
    };
    let mut out = String::with_capacity(pattern.len());
    let mut negated = 0;
    let mut rest = pattern;
    while let Some((start, operator)) = next_group(rest) {
        let body_start = start + 2;
        let Some(body_len) = group_body_len(&rest[body_start..]) else {
            break;
        };
        out.push_str(&rest[..start]);
        let alternatives = alternatives(&rest[body_start..body_start + body_len], nested)?;
        let exclude = matches!(mode, Mode::Exclude);
        match operator {
            b'@' => out.push_str(&brace(&alternatives)),
            b'?' => out.push_str(&optional(alternatives)),
            b'+' => {
                out.push_str(&brace(&alternatives));
                if !exclude {
                    out.push('*');
                }
            }
            b'*' if exclude => out.push_str(&optional(alternatives)),
            b'*' => out.push('*'),
            _ => match mode {
                Mode::Exclude => return None,
                Mode::Include { focus } => {
                    if focus == Some(negated) {
                        out.push_str(&brace(&alternatives));
                    } else {
                        out.push('*');
                    }
                    negated += 1;
                }
            },
        }
        rest = &rest[body_start + body_len + 1..];
    }
    out.push_str(rest);
    Some((out, negated))
}

/// The byte offset and operator of the first extglob group opener in `text`.
fn next_group(text: &str) -> Option<(usize, u8)> {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            operator @ (b'@' | b'?' | b'+' | b'*' | b'!') if bytes[index + 1] == b'(' => {
                return Some((index, operator));
            }
            _ => index += 1,
        }
    }
    None
}

/// The length of a group body up to its closing parenthesis. Returns `None`
/// when the group has no closing parenthesis.
fn group_body_len(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 1,
            b'(' => depth += 1,
            b')' if depth == 0 => return Some(index),
            b')' => depth -= 1,
            _ => {}
        }
        index += 1;
    }
    None
}

/// Split a group body on its top-level `|` and rewrite each alternative.
fn alternatives(body: &str, mode: Mode) -> Option<Vec<String>> {
    let bytes = body.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut part_start = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 1,
            b'(' => depth += 1,
            b')' => depth = depth.saturating_sub(1),
            b'|' if depth == 0 => {
                parts.push(rewrite(&body[part_start..index], mode)?.0);
                part_start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    parts.push(rewrite(&body[part_start..], mode)?.0);
    Some(parts)
}

fn brace(alternatives: &[String]) -> String {
    match alternatives {
        [single] => single.clone(),
        _ => format!("{{{}}}", alternatives.join(",")),
    }
}

/// Zero or one of the alternatives.
fn optional(alternatives: Vec<String>) -> String {
    let mut optional = vec![String::new()];
    optional.extend(alternatives);
    brace(&optional)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, path: &str) -> bool {
        matches_with_excludes(pattern, &[], path)
    }

    fn matches_with_excludes(pattern: &str, excludes: &[&str], path: &str) -> bool {
        ExtglobMatcher::new(pattern, excludes)
            .expect("pattern should compile")
            .is_match(Path::new(path))
    }

    #[test]
    fn negated_group_excludes_its_alternatives() {
        let pattern = "**/!(*.spec).{j,t}s";
        assert!(matches(pattern, "alpha.js"));
        assert!(matches(pattern, "nested/beta.ts"));
        assert!(!matches(pattern, "alpha.spec.js"));
        assert!(!matches(pattern, "nested/beta.spec.ts"));
        assert!(!matches(pattern, "alpha.css"));
    }

    #[test]
    fn negated_group_with_several_alternatives() {
        let pattern = "modules/!(index|*.test).js";
        assert!(matches(pattern, "modules/alpha.js"));
        assert!(!matches(pattern, "modules/index.js"));
        assert!(!matches(pattern, "modules/alpha.test.js"));
    }

    #[test]
    fn every_negated_group_excludes_on_its_own() {
        let pattern = "!(internal)/!(index).js";
        assert!(matches(pattern, "public/alpha.js"));
        assert!(!matches(pattern, "internal/alpha.js"));
        assert!(!matches(pattern, "public/index.js"));
    }

    #[test]
    fn negated_group_exclude_stays_in_one_segment() {
        let pattern = "src/!(*legacy)/**/*.js";
        assert!(matches(pattern, "src/new/legacy/x.js"));
        assert!(!matches(pattern, "src/oldlegacy/x.js"));
        assert!(matches("**/!(_*)/*.js", "_a/b/x.js"));
        assert!(!matches("**/!(_*)/*.js", "a/_b/x.js"));
    }

    #[test]
    fn detects_closed_extglob_groups() {
        assert!(has_extglob_group("./locales/@(en|nl).js"));
        assert!(has_extglob_group("a/!(b).js"));
        assert!(!has_extglob_group("./locales/"));
        assert!(!has_extglob_group("a/!(b.js"));
        assert!(!has_extglob_group(r"a/\!(b).js"));
        assert!(!has_extglob_group("@[(]a|b)/*.js"));
    }

    #[test]
    fn exactly_one_group_matches_only_its_alternatives() {
        assert!(matches("locales/@(en|nl).json", "locales/en.json"));
        assert!(matches("locales/@(en|nl).json", "locales/nl.json"));
        assert!(!matches("locales/@(en|nl).json", "locales/de.json"));
    }

    #[test]
    fn optional_group_also_matches_without_the_group() {
        assert!(matches("*.test.?(c|m)js", "a.test.js"));
        assert!(matches("*.test.?(c|m)js", "a.test.mjs"));
        assert!(!matches("*.test.?(c|m)js", "a.test.tjs"));
    }

    #[test]
    fn repeated_groups_match_at_least_the_tool_matches() {
        assert!(matches("a+(x|y).js", "ax.js"));
        assert!(matches("a+(x|y).js", "axyx.js"));
        assert!(!matches("a+(x|y).js", "a.js"));
        assert!(matches("a*(x|y).js", "a.js"));
        assert!(matches("a*(x|y).js", "axy.js"));
    }

    #[test]
    fn nested_groups_rewrite_inside_out() {
        let pattern = "*.@(mdx|stories.@(js|ts))";
        assert!(matches(pattern, "a.mdx"));
        assert!(matches(pattern, "a.stories.ts"));
        assert!(!matches(pattern, "a.stories.css"));
    }

    #[test]
    fn plain_globs_and_escapes_keep_their_meaning() {
        assert!(matches("**/*.ts", "deep/nested.ts"));
        assert!(matches("{a,b}/*.js", "b/x.js"));
        assert!(matches(r"\!(x).js", "!(x).js"));
        assert!(!matches(r"\!(x).js", "y.js"));
    }

    #[test]
    fn unclosed_group_stays_literal() {
        assert_eq!(
            rewrite_include("a/!(b.js"),
            ExtglobInclude {
                include: "a/!(b.js".to_string(),
                excludes: Vec::new(),
            }
        );
    }

    #[test]
    fn invalid_glob_does_not_compile() {
        assert!(ExtglobMatcher::new("[", &[]).is_none());
    }

    #[test]
    fn include_rewrite_lists_one_exclude_per_negated_group() {
        assert_eq!(
            rewrite_include("src/!(a|b)/!(index).js"),
            ExtglobInclude {
                include: "src/*/*.js".to_string(),
                excludes: vec!["src/{a,b}/*.js".to_string(), "src/*/index.js".to_string()],
            }
        );
        assert_eq!(
            rewrite_include("**/*.+(ts|tsx)").include,
            "**/*.{ts,tsx}*".to_string()
        );
    }

    #[test]
    fn exclude_rewrite_never_matches_more_than_the_pattern() {
        assert_eq!(
            rewrite_exclude("a/@(x|y).js").as_deref(),
            Some("a/{x,y}.js")
        );
        assert_eq!(rewrite_exclude("a/?(x).js").as_deref(), Some("a/{,x}.js"));
        assert_eq!(
            rewrite_exclude("a/+(x|y).js").as_deref(),
            Some("a/{x,y}.js")
        );
        assert_eq!(
            rewrite_exclude("a/*(x|y).js").as_deref(),
            Some("a/{,x,y}.js")
        );
        assert_eq!(rewrite_exclude("a/!(x).js"), None);
        assert_eq!(rewrite_exclude("a/@(x|!(y)).js"), None);
        assert_eq!(rewrite_exclude("a/*.js").as_deref(), Some("a/*.js"));
    }

    #[test]
    fn exclude_patterns_remove_their_matches() {
        let excludes = ["pages/skip.js", "**/draft.js"];
        assert!(matches_with_excludes(
            "pages/*.js",
            &excludes,
            "pages/home.js"
        ));
        assert!(!matches_with_excludes(
            "pages/*.js",
            &excludes,
            "pages/skip.js"
        ));
        assert!(!matches_with_excludes(
            "pages/*.js",
            &excludes,
            "pages/draft.js"
        ));
    }

    #[test]
    fn exclude_pattern_star_stays_in_one_segment() {
        let excludes = ["pages/*.js"];
        assert!(!matches_with_excludes(
            "pages/**/*.js",
            &excludes,
            "pages/a.js"
        ));
        assert!(matches_with_excludes(
            "pages/**/*.js",
            &excludes,
            "pages/deep/a.js"
        ));
    }

    #[test]
    fn exclude_pattern_groups_use_the_narrower_rewrite() {
        let pattern = "pages/*.js";
        assert!(!matches_with_excludes(
            pattern,
            &["pages/@(a|b).js"],
            "pages/a.js"
        ));
        assert!(matches_with_excludes(
            pattern,
            &["pages/@(a|b).js"],
            "pages/c.js"
        ));
        assert!(!matches_with_excludes(
            pattern,
            &["pages/+(a)x.js"],
            "pages/ax.js"
        ));
        // Vite skips `aax.js` too, but the narrower rewrite keeps it. A kept
        // file is the safe direction.
        assert!(matches_with_excludes(
            pattern,
            &["pages/+(a)x.js"],
            "pages/aax.js"
        ));
        assert!(matches_with_excludes(
            pattern,
            &["pages/!(a).js"],
            "pages/b.js"
        ));
    }

    #[test]
    fn invalid_exclude_pattern_removes_no_file() {
        assert!(matches_with_excludes(
            "pages/*.js",
            &["pages/["],
            "pages/a.js"
        ));
    }

    #[test]
    fn one_character_classes_match_literal_metacharacters() {
        let pattern = "[[]id[]]/*.js";
        assert!(matches(pattern, "[id]/page.js"));
        assert!(!matches(pattern, "i/page.js"));
        assert!(matches("[{]group[}]/[*][?]/*.js", "{group}/*?/a.js"));
        assert!(matches("x[(]a)/*.js", "x(a)/b.js"));
    }
}
