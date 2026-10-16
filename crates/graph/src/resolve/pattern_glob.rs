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
//!   include glob matches it and no exclude glob matches it.
//!
//! A negated group inside another group becomes `*` and excludes nothing.

use std::path::Path;

use globset::{Candidate, GlobBuilder, GlobMatcher, GlobSet, GlobSetBuilder};

/// A compiled dynamic import pattern: the paths that the include glob
/// matches, minus the paths that an exclude glob matches.
pub(super) struct PatternGlob {
    include: GlobMatcher,
    excludes: GlobSet,
}

impl PatternGlob {
    /// Compile `pattern`. Returns `None` when the pattern is not a valid glob.
    pub(super) fn new(pattern: &str) -> Option<Self> {
        let (include, negated_groups) = rewrite(pattern, None);
        let include = build_glob(&include)?.compile_matcher();
        let mut excludes = GlobSetBuilder::new();
        for group in 0..negated_groups {
            excludes.add(build_glob(&rewrite(pattern, Some(group)).0)?);
        }
        Some(Self {
            include,
            excludes: excludes.build().ok()?,
        })
    }

    /// Whether `path` matches the pattern.
    pub(super) fn is_match(&self, path: &Path) -> bool {
        let candidate = Candidate::new(path);
        self.include.is_match_candidate(&candidate) && !self.excludes.is_match_candidate(&candidate)
    }
}

fn build_glob(glob: &str) -> Option<globset::Glob> {
    GlobBuilder::new(glob).empty_alternates(true).build().ok()
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
        PatternGlob::new(pattern)
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
        assert!(PatternGlob::new("[").is_none());
    }
}
