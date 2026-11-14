use std::borrow::Cow;
use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

/// Refuse to parse a unified diff larger than this.
pub const MAX_DIFF_BYTES: u64 = 10 * 1024 * 1024;

/// Stop indexing added lines past this count.
pub const MAX_ADDED_LINES: usize = 1_000_000;

/// Parsed, command-neutral index of files and added lines in a unified diff.
///
/// Keys are exactly the paths the diff names in its `+++ b/<path>` headers,
/// so they live in whatever namespace produced the diff — for `git diff`,
/// relative to the repository toplevel. [`DiffIndex::base`] records the
/// directory those keys are relative to, so a finding's absolute path can be
/// mapped into the same namespace before lookup. Without it, an analysis root
/// below the toplevel silently misses every key.
#[derive(Debug, Default, Clone)]
pub struct DiffIndex {
    added_lines: FxHashMap<String, FxHashSet<u64>>,
    changed_paths: FxHashSet<String>,
    touched_files: FxHashSet<String>,
    added_line_count: usize,
    total_added_lines: usize,
    total_removed_lines: usize,
    hunk_count: usize,
    rename_pairs: FxHashMap<String, String>,
    base: Option<PathBuf>,
    root_offset: String,
    base_lines: Box<BaseLineMap>,
}

/// The data that maps head lines back to base lines. It is boxed, so the
/// index stays small in the enums that hold it.
#[derive(Debug, Default, Clone)]
struct BaseLineMap {
    change_blocks: FxHashMap<String, Vec<ChangeBlock>>,
    new_files: FxHashSet<String>,
    lines_capped: bool,
}

/// One run of removed and added lines with no unchanged line in it.
#[derive(Debug, Clone, Copy)]
struct ChangeBlock {
    /// First head line of the run. When the run adds no line, the head line
    /// after it.
    new_start: u64,
    /// Number of added lines.
    new_len: u64,
    /// First base line of the run. When the run removes no line, the base
    /// line after it.
    old_start: u64,
    /// Number of removed lines.
    old_len: u64,
}

/// Where one head line sits relative to the change blocks of its file.
enum HeadLinePlace {
    /// An unchanged line, at this base line.
    Unchanged(u64),
    /// An added line of this block.
    Added(ChangeBlock),
}

/// Mutable cursor state threaded through unified-diff parsing.
#[derive(Default)]
struct DiffParseState {
    current_file: Option<String>,
    new_line: u64,
    old_line: u64,
    pending_old_path: Option<String>,
    pending_rename_from: Option<String>,
    pending_new_file: bool,
}

impl DiffIndex {
    /// Parse a unified diff into an index of changed files and added lines.
    ///
    /// Indexes at most [`MAX_ADDED_LINES`] added lines; totals used by
    /// [`DiffIndex::net_lines`] keep counting past the cap.
    #[must_use]
    pub fn from_unified_diff(diff: &str) -> Self {
        let mut index = Self::default();
        let mut state = DiffParseState::default();

        for line in diff.lines() {
            if index.handle_diff_header_line(line, &mut state) {
                continue;
            }
            index.handle_diff_content_line(line, &mut state);
        }

        index
    }

    fn handle_diff_header_line(&mut self, line: &str, state: &mut DiffParseState) -> bool {
        if line.starts_with("diff --git ") {
            state.current_file = None;
            state.pending_new_file = false;
            state.pending_old_path = None;
            state.pending_rename_from = None;
            return true;
        }
        if let Some(rest) = line.strip_prefix("rename from ") {
            state.pending_rename_from = Some(rest.to_owned());
            return true;
        }
        if let Some(rest) = line.strip_prefix("rename to ") {
            if let Some(from) = state.pending_rename_from.take() {
                self.rename_pairs.insert(rest.to_owned(), from);
                self.changed_paths.insert(rest.to_owned());
                self.touched_files.insert(rest.to_owned());
            }
            return true;
        }
        if let Some(path) = line.strip_prefix("--- a/") {
            state.pending_old_path = Some(path.to_string());
            return true;
        }
        if line.starts_with("--- /dev/null") {
            state.pending_old_path = None;
            state.pending_new_file = true;
            return true;
        }
        if let Some(path) = line.strip_prefix("+++ b/") {
            state.pending_old_path = None;
            if std::mem::take(&mut state.pending_new_file) {
                self.base_lines.new_files.insert(path.to_string());
            }
            state.current_file = Some(path.to_string());
            self.changed_paths.insert(path.to_string());
            self.touched_files.insert(path.to_string());
            return true;
        }
        if line.starts_with("+++ /dev/null") {
            state.current_file = state.pending_old_path.take();
            if let Some(path) = state.current_file.as_ref() {
                self.changed_paths.insert(path.clone());
            }
            return true;
        }
        if let Some(header) = line.strip_prefix("@@ ") {
            if let Some(start) = parse_new_hunk_start(header) {
                state.old_line = hunk_next_line(header, '-').unwrap_or(start);
                state.new_line = hunk_next_line(header, '+').unwrap_or(start);
                self.hunk_count += 1;
            }
            return true;
        }
        false
    }

