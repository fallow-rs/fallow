//! Raw V8 coverage input for `health --coverage` (issue #2906).
//!
//! `NODE_V8_COVERAGE=<dir>` makes Node write one dump per process, and
//! `node --test` runs every test file in its own child process. Each dump
//! lists the scripts that process loaded, with nested `[start, end, count]`
//! block ranges per function.
//!
//! This module turns those dumps into Istanbul `FileCoverage` records, so the
//! existing function matcher consumes them unchanged. The statements come
//! from an AST pass over the file on disk, and the counts come from the V8
//! ranges. That is statement-level precision, where the line-based
//! `v8-to-istanbul` conversion that `c8` uses marks a line as covered when
//! any part of it ran.
//!
//! Three facts measured on real Node dumps shape the code:
//!
//! - Offsets are UTF-16 code units. The pinned `oxc_coverage_instrument`
//!   (0.9) reads them as UTF-8 byte offsets, so every range is translated
//!   first. Releases from 0.11 read UTF-16 offsets themselves; remove the
//!   translation with that bump. `non_ascii_source_counts_the_right_statement`
//!   fails when the offsets are translated twice.
//! - CommonJS modules carry no wrapper offset (`vm.compileFunction`).
//! - The module-level function spans exactly the executed source. Node's
//!   type stripping appends `\n\n//# sourceURL=<url>` to a `.ts` module. Any
//!   other length means the executed source is not the file on disk
//!   (transpiled or stale), and the script is not read as that file.
//!
//! A transpiled script (`tsx`, a bundle) reaches its original files through
//! the source map that Node records in the dump; see [`mapped`].

mod mapped;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use oxc_coverage_instrument::{FileCoverage, Location, V8CoverageRange, V8FunctionCoverage};
use rayon::prelude::*;
use rustc_hash::FxHashSet;
use serde::Deserialize;

/// The trailer Node's type stripping appends to a `.ts` module before V8
/// compiles it, followed by the module URL.
const TYPE_STRIP_SOURCE_URL_TRAILER: &str = "\n\n//# sourceURL=";

/// One `NODE_V8_COVERAGE` dump file.
#[derive(Deserialize)]
struct V8Dump {
    result: Vec<V8Script>,
    /// Source maps of transpiled scripts, keyed by script URL.
    #[serde(default, rename = "source-map-cache")]
    source_map_cache: Option<BTreeMap<String, mapped::SourceMapCacheEntry>>,
}

#[derive(Deserialize)]
struct V8Script {
    url: String,
    functions: Vec<V8FunctionCoverage>,
}

/// Cheap shape probe: a V8 dump is an object whose `result` is an array.
#[derive(Deserialize)]
struct V8DumpProbe {
    result: Option<Vec<serde::de::IgnoredAny>>,
}

/// Whether `json` has the top-level shape of a V8 coverage dump.
pub(super) fn is_v8_dump(json: &str) -> bool {
    serde_json::from_str::<V8DumpProbe>(json).is_ok_and(|probe| probe.result.is_some())
}

/// The `*.json` files of a directory, sorted for a deterministic merge order.
pub(super) fn dump_files_in(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("failed to read coverage directory {}: {e}", dir.display()))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json") && path.is_file())
        .collect();
    files.sort();
    Ok(files)
}

/// Where the V8 scripts of a run may map to.
pub(super) struct V8ScriptScope<'a> {
    pub(super) coverage_root: Option<&'a Path>,
    pub(super) project_root: Option<&'a Path>,
    /// Discovered project sources. `None` in the relocated audit pass, which
    /// then admits any regular file under the project root outside
    /// `node_modules`.
    pub(super) discovered_sources: Option<&'a FxHashSet<PathBuf>>,
}

