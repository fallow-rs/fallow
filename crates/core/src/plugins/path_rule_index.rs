//! An index of compiled path rules by the literal directory of each pattern.
//!
//! A monorepo gives each workspace its own copy of the plugin rules, with the
//! workspace path as prefix (`packages/web/app/**/page.tsx`). A module in one
//! workspace can only match the rules of its own directory chain, so the
//! index tests a path only against those rules, not against the rules of
//! every workspace (issue #5196).

use rustc_hash::FxHashMap;

use super::{CompiledPathRule, PathRule};

/// Characters that start glob syntax. A backslash is an escape, so the text
/// after it is not a plain literal either.
const GLOB_SYNTAX: &[char] = &['*', '?', '[', ']', '{', '}', '\\'];

/// Compiled path rules, each with a value, grouped by the literal directory
/// that every path the rule matches must start with.
///
/// [`Self::matching`] gives the same values in the same order as a test of
/// each rule in sequence.
#[derive(Debug, Clone)]
pub struct PathRuleIndex<T> {
    rules: Vec<(CompiledPathRule, T)>,
    by_dir: FxHashMap<Box<str>, Vec<usize>>,
}

impl<T> Default for PathRuleIndex<T> {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            by_dir: FxHashMap::default(),
        }
    }
}

impl<T> PathRuleIndex<T> {
    /// Compile each rule and index it. A rule with an invalid glob matches no
    /// file, so the index drops it.
    pub fn build<'r>(rules: impl IntoIterator<Item = (&'r PathRule, T)>, rule_kind: &str) -> Self {
        let mut index = Self::default();
        let mut excludes = super::ExcludeCache::default();
        for (rule, value) in rules {
            let Some(compiled) =
                CompiledPathRule::for_entry_rule_cached(rule, rule_kind, &mut excludes)
            else {
                continue;
            };
            let position = index.rules.len();
            index.rules.push((compiled, value));
            index
                .by_dir
                .entry(literal_dir(&rule.pattern).into())
                .or_default()
                .push(position);
        }
        index
    }

    /// Whether the index holds no rule.
    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// The values of the rules that match the project-relative `path`, in the
    /// order of [`Self::build`].
    pub fn matching(&self, path: &str) -> Vec<&T> {
        let mut positions = self.candidates(path);
        positions.sort_unstable();
        positions
            .into_iter()
            .filter_map(|position| {
                let (compiled, value) = &self.rules[position];
                compiled.matches(path).then_some(value)
            })
            .collect()
    }

    /// Whether a rule matches the project-relative `path`.
    pub fn any_match(&self, path: &str) -> bool {
        self.candidates(path)
            .into_iter()
            .any(|position| self.rules[position].0.matches(path))
    }

    /// The positions of the rules whose literal directory is `path` itself
    /// or a directory of `path`. The path itself counts because a glob such
    /// as `src/**` can also match `src`.
    fn candidates(&self, path: &str) -> Vec<usize> {
        let mut positions = Vec::new();
        if self.rules.is_empty() {
            return positions;
        }
        let normalized = normalize_separators(path);
        let mut push = |dir: &str| {
            if let Some(found) = self.by_dir.get(dir) {
                positions.extend_from_slice(found);
            }
        };
        push("");
        for (end, _) in normalized.match_indices('/') {
            if end > 0 {
                push(&normalized[..end]);
            }
        }
        if !normalized.is_empty() {
            push(&normalized);
        }
        positions
    }
}

/// The directory part of the literal text before the first glob syntax of
/// `pattern`. Each path that `pattern` matches starts with this directory and
/// a `/`, or is the directory itself. A pattern with no literal directory
/// gives the empty string, which every path looks up.
fn literal_dir(pattern: &str) -> &str {
    let literal = pattern
        .find(GLOB_SYNTAX)
        .map_or(pattern, |end| &pattern[..end]);
    literal.rfind('/').map_or("", |end| &literal[..end])
}