    fn handle_diff_content_line(&mut self, line: &str, state: &mut DiffParseState) {
        let Some(path) = state.current_file.as_ref() else {
            return;
        };
        if line.starts_with('+') {
            self.total_added_lines += 1;
            if !line.starts_with("+++") && self.added_line_count < MAX_ADDED_LINES {
                self.added_lines
                    .entry(path.clone())
                    .or_default()
                    .insert(state.new_line);
                self.added_line_count += 1;
                self.record_change_line(state, true);
            } else {
                self.base_lines.lines_capped = true;
            }
            state.new_line += 1;
        } else if line.starts_with('-') {
            self.total_removed_lines += 1;
            self.record_change_line(state, false);
            state.old_line += 1;
        } else if !line.starts_with('\\') {
            // A `\ No newline at end of file` marker is not a file line.
            state.new_line += 1;
            state.old_line += 1;
        }
    }

    /// Add one removed or added line to the change block that ends at the
    /// cursor, or open a new block there.
    fn record_change_line(&mut self, state: &DiffParseState, added: bool) {
        let Some(path) = state.current_file.as_ref() else {
            return;
        };
        let blocks = self
            .base_lines
            .change_blocks
            .entry(path.clone())
            .or_default();
        let continues = blocks.last().is_some_and(|block| {
            block.new_start + block.new_len == state.new_line
                && block.old_start + block.old_len == state.old_line
        });
        if !continues {
            blocks.push(ChangeBlock {
                new_start: state.new_line,
                new_len: 0,
                old_start: state.old_line,
                old_len: 0,
            });
        }
        if let Some(block) = blocks.last_mut() {
            if added {
                block.new_len += 1;
            } else {
                block.old_len += 1;
            }
        }
    }

    /// Map an inclusive head line range of `path` to the base tree. `path`
    /// is a key of this diff.
    ///
    /// Unchanged lines map exactly. An added line at the start of the range
    /// maps to the first base line that its block removed. An added line at
    /// the end maps to the last one. A file that the diff does not name maps
    /// unchanged. `None` when the range holds no base line, when the diff
    /// adds `path` as a new file, or when the index stopped at
    /// [`MAX_ADDED_LINES`].
    #[must_use]
    pub fn base_line_range(&self, path: &str, start: u64, end: u64) -> Option<(u64, u64)> {
        let map = &self.base_lines;
        if map.lines_capped || start > end || map.new_files.contains(path) {
            return None;
        }
        let blocks = map.change_blocks.get(path).map_or(&[][..], Vec::as_slice);
        let base_start = match head_line_place(blocks, start) {
            HeadLinePlace::Unchanged(line) => line,
            HeadLinePlace::Added(block) => block.old_start,
        };
        let base_end = match head_line_place(blocks, end) {
            HeadLinePlace::Unchanged(line) => line,
            HeadLinePlace::Added(block) => (block.old_start + block.old_len).checked_sub(1)?,
        };
        (base_start <= base_end).then_some((base_start, base_end))
    }

    /// Pre-rename path for `head_path`, in the diff's key namespace, when the
    /// diff renamed it.
    #[must_use]
    pub fn old_path_for(&self, head_path: &str) -> Option<&str> {
        self.rename_pairs.get(head_path).map(String::as_str)
    }

    /// Number of indexed added lines, capped at [`MAX_ADDED_LINES`].
    #[must_use]
    pub fn added_line_count(&self) -> usize {
        self.added_line_count
    }

