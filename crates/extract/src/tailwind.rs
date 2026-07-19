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
    // Incremental line counter: `find_iter` yields matches in source order, so
    // count only the newlines between the previous match and this one instead of
    // rescanning the whole prefix per match (issue #1843 follow-up: the naive
    // `source[..m.start()]` rescan is O(matches * source_len), worst on a single
    // long line with no newlines).
    let mut last_pos = 0usize;
    let mut last_line = 1usize;
    for m in ARBITRARY_VALUE_RE.find_iter(source) {
        if is_arbitrary_variant_match(source.as_bytes(), m.end()) {
            continue;
        }
        last_line += source
            .get(last_pos..m.start())
            .map_or(0, |s| s.bytes().filter(|&b| b == b'\n').count());
        last_pos = m.start();
        out.push(TailwindArbitraryUse {
            value: m.as_str().to_owned(),
            line: u32::try_from(last_line).unwrap_or(u32::MAX),
        });
    }
    out
}

fn is_arbitrary_variant_match(source: &[u8], end: usize) -> bool {
    if source.get(end) == Some(&b':') {
        return true;
    }
    if source.get(end) != Some(&b'/') {
        return false;
    }
    for &byte in &source[end + 1..] {
        match byte {
            b':' => return true,
            b' ' | b'\n' | b'\r' | b'\t' | b'"' | b'\'' | b'`' => return false,
            _ => {}
        }
    }
    false
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

    #[test]
    fn ignores_arbitrary_variants() {
        let v = values(
            r#"<div class="data-[side=left]:slide-in min-[320px]:text-sm group-data-[collapsible=icon]:hidden group-data-[size=sm]/dialog:grid peer-data-[size=lg]/button:top-2 hover:w-[20px]">x</div>"#,
        );
        assert_eq!(v, vec!["w-[20px]"]);
    }

    #[test]
    fn keeps_arbitrary_value_modifiers() {
        let v = values(r#"<div class="bg-[#fff]/50 ring-[3px]">x</div>"#);
        assert_eq!(v, vec!["bg-[#fff]", "ring-[3px]"]);
    }

    #[test]
    fn line_numbers_match_naive_reference_on_dense_line() {
        // Many arbitrary-value tokens packed onto a single long line (the
        // pathological zero-newline prefix): the incremental line counter must
        // agree byte-for-byte with the naive per-match prefix rescan.
        use std::fmt::Write as _;
        let mut src = String::from("<div class=\"");
        for i in 0..500 {
            let _ = write!(src, "w-[{i}px] ");
        }
        src.push_str("\">x</div>\n<span class=\"h-[3px]\"></span>");

        let got: Vec<(String, u32)> = scan_tailwind_arbitrary_values(&src)
            .into_iter()
            .map(|u| (u.value, u.line))
            .collect();

        // Reference: recompute each match's line via a full prefix rescan.
        let want: Vec<(String, u32)> = ARBITRARY_VALUE_RE
            .find_iter(&src)
            .filter(|m| !is_arbitrary_variant_match(src.as_bytes(), m.end()))
            .map(|m| {
                let line = 1 + src[..m.start()].bytes().filter(|&b| b == b'\n').count();
                (
                    m.as_str().to_owned(),
                    u32::try_from(line).unwrap_or(u32::MAX),
                )
            })
            .collect();

        assert_eq!(got, want);
        assert!(got.len() > 500, "expected the dense line plus the trailer");
        // The trailer sits on line 2; the packed tokens all sit on line 1.
        assert_eq!(got.last().map(|(_, l)| *l), Some(2));
        assert!(got[..got.len() - 1].iter().all(|(_, l)| *l == 1));
    }
}
