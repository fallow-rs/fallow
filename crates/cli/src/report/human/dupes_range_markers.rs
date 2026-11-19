//! Human lines for `fallow-ignore-start` / `fallow-ignore-end` markers with
//! a problem.

use std::path::Path;

use colored::Colorize;
use fallow_types::duplicates::RangeMarkerFinding;

use super::format_path;

/// One heading plus one line per marker with a problem. Empty when no
/// marker has a problem.
pub(super) fn range_marker_problem_lines(
    problems: &[RangeMarkerFinding],
    root: &Path,
) -> Vec<String> {
    if problems.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![format!(
        "{} {}",
        "\u{25cf}".yellow(),
        format!(
            "Range suppression markers with a problem ({})",
            problems.len()
        )
        .yellow()
        .bold()
    )];
    for finding in problems {
        let path = finding.path.strip_prefix(root).unwrap_or(&finding.path);
        let location = format!(
            "{}:{}",
            path.to_string_lossy().replace('\\', "/"),
            finding.problem.line
        );
        let token = finding
            .problem
            .token
            .as_deref()
            .map_or_else(String::new, |token| format!(" (`{token}`)"));
        lines.push(format!(
            "  {} {}{}",
            format_path(&location),
            finding.problem.kind.description().dimmed(),
            token.dimmed(),
        ));
    }
    lines.push(String::new());
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::suppress::{RangeMarkerProblem, RangeMarkerProblemKind};
    use std::path::PathBuf;

    #[test]
    fn lists_each_marker_with_its_problem() {
        let problems = vec![RangeMarkerFinding {
            path: PathBuf::from("/repo/src/a.ts"),
            problem: RangeMarkerProblem {
                line: 3,
                kind: RangeMarkerProblemKind::UnsupportedKind,
                token: Some("unused-export".to_string()),
            },
        }];
        let lines: Vec<String> = range_marker_problem_lines(&problems, Path::new("/repo"))
            .iter()
            .map(|line| super::super::strip_ansi(line))
            .collect();
        assert_eq!(
            lines[0],
            "\u{25cf} Range suppression markers with a problem (1)"
        );
        assert_eq!(
            lines[1],
            "  src/a.ts:3 range markers support only code-duplication; this kind is not suppressed (`unused-export`)"
        );
        assert!(range_marker_problem_lines(&[], Path::new("/repo")).is_empty());
    }
}