    /// Number of parsed unified-diff hunk headers.
    #[must_use]
    pub fn hunk_count(&self) -> usize {
        self.hunk_count
    }

    /// Total added lines minus total removed lines, independent of the added
    /// line indexing cap.
    #[must_use]
    pub fn net_lines(&self) -> i64 {
        let added = i64::try_from(self.total_added_lines).unwrap_or(i64::MAX);
        let removed = i64::try_from(self.total_removed_lines).unwrap_or(i64::MAX);
        added.saturating_sub(removed)
    }

    /// Whether this diff changes the path, including when the path is deleted.
    #[must_use]
    pub fn changes_path(&self, path: &str) -> bool {
        self.changed_paths.contains(path)
    }

    /// Paths changed by the diff, using the old path for deleted files.
    pub fn changed_paths(&self) -> impl Iterator<Item = &str> {
        self.changed_paths.iter().map(String::as_str)
    }

    /// Whether this diff touches an analyzable path in the head tree.
    #[must_use]
    pub fn touches_file(&self, path: &str) -> bool {
        self.touched_files.contains(path)
    }

    /// Whether any added line in `path` falls inside the inclusive 1-based
    /// `start..=end` range.
    #[must_use]
    pub fn range_overlaps_added(&self, path: &str, start: u64, end: u64) -> bool {
        self.first_added_line_in_range(path, start, end).is_some()
    }

    /// Lowest 1-based added line inside the inclusive `start..=end` range.
    /// Returns `None` for an invalid range or when the diff adds no line in it.
    #[must_use]
    pub fn first_added_line_in_range(&self, path: &str, start: u64, end: u64) -> Option<u64> {
        if end < start {
            return None;
        }
        let added = self.added_lines.get(path)?;
        let lo = start.max(1);
        added
            .iter()
            .copied()
            .filter(|line| *line >= lo && *line <= end)
            .min()
    }

    /// Whether the 1-based `line` in `path` was added by this diff.
    #[must_use]
    pub fn line_is_added(&self, path: &str, line: u64) -> bool {
        self.added_lines
            .get(path)
            .is_some_and(|lines| lines.contains(&line))
    }

    /// Whether `line` sits within `radius` lines of any added line in `path`.
    #[must_use]
    pub fn line_within_added_context(&self, path: &str, line: u64, radius: u64) -> bool {
        self.added_lines
            .get(path)
            .is_some_and(|lines| lines.iter().any(|added| line.abs_diff(*added) <= radius))
    }

    /// Set of 1-based added line numbers for `path`, when the diff added any.
    #[must_use]
    pub fn added_lines_in(&self, path: &str) -> Option<&FxHashSet<u64>> {
        self.added_lines.get(path)
    }

    /// Declare the directory this diff's paths are relative to (the git
    /// toplevel for `git diff` output).
    #[must_use]
    pub fn with_base(mut self, base: impl Into<PathBuf>) -> Self {
        self.base = Some(base.into());
        self
    }

    /// Declare where the analysis root sits below [`DiffIndex::base`], as a
    /// forward-slashed relative path (empty when they are the same directory).
    ///
    /// Findings are addressed relative to the analysis root; this diff's keys
    /// are relative to its base. Everything that looks a finding up in this
    /// index has to cross that gap, so the index carries the offset rather than
    /// making each caller rediscover it.
    #[must_use]
    pub fn with_root_offset(mut self, offset: impl Into<String>) -> Self {
        let mut offset = offset.into();
        // A trailing separator would make `strip_path_component_prefix` demand a
        // second one and never match. Normalize rather than trust the caller.
        offset.truncate(offset.trim_end_matches('/').len());
        self.root_offset = offset;
        self
    }

    /// Relative path of the analysis root below [`DiffIndex::base`]; empty
    /// when they coincide.
    #[must_use]
    pub fn root_offset(&self) -> &str {
        &self.root_offset
    }

