//! Source-mapped V8 scripts: code that a loader or bundler transpiled
//! (`tsx`, `ts-node`, esbuild, webpack) before V8 compiled it.
//!
//! Node records the source map of such a script in the dump's
//! `source-map-cache`, together with the line lengths of the generated code,
//! but not the generated code itself. So the mapping runs in reverse: the
//! statements come from the original file on disk, each statement start maps
//! to a generated position through the source map, and the line lengths turn
//! that position into the offset that the V8 ranges use.

use std::path::{Path, PathBuf};

use oxc_coverage_instrument::{FileCoverage, Position, V8CoverageRange, V8FunctionCoverage};
use rustc_hash::FxHashSet;
use serde::Deserialize;
use srcmap_sourcemap::{Bias, SourceMap};

/// One `source-map-cache` entry of a V8 dump.
#[derive(Deserialize)]
pub(super) struct SourceMapCacheEntry {
    #[serde(rename = "lineLengths")]
    line_lengths: Vec<u32>,
    data: serde_json::Value,
}

/// The generated side of a source-mapped script.
pub(super) struct GeneratedScript {
    map: SourceMap,
    /// UTF-16 offset of every generated line start.
    line_starts: Vec<u32>,
    /// UTF-16 length of the generated code.
    generated_len: u32,
}

impl GeneratedScript {
    pub(super) fn parse(entry: &SourceMapCacheEntry) -> Option<Self> {
        let map = SourceMap::from_json(&entry.data.to_string()).ok()?;
        let mut line_starts = Vec::with_capacity(entry.line_lengths.len());
        let mut offset = 0u32;
        for length in &entry.line_lengths {
            line_starts.push(offset);
            // Node measures each line without its terminator and assumes one
            // unit per line break.
            offset = offset.saturating_add(*length).saturating_add(1);
        }
        let generated_len = offset.saturating_sub(1);
        Some(Self {
            map,
            line_starts,
            generated_len,
        })
    }

    /// The sources of the map as file paths, resolved against the directory
    /// of the generated script. Remote and virtual sources (`webpack://`) are
    /// left out.
    pub(super) fn source_paths(&self, script_url: &str) -> Vec<(u32, PathBuf)> {
        let script_dir = url::Url::parse(script_url)
            .ok()
            .and_then(|url| url.to_file_path().ok())
            .and_then(|path| path.parent().map(Path::to_path_buf));
        self.map
            .sources
            .iter()
            .enumerate()
            .filter_map(|(index, source)| {
                let path = source_path(source, script_dir.as_deref())?;
                Some((u32::try_from(index).ok()?, path))
            })
            .collect()
    }

    /// Whether the module-level range spans exactly the generated code that
    /// the line lengths describe.
    pub(super) fn matches(&self, functions: &[V8FunctionCoverage]) -> bool {
        super::module_range_end(functions) == Some(self.generated_len)
    }

    /// Whether the embedded source text, when the map carries it, still
    /// equals the file on disk.
    pub(super) fn content_matches(&self, source_index: u32, text: &str) -> bool {
        let strip = |value: &str| value.strip_prefix('\u{FEFF}').unwrap_or(value).to_owned();
        match self
            .map
            .sources_content
            .get(source_index as usize)
            .and_then(Option::as_deref)
        {
            Some(content) => strip(content) == strip(text),
            None => true,
        }
    }

    /// Fill the statement and function counts of `coverage`, built from the
    /// original source, from the ranges of the generated script. Returns the
    /// statement ids that have no generated code, such as a statement that a
    /// transpiler removed.
    pub(super) fn apply(
        &self,
        coverage: &mut FileCoverage,
        source_index: u32,
        functions: &[V8FunctionCoverage],
    ) -> FxHashSet<String> {
        let ranges = ranges_by_width(functions);
        let source = self.map.source(source_index).to_owned();
        let count_at = |position: &Position| -> Option<u32> {
            let offset = self.generated_offset(&source, position)?;
            Some(smallest_range_count(&ranges, offset))
        };

        let mut unmapped = FxHashSet::default();
        let mut statement_counts = Vec::with_capacity(coverage.statement_map.len());
        for (id, location) in &coverage.statement_map {
            match count_at(&location.start) {
                Some(count) => statement_counts.push((id.clone(), count)),
                None => {
                    unmapped.insert(id.clone());
                }
            }
        }
        for (id, count) in statement_counts {
            coverage.s.insert(id, count);
        }
        let function_counts: Vec<(String, u32)> = coverage
            .fn_map
            .iter()
            .map(|(id, entry)| (id.clone(), count_at(&entry.loc.start).unwrap_or(0)))
            .collect();
        for (id, count) in function_counts {
            coverage.f.insert(id, count);
        }
        unmapped
    }

