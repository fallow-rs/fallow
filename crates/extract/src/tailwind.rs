//! Tailwind CSS arbitrary-value detection.
//!
//! Tailwind "arbitrary value" utilities (`w-[13px]`, `bg-[#abc]`,
//! `grid-cols-[1fr_2fr]`) hardcode a one-off value in markup instead of using a
//! configured scale token. They are not wrong, but a high count is a design-
//! token-bypass signal that no per-rule linter aggregates across a codebase, and
//! AI-assisted edits over-produce them. This scanner finds them in markup so
//! `fallow health --css` can surface them as candidates. The caller MUST gate on
//! the project actually using Tailwind: the `prefix-[value]` shape is Tailwind-
//! specific in practice but not formally exclusive.

use std::sync::LazyLock;

/// Matches a Tailwind arbitrary-value utility token: a lowercase kebab utility
/// prefix followed immediately by a bracketed value, e.g. `w-[13px]`,
/// `grid-cols-[1fr_2fr]`, `bg-[#abc]`. The bracketed value excludes single and
/// double quotes, backticks, brackets, and whitespace, and is length-capped to
/// avoid runaway matches.
/// Variant prefixes (`hover:`, `md:`, `dark:`) are not captured: the utility +
/// value token is the unit of interest, and the same `w-[13px]` under different
/// variants is the same bypass.
static ARBITRARY_VALUE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"[a-z][a-z0-9]*(?:-[a-z0-9]+)*-\[[^\]\[\s"'`]{1,100}\]"#)
});

/// One use of a Tailwind arbitrary-value utility, with the 1-based line it
/// appears on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TailwindArbitraryUse {
    /// The matched `prefix-[value]` token.
    pub value: String,
    /// 1-based line in the source.
    pub line: u32,
}

/// Scan markup source for Tailwind arbitrary-value utility tokens, one entry per
/// occurrence. The caller must gate this on the project using Tailwind (the
/// token shape is Tailwind-specific but not exclusive).
#[must_use]
pub fn scan_tailwind_arbitrary_values(source: &str) -> Vec<TailwindArbitraryUse> {
    let mut out = Vec::new();
    for m in ARBITRARY_VALUE_RE.find_iter(source) {
        let line = 1 + source[..m.start()].bytes().filter(|&b| b == b'\n').count();
        out.push(TailwindArbitraryUse {
            value: m.as_str().to_owned(),
            line: u32::try_from(line).unwrap_or(u32::MAX),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(source: &str) -> Vec<String> {
        scan_tailwind_arbitrary_values(source)
            .into_iter()
            .map(|u| u.value)
            .collect()
    }

    #[test]
    fn matches_common_arbitrary_value_shapes() {
        let v = values(r#"<div class="w-[13px] bg-[#abc] grid-cols-[1fr_2fr] top-[7px]">x</div>"#);
        assert_eq!(
            v,
            vec!["w-[13px]", "bg-[#abc]", "grid-cols-[1fr_2fr]", "top-[7px]"]
        );
    }

    #[test]
    fn ignores_plain_scale_utilities() {
        // No brackets -> not an arbitrary value.
        let v = values(r#"<div class="w-4 bg-red-500 grid-cols-3">x</div>"#);
        assert!(v.is_empty(), "got {v:?}");
    }

    #[test]
    fn does_not_match_attribute_selectors() {
        // `a[href]` / `[data-x]` are not `prefix-[value]` (no dash before bracket).
        let v = values("a[href] { color: red; } [data-state] { color: blue; }");
        assert!(v.is_empty(), "got {v:?}");
    }

    #[test]
    fn reports_one_based_line() {
        let uses = scan_tailwind_arbitrary_values("\n\n<i class=\"h-[3px]\"></i>");
        assert_eq!(uses.len(), 1);
        assert_eq!(uses[0].line, 3);
    }

    #[test]
    fn captures_utility_prefix_not_variant() {
        // The `hover:` variant is not part of the captured token; the utility +
        // value is.
        let v = values(r#"<a class="hover:w-[20px]">x</a>"#);
        assert_eq!(v, vec!["w-[20px]"]);
    }
}
