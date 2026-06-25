use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

/// Refuse to parse a unified diff larger than this.
pub const MAX_DIFF_BYTES: u64 = 10 * 1024 * 1024;

/// Stop indexing added lines past this count.
pub const MAX_ADDED_LINES: usize = 1_000_000;

/// Parsed, command-neutral index of files and added lines in a unified diff.
#[derive(Debug, Default, Clone)]
pub struct DiffIndex {
    added_lines: FxHashMap<String, FxHashSet<u64>>,
    touched_files: FxHashSet<String>,
    added_line_count: usize,
    rename_pairs: FxHashMap<String, String>,
}

/// Mutable cursor state threaded through unified-diff parsing.
#[derive(Default)]
struct DiffParseState {
    current_file: Option<String>,
    new_line: u64,
    pending_rename_from: Option<String>,
}

impl DiffIndex {
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
                self.touched_files.insert(rest.to_owned());
            }
            return true;
        }
        if let Some(path) = line.strip_prefix("+++ b/") {
            state.current_file = Some(path.to_string());
            self.touched_files.insert(path.to_string());
            return true;
        }
        if line.starts_with("+++ /dev/null") {
            state.current_file = None;
            return true;
        }
        if let Some(header) = line.strip_prefix("@@ ") {
            if let Some(start) = parse_new_hunk_start(header) {
                state.new_line = start;
            }
            return true;
        }
        false
    }

    fn handle_diff_content_line(&mut self, line: &str, state: &mut DiffParseState) {
        let Some(path) = state.current_file.as_ref() else {
            return;
        };
        if line.starts_with('+') && !line.starts_with("+++") {
            if self.added_line_count < MAX_ADDED_LINES {
                self.added_lines
                    .entry(path.clone())
                    .or_default()
                    .insert(state.new_line);
                self.added_line_count += 1;
            }
            state.new_line += 1;
        } else if !line.starts_with('-') {
            state.new_line += 1;
        }
    }

    #[must_use]
    pub fn old_path_for(&self, head_path: &str) -> Option<&str> {
        self.rename_pairs.get(head_path).map(String::as_str)
    }

    #[must_use]
    pub fn added_line_count(&self) -> usize {
        self.added_line_count
    }

    #[must_use]
    pub fn touches_file(&self, path: &str) -> bool {
        self.touched_files.contains(path)
    }

    #[must_use]
    pub fn range_overlaps_added(&self, path: &str, start: u64, end: u64) -> bool {
        if end < start {
            return false;
        }
        let Some(added) = self.added_lines.get(path) else {
            return false;
        };
        let lo = start.max(1);
        added.iter().any(|&line| line >= lo && line <= end)
    }

    #[must_use]
    pub fn line_is_added(&self, path: &str, line: u64) -> bool {
        self.added_lines
            .get(path)
            .is_some_and(|lines| lines.contains(&line))
    }

    #[must_use]
    pub fn line_within_added_context(&self, path: &str, line: u64, radius: u64) -> bool {
        self.added_lines
            .get(path)
            .is_some_and(|lines| lines.iter().any(|added| line.abs_diff(*added) <= radius))
    }

    #[must_use]
    pub fn added_lines_in(&self, path: &str) -> Option<&FxHashSet<u64>> {
        self.added_lines.get(path)
    }
}

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
    fn delete_only_diff_records_no_added_lines() {
        let diff = "\
diff --git a/src/a.ts b/src/a.ts
--- a/src/a.ts
+++ /dev/null
@@ -1,1 +0,0 @@
-old
";
        let index = DiffIndex::from_unified_diff(diff);
        assert_eq!(index.added_line_count(), 0);
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
}
