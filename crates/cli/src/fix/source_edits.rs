//! The analysed text of each source file that `fallow fix` changes, and the
//! byte edits that the source fixers queue against it.
//!
//! Each source fixer reads a file through [`read_source_snapshot`] and gets
//! the text that the analysis read, also after another fixer queued edits for
//! the file. The line numbers of the analysis are valid in that text. The plan
//! applies the queued edits of a file one time, at commit.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use super::io::{EncodingError, EncodingMetadata, bytes_with_optional_bom, read_source};
use super::plan::{CapturedHashes, FixPlan, SkipReason};

/// One replacement of a byte range of the analysed text of a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SourceEdit {
    pub(super) start: usize,
    pub(super) end: usize,
    pub(super) replacement: String,
}

impl SourceEdit {
    fn is_deletion(&self) -> bool {
        self.replacement.is_empty()
    }

    fn overlaps(&self, other: &Self) -> bool {
        self.start < other.end && other.start < self.end
    }

    /// True when `self` and `other` overlap and cannot both apply. Two line
    /// deletions may share one line ending: the deletion of the last line
    /// starts at the line ending before it, which the deletion of the line
    /// above also removes. The plan applies such a pair as one deletion.
    fn conflicts_with(&self, other: &Self, content: &str, line_ending: &str) -> bool {
        if !self.overlaps(other) {
            return false;
        }
        let shared = self.start.max(other.start)..self.end.min(other.end);
        !(self.is_deletion() && other.is_deletion() && content.get(shared) == Some(line_ending))
    }
}

/// The result of a request to queue a group of edits for one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum QueueOutcome {
    /// The plan holds the edits of the group.
    Queued,
    /// Each edit of the group equals a queued edit or lies inside a queued
    /// deletion. The plan holds no new edit.
    Covered,
    /// One edit of the group is not valid, overlaps another edit of the
    /// group, or overlaps a queued edit. The plan holds no edit of the group.
    Conflict,
}

struct SourceSnapshot {
    content: String,
    meta: EncodingMetadata,
    edits: Vec<SourceEdit>,
}

/// The analysed text of each source file that a fixer read, with the edits
/// that the fixers queued against it.
#[derive(Default)]
pub(super) struct SourceSnapshots {
    order: Vec<PathBuf>,
    by_path: FxHashMap<PathBuf, SourceSnapshot>,
}

impl SourceSnapshots {
    fn get(&self, path: &Path) -> Option<(String, EncodingMetadata)> {
        self.by_path
            .get(path)
            .map(|snapshot| (snapshot.content.clone(), snapshot.meta))
    }

    fn insert(&mut self, path: &Path, content: String, meta: EncodingMetadata) {
        self.order.push(path.to_path_buf());
        self.by_path.insert(
            path.to_path_buf(),
            SourceSnapshot {
                content,
                meta,
                edits: Vec::new(),
            },
        );
    }

    /// Queue `group` for `path`. All edits of the group go in, or none.
    pub(super) fn queue(&mut self, path: &Path, mut group: Vec<SourceEdit>) -> QueueOutcome {
        let Some(snapshot) = self.by_path.get_mut(path) else {
            return QueueOutcome::Conflict;
        };
        let content = snapshot.content.as_str();
        let line_ending = snapshot.meta.line_ending;
        let valid = |edit: &SourceEdit| {
            edit.start < edit.end
                && edit.end <= content.len()
                && content.is_char_boundary(edit.start)
                && content.is_char_boundary(edit.end)
        };
        if !group.iter().all(valid) {
            return QueueOutcome::Conflict;
        }
        group.sort_by_key(|edit| (edit.start, edit.end));
        group.dedup();
        if group
            .windows(2)
            .any(|pair| pair[0].conflicts_with(&pair[1], content, line_ending))
        {
            return QueueOutcome::Conflict;
        }

        let mut fresh = Vec::with_capacity(group.len());
        for edit in group {
            let mut covered = false;
            for queued in &snapshot.edits {
                if *queued == edit
                    || (queued.is_deletion()
                        && queued.start <= edit.start
                        && edit.end <= queued.end)
                {
                    covered = true;
                } else if edit.conflicts_with(queued, content, line_ending) {
                    return QueueOutcome::Conflict;
                }
            }
            if !covered {
                fresh.push(edit);
            }
        }
        if fresh.is_empty() {
            return QueueOutcome::Covered;
        }
        snapshot.edits.extend(fresh);
        QueueOutcome::Queued
    }