/// Read V8 dumps and convert every project script into an Istanbul record,
/// keyed and pathed by the canonical file path. Counts of one file across
/// several dumps are summed.
///
/// A test process that was killed can leave a truncated dump, so a dump that
/// cannot be read or parsed is skipped. The load fails only when no dump
/// parses.
pub(super) fn load_v8_coverage_map(
    dump_files: &[PathBuf],
    scope: &V8ScriptScope<'_>,
) -> Result<BTreeMap<String, FileCoverage>, String> {
    let mut views_by_file: BTreeMap<PathBuf, Vec<ScriptView>> = BTreeMap::new();
    let mut first_error = None;
    let mut parsed_dumps = 0usize;
    for (dump_index, dump_file) in dump_files.iter().enumerate() {
        let dump = match read_dump(dump_file) {
            Ok(dump) => dump,
            Err(error) => {
                first_error.get_or_insert(error);
                continue;
            }
        };
        parsed_dumps += 1;
        let mut source_maps = dump.source_map_cache.unwrap_or_default();
        for (script_index, script) in dump.result.into_iter().enumerate() {
            let id = (dump_index, script_index);
            let functions: Arc<[V8FunctionCoverage]> = Arc::from(script.functions);
            if let Some(path) = file_url_path(&script.url).and_then(|p| project_path(p, scope)) {
                views_by_file
                    .entry(path)
                    .or_default()
                    .push(ScriptView::Direct {
                        id,
                        url: script.url.clone(),
                        functions: Arc::clone(&functions),
                    });
            }
            let Some(entry) = source_maps.remove(&script.url) else {
                continue;
            };
            let Some(generated) = mapped::GeneratedScript::parse(&entry) else {
                continue;
            };
            let entry = Arc::new(entry);
            for (source_index, source) in generated.source_paths(&script.url) {
                if let Some(path) = project_path(source, scope) {
                    views_by_file
                        .entry(path)
                        .or_default()
                        .push(ScriptView::Mapped {
                            id,
                            functions: Arc::clone(&functions),
                            entry: Arc::clone(&entry),
                            source_index,
                        });
                }
            }
        }
    }

    if parsed_dumps == 0
        && let Some(error) = first_error
    {
        return Err(error);
    }

    Ok(views_by_file
        .into_par_iter()
        .filter_map(|(path, views)| {
            let coverage = convert_file(&path, &views)?;
            Some((coverage.path.clone(), coverage))
        })
        .collect())
}

fn read_dump(dump_file: &Path) -> Result<V8Dump, String> {
    let json = std::fs::read_to_string(dump_file).map_err(|e| {
        format!(
            "failed to read V8 coverage file {}: {e}",
            dump_file.display()
        )
    })?;
    serde_json::from_str(&json).map_err(|e| {
        format!(
            "failed to parse V8 coverage file {}: {e}",
            dump_file.display()
        )
    })
}

/// Identifies one script of one dump.
type ScriptId = (usize, usize);

/// One dump's view of one project file.
enum ScriptView {
    /// The script is the file itself.
    Direct {
        id: ScriptId,
        url: String,
        functions: Arc<[V8FunctionCoverage]>,
    },
    /// The file is one source of a source-mapped script.
    Mapped {
        id: ScriptId,
        functions: Arc<[V8FunctionCoverage]>,
        entry: Arc<mapped::SourceMapCacheEntry>,
        source_index: u32,
    },
}

/// The path of a `file://` script URL, or `None` for Node internals and
/// remote URLs.
fn file_url_path(url: &str) -> Option<PathBuf> {
    let parsed = url::Url::parse(url).ok()?;
    if parsed.scheme() != "file" {
        return None;
    }
    parsed.to_file_path().ok()
}

