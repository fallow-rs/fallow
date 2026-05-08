use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};

use super::pr_comment::CiIssue;

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
                index
                    .added_lines
                    .entry(path.clone())
                    .or_default()
                    .insert(new_line);
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
    let Some(path) = std::env::var_os("FALLOW_DIFF_FILE") else {
        return issues;
    };
    let Ok(diff) = std::fs::read_to_string(Path::new(&path)) else {
        return issues;
    };
    let mode = DiffFilterMode::from_env();
    let radius = context_radius_from_env();
    let index = DiffIndex::from_unified_diff(&diff);
    issues
        .into_iter()
        .filter(|issue| index.keeps_with_context(issue, mode, radius))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

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