    /// `(path, original bytes, new bytes)` for each file with a queued edit,
    /// in the order of the first read.
    pub(super) fn into_rewrites(mut self) -> Vec<(PathBuf, Vec<u8>, Vec<u8>)> {
        let mut rewrites = Vec::new();
        for path in self.order {
            let Some(snapshot) = self.by_path.remove(&path) else {
                continue;
            };
            if snapshot.edits.is_empty() {
                continue;
            }
            let line_ending = snapshot.meta.line_ending;
            let mut result = snapshot.content.clone();
            for edit in merge_shared_line_endings(snapshot.edits).iter().rev() {
                result.replace_range(edit.start..edit.end, &edit.replacement);
            }
            if snapshot.content.ends_with(line_ending) && !result.ends_with(line_ending) {
                result.push_str(line_ending);
            }
            let original = bytes_with_optional_bom(snapshot.content, &snapshot.meta);
            let rewritten = bytes_with_optional_bom(result, &snapshot.meta);
            rewrites.push((path, original, rewritten));
        }
        rewrites
    }
}

/// The edits sorted by start, with each pair of deletions that share a line
/// ending joined to one deletion.
fn merge_shared_line_endings(mut edits: Vec<SourceEdit>) -> Vec<SourceEdit> {
    edits.sort_by_key(|edit| (edit.start, edit.end));
    let mut merged: Vec<SourceEdit> = Vec::with_capacity(edits.len());
    for edit in edits {
        if let Some(last) = merged.last_mut()
            && last.is_deletion()
            && edit.is_deletion()
            && edit.start < last.end
        {
            last.end = last.end.max(edit.end);
            continue;
        }
        merged.push(edit);
    }
    merged
}

/// The analysed text of `path`. The first call reads the file and checks its
/// hash. Each later call returns the same text.
pub(super) fn read_source_snapshot(
    root: &Path,
    path: &Path,
    hashes: &CapturedHashes,
    plan: &mut FixPlan,
) -> Option<(String, EncodingMetadata)> {
    if let Some(snapshot) = plan.sources_mut().get(path) {
        return Some(snapshot);
    }
    let read_result = match read_source(root, path) {
        Ok(opt) => opt,
        Err(EncodingError::MixedLineEndings { .. }) => {
            plan.skip(path.to_path_buf(), SkipReason::MixedLineEndings);
            return None;
        }
    };
    let (content, meta) = read_result?;
    if let Some(&expected) = hashes.get(path) {
        let actual = xxhash_rust::xxh3::xxh3_64(content.as_bytes());
        if actual != expected {
            plan.skip(path.to_path_buf(), SkipReason::ContentChanged);
            return None;
        }
    }
    plan.sources_mut().insert(path, content.clone(), meta);
    Some((content, meta))
}

/// The byte offset of the start of each element of
/// `content.split(line_ending)`.
pub(super) fn split_line_starts(content: &str, line_ending: &str) -> Vec<usize> {
    std::iter::once(0)
        .chain(
            content
                .match_indices(line_ending)
                .map(|(offset, _)| offset + line_ending.len()),
        )
        .collect()
}

/// The edit that replaces the split elements `first..=last` of `content`
/// with `replacement`, or deletes them when `replacement` is `None`.
pub(super) fn line_edit(
    content: &str,
    line_ending: &str,
    first: usize,
    last: usize,
    replacement: Option<&[String]>,
) -> SourceEdit {
    let starts = split_line_starts(content, line_ending);
    let count = starts.len();
    let text_end = |index: usize| {
        if index + 1 == count {
            content.len()
        } else {
            starts[index + 1] - line_ending.len()
        }
    };
    match replacement {
        Some(lines) if !lines.is_empty() => SourceEdit {
            start: starts[first],
            end: text_end(last),
            replacement: lines.join(line_ending),
        },
        _ if last + 1 < count => SourceEdit {
            start: starts[first],
            end: starts[last + 1],
            replacement: String::new(),
        },
        _ => SourceEdit {
            start: if first > 0 {
                starts[first] - line_ending.len()
            } else {
                0
            },
            end: content.len(),
            replacement: String::new(),
        },
    }
}

/// One deletion for each run of neighbour indexes in `lines`.
pub(super) fn line_deletions(content: &str, line_ending: &str, lines: &[usize]) -> Vec<SourceEdit> {
    let mut sorted = lines.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut edits = Vec::new();
    let mut iter = sorted.into_iter();
    let Some(mut run_first) = iter.next() else {
        return edits;
    };
    let mut run_last = run_first;
    for index in iter {
        if index == run_last + 1 {
            run_last = index;
            continue;
        }
        edits.push(line_edit(content, line_ending, run_first, run_last, None));
        run_first = index;
        run_last = index;
    }
    edits.push(line_edit(content, line_ending, run_first, run_last, None));
    edits
}

#[cfg(test)]
#[path = "source_edits_tests.rs"]
mod tests;
