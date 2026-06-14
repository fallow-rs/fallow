//! Markup CSS-class reference scanning and class-name similarity.
//!
//! Supports the `fallow health --css` class-reach candidates (the CSS analogue
//! of `unresolved-import`). [`scan_markup_class_tokens`] pulls the STATIC class
//! tokens out of `class` / `className` attributes across every markup surface
//! fallow visits (JSX/TSX, HTML, Vue/Svelte/Astro), and flags whether the file
//! also constructs classes DYNAMICALLY (`clsx(...)`, `` `btn-${x}` ``,
//! `:class`, spread props), which downstream consumers use to abstain.
//!
//! The scanner is intentionally regex-based and conservative: it only collects
//! tokens from a fully-static quoted attribute value, and treats anything that
//! could be an interpolation as a dynamic signal rather than a token. It never
//! tries to evaluate a dynamic expression.

use std::sync::LazyLock;

/// A static class token referenced in markup, with the 1-based line it sits on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MarkupClassToken {
    /// The bare class name (no dot), e.g. `card-title`.
    pub value: String,
    /// 1-based line of the attribute in the source.
    pub line: u32,
}

/// The result of scanning one markup source for class references.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MarkupClassScan {
    /// Class tokens from fully-static `class` / `className` attribute values.
    pub static_tokens: Vec<MarkupClassToken>,
    /// True when the file constructs classes dynamically anywhere (`clsx(...)`,
    /// template literals, `:class`, spread/computed props). Consumers that need
    /// to prove a class unused must abstain on dynamic files; a typo check on a
    /// static token can still fire.
    pub has_dynamic: bool,
}

/// Matches a fully-static `class="..."` / `className="..."` attribute (double or
/// single quoted) and captures the raw value. The value is split into tokens by
/// the caller; a value containing `{`, `}`, `$`, or a backtick is treated as a
/// dynamic interpolation (Svelte `class="a-{b}"`, Vue mustache) and skipped for
/// token extraction.
static STATIC_CLASS_ATTR_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r#"(?:\bclass|\bclassName)\s*=\s*(?:"([^"]*)"|'([^']*)')"#)
});

/// Substrings that prove a markup file constructs class names dynamically. Any
/// hit sets [`MarkupClassScan::has_dynamic`].
const DYNAMIC_CLASS_MARKERS: &[&str] = &[
    "className={", // JSX expression container
    "className ={",
    "class={",      // Svelte / JSX
    "class ={",     // tolerate whitespace
    ":class",       // Vue v-bind shorthand
    "v-bind:class", // Vue v-bind long form
    "[class]",      // Angular property binding
    "[ngClass]",    // Angular ngClass
    "class:",       // Svelte class directive `class:active`
    "clsx(",        // common class-combiner libraries
    "classnames(",
    "classNames(",
    "cx(",
    "cva(",
    "twMerge(",
    "tw`",       // tailwind tagged template
    "classList", // DOM classList manipulation
];

/// True when a static class value carries an interpolation and must not be
/// tokenized (the tokens would be partial / wrong). Such a value also implies
/// the file is dynamic.
fn value_is_interpolated(value: &str) -> bool {
    value.contains('{') || value.contains('}') || value.contains('$') || value.contains('`')
}

/// A token is a usable class name only if it looks like an authored class: it is
/// non-empty, contains no whitespace (already split), and carries no markup /
/// interpolation punctuation. Tailwind variant (`hover:`) and opacity (`/50`)
/// shapes are left in (they simply never match an authored CSS class or a near
/// miss downstream), but obvious non-class noise is dropped.
fn is_plausible_class_token(token: &str) -> bool {
    !token.is_empty() && !token.contains(['{', '}', '$', '`', '"', '\'', '(', ')', '<', '>', '='])
}

