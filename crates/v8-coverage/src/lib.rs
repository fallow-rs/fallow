//! V8 `ScriptCoverage` JSON parser and UTF-16 source-offset mapper.
//!
//! This is the open-source layer of fallow's runtime-coverage pipeline. It
//! provides the two things the `fallow` CLI consumes:
//!
//! 1. Serde input types for the V8 coverage dump format emitted by
//!    `node --experimental-test-coverage`, `c8`, the Inspector protocol, or
//!    any V8 isolate ([`V8CoverageDump`] and friends).
//! 2. [`LineOffsetTable`], which converts V8 source offsets into 1-indexed
//!    line / 0-indexed column [`IstanbulPosition`]s.
//!
//! ## Offset semantics (load-bearing)
//!
//! V8 reports coverage offsets in **UTF-16 code units**, not UTF-8 bytes (V8
//! strings are UTF-16). Verified against real Node: a function preceded by a
//! `😀` (2 UTF-16 units / 4 UTF-8 bytes) on the same line is reported at the
//! UTF-16 offset, not the byte offset. [`LineOffsetTable`] therefore stores
//! line starts in UTF-16 units. This is the invariant the `line_table_*` tests
//! pin, and the one a byte-offset implementation gets wrong.
//!
//! ## Relationship to `oxc_coverage_v8`
//!
//! `oxc_coverage_v8` (in `oxc-coverage-instrument`) solves the inverse problem:
//! it takes an AST-built Istanbul `FileCoverage` and fills its statement /
//! function / branch counts by converting Istanbul positions into **byte**
//! offsets. The two crates are intentionally not consolidated: opposite
//! directions, opposite unit spaces, and different producers (real Node V8
//! dumps here vs. an instrumenter-controlled pipeline there). See
//! `decisions/010-v8-coverage-vs-oxc-coverage-boundary.md`.
//!
//! The closed-source cross-reference, combined scoring, hot-path heuristics and
//! verdict generation live in `fallow-cov` (private) and consume the CLI's
//! remapped function output via the `fallow-cov-protocol` envelope.

#![forbid(unsafe_code)]

use serde::{Deserialize, Deserializer, Serialize};

// -- V8 input types ---------------------------------------------------------

/// Top-level shape emitted by Node's `NODE_V8_COVERAGE` directory: one file
/// per worker / process containing a `result` array of [`ScriptCoverage`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct V8CoverageDump {
    /// Per-script coverage entries.
    pub result: Vec<ScriptCoverage>,
    /// Optional source-map cache emitted by Node 13+.
    #[serde(default, rename = "source-map-cache")]
    pub source_map_cache: Option<serde_json::Value>,
}

/// V8's per-script coverage record. Field names mirror the V8 inspector
/// protocol verbatim.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptCoverage {
    /// V8 script identifier.
    #[serde(rename = "scriptId")]
    pub script_id: String,
    /// File URL — typically `file:///abs/path` for Node, `https://…` for
    /// browsers. Callers normalize to absolute paths before merging.
    pub url: String,
    /// One entry per function (including the implicit module-level function).
    pub functions: Vec<FunctionCoverage>,
}

/// V8 per-function coverage. Each function carries one or more
/// [`CoverageRange`]s — block-level for instrumented coverage, function-level
/// for `--coverage=best-effort`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCoverage {
    /// Source-as-written function name. Empty for the module-level wrapper
    /// and anonymous functions.
    #[serde(rename = "functionName")]
    pub function_name: String,
    /// Coverage ranges, UTF-16 code-unit offsets relative to the script's
    /// source text (see the crate-level "Offset semantics" note).
    pub ranges: Vec<CoverageRange>,
    /// True when V8 emitted block-level data for this function (instrumented
    /// coverage). False when only the outer function range is reliable
    /// (best-effort / runtime coverage).
    #[serde(rename = "isBlockCoverage", default)]
    pub is_block_coverage: bool,
}

/// A single coverage range. `count == 0` means the range was never hit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageRange {
    /// Inclusive UTF-16 code-unit offset into the script's source.
    #[serde(rename = "startOffset")]
    pub start_offset: u32,
    /// Exclusive UTF-16 code-unit offset into the script's source.
    #[serde(rename = "endOffset")]
    pub end_offset: u32,
    /// Number of times the range was executed.
    pub count: u64,
}

// -- Istanbul position type -------------------------------------------------

/// 1-indexed line + 0-indexed column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IstanbulPosition {
    /// 1-indexed line number.
    pub line: u32,
    /// 0-indexed column within the line.
    ///
    /// Some real Istanbul producers (including Vitest in certain transforms)
    /// emit `null` for end columns. We normalize those to `0` at parse time
    /// so downstream CRAP/prod-coverage consumers can still ingest the file.
    #[serde(deserialize_with = "deserialize_nullable_u32")]
    pub column: u32,
}

