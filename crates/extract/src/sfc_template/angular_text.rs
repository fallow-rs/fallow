//! Text helpers of the Angular template scanner: ICU message heads and
//! string literal masks.

/// ICU message types that Angular accepts after the head expression
/// (`{count, plural, ...}`).
const ICU_MESSAGE_TYPES: [&str; 3] = ["plural", "select", "selectordinal"];

/// Longest ICU head expression that the scanner looks for. Each bare `{` in
/// the text starts a head search, so a search to the end of the template at
/// each brace would make the scan quadratic. Real heads are short member
/// reads or calls, with no braces.
const MAX_ICU_HEAD_BYTES: usize = 256;

/// Read the head of an ICU message that starts at the `{` at `index`
/// (`{count, plural, =1 {one} other {many}}`).
///
/// Returns the head expression and the index after the comma that follows the
/// message type. Returns `None` when the text after the brace is not an ICU
/// message head.
pub(super) fn icu_message_head(source: &str, index: usize) -> Option<(&str, usize)> {
    let head_start = index + 1;
    let head_end = head_start + top_level_comma(&source[head_start..])?;
    let head = &source[head_start..head_end];
    if head.trim().is_empty() {
        return None;
    }
    let rest = &source[head_end + 1..];
    let type_text = rest.trim_start();
    let type_len = type_text
        .find(|ch: char| !ch.is_ascii_alphabetic())
        .unwrap_or(type_text.len());
    if !ICU_MESSAGE_TYPES.contains(&&type_text[..type_len]) {
        return None;
    }
    let after_type = type_text[type_len..].trim_start();
    let after_comma = after_type.strip_prefix(',')?;
    Some((head, source.len() - after_comma.len()))
}

/// Byte offset of the first `,` outside brackets and strings in `text`, or
/// `None` when a brace, a `<`, the end of `text` or [`MAX_ICU_HEAD_BYTES`]
/// comes first. The stop at the next `{` keeps the searches from consecutive
/// braces apart.
fn top_level_comma(text: &str) -> Option<usize> {
    let mut depth = 0u32;
    let mut in_string: Option<u8> = None;
    let mut escaped = false;
    for (offset, byte) in text.bytes().enumerate().take(MAX_ICU_HEAD_BYTES) {
        crate::cost::walk_step();
        if let Some(quote) = in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == quote {
                in_string = None;
            }
            continue;
        }
        match byte {
            b'\'' | b'"' | b'`' => in_string = Some(byte),
            b'(' | b'[' => depth += 1,
            b')' | b']' => depth = depth.checked_sub(1)?,
            b'{' | b'}' | b'<' => return None,
            b',' if depth == 0 => return Some(offset),
            _ => {}
        }
    }
    None
}