/// Scan a markup source for static class tokens and a dynamic-construction flag.
///
/// `class="a b c"` yields three tokens; `className={clsx(...)}` and
/// `class="a-{x}"` yield no tokens but set `has_dynamic`.
#[must_use]
pub fn scan_markup_class_tokens(source: &str) -> MarkupClassScan {
    let has_dynamic = DYNAMIC_CLASS_MARKERS.iter().any(|m| source.contains(m));
    let mut static_tokens = Vec::new();
    let mut any_interpolated = false;

    for caps in STATIC_CLASS_ATTR_RE.captures_iter(source) {
        let Some(m) = caps.get(0) else { continue };
        let value = caps
            .get(1)
            .or_else(|| caps.get(2))
            .map_or("", |g| g.as_str());
        if value_is_interpolated(value) {
            any_interpolated = true;
            continue;
        }
        let line = 1 + source[..m.start()].bytes().filter(|&b| b == b'\n').count();
        let line = u32::try_from(line).unwrap_or(u32::MAX);
        for token in value.split_whitespace() {
            if is_plausible_class_token(token) {
                static_tokens.push(MarkupClassToken {
                    value: token.to_owned(),
                    line,
                });
            }
        }
    }

    MarkupClassScan {
        static_tokens,
        has_dynamic: has_dynamic || any_interpolated,
    }
}

/// True when `a` and `b` differ by exactly one single-character edit (one
/// substitution, insertion, or deletion). Equal strings return false. Runs in
/// O(min(len)) without building a full edit-distance matrix.
///
/// Used to surface a likely className typo: a markup token that matches no
/// defined class but is one edit from a class that IS defined (`card-tite` vs
/// `card-title`). Restricting to distance one keeps the suggestion near-zero
/// false-positive.
#[must_use]
pub fn is_edit_distance_one(a: &str, b: &str) -> bool {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    let (la, lb) = (ab.len(), bb.len());
    if la == lb {
        // Same length: exactly one substitution.
        let mut diffs = 0;
        for i in 0..la {
            if ab[i] != bb[i] {
                diffs += 1;
                if diffs > 1 {
                    return false;
                }
            }
        }
        return diffs == 1;
    }
    // Differ by one in length: exactly one insertion/deletion. Walk both,
    // allowing a single skip in the longer string.
    if la.abs_diff(lb) != 1 {
        return false;
    }
    let (short, long) = if la < lb { (ab, bb) } else { (bb, ab) };
    let (mut i, mut j, mut skipped) = (0usize, 0usize, false);
    while i < short.len() && j < long.len() {
        if short[i] == long[j] {
            i += 1;
        } else {
            if skipped {
                return false;
            }
            skipped = true; // skip one char in the longer string
        }
        j += 1;
    }
    true
}