    /// Lift an analysis-root-relative path into this diff's key namespace.
    #[must_use]
    pub fn key_for_root_relative<'a>(&self, rel: &'a str) -> Cow<'a, str> {
        if self.root_offset.is_empty() {
            return Cow::Borrowed(rel);
        }
        Cow::Owned(format!("{}/{rel}", self.root_offset))
    }

    /// Lower one of this diff's keys back to an analysis-root-relative path.
    /// `None` when the key names a file outside the analysis root.
    #[must_use]
    pub fn root_relative_from_key<'a>(&self, key: &'a str) -> Option<Cow<'a, str>> {
        if self.root_offset.is_empty() {
            return Some(Cow::Borrowed(key));
        }
        strip_path_component_prefix(key, &self.root_offset).map(Cow::Borrowed)
    }

    /// The pre-rename path of an analysis-root-relative path, itself
    /// analysis-root-relative. Crosses into the diff's key namespace and back,
    /// so a monorepo package below the diff's base resolves its renames.
    #[must_use]
    pub fn old_path_for_root_relative<'a>(&'a self, rel: &str) -> Option<Cow<'a, str>> {
        let old = self.old_path_for(&self.key_for_root_relative(rel))?;
        self.root_relative_from_key(old)
    }

    /// Directory this diff's keys are relative to, when declared via
    /// [`DiffIndex::with_base`].
    #[must_use]
    pub fn base(&self) -> Option<&Path> {
        self.base.as_deref()
    }

    /// Analyzable head-tree paths this diff touches, in the diff's key
    /// namespace.
    pub fn touched_files(&self) -> impl Iterator<Item = &str> {
        self.touched_files.iter().map(String::as_str)
    }

    /// Map a finding's path into this diff's key namespace.
    ///
    /// Relativizes against [`DiffIndex::base`] when one was declared, else
    /// against `fallback_root`. When base == `fallback_root` (the analysis
    /// root is the repository toplevel) both agree, so behavior is unchanged.
    #[must_use]
    pub fn key_for(&self, path: &Path, fallback_root: &Path) -> Option<String> {
        relative_to_diff_path(path, self.base.as_deref().unwrap_or(fallback_root))
    }
}

/// Relativize `path` against `root` with forward slashes, passing an already
/// relative path through unchanged. `None` for absolute paths outside `root`.
#[must_use]
pub fn relative_to_diff_path(path: &Path, root: &Path) -> Option<String> {
    if let Ok(stripped) = path.strip_prefix(root) {
        return Some(stripped.to_string_lossy().replace('\\', "/"));
    }
    if fallow_types::path_util::is_absolute_path_any_platform(path) {
        return None;
    }
    Some(path.to_string_lossy().replace('\\', "/"))
}

/// Strip `prefix` and its trailing separator, only on a path-component
/// boundary, so `packages/pkg-extra/a.ts` is never read as `packages/pkg`
/// plus `-extra/a.ts`.
#[must_use]
pub fn strip_path_component_prefix<'a>(path: &'a str, prefix: &str) -> Option<&'a str> {
    path.strip_prefix(prefix)?.strip_prefix('/')
}

/// Place one head line among the change blocks of its file, in head order.
fn head_line_place(blocks: &[ChangeBlock], line: u64) -> HeadLinePlace {
    let mut shift: i128 = 0;
    for block in blocks {
        if line < block.new_start {
            break;
        }
        if line < block.new_start + block.new_len {
            return HeadLinePlace::Added(*block);
        }
        shift += i128::from(block.old_len) - i128::from(block.new_len);
    }
    HeadLinePlace::Unchanged(u64::try_from(i128::from(line) + shift).unwrap_or(0))
}

/// The first line of one side of a `@@ -a,b +c,d @@` hunk. `side` is `-` for
/// the base and `+` for the head. A side with no line (`b` or `d` is 0) names
/// the line before the hunk, so its first line is the next one.
fn hunk_next_line(header: &str, side: char) -> Option<u64> {
    let range = header
        .split_whitespace()
        .find_map(|part| part.strip_prefix(side))?;
    let (start, len) = match range.split_once(',') {
        Some((start, len)) => (start.parse::<u64>().ok()?, len.parse::<u64>().ok()?),
        None => (range.parse::<u64>().ok()?, 1),
    };
    Some(if len == 0 { start + 1 } else { start })
}