/// Replace the text of each string literal in `snippet` with spaces, so that a
/// `this.member` pattern inside a string is not read as a member reference.
/// The expressions inside a template literal (`${this.member}`) stay.
pub(super) fn mask_string_contents(snippet: &str) -> String {
    let mut masked = snippet.as_bytes().to_vec();
    // `None` is code. `Some(depth)` is a `${...}` interpolation with `depth`
    // open braces of its own.
    let mut template_frames: Vec<Option<u32>> = Vec::new();
    let mut in_quote: Option<u8> = None;
    let mut index = 0;
    while index < masked.len() {
        let byte = masked[index];
        if let Some(quote) = in_quote {
            if byte == quote {
                in_quote = None;
            } else {
                masked[index] = b' ';
                if byte == b'\\' && index + 1 < masked.len() {
                    index += 1;
                    masked[index] = b' ';
                }
            }
            index += 1;
            continue;
        }
        match template_frames.last_mut() {
            Some(None) => {
                if byte == b'`' {
                    template_frames.pop();
                } else if byte == b'$' && masked.get(index + 1) == Some(&b'{') {
                    template_frames.push(Some(0));
                    index += 1;
                } else {
                    masked[index] = b' ';
                    if byte == b'\\' && index + 1 < masked.len() {
                        index += 1;
                        masked[index] = b' ';
                    }
                }
            }
            frame => match byte {
                b'\'' | b'"' => in_quote = Some(byte),
                b'`' => template_frames.push(None),
                b'{' => {
                    if let Some(Some(depth)) = frame {
                        *depth += 1;
                    }
                }
                b'}' => match frame {
                    Some(Some(0)) => {
                        template_frames.pop();
                    }
                    Some(Some(depth)) => *depth -= 1,
                    _ => {}
                },
                _ => {}
            },
        }
        index += 1;
    }
    // Only whole string contents between ASCII delimiters become spaces, so
    // every multi-byte character is replaced in full.
    String::from_utf8(masked).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::super::angular::collect_angular_template_refs;

    #[test]
    #[expect(
        clippy::literal_string_with_formatting_args,
        reason = "template braces are Angular syntax, not Rust formatting arguments"
    )]
    fn object_literal_at_interpolation_start_still_scans() {
        let refs = collect_angular_template_refs("<p>{{{a:first}}}</p><p>{{ second }}</p>");
        assert!(refs.identifiers.contains("first"), "{:?}", refs.identifiers);
        assert!(
            refs.identifiers.contains("second"),
            "{:?}",
            refs.identifiers
        );
    }

    #[test]
    fn icu_message_head_is_credited() {
        let refs = collect_angular_template_refs(
            "<p>{count, plural, =1 {one} other {many}}</p>\n\
             <p>{gender(), select, male {he} other {they}}</p>\n\
             <p>{pick(a, b), selectordinal, one {st} other {th}}</p>\n\
             <p>{{ later }}</p>",
        );
        for name in ["count", "gender", "pick", "a", "b", "later"] {
            assert!(
                refs.identifiers.contains(name),
                "{name} missing: {:?}",
                refs.identifiers
            );
        }
        for case_text in ["one", "many", "male", "he", "they", "other"] {
            assert!(
                !refs.identifiers.contains(case_text),
                "{:?}",
                refs.identifiers
            );
        }
    }

    #[test]
    fn plain_brace_text_is_not_an_icu_head() {
        let refs = collect_angular_template_refs("<p>{ not, an, icu }</p><p>{{ later }}</p>");
        assert!(!refs.identifiers.contains("not"), "{:?}", refs.identifiers);
        assert!(refs.identifiers.contains("later"), "{:?}", refs.identifiers);
    }

    #[test]
    fn this_member_inside_a_string_is_not_credited() {
        let refs = collect_angular_template_refs(
            r#"<p>{{ 'see this.fake' + "this.fake2" }}</p><button (click)="go('this.alsoFake')">x</button>"#,
        );
        for name in ["fake", "alsoFake", "fake2"] {
            assert!(
                !refs.identifiers.contains(name),
                "{name}: {:?}",
                refs.identifiers
            );
        }
        assert!(refs.identifiers.contains("go"), "{:?}", refs.identifiers);
        let refs = collect_angular_template_refs(
            "<p>{{ `see this.fake and ${this.inTemplate} {}` + 'it\\'s this.fake2' }}</p>",
        );
        assert!(
            refs.identifiers.contains("inTemplate"),
            "{:?}",
            refs.identifiers
        );
        assert!(!refs.identifiers.contains("fake"), "{:?}", refs.identifiers);
        assert!(
            !refs.identifiers.contains("fake2"),
            "{:?}",
            refs.identifiers
        );
    }

    #[test]
    fn non_null_this_member_is_credited() {
        let refs = collect_angular_template_refs("<p>{{ this!.foo }}</p><p>{{ this ! . bar }}</p>");
        assert!(refs.identifiers.contains("foo"), "{:?}", refs.identifiers);
        assert!(refs.identifiers.contains("bar"), "{:?}", refs.identifiers);
    }

    #[test]
    fn this_prefixed_member_is_credited() {
        let refs = collect_angular_template_refs(
            r#"<button (click)="this.viaThis.emit()">x</button><p>{{ this.label }}</p>"#,
        );
        assert!(
            refs.identifiers.contains("viaThis"),
            "{:?}",
            refs.identifiers
        );
        assert!(refs.identifiers.contains("label"), "{:?}", refs.identifiers);
    }

    #[test]
    fn icu_case_opening_into_interpolation_keeps_later_refs() {
        let refs = collect_angular_template_refs(
            "<button (click)=\"beforeIcu.emit()\">b</button>\n\
             <p>{n, plural, =1 {one} other {{{n}} many} }</p>\n\
             <button (click)=\"afterIcu.emit()\">a</button>\n\
             <p>{{ later }}</p>",
        );
        for name in ["beforeIcu", "n", "afterIcu", "later"] {
            assert!(
                refs.identifiers.contains(name),
                "{name} missing: {:?}",
                refs.identifiers
            );
        }
    }

    #[test]
    fn object_literal_interpolation_still_scans() {
        let refs = collect_angular_template_refs("<p>{{{ a: first }.a}}</p><p>{{ second }}</p>");
        assert!(refs.identifiers.contains("first"), "{:?}", refs.identifiers);
        assert!(
            refs.identifiers.contains("second"),
            "{:?}",
            refs.identifiers
        );
    }

    #[test]
    fn unterminated_interpolation_does_not_hide_later_refs() {
        let refs = collect_angular_template_refs(
            "<p>{{ broken </p>\n<button (click)=\"save()\">s</button>",
        );
        assert!(refs.identifiers.contains("save"), "{:?}", refs.identifiers);
    }

    #[test]
    fn unterminated_interpolations_and_icu_heads_scan_in_linear_work() {
        const REPEATS: [usize; 2] = [512, 2048];
        const MAX_SCAN_STEPS_PER_SOURCE_BYTE: usize = 16;

        for unit in ["{{ a ", "{ a "] {
            for repeats in REPEATS {
                let source = format!(
                    "{}<button (click)=\"save()\">s</button>",
                    unit.repeat(repeats)
                );
                let (refs, work) = crate::cost::measure(|| collect_angular_template_refs(&source));
                assert!(refs.identifiers.contains("save"), "{:?}", refs.identifiers);
                assert!(
                    work.walk_steps <= MAX_SCAN_STEPS_PER_SOURCE_BYTE * source.len(),
                    "{unit:?} x {repeats} exceeded the linear scan budget: {} steps for {} source bytes",
                    work.walk_steps,
                    source.len()
                );
            }
        }
    }
}