/// True when `defined` is a likely TYPO target for `token`: exactly one edit
/// apart AND that edit is a believable mistake, not a deliberate naming
/// variation. This is stricter than [`is_edit_distance_one`] because real
/// codebases are full of one-edit class pairs that are NOT typos:
///
/// - **Numeric-scale families** (`col-lg-6` vs `col-lg-4`, `display-4` vs
///   `display-5`, `gap-2` vs `gap-3`): adjacent members of a Bootstrap /
///   utility scale differ by one digit but are distinct intentional classes.
///   Any edit whose changed / inserted / deleted character is an ASCII digit is
///   rejected.
/// - **Singular/plural pairs** (`button` vs `buttons`): a single trailing `s`
///   is a morphological variant, not a typo. Rejected.
///
/// Real typos (`card-tite` vs `card-title`, `sidebar-nev` vs `sidebar-nav`) are
/// alphabetic edits and pass. Caught by real-world smoke on Bootstrap, where the
/// bare near-miss produced 117 false positives, all numeric-scale or plural.
#[must_use]
pub fn is_typo_edit(token: &str, defined: &str) -> bool {
    let (tb, db) = (token.as_bytes(), defined.as_bytes());
    let (lt, ld) = (tb.len(), db.len());
    if lt == ld {
        // Substitution: find the single differing index; reject if a digit is on
        // either side (a numeric-scale value, not a typo).
        let mut diff = None;
        for i in 0..lt {
            if tb[i] != db[i] {
                if diff.is_some() {
                    return false;
                }
                diff = Some(i);
            }
        }
        return diff.is_some_and(|i| !tb[i].is_ascii_digit() && !db[i].is_ascii_digit());
    }
    if lt.abs_diff(ld) != 1 {
        return false;
    }
    let (short, long) = if lt < ld { (tb, db) } else { (db, tb) };
    // Singular/plural: the longer is the shorter plus a trailing `s`.
    if long.last() == Some(&b's') && short == &long[..long.len() - 1] {
        return false;
    }
    // Locate the single inserted / deleted character.
    let (mut i, mut j, mut skipped) = (0usize, 0usize, false);
    let mut edit_byte = *long.last().unwrap_or(&0);
    while i < short.len() && j < long.len() {
        if short[i] == long[j] {
            i += 1;
        } else {
            if skipped {
                return false;
            }
            skipped = true;
            edit_byte = long[j];
        }
        j += 1;
    }
    // Reject a digit insertion/deletion (numeric-scale variant, not a typo).
    !edit_byte.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(source: &str) -> Vec<String> {
        scan_markup_class_tokens(source)
            .static_tokens
            .into_iter()
            .map(|t| t.value)
            .collect()
    }

    #[test]
    fn extracts_static_class_and_classname_tokens() {
        assert_eq!(
            tokens(r#"<div class="card card-title">x</div>"#),
            vec!["card", "card-title"]
        );
        assert_eq!(
            tokens(r#"<div className="btn btn-primary">x</div>"#),
            vec!["btn", "btn-primary"]
        );
        assert_eq!(tokens(r"<i class='solo'></i>"), vec!["solo"]);
    }

    #[test]
    fn reports_one_based_line() {
        let scan = scan_markup_class_tokens("\n\n<i class=\"on-line-three\"></i>");
        assert_eq!(scan.static_tokens.len(), 1);
        assert_eq!(scan.static_tokens[0].line, 3);
    }

    #[test]
    fn flags_dynamic_construction_and_skips_its_tokens() {
        for src in [
            r#"<div className={clsx("a", x)}>y</div>"#,
            r"<div className={`btn-${size}`}>y</div>",
            r#"<div :class="{ active: isOn }">y</div>"#,
            r#"<div class="a-{cls}">y</div>"#, // Svelte interpolation
            r#"el.classList.add("toggled")"#,
        ] {
            let scan = scan_markup_class_tokens(src);
            assert!(scan.has_dynamic, "expected dynamic for {src:?}");
        }
    }

    #[test]
    fn static_attr_in_dynamic_file_still_yields_its_tokens() {
        // A static class attribute is tokenized even when the file is dynamic;
        // the typo check needs the static token.
        let scan = scan_markup_class_tokens(
            r#"<div className={clsx(x)}>a</div><span class="card-tite">b</span>"#,
        );
        assert!(scan.has_dynamic);
        assert_eq!(
            scan.static_tokens
                .iter()
                .map(|t| t.value.as_str())
                .collect::<Vec<_>>(),
            vec!["card-tite"]
        );
    }

    #[test]
    fn edit_distance_one_substitution() {
        assert!(is_edit_distance_one("card-tite", "card-tit=")); // sanity, one sub
        assert!(is_edit_distance_one("btn-primary", "btn-primery"));
        assert!(!is_edit_distance_one("btn", "btn")); // equal is not distance one
        assert!(!is_edit_distance_one("btn-primary", "btn-secondary"));
    }

    #[test]
    fn edit_distance_one_insertion_deletion() {
        assert!(is_edit_distance_one("card-title", "card-titl")); // deletion
        assert!(is_edit_distance_one("card-titl", "card-title")); // insertion
        assert!(is_edit_distance_one("nav", "navs")); // append
        assert!(!is_edit_distance_one("nav", "navxs")); // distance two
        assert!(!is_edit_distance_one("nav", "xyz")); // unrelated
    }

    #[test]
    fn typo_edit_accepts_real_alphabetic_typos() {
        assert!(is_typo_edit("card-tite", "card-title")); // missing letter
        assert!(is_typo_edit("sidebar-nev", "sidebar-nav")); // wrong letter
        assert!(is_typo_edit("widget-labl", "widget-label")); // dropped letter (not plural)
        assert!(is_typo_edit("headar", "header")); // one letter substitution
    }

    #[test]
    fn typo_edit_rejects_numeric_scale_families() {
        // Adjacent Bootstrap / utility scale members are one digit apart but are
        // distinct intentional classes, never typos.
        assert!(!is_typo_edit("col-lg-6", "col-lg-4")); // digit substitution
        assert!(!is_typo_edit("display-4", "display-5"));
        assert!(!is_typo_edit("gap-2", "gap-3"));
        assert!(!is_typo_edit("display-4", "display-")); // digit deletion
        assert!(!is_typo_edit("z-10", "z-50")); // digit substitution
    }

    #[test]
    fn typo_edit_rejects_singular_plural() {
        assert!(!is_typo_edit("button", "buttons"));
        assert!(!is_typo_edit("buttons", "button"));
        assert!(!is_typo_edit("card", "cards"));
    }
}