/// The glob matcher reads a backslash in a candidate path as a separator on
/// Windows only, so the lookup does the same.
fn normalize_separators(path: &str) -> std::borrow::Cow<'_, str> {
    if cfg!(windows) && path.contains('\\') {
        std::borrow::Cow::Owned(path.replace('\\', "/"))
    } else {
        std::borrow::Cow::Borrowed(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(patterns: &[&str]) -> (Vec<PathRule>, PathRuleIndex<usize>) {
        let rules: Vec<PathRule> = patterns.iter().map(|p| PathRule::new(*p)).collect();
        let index = PathRuleIndex::build(rules.iter().zip(0..), "test pattern");
        (rules, index)
    }

    /// The order and result of a test of each rule in sequence.
    fn linear(rules: &[PathRule], path: &str) -> Vec<usize> {
        rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| {
                CompiledPathRule::for_used_export_rule(rule, "test pattern")
                    .is_some_and(|compiled| compiled.matches(path))
            })
            .map(|(position, _)| position)
            .collect()
    }

    #[test]
    fn literal_dir_stops_at_glob_syntax() {
        assert_eq!(
            literal_dir("packages/web/app/**/page.tsx"),
            "packages/web/app"
        );
        assert_eq!(literal_dir("packages/web/src/x.ts"), "packages/web/src");
        assert_eq!(literal_dir("packages/w*/x.ts"), "packages");
        assert_eq!(literal_dir("packages/{a,b}/x.ts"), "packages");
        assert_eq!(literal_dir("mocks/[[]exact[]].js"), "mocks");
        assert_eq!(literal_dir("pkg\\*/x.ts"), "");
        assert_eq!(literal_dir("**/page.tsx"), "");
        assert_eq!(literal_dir("next.config.ts"), "");
        assert_eq!(literal_dir("/abs/x/*.ts"), "/abs/x");
    }

    #[test]
    fn matches_the_same_rules_in_the_same_order_as_a_linear_scan() {
        let patterns = [
            "**/*.ts",
            "packages/a/src/**",
            "packages/b/src/**/*.ts",
            "packages/a/src/m1.ts",
            "packages/*/src/m1.ts",
            "packages/{a,b}/src/*.ts",
            "packages/a/**",
            "src/**",
            "packages/a/src/[",
            "packages/a/src/m?.ts",
            "packages/ab/src/**",
            "/abs/x/*.ts",
            "/*.ts",
            "./src/x.ts",
            "packages/a/../a/src/m1.ts",
            "packages//a/src/m1.ts",
            "packages\\a/src/m1.ts",
        ];
        let (mut rules, _) = index(&patterns);
        rules.push(
            PathRule::new("packages/a/src/*.ts").with_excluded_globs(["packages/a/src/m1.ts"]),
        );
        let index = PathRuleIndex::build(rules.iter().zip(0..), "test pattern");
        for path in [
            "/abs/x/a.ts",
            "/root.ts",
            "./src/x.ts",
            "packages/a/src/m2.ts",
            "packages//a/src/m1.ts",
            "packages/a/src/m1.ts",
            "packages/b/src/m1.ts",
            "packages/ab/src/m1.ts",
            "packages/a",
            "packages/a/src",
            "src",
            "src/x.ts",
            "root.ts",
            "",
        ] {
            let found: Vec<usize> = index.matching(path).into_iter().copied().collect();
            assert_eq!(found, linear(&rules, path), "path {path}");
            assert_eq!(index.any_match(path), !found.is_empty(), "path {path}");
        }
    }

    #[test]
    fn a_module_only_meets_the_rules_of_its_own_workspace() {
        let patterns: Vec<String> = (0..50)
            .map(|w| format!("packages/pkg{w:02}/app/**/page.tsx"))
            .collect();
        let refs: Vec<&str> = patterns.iter().map(String::as_str).collect();
        let (_, index) = index(&refs);
        assert_eq!(index.candidates("packages/pkg07/app/x/page.tsx"), vec![7]);
        assert!(index.candidates("packages/pkg07/src/m.ts").is_empty());
        let found: Vec<usize> = index
            .matching("packages/pkg07/app/x/page.tsx")
            .into_iter()
            .copied()
            .collect();
        assert_eq!(found, vec![7]);
    }

    #[cfg(windows)]
    #[test]
    fn looks_up_a_path_with_backslashes_on_windows() {
        let (rules, index) = index(&["packages/a/src/**", "packages/b/**"]);
        let path = "packages\\a\\src\\m1.ts";
        let found: Vec<usize> = index.matching(path).into_iter().copied().collect();
        assert_eq!(found, linear(&rules, path));
        assert_eq!(found, vec![0]);
    }

    #[test]
    fn drops_invalid_globs() {
        let (_, index) = index(&["src/["]);
        assert!(index.is_empty());
    }
}
