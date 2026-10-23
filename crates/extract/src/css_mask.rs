//! Byte-preserving masks for the CSS Modules class scan.
//!
//! Each mask replaces bytes with ASCII spaces (`0x20`) of equal byte length.
//! Byte offsets in the masked string thus stay equal to the offsets in the
//! original source.

use std::sync::LazyLock;

/// Regex to find a Sass module member call such as `math.div(` or
/// `utils.bar(`. Group 1 is the `.` between the namespace and the member.
/// A selector never puts `(` right after a class name, so `div.active` and
/// `a.link:hover` do not match (issue #3839).
static SCSS_NAMESPACED_CALL_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r"[a-zA-Z_][a-zA-Z0-9_-]*(\.)[a-zA-Z_][a-zA-Z0-9_-]*\s*\(")
});

/// Regex to find a namespaced mixin include such as `@include utils.baz`,
/// also without an argument list. Group 1 is the `.` between the namespace and
/// the mixin name (issue #3839).
static SCSS_NAMESPACED_INCLUDE_RE: LazyLock<regex::Regex> = LazyLock::new(|| {
    crate::static_regex(r"@include\s+[a-zA-Z_][a-zA-Z0-9_-]*(\.)[a-zA-Z_][a-zA-Z0-9_-]*")
});

/// Mask every regex match in `src` with ASCII spaces (`0x20`) of equal byte
/// length, so byte offsets in the returned string correspond 1:1 to byte
/// offsets in the original.
///
/// Used to neutralise CSS comments, quoted strings, `url(...)`, and at-rule
/// preludes before scanning for `.class` selectors, while preserving the
/// original-source positions that callers need to populate `ExportInfo.span`
/// (issue #549). The `regex` crate guarantees match boundaries respect UTF-8
/// char boundaries, so the masked buffer is always valid UTF-8.
pub fn mask_with_whitespace(src: &str, re: &regex::Regex) -> String {
    let mut out = String::with_capacity(src.len());
    let mut cursor = 0;
    for m in re.find_iter(src) {
        out.push_str(&src[cursor..m.start()]);
        out.extend(std::iter::repeat_n(' ', m.len()));
        cursor = m.end();
    }
    out.push_str(&src[cursor..]);
    out
}

/// Mask capture group 1 of every match of `re` in `src` with ASCII spaces of
/// equal byte length. The rest of each match stays, so byte offsets and
/// parentheses are kept.
fn mask_capture_with_whitespace(src: &str, re: &regex::Regex) -> String {
    let mut out = String::with_capacity(src.len());
    let mut cursor = 0;
    for cap in re.captures_iter(src) {
        let Some(group) = cap.get(1) else {
            continue;
        };
        out.push_str(&src[cursor..group.start()]);
        out.extend(std::iter::repeat_n(' ', group.len()));
        cursor = group.end();
    }
    out.push_str(&src[cursor..]);
    out
}

/// Mask the `.` of Sass module members such as `math.div(...)` and
/// `@include utils.baz`, so the class scan does not read the member name as a
/// class (issue #3839).
pub fn mask_sass_module_members(src: &str) -> String {
    let masked = mask_capture_with_whitespace(src, &SCSS_NAMESPACED_CALL_RE);
    mask_capture_with_whitespace(&masked, &SCSS_NAMESPACED_INCLUDE_RE)
}
