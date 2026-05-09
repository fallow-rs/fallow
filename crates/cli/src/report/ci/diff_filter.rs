use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use super::pr_comment::CiIssue;

/// Refuse to parse a unified diff larger than this. The cap matches the
/// SARIF upload limit (10 MiB) and is also far above what any sane PR
/// produces. A pathologically large diff (binary blob, vendored dump) would
/// otherwise eat memory proportional to its size before we can inspect it.
const MAX_DIFF_BYTES: u64 = 10 * 1024 * 1024;

/// Stop indexing added lines past this count. A 1M-line "diff" is a sign of
/// a regenerated lockfile or vendored bundle and is not useful for filtering;
/// emit a warning and proceed with whatever we already indexed.
const MAX_ADDED_LINES: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiffFilterMode {
    Added,
    DiffContext,
    File,
    NoFilter,
}

impl DiffFilterMode {
    #[must_use]
    pub fn from_env() -> Self {
        match std::env::var("FALLOW_DIFF_FILTER")
            .unwrap_or_else(|_| "added".into())
            .as_str()
        {
            "diff_context" | "context" => Self::DiffContext,
            "file" => Self::File,
            "nofilter" | "none" => Self::NoFilter,
            _ => Self::Added,
        }
    }
}

#[derive(Debug, Default)]
pub struct DiffIndex {
    added_lines: FxHashMap<String, FxHashSet<u64>>,
    touched_files: FxHashSet<String>,
}

impl DiffIndex {
    #[must_use]
    pub fn from_unified_diff(diff: &str) -> Self {
        let mut index = Self::default();
        let mut current_file: Option<String> = None;
        let mut new_line = 0_u64;
        let mut added_count: usize = 0;
        let mut warned_overflow = false;

        for line in diff.lines() {
            if let Some(path) = line.strip_prefix("+++ b/") {
                current_file = Some(path.to_string());
                index.touched_files.insert(path.to_string());
                continue;
            }
            if line.starts_with("+++ /dev/null") {
                current_file = None;
                continue;
            }
            if let Some(header) = line.strip_prefix("@@ ") {
                if let Some(start) = parse_new_hunk_start(header) {
                    new_line = start;
                }
                continue;
            }
            let Some(path) = current_file.as_ref() else {
                continue;
            };
            if line.starts_with('+') && !line.starts_with("+++") {
                if added_count < MAX_ADDED_LINES {
                    index
                        .added_lines
                        .entry(path.clone())
                        .or_default()
                        .insert(new_line);
                    added_count += 1;
                } else if !warned_overflow {
                    eprintln!(
                        "fallow: diff exceeds {MAX_ADDED_LINES} added lines; \
                         indexed prefix only, later additions skipped"
                    );
                    warned_overflow = true;
                }
                new_line += 1;
            } else if !line.starts_with('-') {
                new_line += 1;
            }
        }

        index
    }

    #[cfg(test)]
    #[must_use]
    pub fn keeps(&self, issue: &CiIssue, mode: DiffFilterMode) -> bool {
        self.keeps_with_context(issue, mode, context_radius_from_env())
    }

    #[must_use]
    pub fn keeps_with_context(&self, issue: &CiIssue, mode: DiffFilterMode, radius: u64) -> bool {
        match mode {
            DiffFilterMode::NoFilter => true,
            DiffFilterMode::File => self.touched_files.contains(&issue.path),
            DiffFilterMode::DiffContext => self.added_lines.get(&issue.path).is_some_and(|lines| {
                lines
                    .iter()
                    .any(|line| issue.line.abs_diff(*line) <= radius)
            }),
            DiffFilterMode::Added => self
                .added_lines
                .get(&issue.path)
                .is_some_and(|lines| lines.contains(&issue.line)),
        }
    }

    /// Added-line numbers for `path` (repo-root-relative, forward-slashed),
    /// or `None` when the file does not appear in the diff. Used by the
    /// runtime-coverage filter to do line-overlap matching against hot-path
    /// `[start_line, end_line]` ranges, so a PR touching the body of a hot
    /// function flips the verdict to `hot-path-touched` while edits to
    /// other functions in the same file do not.
    #[must_use]
    pub fn added_lines_in(&self, path: &str) -> Option<&FxHashSet<u64>> {
        self.added_lines.get(path)
    }
}

fn context_radius_from_env() -> u64 {
    std::env::var("FALLOW_DIFF_CONTEXT")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(3)
}

fn parse_new_hunk_start(header: &str) -> Option<u64> {
    let plus = header.find('+')?;
    let rest = &header[plus + 1..];
    let end = rest
        .find(|c: char| c == ',' || c.is_ascii_whitespace())
        .unwrap_or(rest.len());
    rest[..end].parse().ok()
}

