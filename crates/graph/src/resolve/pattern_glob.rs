//! Glob matching for dynamic import patterns, with extglob groups.
//!
//! Vite matches `import.meta.glob` patterns with picomatch, which supports the
//! extglob groups `@(a|b)`, `?(a|b)`, `+(a|b)`, `*(a|b)` and `!(a|b)`. The
//! `globset` crate reads these groups as literal text, so a pattern such as
//! `./**/!(*.spec).js` matches no file. This module rewrites each group into
//! `globset` syntax before it compiles the pattern.
//!
//! The rewrite never matches fewer files than picomatch does:
//!
//! - `@(a|b)` becomes `{a,b}` and `?(a|b)` becomes `{,a,b}`. Both are exact.
//! - `+(a|b)` becomes `{a,b}*` and `*(a|b)` becomes `*`. Both match more files,
//!   because `globset` cannot repeat a group.
//! - `!(a|b)` becomes `*` in the include glob. One exclude glob per negated
//!   group puts `{a,b}` at the position of that group. A path matches when the
//!   include glob matches it and no exclude glob matches it. In an exclude
//!   glob, `*` does not match `/`, so an exclude glob never removes a path
//!   that picomatch matches through a different path segment.
//!
//! A negated group inside another group becomes `*` and excludes nothing.
//!
//! A pattern can also have exclude globs of its own, from the negative entries
//! of an `import.meta.glob` array. An exclude glob must never match more files
//! than picomatch does, so only an exclude glob with exact groups (`@(..)` and
//! `?(..)`) or no groups removes files. An exclude glob that is not valid
//! removes no file.

use std::path::Path;

use globset::{Candidate, GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};

/// A compiled dynamic import pattern: the paths that the include glob
/// matches, minus the paths that an exclude glob matches.
pub(super) struct PatternGlob {
    include: GlobMatcher,
    excludes: GlobSet,
}

impl PatternGlob {
    /// Compile `pattern` with the exclude globs `excludes`. Returns `None`
    /// when `pattern` is not a valid glob.
    pub(super) fn new(pattern: &str, excludes: &[&str]) -> Option<Self> {
        let (include, negated_groups) = rewrite(pattern, None);
        let include = build_glob(&include, false)?.compile_matcher();
        let mut exclude_set = GlobSetBuilder::new();
        for group in 0..negated_groups {
            exclude_set.add(build_glob(&rewrite(pattern, Some(group)).0, true)?);
        }
        for exclude in excludes {
            if has_only_exact_groups(exclude)
                && let Some(glob) = build_glob(&rewrite(exclude, None).0, true)
            {
                exclude_set.add(glob);
            }
        }
        Some(Self {
            include,
            excludes: exclude_set.build().ok()?,
        })
    }

    /// Whether `path` matches the pattern.
    pub(super) fn is_match(&self, path: &Path) -> bool {
        let candidate = Candidate::new(path);
        self.include.is_match_candidate(&candidate) && !self.excludes.is_match_candidate(&candidate)
    }
}

/// Whether `pattern` contains a closed extglob group, such as `@(a|b)`.
pub(super) fn has_extglob_group(pattern: &str) -> bool {
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

/// Whether every closed extglob group of `pattern` has an exact rewrite.
/// Only `@(..)` and `?(..)` do, also when they are nested.
fn has_only_exact_groups(pattern: &str) -> bool {
    let mut rest = pattern;
    while let Some((start, operator)) = next_group(rest) {
        let body_start = start + 2;
        if matches!(operator, b'+' | b'*' | b'!') && group_body_len(&rest[body_start..]).is_some() {
            return false;
        }
        rest = &rest[body_start..];
    }
    true
}

/// Build a glob. An exclude glob sets `literal_separator`, so that its `*`
/// stays in one path segment. A narrower exclude glob is the safe direction:
/// it can keep an extra file, but it cannot drop a file that Vite loads.
fn build_glob(glob: &str, literal_separator: bool) -> Option<globset::Glob> {
    GlobBuilder::new(glob)
        .empty_alternates(true)
        .literal_separator(literal_separator)
        .build()
        .ok()
}

/// Rewrite the extglob groups of `pattern` into `globset` syntax.
///
/// Each top-level negated group becomes `*`, except the group with the index
/// `focus`, which becomes its alternatives. Returns the rewritten glob and the
/// number of top-level negated groups.
fn rewrite(pattern: &str, focus: Option<usize>) -> (String, usize) {
    let mut out = String::with_capacity(pattern.len());
    let mut negated = 0;
    let mut rest = pattern;
    while let Some((start, operator)) = next_group(rest) {
        let body_start = start + 2;
        let Some(body_len) = group_body_len(&rest[body_start..]) else {
            break;
        };
        out.push_str(&rest[..start]);
        let alternatives = alternatives(&rest[body_start..body_start + body_len]);
        match operator {
            b'@' => out.push_str(&brace(&alternatives)),
            b'?' => {
                let mut optional = vec![String::new()];
                optional.extend(alternatives);
                out.push_str(&brace(&optional));
            }
            b'+' => {
                out.push_str(&brace(&alternatives));
                out.push('*');
            }
            b'!' => {
                if focus == Some(negated) {
                    out.push_str(&brace(&alternatives));
                } else {
                    out.push('*');
                }
                negated += 1;
            }
            _ => out.push('*'),
        }
        rest = &rest[body_start + body_len + 1..];
    }
    out.push_str(rest);
    (out, negated)
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
fn alternatives(body: &str) -> Vec<String> {
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
                parts.push(rewrite(&body[part_start..index], None).0);
                part_start = index + 1;
            }
            _ => {}
        }
        index += 1;
    }
    parts.push(rewrite(&body[part_start..], None).0);
    parts
}

fn brace(alternatives: &[String]) -> String {
    match alternatives {
        [single] => single.clone(),
        _ => format!("{{{}}}", alternatives.join(",")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn matches(pattern: &str, path: &str) -> bool {
        matches_with_excludes(pattern, &[], path)
    }

    fn matches_with_excludes(pattern: &str, excludes: &[&str], path: &str) -> bool {
        PatternGlob::new(pattern, excludes)
            .expect("pattern should compile")
            .is_match(Path::new(path))
    }

    #[test]
    fn exclude_globs_remove_their_matches() {
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
    fn exclude_glob_star_stays_in_one_segment() {
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
    fn exclude_glob_keeps_only_exact_extglob_groups() {
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
        // A wider rewrite of `+(..)`, `*(..)` or `!(..)` could drop a file that
        // Vite loads, so such an exclude glob removes no file.
        assert!(matches_with_excludes(
            pattern,
            &["pages/+(a)x.js"],
            "pages/ax.js"
        ));
        assert!(matches_with_excludes(
            pattern,
            &["pages/!(a).js"],
            "pages/b.js"
        ));
    }

    #[test]
    fn invalid_exclude_glob_removes_no_file() {
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
        assert!(!matches(pattern, "d/page.js"));
        assert!(matches("[{]group[}]/[*][?]/*.js", "{group}/*?/a.js"));
        assert!(matches("x[(]a)/*.js", "x(a)/b.js"));
        assert!(!has_extglob_group("@[(]a|b)/*.js"));
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
    fn repeated_groups_match_at_least_the_picomatch_matches() {
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
        assert_eq!(rewrite("a/!(b.js", None), ("a/!(b.js".to_string(), 0));
    }

    #[test]
    fn invalid_glob_does_not_compile() {
        assert!(PatternGlob::new("[", &[]).is_none());
    }
}