fn deserialize_nullable_u32<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<u32>::deserialize(deserializer)?.unwrap_or(0))
}

// -- V8 offset to line/column mapper ---------------------------------------

/// Pre-computed line-start table for converting V8 source offsets into
/// Istanbul line/column positions in O(log n) per lookup.
///
/// V8 reports offsets in JavaScript source positions: UTF-16 code units, not
/// UTF-8 bytes. Istanbul columns use the same 0-indexed source-position model,
/// so this table stores line starts in UTF-16 units.
///
/// The source is consumed once at construction; subsequent lookups are
/// allocation-free.
#[derive(Debug)]
pub struct LineOffsetTable {
    /// UTF-16 offset of the first character of each line. `line_starts[0]`
    /// is always `0` (the start of the file).
    line_starts: Vec<u32>,
}

impl LineOffsetTable {
    /// Build a table from the full source text. The source must be UTF-8 with
    /// LF, CRLF, or CR line endings (mixed endings are tolerated).
    #[must_use]
    pub fn from_source(source: &str) -> Self {
        let mut line_starts = Vec::with_capacity(source.lines().count() + 1);
        line_starts.push(0);
        let mut offset = 0u32;
        let mut chars = source.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\n' => {
                    offset = offset.saturating_add(1);
                    line_starts.push(offset);
                }
                '\r' => {
                    offset = offset.saturating_add(1);
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                        offset = offset.saturating_add(1);
                    }
                    line_starts.push(offset);
                }
                _ => offset = offset.saturating_add(ch.len_utf16() as u32),
            }
        }
        Self { line_starts }
    }

    /// Build a table from V8's `source-map-cache.lineLengths` data.
    ///
    /// `lineLengths` are already measured in JavaScript source positions. The
    /// cache does not carry line-ending widths, so this preserves the existing
    /// Node fallback behavior and advances one source position between lines.
    #[must_use]
    pub fn from_v8_line_lengths(line_lengths: &[u32]) -> Option<Self> {
        if line_lengths.is_empty() {
            return None;
        }

        let mut line_starts = Vec::with_capacity(line_lengths.len());
        line_starts.push(0);
        let mut offset = 0u32;
        for length in line_lengths
            .iter()
            .take(line_lengths.len().saturating_sub(1))
        {
            offset = offset.saturating_add(*length).saturating_add(1);
            line_starts.push(offset);
        }
        Some(Self { line_starts })
    }

    /// Convert a V8 source offset to a 1-indexed line + 0-indexed column.
    ///
    /// Offsets at or past the end of the source clamp to the last line +
    /// remaining column.
    #[must_use]
    pub fn position(&self, source_offset: u32) -> IstanbulPosition {
        // Binary search for the last line_start <= source_offset.
        let line_zero_indexed = match self.line_starts.binary_search(&source_offset) {
            Ok(exact) => exact,
            Err(insertion_point) => insertion_point.saturating_sub(1),
        };
        let line_start = self.line_starts[line_zero_indexed];
        IstanbulPosition {
            line: (line_zero_indexed as u32) + 1,
            column: source_offset.saturating_sub(line_start),
        }
    }
}

// Manual Copy impls: the CLI consumer `.copied()`s `CoverageRange` out of a
// function's `ranges`, and `IstanbulPosition` is a small value type returned by
// `LineOffsetTable::position`.
impl Copy for CoverageRange {}
impl Copy for IstanbulPosition {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_table_handles_lf() {
        let table = LineOffsetTable::from_source("a\nbb\nccc");
        assert_eq!(table.position(0).line, 1);
        assert_eq!(table.position(0).column, 0);
        assert_eq!(table.position(2).line, 2);
        assert_eq!(table.position(2).column, 0);
        assert_eq!(table.position(5).line, 3);
        assert_eq!(table.position(5).column, 0);
    }

    #[test]
    fn line_table_handles_crlf() {
        let table = LineOffsetTable::from_source("a\r\nbb\r\nccc");
        assert_eq!(table.position(3).line, 2);
        assert_eq!(table.position(3).column, 0);
    }

    #[test]
    fn line_table_handles_lone_cr() {
        let table = LineOffsetTable::from_source("a\rbb");
        assert_eq!(table.position(2).line, 2);
        assert_eq!(table.position(2).column, 0);
    }