    /// The generated UTF-16 offset of an original Istanbul position (1-based
    /// line, 0-based UTF-16 column).
    fn generated_offset(&self, source: &str, position: &Position) -> Option<u32> {
        let line = position.line.checked_sub(1)?;
        let generated = self
            .map
            .generated_position_for(source, line, position.column)
            .or_else(|| {
                self.map.generated_position_for_with_bias(
                    source,
                    line,
                    position.column,
                    Bias::LeastUpperBound,
                )
            })?;
        let line_start = *self.line_starts.get(generated.line as usize)?;
        Some(line_start.saturating_add(generated.column))
    }
}

fn source_path(source: &str, script_dir: Option<&Path>) -> Option<PathBuf> {
    if let Ok(url) = url::Url::parse(source) {
        return if url.scheme() == "file" {
            url.to_file_path().ok()
        } else if fallow_types::path_util::looks_like_windows_absolute_path(source) {
            Some(PathBuf::from(source))
        } else {
            None
        };
    }
    let path = PathBuf::from(source);
    if fallow_types::path_util::is_absolute_path_any_platform(&path) {
        return Some(path);
    }
    Some(script_dir?.join(path))
}

fn ranges_by_width(functions: &[V8FunctionCoverage]) -> Vec<V8CoverageRange> {
    let mut ranges: Vec<V8CoverageRange> = functions
        .iter()
        .flat_map(|function| function.ranges.iter().copied())
        .collect();
    ranges.sort_by_key(|range| range.end_offset.saturating_sub(range.start_offset));
    ranges
}

/// The count of the smallest range that contains `offset`. V8 nests block
/// ranges inside function ranges, so the smallest one is the most specific.
fn smallest_range_count(ranges_by_width: &[V8CoverageRange], offset: u32) -> u32 {
    ranges_by_width
        .iter()
        .find(|range| range.start_offset <= offset && offset < range.end_offset)
        .map_or(0, |range| range.count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_lengths_give_line_starts_and_length() {
        let entry = SourceMapCacheEntry {
            line_lengths: vec![10, 0, 5],
            data: serde_json::json!({ "version": 3, "sources": [], "mappings": "" }),
        };
        let script = GeneratedScript::parse(&entry).unwrap();
        assert_eq!(script.line_starts, vec![0, 11, 12]);
        assert_eq!(script.generated_len, 17);
    }

    #[test]
    fn sources_resolve_against_the_script_directory() {
        let dir = if cfg!(windows) {
            "file:///C:/p/dist/"
        } else {
            "file:///p/dist/"
        };
        let entry = SourceMapCacheEntry {
            line_lengths: vec![1],
            data: serde_json::json!({
                "version": 3,
                "sources": ["../src/a.ts", "webpack://app/./b.ts"],
                "mappings": ""
            }),
        };
        let script = GeneratedScript::parse(&entry).unwrap();
        let paths = script.source_paths(&format!("{dir}bundle.js"));
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0].0, 0);
        assert!(paths[0].1.ends_with(Path::new("dist/../src/a.ts")));
    }

    #[test]
    fn smallest_range_wins() {
        let ranges = ranges_by_width(&[V8FunctionCoverage {
            function_name: String::new(),
            is_block_coverage: true,
            ranges: vec![
                V8CoverageRange {
                    start_offset: 0,
                    end_offset: 100,
                    count: 1,
                },
                V8CoverageRange {
                    start_offset: 40,
                    end_offset: 60,
                    count: 0,
                },
            ],
        }]);
        assert_eq!(smallest_range_count(&ranges, 10), 1);
        assert_eq!(smallest_range_count(&ranges, 50), 0);
        assert_eq!(smallest_range_count(&ranges, 100), 0);
    }
}