/// Map a recorded path onto a canonical project source, or `None` for
/// dependencies and files outside the project.
fn project_path(recorded: PathBuf, scope: &V8ScriptScope<'_>) -> Option<PathBuf> {
    let rebased = match (scope.coverage_root, scope.project_root) {
        (Some(coverage_root), Some(project_root)) => recorded
            .strip_prefix(coverage_root)
            .map_or_else(|_| recorded.clone(), |rel| project_root.join(rel)),
        _ => recorded,
    };
    let canonical = dunce::canonicalize(&rebased).ok()?;
    if let Some(sources) = scope.discovered_sources {
        return sources.contains(&canonical).then_some(canonical);
    }
    let root = dunce::canonicalize(scope.project_root?).ok()?;
    let in_project = canonical.starts_with(&root)
        && !canonical
            .components()
            .any(|component| component.as_os_str() == "node_modules");
    in_project.then_some(canonical)
}

/// Convert every dump's view of one file and sum the counts. Returns `None`
/// when the file cannot be read or parsed, or when no dump ran the source
/// that is on disk now.
#[expect(
    clippy::filetype_is_file,
    reason = "coverage provenance must admit regular files and reject every special file type"
)]
fn convert_file(path: &Path, views: &[ScriptView]) -> Option<FileCoverage> {
    if !std::fs::symlink_metadata(path).ok()?.file_type().is_file() {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let filename = path.to_string_lossy();
    let full = ExecutedSource::new(&text, false);
    // With a byte order mark, V8 compiles a CommonJS module with the mark
    // and an ES module without it. The module range tells which one ran.
    let without_bom = text
        .strip_prefix('\u{FEFF}')
        .map(|stripped| ExecutedSource::new(stripped, true));

    let mut merged: Option<FileCoverage> = None;
    let mut merge = |coverage: FileCoverage| match merged.as_mut() {
        Some(total) => add_counts(total, &coverage),
        None => merged = Some(coverage),
    };
    let mut direct_ids = FxHashSet::default();
    for view in views {
        let ScriptView::Direct { id, url, functions } = view else {
            continue;
        };
        let executed = if executed_source_matches(functions, full.offsets.utf16_len(), url) {
            &full
        } else if let Some(stripped) = without_bom.as_ref().filter(|candidate| {
            executed_source_matches(functions, candidate.offsets.utf16_len(), url)
        }) {
            stripped
        } else {
            continue;
        };
        let byte_functions = executed.offsets.translate_functions(functions);
        let Ok(mut coverage) =
            oxc_coverage_instrument::v8_to_istanbul(executed.source, &filename, &byte_functions, 0)
        else {
            return None;
        };
        if executed.bom_stripped {
            shift_first_line_columns(&mut coverage);
        }
        direct_ids.insert(*id);
        merge(coverage);
    }

    // A statement without generated code in every mapped view (a
    // transpiler removed it) does not count. A direct view knows every
    // statement, so it empties the set.
    let mut unmapped: Option<FxHashSet<String>> = (!direct_ids.is_empty()).then(FxHashSet::default);
    let original = without_bom.as_ref().unwrap_or(&full);
    let mut original_map: Option<FileCoverage> = None;
    for view in views {
        let ScriptView::Mapped {
            id,
            functions,
            entry,
            source_index,
        } = view
        else {
            continue;
        };
        if direct_ids.contains(id) {
            continue;
        }
        let Some(generated) = mapped::GeneratedScript::parse(entry) else {
            continue;
        };
        if !generated.matches(functions) || !generated.content_matches(*source_index, &text) {
            continue;
        }
        if original_map.is_none() {
            original_map =
                oxc_coverage_instrument::v8_to_istanbul(original.source, &filename, &[], 0).ok();
        }
        let mut coverage = original_map.clone()?;
        let missing = generated.apply(&mut coverage, *source_index, functions);
        if original.bom_stripped {
            shift_first_line_columns(&mut coverage);
        }
        unmapped = Some(match unmapped {
            Some(previous) => previous.intersection(&missing).cloned().collect(),
            None => missing,
        });
        merge(coverage);
    }

    let mut merged = merged?;
    for id in unmapped.unwrap_or_default() {
        merged.statement_map.remove(&id);
        merged.s.remove(&id);
    }
    merged.path = filename.into_owned();
    Some(merged)
}

/// One candidate for the source text that V8 compiled.
struct ExecutedSource<'a> {
    source: &'a str,
    offsets: Utf16ToByteOffsets,
    bom_stripped: bool,
}

