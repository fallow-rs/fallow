//! Small invariants shared by the CSS-in-JS front-ends (Phases 3b / 3c / 3d).

/// The synthetic CSS selector every CSS-in-JS front-end wraps its lifted rules
/// in, so top-level declarations are counted as a rule by `compute_css_analytics`.
/// Single-sourced here because the template lifter (3b) and the object serializer
/// (3c) MUST emit the SAME wrapper for the analytics to treat both forms alike;
/// a drift between the two would silently split otherwise-identical clones.
pub(super) const WRAPPER: &str = ".fallow-css-in-js";

/// Count `\n` bytes in `s`. Used by the template and object front-ends to
/// blank-line-pad a lifted rule to its real source line so metric line numbers
/// map back onto the source.
pub(super) fn count_newlines(s: &str) -> usize {
    s.bytes().filter(|&b| b == b'\n').count()
}
