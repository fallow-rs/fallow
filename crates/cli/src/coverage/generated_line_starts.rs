//! Line starts of a generated script for the `x_fallow_line_starts` source
//! map field.
//!
//! A browser Long Animation Frame names its entry point by a character offset
//! in the served script, and a source map needs a generated line and column.
//! The upload adds one offset per generated line of the map: the offset in the
//! served script of column 0 of that line. With it, the cloud can attribute an
//! offset in a bundle that has more than one line, for example a chunk with a
//! template literal that holds a line break.
//!
//! Offsets count UTF-16 code units, like a JavaScript string index and a
//! source map column. Lines end at `\n`. A UTF-8 byte order mark does not
//! count, because the browser drops it before it counts offsets.
//!
//! `posthog-cli sourcemap process` puts a chunk id snippet in front of line 1
//! and a `//# chunkId=` line at the end of a script, and does not change the
//! map. Line 1 then starts after the snippet. When the script shows only one
//! half of that layout, the offsets are unknown and the field is left out.
//!
//! `tests/fixtures/served-line-starts-contract.json` holds the cases that this
//! code and the Fallow Cloud upload helper must both pass.

/// The source map field.
pub(super) const LINE_STARTS_FIELD: &str = "x_fallow_line_starts";

/// The cloud reads at most this many line starts.
const MAX_LINES: usize = 1_000_000;

/// A larger generated file gets no field.
pub(super) const MAX_GENERATED_BYTES: u64 = 50 * 1024 * 1024;

const BYTE_ORDER_MARK: char = '\u{feff}';
const POSTHOG_SNIPPET_START: &str = "!function(){try{var e=\"undefined\"!=typeof window";
const POSTHOG_ID_START: &str = "e._posthogChunkIds[n]=\"";
const POSTHOG_SNIPPET_END: &str = "\")}catch(e){}}();";
const POSTHOG_TRAILER: &str = "\n//# chunkId=";
const CHUNK_ID_LEN: usize = 36;

/// The served offset of column 0 of each generated line, or `None` for an
/// unknown layout or a script with more lines than the cloud reads.
pub(super) fn served_line_starts(served: &str) -> Option<Vec<u64>> {
    let text = served.strip_prefix(BYTE_ORDER_MARK).unwrap_or(served);
    let prefix = injected_prefix_len(text)?;
    let mut starts = vec![utf16_len(&text[..prefix])];
    let mut units: u64 = 0;
    let mut from = 0;
    for (index, _) in text.match_indices('\n') {
        if starts.len() == MAX_LINES {
            return None;
        }
        units += utf16_len(&text[from..=index]);
        from = index + 1;
        starts.push(units);
    }
    Some(starts)
}

/// One half of the PostHog layout in a script.
enum Part<'a> {
    Absent,
    /// The chunk id, and for the snippet its byte length.
    Found(&'a str, usize),
    /// The start of the half is there, but it does not parse.
    Unknown,
}

/// The byte length of the PostHog snippet in front of line 1: 0 without one,
/// `None` when the snippet and the trailer do not belong together or one of
/// them does not parse.
fn injected_prefix_len(text: &str) -> Option<usize> {
    match (posthog_snippet(text), posthog_trailer(text)) {
        (Part::Absent, Part::Absent) => Some(0),
        (Part::Found(id, len), Part::Found(trailer, _)) if id == trailer => Some(len),
        _ => None,
    }
}

/// The PostHog snippet at the start of the script.
fn posthog_snippet(text: &str) -> Part<'_> {
    if !text.starts_with(POSTHOG_SNIPPET_START) {
        return Part::Absent;
    }
    let line = text.split('\n').next().unwrap_or_default();
    let Some(id_start) = line
        .find(POSTHOG_ID_START)
        .map(|at| at + POSTHOG_ID_START.len())
    else {
        return Part::Unknown;
    };
    let id_end = id_start + CHUNK_ID_LEN;
    match (line.get(id_start..id_end), line.get(id_end..)) {
        (Some(id), Some(rest)) if is_chunk_id(id) && rest.starts_with(POSTHOG_SNIPPET_END) => {
            Part::Found(id, id_end + POSTHOG_SNIPPET_END.len())
        }
        _ => Part::Unknown,
    }
}