impl<'a> ExecutedSource<'a> {
    fn new(source: &'a str, bom_stripped: bool) -> Self {
        Self {
            source,
            offsets: Utf16ToByteOffsets::new(source),
            bom_stripped,
        }
    }
}

/// Move every position on line 1 one column right, back into the coordinates
/// of the file on disk, where the byte order mark is one UTF-16 unit.
fn shift_first_line_columns(coverage: &mut FileCoverage) {
    let shift = |location: &mut Location| {
        for position in [&mut location.start, &mut location.end] {
            if position.line == 1 {
                position.column = position.column.saturating_add(1);
            }
        }
    };
    coverage.statement_map.values_mut().for_each(shift);
    for entry in coverage.fn_map.values_mut() {
        shift(&mut entry.decl);
        shift(&mut entry.loc);
    }
    for entry in coverage.branch_map.values_mut() {
        shift(&mut entry.loc);
        entry.locations.iter_mut().for_each(shift);
    }
}

/// The end of the module-level range: the length of the executed source.
fn module_range_end(functions: &[V8FunctionCoverage]) -> Option<u32> {
    functions
        .iter()
        .filter_map(|function| function.ranges.first())
        .filter(|range| range.start_offset == 0)
        .map(|range| range.end_offset)
        .max()
}

/// Whether the module-level range of a script spans exactly the file on
/// disk, with or without the type-stripping `sourceURL` trailer.
fn executed_source_matches(functions: &[V8FunctionCoverage], source_len: u32, url: &str) -> bool {
    let Some(module_end) = module_range_end(functions) else {
        return false;
    };
    let trailer_len = TYPE_STRIP_SOURCE_URL_TRAILER
        .encode_utf16()
        .chain(url.encode_utf16())
        .count();
    module_end == source_len
        || u32::try_from(trailer_len)
            .ok()
            .and_then(|len| source_len.checked_add(len))
            == Some(module_end)
}

/// Add the hit counts of `other` into `total`. Both records come from the
/// same source text, so their statement, function and branch ids agree.
fn add_counts(total: &mut FileCoverage, other: &FileCoverage) {
    for (id, count) in &other.s {
        if let Some(slot) = total.s.get_mut(id) {
            *slot = slot.saturating_add(*count);
        }
    }
    for (id, count) in &other.f {
        if let Some(slot) = total.f.get_mut(id) {
            *slot = slot.saturating_add(*count);
        }
    }
    for (id, arms) in &other.b {
        if let Some(slots) = total.b.get_mut(id) {
            for (slot, count) in slots.iter_mut().zip(arms) {
                *slot = slot.saturating_add(*count);
            }
        }
    }
}

/// UTF-16 code-unit offset to UTF-8 byte offset table for one source.
struct Utf16ToByteOffsets {
    /// Byte offset of every UTF-16 unit, plus the source length at the end.
    /// Empty for an ASCII source, where the two offsets are equal.
    bytes: Vec<u32>,
    utf16_len: u32,
}

impl Utf16ToByteOffsets {
    fn new(source: &str) -> Self {
        let clamp = |value: usize| u32::try_from(value).unwrap_or(u32::MAX);
        if source.is_ascii() {
            return Self {
                bytes: Vec::new(),
                utf16_len: clamp(source.len()),
            };
        }
        let mut bytes = Vec::with_capacity(source.len() + 1);
        for (byte_offset, ch) in source.char_indices() {
            for _ in 0..ch.len_utf16() {
                bytes.push(clamp(byte_offset));
            }
        }
        let utf16_len = clamp(bytes.len());
        bytes.push(clamp(source.len()));
        Self { bytes, utf16_len }
    }

    const fn utf16_len(&self) -> u32 {
        self.utf16_len
    }