    #[test]
    fn line_table_uses_utf16_offsets_for_non_ascii_source() {
        let source = "const smile = \"😀\";\nfunction after_emoji() {}\n";
        let function_byte_offset = source
            .find("function")
            .expect("test source should contain function");
        let function_v8_offset = source[..function_byte_offset].encode_utf16().count() as u32;

        assert_ne!(function_v8_offset, function_byte_offset as u32);

        let table = LineOffsetTable::from_source(source);
        let pos = table.position(function_v8_offset);

        assert_eq!(pos.line, 2);
        assert_eq!(pos.column, 0);
    }

    /// The discriminating case: a multibyte character and the offset live on the
    /// SAME line, so the UTF-16-vs-byte distinction shows up as the *column*,
    /// not just line counting. A byte-offset implementation would report a
    /// strictly larger column here. `😀` is 2 UTF-16 units / 4 UTF-8 bytes, so
    /// two of them put the byte offset 4 ahead of the V8 (UTF-16) offset. This
    /// mirrors what real Node emits (`Profiler.takePreciseCoverage` reports the
    /// UTF-16 offset, e.g. 18 rather than the byte offset 22 for this shape).
    #[test]
    fn line_table_maps_columns_in_utf16_units_within_a_line() {
        let source = "const e = \"😀😀\"; function f(){}\n";
        let function_byte_offset = source
            .find("function")
            .expect("test source should contain function")
            as u32;
        let function_v8_offset = source[..function_byte_offset as usize]
            .encode_utf16()
            .count() as u32;

        // The fixture must actually exercise the multibyte gap, else a byte
        // implementation would pass this test by accident.
        assert!(
            function_v8_offset < function_byte_offset,
            "fixture must place a multibyte char before the function",
        );

        let table = LineOffsetTable::from_source(source);
        let pos = table.position(function_v8_offset);

        // Line 1 starts at offset 0, so the column equals the V8 (UTF-16)
        // offset. A byte model would report `function_byte_offset` instead.
        assert_eq!(pos.line, 1);
        assert_eq!(pos.column, function_v8_offset);
        assert!(
            pos.column < function_byte_offset,
            "column must be measured in UTF-16 units, not bytes",
        );
    }

    #[test]
    fn line_table_builds_from_v8_line_lengths() {
        let table = LineOffsetTable::from_v8_line_lengths(&[20, 12])
            .expect("line lengths should build table");

        assert_eq!(table.position(20).line, 1);
        assert_eq!(table.position(20).column, 20);
        assert_eq!(table.position(21).line, 2);
        assert_eq!(table.position(21).column, 0);
    }

    #[test]
    fn line_table_clamps_past_end() {
        let table = LineOffsetTable::from_source("abc");
        let pos = table.position(100);
        assert_eq!(pos.line, 1);
        assert_eq!(pos.column, 100);
    }

    #[test]
    fn parse_node_v8_coverage_dump() {
        let raw = serde_json::json!({
            "result": [{
                "scriptId": "42",
                "url": "file:///t/x.js",
                "functions": [{
                    "functionName": "a",
                    "ranges": [{"startOffset": 0, "endOffset": 10, "count": 3}],
                    "isBlockCoverage": false
                }]
            }]
        });
        let dump: V8CoverageDump = serde_json::from_value(raw).unwrap();
        assert_eq!(dump.result.len(), 1);
        assert_eq!(dump.result[0].functions[0].function_name, "a");
    }

    /// Some real Istanbul producers (e.g. Vitest under certain transforms) emit
    /// `null` for end columns. [`IstanbulPosition`] tolerates that via
    /// `deserialize_nullable_u32`, normalizing `null` to `0` so a downstream
    /// consumer deserializing positions does not choke. Pinned directly on the
    /// position type since that is where the custom deserializer lives.
    #[test]
    fn istanbul_position_normalizes_null_column_to_zero() {
        let with_null: IstanbulPosition =
            serde_json::from_value(serde_json::json!({ "line": 76, "column": null })).unwrap();
        assert_eq!(with_null.line, 76);
        assert_eq!(with_null.column, 0);

        let with_value: IstanbulPosition =
            serde_json::from_value(serde_json::json!({ "line": 66, "column": 4 })).unwrap();
        assert_eq!(with_value.column, 4);
    }

    /// Property tests for the UTF-16-offset-to-line/column mapper.
    ///
    /// The `position` mapper backs every Istanbul range fallow emits for runtime
    /// coverage, so its invariants are encoded as properties rather than relying
    /// on hand-picked examples. The line-boundary tests build their input from
    /// known line bodies and join them with a chosen ending, so the expected
    /// offsets are computed independently of the char-walking construction loop.
    mod proptests {
        use super::*;
        use proptest::prelude::*;