#[must_use]
pub fn filter_issues_from_env(issues: Vec<CiIssue>) -> Vec<CiIssue> {
    let Some(raw_path) = std::env::var_os("FALLOW_DIFF_FILE") else {
        return issues;
    };
    filter_issues_from_path(
        issues,
        Path::new(&raw_path),
        DiffFilterMode::from_env(),
        context_radius_from_env(),
    )
}

#[must_use]
pub fn filter_issues_from_path(
    issues: Vec<CiIssue>,
    path: &Path,
    mode: DiffFilterMode,
    radius: u64,
) -> Vec<CiIssue> {
    // Reject diffs above the size cap before reading them into memory. A
    // pathological diff (vendored dump, binary blob mistakenly committed)
    // would otherwise allocate proportional memory before we can filter.
    match std::fs::metadata(path) {
        Ok(meta) if meta.len() > MAX_DIFF_BYTES => {
            eprintln!(
                "fallow: FALLOW_DIFF_FILE '{}' is {} bytes (cap {MAX_DIFF_BYTES}); \
                 skipping diff filter, reporting all findings",
                path.display(),
                meta.len()
            );
            return issues;
        }
        Ok(_) => {}
        Err(err) => {
            eprintln!(
                "fallow: FALLOW_DIFF_FILE '{}' could not be stat'd ({err}); \
                 skipping diff filter, reporting all findings",
                path.display()
            );
            return issues;
        }
    }

    let Ok(diff) = std::fs::read_to_string(path) else {
        eprintln!(
            "fallow: FALLOW_DIFF_FILE '{}' could not be read; \
             skipping diff filter, reporting all findings",
            path.display()
        );
        return issues;
    };
    let index = DiffIndex::from_unified_diff(&diff);
    issues
        .into_iter()
        .filter(|issue| index.keeps_with_context(issue, mode, radius))
        .collect()
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    #[test]
    fn from_unified_diff_caps_added_lines_at_threshold() {
        // Synthesize a diff with MAX_ADDED_LINES + 100 added lines and verify
        // we stop indexing past the cap. The exact split: index size <= cap.
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
        let total: usize = index.added_lines.values().map(FxHashSet::len).sum();
        assert!(
            total <= MAX_ADDED_LINES,
            "indexed {total} lines, cap is {MAX_ADDED_LINES}"
        );
    }

    #[test]
    fn filter_issues_from_path_skips_oversize_diff() {
        // Write a diff just over the byte cap and verify the cap-check
        // short-circuits, returning issues unfiltered with a warning.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("oversize.diff");
        let mut file = std::fs::File::create(&path).expect("create");
        let chunk = "+ filler line\n";
        let bytes_per_chunk = chunk.len() as u64;
        let chunks_needed = (MAX_DIFF_BYTES / bytes_per_chunk) + 100_000;
        for _ in 0..chunks_needed {
            file.write_all(chunk.as_bytes()).expect("write");
        }
        drop(file);

        let issue = CiIssue {
            rule_id: "r".into(),
            description: "d".into(),
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            fingerprint: "abc".into(),
        };
        let kept = filter_issues_from_path(vec![issue], &path, DiffFilterMode::Added, 3);
        assert_eq!(kept.len(), 1, "oversize diff must fall through unfiltered");
    }

    #[test]
    fn filter_issues_from_path_handles_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("does-not-exist.diff");
        let issue = CiIssue {
            rule_id: "r".into(),
            description: "d".into(),
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            fingerprint: "abc".into(),
        };
        let kept = filter_issues_from_path(vec![issue], &path, DiffFilterMode::Added, 3);
        assert_eq!(kept.len(), 1, "missing diff must fall through unfiltered");
    }

    #[test]
    fn added_mode_keeps_only_added_lines() {
        let diff = "\
diff --git a/src/a.ts b/src/a.ts
--- a/src/a.ts
+++ b/src/a.ts
@@ -1,2 +1,3 @@
 old
+new
 ctx
";
        let index = DiffIndex::from_unified_diff(diff);
        let keep = CiIssue {
            rule_id: "r".into(),
            description: "d".into(),
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 2,
            fingerprint: "a".into(),
        };
        let drop = CiIssue {
            line: 3,
            ..keep.clone()
        };
        assert!(index.keeps(&keep, DiffFilterMode::Added));
        assert!(!index.keeps(&drop, DiffFilterMode::Added));
        assert!(index.keeps(&drop, DiffFilterMode::DiffContext));
        assert!(index.keeps(&drop, DiffFilterMode::File));
    }
}