    /// The byte offset of a UTF-16 offset. Offsets past the source (the
    /// type-stripping trailer) clamp to the source end.
    fn byte_offset(&self, utf16_offset: u32) -> u32 {
        if self.bytes.is_empty() {
            return utf16_offset.min(self.utf16_len);
        }
        let index = usize::try_from(utf16_offset.min(self.utf16_len)).unwrap_or(usize::MAX);
        self.bytes.get(index).copied().unwrap_or(u32::MAX)
    }

    fn translate_functions(&self, functions: &[V8FunctionCoverage]) -> Vec<V8FunctionCoverage> {
        functions
            .iter()
            .map(|function| V8FunctionCoverage {
                function_name: function.function_name.clone(),
                is_block_coverage: function.is_block_coverage,
                ranges: function
                    .ranges
                    .iter()
                    .map(|range| V8CoverageRange {
                        start_offset: self.byte_offset(range.start_offset),
                        end_offset: self.byte_offset(range.end_offset),
                        count: range.count,
                    })
                    .collect(),
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range(start: u32, end: u32, count: u32) -> V8CoverageRange {
        V8CoverageRange {
            start_offset: start,
            end_offset: end,
            count,
        }
    }

    fn direct(index: usize, url: &str, functions: Vec<V8FunctionCoverage>) -> ScriptView {
        ScriptView::Direct {
            id: (index, 0),
            url: url.to_string(),
            functions: Arc::from(functions),
        }
    }

    fn function(name: &str, ranges: Vec<V8CoverageRange>) -> V8FunctionCoverage {
        V8FunctionCoverage {
            function_name: name.to_string(),
            ranges,
            is_block_coverage: true,
        }
    }

    #[test]
    fn probe_tells_v8_dumps_from_istanbul_maps() {
        assert!(is_v8_dump(r#"{"result":[]}"#));
        assert!(is_v8_dump(
            r#"{"result":[{"url":"file:///a.js","functions":[]}]}"#
        ));
        assert!(!is_v8_dump(r#"{"/a.js":{"path":"/a.js"}}"#));
        assert!(!is_v8_dump(r#"{"result":{"path":"result"}}"#));
        assert!(!is_v8_dump("not json"));
    }

    #[test]
    fn utf16_offsets_translate_to_bytes() {
        // `é` is one UTF-16 unit and two bytes; `😀` is two units and four bytes.
        let source = "const a = \"é😀\";\nf();\n";
        let offsets = Utf16ToByteOffsets::new(source);
        assert_eq!(offsets.utf16_len(), 22);
        let utf16_f = 17;
        let byte_f = source.find("f()").unwrap();
        assert_eq!(offsets.byte_offset(utf16_f) as usize, byte_f);
        assert_eq!(
            offsets.byte_offset(offsets.utf16_len()) as usize,
            source.len()
        );
        assert_eq!(offsets.byte_offset(u32::MAX) as usize, source.len());
    }

    #[test]
    fn ascii_offsets_are_identity() {
        let offsets = Utf16ToByteOffsets::new("let x = 1;\n");
        assert_eq!(offsets.byte_offset(4), 4);
        assert_eq!(offsets.byte_offset(99), 11);
    }

    #[test]
    fn module_range_must_span_the_file_on_disk() {
        let url = "file:///p/src/a.ts";
        let module = |end| vec![function("", vec![range(0, end, 1)])];
        assert!(executed_source_matches(&module(88), 88, url));
        let stripped_end = 88 + 16 + u32::try_from(url.len()).unwrap();
        assert!(executed_source_matches(&module(stripped_end), 88, url));
        assert!(!executed_source_matches(&module(120), 88, url));
        assert!(!executed_source_matches(&[], 88, url));
    }

    #[test]
    fn counts_from_two_processes_are_summed() {
        let source = "function f(x) {\n  if (x) { return 1; }\n  return 2;\n}\nf(1);\n";
        let len = u32::try_from(source.len()).unwrap();
        let body_start = u32::try_from(source.find("function").unwrap()).unwrap();
        let body_end = u32::try_from(source.find("}\nf(1)").unwrap() + 1).unwrap();
        let tail_start = u32::try_from(source.find("  return 2").unwrap()).unwrap();
        // Process one takes the early return, so `return 2` did not run.
        let first = vec![
            function("", vec![range(0, len, 1)]),
            function(
                "f",
                vec![
                    range(body_start, body_end, 1),
                    range(tail_start, body_end - 1, 0),
                ],
            ),
        ];
        // Process two takes the other path.
        let second = vec![
            function("", vec![range(0, len, 1)]),
            function("f", vec![range(body_start, body_end, 1)]),
        ];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.js");
        std::fs::write(&path, source).unwrap();
        let url = "file:///a.js".to_string();

        let one = convert_file(&path, &[direct(0, &url, first.clone())]).unwrap();
        let both = convert_file(&path, &[direct(0, &url, first), direct(1, &url, second)]).unwrap();
        let executed = |coverage: &FileCoverage| coverage.s.values().filter(|c| **c > 0).count();
        assert!(executed(&both) > executed(&one));
        assert_eq!(both.path, path.to_string_lossy());
    }

    #[test]
    fn byte_order_mark_follows_the_module_range() {
        let dir = tempfile::tempdir().unwrap();
        let source = "export const a = 1;\n";
        let with_bom = dir.path().join("a.js");
        std::fs::write(&with_bom, format!("\u{FEFF}{source}")).unwrap();
        let plain_path = dir.path().join("b.js");
        std::fs::write(&plain_path, source).unwrap();
        let len = u32::try_from(source.len()).unwrap();
        let convert = |path: &Path, end| {
            let module = vec![function("", vec![range(0, end, 1)])];
            convert_file(path, &[direct(0, "file:///a.js", module)]).unwrap()
        };
        let columns = |coverage: &FileCoverage| -> Vec<u32> {
            coverage
                .statement_map
                .values()
                .map(|loc| loc.start.column)
                .collect()
        };
        let plain = columns(&convert(&plain_path, len));
        let shifted: Vec<u32> = plain.iter().map(|column| column + 1).collect();
        assert!(!plain.is_empty());
        // An ES module ran without the mark; positions move back to disk columns.
        assert_eq!(columns(&convert(&with_bom, len)), shifted);
        // A CommonJS module ran with the mark as one UTF-16 unit.
        assert_eq!(columns(&convert(&with_bom, len + 1)), shifted);
    }

    #[test]
    fn a_broken_dump_is_skipped_while_another_parses() {
        let dir = tempfile::tempdir().unwrap();
        let source_path = dir.path().join("a.js");
        let source = "export const a = 1;\n";
        std::fs::write(&source_path, source).unwrap();
        let canonical = dunce::canonicalize(&source_path).unwrap();
        let url = url::Url::from_file_path(&canonical).unwrap().to_string();
        let good = dir.path().join("coverage-1.json");
        let broken = dir.path().join("coverage-2.json");
        let dump = serde_json::json!({ "result": [{
            "url": url,
            "functions": [{
                "functionName": "",
                "isBlockCoverage": false,
                "ranges": [{ "startOffset": 0, "endOffset": source.len(), "count": 1 }]
            }]
        }]});
        std::fs::write(&good, dump.to_string()).unwrap();
        std::fs::write(&broken, "{\"result\": [").unwrap();
        let sources: FxHashSet<PathBuf> = std::iter::once(canonical.clone()).collect();
        let scope = V8ScriptScope {
            coverage_root: None,
            project_root: Some(dir.path()),
            discovered_sources: Some(&sources),
        };

        let map = load_v8_coverage_map(&[good, broken.clone()], &scope).unwrap();
        assert!(map.contains_key(canonical.to_string_lossy().as_ref()));
        let error = load_v8_coverage_map(&[broken], &scope).unwrap_err();
        assert!(
            error.contains("failed to parse V8 coverage file"),
            "{error}"
        );
    }

    #[test]
    fn non_ascii_source_counts_the_right_statement() {
        let source = "const s = \"\u{e9}\u{1F600}\u{1F600}\";\nfunction f(x) {\n  if (x) { return 1; }\n  return 2;\n}\nf(1);\n";
        let utf16 = |byte: usize| u32::try_from(source[..byte].encode_utf16().count()).unwrap();
        let body_start = utf16(source.find("function").unwrap());
        let body_end = utf16(source.find("}\nf(1)").unwrap() + 1);
        let tail_start = utf16(source.find("  return 2").unwrap());
        let functions = vec![
            function("", vec![range(0, utf16(source.len()), 1)]),
            function(
                "f",
                vec![
                    range(body_start, body_end, 1),
                    range(tail_start, body_end - 1, 0),
                ],
            ),
        ];
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.js");
        std::fs::write(&path, source).unwrap();

        let coverage = convert_file(&path, &[direct(0, "file:///a.js", functions)]).unwrap();
        let count_on_line = |line: u32| -> Vec<u32> {
            coverage
                .statement_map
                .iter()
                .filter(|(_, loc)| loc.start.line == line)
                .map(|(id, _)| coverage.s[id])
                .collect()
        };
        assert_eq!(count_on_line(4), vec![0], "`return 2` did not run");
        assert!(
            count_on_line(3).iter().all(|count| *count > 0),
            "the early return ran"
        );
    }

    /// A generated script that is the original source one line lower, with
    /// a mapping for every column, plus V8 ranges where `return 2` did not run.
    struct MappedFixture {
        dir: tempfile::TempDir,
        path: PathBuf,
        functions: Vec<V8FunctionCoverage>,
        entry: serde_json::Value,
    }

    fn mapped_fixture(embedded_content: &str, map_first_line: bool) -> MappedFixture {
        let source = "export function pick(x) {\n  if (x) { return 1; }\n  return 2;\n}\n";
        let banner = "\"use strict\";\n";
        let generated = format!("{banner}{source}");
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pick.ts");
        std::fs::write(&path, source).unwrap();

        let mut mappings = Vec::new();
        for (line, text) in source.lines().enumerate() {
            if line == 0 && !map_first_line {
                continue;
            }
            for column in 0..text.len() {
                mappings.push(srcmap_sourcemap::Mapping {
                    generated_line: u32::try_from(line + 1).unwrap(),
                    generated_column: u32::try_from(column).unwrap(),
                    source: 0,
                    original_line: u32::try_from(line).unwrap(),
                    original_column: u32::try_from(column).unwrap(),
                    name: u32::MAX,
                    is_range_mapping: false,
                });
            }
        }
        let map = srcmap_sourcemap::SourceMap::builder()
            .sources([url::Url::from_file_path(&path).unwrap().to_string()])
            .sources_content([Some(embedded_content.to_string())])
            .mappings(mappings)
            .build();
        let line_lengths: Vec<usize> = generated.split('\n').map(str::len).collect();
        let offset = |needle: &str| u32::try_from(generated.find(needle).unwrap()).unwrap();
        let body_end = u32::try_from(generated.rfind("}\n").unwrap()).unwrap() + 1;
        let functions = vec![
            function(
                "",
                vec![range(0, u32::try_from(generated.len()).unwrap(), 1)],
            ),
            function(
                "pick",
                vec![
                    range(offset("export"), body_end, 1),
                    range(offset("  return 2"), body_end - 1, 0),
                ],
            ),
        ];
        let entry = serde_json::json!({
            "lineLengths": line_lengths,
            "data": serde_json::from_str::<serde_json::Value>(&map.to_json()).unwrap(),
        });
        MappedFixture {
            dir,
            path,
            functions,
            entry,
        }
    }

    fn mapped_view(fixture: &MappedFixture, index: usize) -> ScriptView {
        ScriptView::Mapped {
            id: (index, 0),
            functions: Arc::from(fixture.functions.clone()),
            entry: Arc::new(serde_json::from_value(fixture.entry.clone()).unwrap()),
            source_index: 0,
        }
    }

    #[test]
    fn source_mapped_script_counts_the_original_statements() {
        let source = "export function pick(x) {\n  if (x) { return 1; }\n  return 2;\n}\n";
        let fixture = mapped_fixture(source, true);
        let coverage = convert_file(&fixture.path, &[mapped_view(&fixture, 0)]).unwrap();
        let count_on_line = |line: u32| -> Vec<u32> {
            coverage
                .statement_map
                .iter()
                .filter(|(_, loc)| loc.start.line == line)
                .map(|(id, _)| coverage.s[id])
                .collect()
        };
        assert_eq!(count_on_line(3), vec![0], "`return 2` did not run");
        assert!(!count_on_line(2).is_empty());
        assert!(count_on_line(2).iter().all(|count| *count > 0));
        assert!(fixture.dir.path().exists());
    }

    #[test]
    fn unmapped_function_start_takes_its_first_statement_count() {
        let source = "export function pick(x) {\n  if (x) { return 1; }\n  return 2;\n}\n";
        let fixture = mapped_fixture(source, false);
        let coverage = convert_file(&fixture.path, &[mapped_view(&fixture, 0)]).unwrap();
        assert!(!coverage.f.is_empty());
        assert!(
            coverage.f.values().all(|count| *count > 0),
            "{:?}",
            coverage.f
        );
    }

    #[test]
    fn workspace_package_under_node_modules_maps_to_its_sources() {
        let source = "export function pick(x) {\n  if (x) { return 1; }\n  return 2;\n}\n";
        let fixture = mapped_fixture(source, true);
        let package_dir = fixture.dir.path().join("node_modules/pkg/dist");
        std::fs::create_dir_all(&package_dir).unwrap();
        let script_url = url::Url::from_file_path(package_dir.join("index.js"))
            .unwrap()
            .to_string();
        let dump = serde_json::json!({
            "result": [{ "url": script_url, "functions": fixture.functions }],
            "source-map-cache": { script_url: fixture.entry },
        });
        let dump_path = fixture.dir.path().join("coverage-1.json");
        std::fs::write(&dump_path, dump.to_string()).unwrap();
        let canonical = dunce::canonicalize(&fixture.path).unwrap();
        let sources: FxHashSet<PathBuf> = std::iter::once(canonical.clone()).collect();
        let scope = V8ScriptScope {
            coverage_root: None,
            project_root: Some(fixture.dir.path()),
            discovered_sources: Some(&sources),
        };

        let map = load_v8_coverage_map(&[dump_path], &scope).unwrap();
        assert!(map.contains_key(canonical.to_string_lossy().as_ref()));
    }

    #[test]
    fn source_mapped_script_with_changed_source_is_skipped() {
        let fixture = mapped_fixture("export function pick(x) { return 3; }\n", true);
        assert!(convert_file(&fixture.path, &[mapped_view(&fixture, 0)]).is_none());
    }

    #[test]
    fn source_mapped_script_must_span_the_generated_code() {
        let source = "export function pick(x) {\n  if (x) { return 1; }\n  return 2;\n}\n";
        let mut fixture = mapped_fixture(source, true);
        fixture.functions[0].ranges[0].end_offset += 7;
        assert!(convert_file(&fixture.path, &[mapped_view(&fixture, 0)]).is_none());
    }

    #[test]
    fn stale_or_transpiled_scripts_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.js");
        std::fs::write(&path, "export const a = 1;\n").unwrap();
        let generated = vec![function("", vec![range(0, 500, 1)])];
        assert!(convert_file(&path, &[direct(0, "file:///a.js", generated)]).is_none());
    }
}