        /// A line body drawn from an alphabet that exercises the UTF-16 width
        /// branch: ASCII (1 unit), `€` (1 unit / 3 UTF-8 bytes), and `😀` (a
        /// surrogate pair, 2 units / 4 UTF-8 bytes). Never contains CR or LF, so
        /// the only line breaks are the ones the harness inserts deliberately.
        fn line_body() -> impl Strategy<Value = String> {
            prop::collection::vec(prop::sample::select(vec!['a', 'b', ' ', '€', '😀']), 0..12)
                .prop_map(|chars| chars.into_iter().collect())
        }

        /// UTF-16 length of a `str`, matching the units `LineOffsetTable` stores.
        fn utf16_len(s: &str) -> u32 {
            s.encode_utf16().count() as u32
        }

        proptest! {
            /// `position` is monotonic: a non-decreasing offset never yields an
            /// earlier `(line, column)`. Guards the `binary_search` Err-branch
            /// `saturating_sub(1)` and the saturating column subtraction against
            /// off-by-one regressions, for any source including past-end offsets.
            #[test]
            fn position_is_monotonic_in_offset(
                source in prop::collection::vec(any::<char>(), 0..200)
                    .prop_map(|chars| chars.into_iter().collect::<String>()),
                a in any::<u32>(),
                b in any::<u32>(),
            ) {
                let table = LineOffsetTable::from_source(&source);
                let (lo, hi) = (a.min(b), a.max(b));
                let p_lo = table.position(lo);
                let p_hi = table.position(hi);
                prop_assert!(p_lo.line >= 1, "line numbers are 1-indexed");
                prop_assert!(
                    (p_lo.line, p_lo.column) <= (p_hi.line, p_hi.column),
                    "position({lo}) = {p_lo:?} should not exceed position({hi}) = {p_hi:?}",
                );
            }

            /// Every true line boundary maps back to column 0 on the right line,
            /// and offsets within a line recover their column. Input is assembled
            /// from known bodies + ending, so the expectation is independent of
            /// the mapper's own line-splitting logic.
            #[test]
            fn line_starts_and_columns_round_trip(
                bodies in prop::collection::vec(line_body(), 1..8),
                ending in prop::sample::select(vec!["\n", "\r\n", "\r"]),
            ) {
                let source = bodies.join(ending);
                let table = LineOffsetTable::from_source(&source);
                let ending_units = utf16_len(ending);

                let mut line_start = 0u32;
                for (index, body) in bodies.iter().enumerate() {
                    let body_units = utf16_len(body);
                    // Column 0 of each line lands on the line's first offset.
                    let at_start = table.position(line_start);
                    prop_assert_eq!(at_start.line, index as u32 + 1);
                    prop_assert_eq!(at_start.column, 0);
                    // Offsets inside the line (up to its width) recover the column.
                    for column in 0..=body_units {
                        let pos = table.position(line_start + column);
                        prop_assert_eq!(pos.line, index as u32 + 1);
                        prop_assert_eq!(pos.column, column);
                    }
                    line_start += body_units;
                    if index + 1 < bodies.len() {
                        line_start += ending_units;
                    }
                }
            }

            /// `from_v8_line_lengths` advances one source position per line. The
            /// cumulative line starts are strictly increasing and each maps to
            /// column 0 on its line; offsets within a non-final line recover the
            /// column. Lengths are bounded so the cumulative offset never
            /// saturates, keeping the reference model exact.
            #[test]
            fn v8_line_lengths_build_consistent_table(
                lengths in prop::collection::vec(0u32..1000, 1..20),
            ) {
                let table = LineOffsetTable::from_v8_line_lengths(&lengths)
                    .expect("non-empty lengths build a table");

                // Reconstruct the expected line starts: +1 separator per line.
                let mut starts = vec![0u32];
                let mut acc = 0u32;
                for length in &lengths[..lengths.len() - 1] {
                    acc += length + 1;
                    starts.push(acc);
                }

                let mut previous: Option<u32> = None;
                for (index, &start) in starts.iter().enumerate() {
                    if let Some(prev) = previous {
                        prop_assert!(start > prev, "line starts must strictly increase");
                    }
                    previous = Some(start);

                    let at_start = table.position(start);
                    prop_assert_eq!(at_start.line, index as u32 + 1);
                    prop_assert_eq!(at_start.column, 0);

                    // Within a non-final line the recorded length bounds the columns.
                    if index + 1 < lengths.len() {
                        for column in 0..=lengths[index] {
                            let pos = table.position(start + column);
                            prop_assert_eq!(pos.line, index as u32 + 1);
                            prop_assert_eq!(pos.column, column);
                        }
                    }
                }
            }
        }
    }
}