/// Parse the new-file start line out of a `@@ -a,b +c,d @@` hunk header.
pub fn parse_new_hunk_start(header: &str) -> Option<u64> {
    let plus = header.find('+')?;
    let rest = &header[plus + 1..];
    let end = rest
        .find(|c: char| c == ',' || c.is_ascii_whitespace())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EDIT_DIFF: &str = "diff --git a/a.js b/a.js\n--- a/a.js\n+++ b/a.js\n@@ -7 +7 @@\n-old seven\n+new seven\n@@ -10,0 +11,2 @@\n+added eleven\n+added twelve\n@@ -20,2 +21,0 @@\n-gone twenty\n-gone twenty-one\n";

    #[test]
    fn base_line_range_maps_unchanged_lines_across_blocks() {
        let index = DiffIndex::from_unified_diff(EDIT_DIFF);
        assert_eq!(index.base_line_range("a.js", 1, 6), Some((1, 6)));
        // Lines after the two-line insertion move up by two.
        assert_eq!(index.base_line_range("a.js", 13, 21), Some((11, 19)));
        // Lines after the two-line removal move back down by two.
        assert_eq!(index.base_line_range("a.js", 22, 25), Some((22, 25)));
    }

    #[test]
    fn base_line_range_maps_a_replaced_line_to_its_removed_line() {
        let index = DiffIndex::from_unified_diff(EDIT_DIFF);
        assert_eq!(index.base_line_range("a.js", 7, 7), Some((7, 7)));
        assert_eq!(index.base_line_range("a.js", 1, 12), Some((1, 10)));
    }

    #[test]
    fn base_line_range_has_no_base_line_for_pure_additions() {
        let index = DiffIndex::from_unified_diff(EDIT_DIFF);
        assert_eq!(index.base_line_range("a.js", 11, 12), None);
        let new_file = DiffIndex::from_unified_diff(
            "diff --git a/n.js b/n.js\n--- /dev/null\n+++ b/n.js\n@@ -0,0 +1,2 @@\n+one\n+two\n",
        );
        assert_eq!(new_file.base_line_range("n.js", 1, 2), None);
        // A new file has no base line, also outside its added lines.
        assert_eq!(new_file.base_line_range("n.js", 3, 4), None);
    }

    #[test]
    fn base_line_range_keeps_files_outside_the_diff() {
        let index = DiffIndex::from_unified_diff(EDIT_DIFF);
        assert_eq!(index.base_line_range("other.js", 3, 9), Some((3, 9)));
    }

    #[test]
    fn no_newline_marker_does_not_move_the_line_cursor() {
        let index = DiffIndex::from_unified_diff(
            "diff --git a/a.js b/a.js\n--- a/a.js\n+++ b/a.js\n@@ -3 +3,2 @@\n-last\n\\ No newline at end of file\n+last\n+more\n",
        );
        assert!(index.line_is_added("a.js", 3));
        assert!(index.line_is_added("a.js", 4));
        assert!(!index.line_is_added("a.js", 5));
    }

    #[test]
    fn from_unified_diff_caps_added_lines_at_threshold() {
        let header =
            "diff --git a/big.txt b/big.txt\n--- a/big.txt\n+++ b/big.txt\n@@ -0,0 +1,100 @@\n";
        let mut body = String::with_capacity(MAX_ADDED_LINES * 16);
        for _ in 0..(MAX_ADDED_LINES + 100) {
            body.push_str("+x\n");
        }
        let mut diff = String::with_capacity(header.len() + body.len());
        diff.push_str(header);
        diff.push_str(&body);

        let index = DiffIndex::from_unified_diff(&diff);
        assert!(
            index.added_line_count() <= MAX_ADDED_LINES,
            "indexed {} lines, cap is {MAX_ADDED_LINES}",
            index.added_line_count()
        );
        assert_eq!(
            Some(index.net_lines()),
            i64::try_from(MAX_ADDED_LINES + 100).ok()
        );
        assert_eq!(index.hunk_count(), 1);
    }

    #[test]
    fn from_unified_diff_counts_additions_removals_and_hunks() {
        let diff = "\
diff --git a/src/a.ts b/src/a.ts
--- a/src/a.ts
+++ b/src/a.ts
@@ -1,3 +1,4 @@
-old
+new
+extra
+++flag
 context
@@ -10,2 +11,1 @@
-removed
---flag
 kept
";
        let index = DiffIndex::from_unified_diff(diff);

        assert_eq!(index.hunk_count(), 2);
        assert_eq!(index.net_lines(), 0);
        assert!(index.changes_path("src/a.ts"));
        assert!(index.touches_file("src/a.ts"));
    }

    #[test]
    fn rename_only_diff_has_no_line_or_hunk_changes() {
        let diff = "\
diff --git a/src/old.ts b/src/new.ts
similarity index 100%
rename from src/old.ts
rename to src/new.ts
";
        let index = DiffIndex::from_unified_diff(diff);

        assert_eq!(index.hunk_count(), 0);
        assert_eq!(index.net_lines(), 0);
        assert!(index.changes_path("src/new.ts"));
        assert!(index.touches_file("src/new.ts"));
    }

    #[test]
    fn range_overlaps_added_hotspot_starting_before_diff_touches_inside() {
        let diff = "\
diff --git a/src/big.ts b/src/big.ts
--- a/src/big.ts
+++ b/src/big.ts
@@ -114,1 +114,2 @@
 ctx
+touched
";
        let index = DiffIndex::from_unified_diff(diff);
        assert!(index.range_overlaps_added("src/big.ts", 10, 120));
        assert!(!index.range_overlaps_added("src/other.ts", 10, 120));
        assert!(!index.range_overlaps_added("src/big.ts", 10, 100));
        assert!(!index.range_overlaps_added("src/big.ts", 200, 100));
    }

    #[test]
    fn rename_header_records_old_path() {
        let diff = "\
diff --git a/src/old.ts b/src/new.ts
similarity index 90%
rename from src/old.ts
rename to src/new.ts
--- a/src/old.ts
+++ b/src/new.ts
@@ -1,1 +1,1 @@
-old
+new
";
        let index = DiffIndex::from_unified_diff(diff);
        assert_eq!(index.old_path_for("src/new.ts"), Some("src/old.ts"));
        assert!(index.touches_file("src/new.ts"));
    }

    #[test]
    fn empty_diff_has_zero_added_lines_and_no_touched_files() {
        let index = DiffIndex::from_unified_diff("");
        assert_eq!(index.added_line_count(), 0);
        assert!(!index.touches_file("src/a.ts"));
    }

    #[test]
    fn delete_only_diff_records_removal_without_touching_head_file() {
        let diff = "\
diff --git a/src/a.ts b/src/a.ts
--- a/src/a.ts
+++ /dev/null
@@ -1,1 +0,0 @@
-old
";
        let index = DiffIndex::from_unified_diff(diff);
        assert_eq!(index.added_line_count(), 0);
        assert_eq!(index.hunk_count(), 1);
        assert_eq!(index.net_lines(), -1);
        assert!(index.changes_path("src/a.ts"));
        assert!(!index.touches_file("src/a.ts"));
    }

    #[test]
    fn relative_to_diff_path_strips_absolute_root() {
        let root = Path::new("/project");
        let path = Path::new("/project/src/a.ts");
        assert_eq!(
            relative_to_diff_path(path, root).as_deref(),
            Some("src/a.ts")
        );
    }

    #[test]
    fn relative_to_diff_path_passes_through_relative() {
        let root = Path::new("/project");
        let path = Path::new("src/a.ts");
        assert_eq!(
            relative_to_diff_path(path, root).as_deref(),
            Some("src/a.ts")
        );
    }

    #[test]
    fn relative_to_diff_path_returns_none_for_path_outside_root() {
        let root = Path::new("/project");
        let path = Path::new("/elsewhere/src/a.ts");
        assert!(relative_to_diff_path(path, root).is_none());
    }

    #[test]
    fn key_for_without_base_relativizes_against_the_fallback_root() {
        let index = DiffIndex::default();
        assert_eq!(
            index
                .key_for(Path::new("/repo/pkg/src/a.ts"), Path::new("/repo/pkg"))
                .as_deref(),
            Some("src/a.ts")
        );
    }

    #[test]
    fn key_for_with_base_equal_to_root_is_unchanged() {
        let index = DiffIndex::default().with_base("/repo");
        assert_eq!(
            index
                .key_for(Path::new("/repo/src/a.ts"), Path::new("/repo"))
                .as_deref(),
            Some("src/a.ts")
        );
    }

    /// The regression: an analysis root below the repo toplevel must still
    /// produce the toplevel-relative key `git diff` writes.
    #[test]
    fn key_for_with_base_above_root_yields_repo_root_relative_key() {
        let index = DiffIndex::default().with_base("/repo");
        assert_eq!(
            index
                .key_for(Path::new("/repo/pkg/src/a.ts"), Path::new("/repo/pkg"))
                .as_deref(),
            Some("pkg/src/a.ts")
        );
    }

    #[test]
    fn key_for_with_base_above_root_matches_a_repo_root_relative_diff() {
        let diff = "\
diff --git a/pkg/src/a.ts b/pkg/src/a.ts
--- a/pkg/src/a.ts
+++ b/pkg/src/a.ts
@@ -1,0 +2,1 @@
+added
";
        let index = DiffIndex::from_unified_diff(diff).with_base("/repo");
        let key = index
            .key_for(Path::new("/repo/pkg/src/a.ts"), Path::new("/repo/pkg"))
            .expect("finding path is under the base");

        assert!(index.touches_file(&key));
        assert!(index.line_is_added(&key, 2));

        // Without the base, the same finding keys as `src/a.ts` and misses.
        let unbased = DiffIndex::from_unified_diff(diff);
        let missed = unbased
            .key_for(Path::new("/repo/pkg/src/a.ts"), Path::new("/repo/pkg"))
            .expect("still relativizable");
        assert_eq!(missed, "src/a.ts");
        assert!(!unbased.touches_file(&missed));
    }

    #[test]
    fn key_for_returns_none_for_path_outside_the_base() {
        let index = DiffIndex::default().with_base("/repo");
        assert!(
            index
                .key_for(Path::new("/elsewhere/a.ts"), Path::new("/repo/pkg"))
                .is_none()
        );
    }

    #[test]
    fn old_path_for_root_relative_crosses_the_namespace_and_back() {
        let diff = "\
diff --git a/pkg/src/old.ts b/pkg/src/new.ts
similarity index 90%
rename from pkg/src/old.ts
rename to pkg/src/new.ts
--- a/pkg/src/old.ts
+++ b/pkg/src/new.ts
@@ -1,1 +1,1 @@
-old
+new
";
        let index = DiffIndex::from_unified_diff(diff)
            .with_base("/repo")
            .with_root_offset("pkg");

        // The finding is addressed `src/new.ts`; the diff says `pkg/src/new.ts`.
        assert_eq!(
            index.old_path_for_root_relative("src/new.ts").as_deref(),
            Some("src/old.ts")
        );
        // The raw lookup, in the diff's own namespace, still works.
        assert_eq!(index.old_path_for("pkg/src/new.ts"), Some("pkg/src/old.ts"));
        assert_eq!(index.old_path_for_root_relative("src/absent.ts"), None);
    }

    #[test]
    fn root_relative_key_round_trips() {
        let index = DiffIndex::default().with_root_offset("packages/pkg");
        assert_eq!(
            index.key_for_root_relative("src/a.ts"),
            "packages/pkg/src/a.ts"
        );
        assert_eq!(
            index
                .root_relative_from_key("packages/pkg/src/a.ts")
                .as_deref(),
            Some("src/a.ts")
        );
        // A key outside the analysis root has no root-relative form.
        assert_eq!(index.root_relative_from_key("other/src/a.ts"), None);
        // Sibling directory sharing a name prefix is not a match.
        assert_eq!(
            index.root_relative_from_key("packages/pkg-extra/a.ts"),
            None
        );
    }

    #[test]
    fn empty_root_offset_is_identity() {
        let index = DiffIndex::default();
        assert_eq!(index.key_for_root_relative("src/a.ts"), "src/a.ts");
        assert_eq!(
            index.root_relative_from_key("src/a.ts").as_deref(),
            Some("src/a.ts")
        );
    }

    #[test]
    fn touched_files_enumerates_diff_header_paths() {
        let diff = "\
diff --git a/pkg/a.ts b/pkg/a.ts
--- a/pkg/a.ts
+++ b/pkg/a.ts
@@ -0,0 +1,1 @@
+x
";
        let index = DiffIndex::from_unified_diff(diff);
        assert_eq!(index.touched_files().collect::<Vec<_>>(), vec!["pkg/a.ts"]);
    }
}