/// The `//# chunkId=` line at the end of the script.
fn posthog_trailer(text: &str) -> Part<'_> {
    // ASCII whitespace only, as in the Fallow Cloud upload helper, so both
    // agree on characters such as U+FEFF and U+0085 after the trailer.
    let trimmed = text.trim_end_matches(|ch: char| ch.is_ascii_whitespace());
    let Some(at) = trimmed.rfind(POSTHOG_TRAILER) else {
        return Part::Absent;
    };
    let id = &trimmed[at + POSTHOG_TRAILER.len()..];
    if id.contains('\n') {
        // The comment is not the last line, so it is not a trailer.
        return Part::Absent;
    }
    if id.len() == CHUNK_ID_LEN && is_chunk_id(id) {
        Part::Found(id, 0)
    } else {
        Part::Unknown
    }
}

fn is_chunk_id(id: &str) -> bool {
    id.bytes()
        .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f' | b'-'))
}

fn utf16_len(text: &str) -> u64 {
    if text.is_ascii() {
        return text.len() as u64;
    }
    text.chars().map(|ch| ch.len_utf16() as u64).sum()
}

/// Puts the line starts of `served` into the map as its last key, or removes
/// a stale field when `served` is `None` or its layout is unknown. The cloud
/// reads the field from the end of the stored map.
pub(super) fn set_line_starts(source_map: &mut serde_json::Value, served: Option<&str>) {
    let Some(object) = source_map.as_object_mut() else {
        return;
    };
    object.shift_remove(LINE_STARTS_FIELD);
    if let Some(starts) = served.and_then(served_line_starts) {
        object.insert(LINE_STARTS_FIELD.to_owned(), serde_json::json!(starts));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHUNK_ID: &str = "01a1843f-c040-7940-891b-0ae120e501fc";
    const BUILT: &str = "import{a}from'./a.js';\nconst t=`x\ny`;\nexport{t};";

    /// The snippet that `posthog-cli sourcemap process` 0.7.14 writes.
    fn snippet(id: &str) -> String {
        format!(
            "!function(){{try{{var e=\"undefined\"!=typeof window?window:\"undefined\"!=typeof global?global:\"undefined\"!=typeof globalThis?globalThis:\"undefined\"!=typeof self?self:{{}},n=(new e.Error).stack;n&&(e._posthogChunkIds=e._posthogChunkIds||{{}},e._posthogChunkIds[n]=\"{id}\")}}catch(e){{}}}}();"
        )
    }

    #[test]
    fn gives_the_offset_of_each_line() {
        assert_eq!(served_line_starts(BUILT), Some(vec![0, 23, 34, 38]));
        assert_eq!(served_line_starts(""), Some(vec![0]));
    }

    #[test]
    fn starts_line_one_after_the_posthog_snippet() {
        let prefix = snippet(CHUNK_ID).len() as u64;
        assert_eq!(prefix, 309);
        let served = format!("{}{BUILT}\n//# chunkId={CHUNK_ID}\n", snippet(CHUNK_ID));
        // The trailer line and the empty line after its line break follow the
        // four lines of the chunk.
        assert_eq!(
            served_line_starts(&served),
            Some(vec![
                prefix,
                prefix + 23,
                prefix + 34,
                prefix + 38,
                prefix + 49,
                prefix + 98,
            ])
        );
    }

    #[test]
    fn gives_no_starts_when_the_snippet_and_trailer_do_not_belong_together() {
        let other = "11111111-2222-3333-4444-555555555555";
        let mismatched = format!("{}{BUILT}\n//# chunkId={other}", snippet(CHUNK_ID));
        let trailer_only = format!("{BUILT}\n//# chunkId={CHUNK_ID}");
        let snippet_only = format!("{}{BUILT}", snippet(CHUNK_ID));
        assert_eq!(served_line_starts(&mismatched), None);
        assert_eq!(served_line_starts(&trailer_only), None);
        assert_eq!(served_line_starts(&snippet_only), None);
    }

    #[test]
    fn accepts_whitespace_after_the_trailer_and_refuses_an_uppercase_id() {
        let served = format!("{}{BUILT}\n//# chunkId={CHUNK_ID}  \n\n", snippet(CHUNK_ID));
        assert_eq!(
            served_line_starts(&served).map(|starts| starts[0]),
            Some(309)
        );
        let upper = CHUNK_ID.to_uppercase();
        let served = format!("{}{BUILT}\n//# chunkId={upper}", snippet(&upper));
        assert_eq!(served_line_starts(&served), None);
    }

    #[test]
    fn gives_no_starts_when_a_half_of_the_layout_starts_but_does_not_parse() {
        let broken_snippet = format!("{}{BUILT}", snippet("not-a-chunk-id"));
        let broken_trailer = format!("{BUILT}\n//# chunkId=not-a-chunk-id");
        assert_eq!(served_line_starts(&broken_snippet), None);
        assert_eq!(served_line_starts(&broken_trailer), None);
    }

    #[test]
    fn keeps_the_starts_when_a_chunk_id_comment_is_not_the_last_line() {
        let served = format!("//# chunkId={CHUNK_ID}\nx");
        assert_eq!(served_line_starts(&served), Some(vec![0, 49]));
    }

    #[test]
    fn counts_utf16_units_and_drops_a_byte_order_mark() {
        // "é" is one UTF-16 unit and two UTF-8 bytes; "😀" is two units and four bytes.
        assert_eq!(served_line_starts("é\n😀\nx"), Some(vec![0, 2, 5]));
        assert_eq!(served_line_starts("\u{feff}ab\ncd"), Some(vec![0, 3]));
    }

    /// Fallow Cloud runs the same cases against its upload helper, so a change
    /// on either side that alters a result fails one of the two suites.
    #[test]
    fn served_line_starts_match_the_shared_contract() {
        let path = std::path::Path::new(
            &std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
        )
        .join("../../tests/fixtures/served-line-starts-contract.json");
        let text = std::fs::read_to_string(&path).expect("read contract fixture");
        let contract: serde_json::Value = serde_json::from_str(&text).expect("parse contract");
        assert_eq!(contract["schemaVersion"], 1);
        let cases = contract["cases"].as_array().expect("cases array");
        assert!(!cases.is_empty());
        for case in cases {
            let name = case["name"].as_str().expect("name");
            let served = case["served"].as_str().expect("served");
            let expected: Option<Vec<u64>> = case["starts"].as_array().map(|starts| {
                starts
                    .iter()
                    .map(|start| start.as_u64().expect("start number"))
                    .collect()
            });
            assert_eq!(served_line_starts(served), expected, "case: {name}");
        }
    }

    #[test]
    fn gives_no_starts_above_the_line_cap() {
        let at_cap = "\n".repeat(MAX_LINES - 1);
        assert_eq!(
            served_line_starts(&at_cap).map(|starts| starts.len()),
            Some(MAX_LINES)
        );
        assert_eq!(served_line_starts(&"\n".repeat(MAX_LINES)), None);
    }

    #[test]
    fn set_line_starts_appends_the_field_last_and_drops_a_stale_one() {
        let mut map = serde_json::json!({
            "version": 3,
            LINE_STARTS_FIELD: [0, 1],
            "mappings": "",
        });
        set_line_starts(&mut map, Some("a\nb"));
        let keys: Vec<&String> = map.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["version", "mappings", LINE_STARTS_FIELD]);
        assert_eq!(map[LINE_STARTS_FIELD], serde_json::json!([0, 2]));

        set_line_starts(&mut map, None);
        assert!(map.get(LINE_STARTS_FIELD).is_none());
    }
}
