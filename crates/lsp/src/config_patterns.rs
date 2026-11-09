//! Config entries that had no effect, as diagnostics on the config file: the
//! patterns that matched nothing (`ignoreFindings`, `ignoreDependencies`), the
//! redundant entries (`entry`, `ignoreDependencies`, `ignorePatterns`) and the
//! `overrides` rules that cannot change a result.
//!
//! The CLI reports these patterns in `workspace_diagnostics[]`. The editor
//! marks the entry in the config file that declares it. A pattern whose entry
//! the server cannot find (for example one from a remote `extends` config)
//! goes to the output log once per changed set.

use std::path::{Path, PathBuf};

use fallow_config::{ConfigEntrySpan, FallowConfig, WorkspaceDiagnostic};
use ls_types::{
    Diagnostic, DiagnosticSeverity, DiagnosticTag, NumberOrString, Position, Range, Uri,
};
use rustc_hash::FxHashMap;

use crate::position::byte_order_mark_len;

/// One config entry that had no effect in the latest run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnmatchedConfigPattern {
    /// The `workspace_diagnostics[]` kind, used as the diagnostic code.
    pub code: &'static str,
    /// The same text as the `message` of the CLI entry.
    pub message: String,
    /// The project root of the analysis that reported the pattern.
    pub project_root: PathBuf,
    /// The entry in the config file, or `None` when the server cannot find it.
    pub location: Option<PatternLocation>,
}

/// The place of a pattern in the config file that declares it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternLocation {
    pub path: PathBuf,
    pub range: Range,
}

/// Build the patterns of one project root from its unmatched-pattern entries.
/// `config_path` is the config file that the session loaded.
pub fn collect(
    project_root: &Path,
    config_path: Option<&Path>,
    diagnostics: Vec<WorkspaceDiagnostic>,
) -> Vec<UnmatchedConfigPattern> {
    diagnostics
        .into_iter()
        .filter_map(|diagnostic| {
            let span = if let Some((setting, pattern)) = diagnostic.kind.unmatched_config_pattern()
            {
                config_path.and_then(|path| FallowConfig::locate_list_entry(path, setting, pattern))
            } else {
                let (rule, files, _) = diagnostic.kind.ineffective_override()?;
                config_path.and_then(|path| FallowConfig::locate_override_rule(path, rule, files))
            };
            let location = span.and_then(pattern_location);
            Some(UnmatchedConfigPattern {
                code: diagnostic.kind.id(),
                message: diagnostic.message,
                project_root: project_root.to_path_buf(),
                location,
            })
        })
        .collect()
}

/// A config span counts its byte offsets in the file on disk, with the byte
/// order mark. The editor buffer does not hold the mark, so the position
/// counts from the end of the mark.
fn pattern_location(span: ConfigEntrySpan) -> Option<PatternLocation> {
    let content = std::fs::read_to_string(&span.path).ok()?;
    let mark_len = byte_order_mark_len(&content);
    let text = &content[mark_len..];
    Some(PatternLocation {
        range: Range {
            start: position_at(text, span.start.saturating_sub(mark_len)),
            end: position_at(text, span.end.saturating_sub(mark_len)),
        },
        path: span.path,
    })
}

/// The LSP position (0-based line, UTF-16 column) of a byte offset.
fn position_at(content: &str, offset: usize) -> Position {
    let mut offset = offset.min(content.len());
    while offset > 0 && !content.is_char_boundary(offset) {
        offset -= 1;
    }
    let before = &content[..offset];
    let line_start = before.rfind('\n').map_or(0, |index| index + 1);
    let line = before.bytes().filter(|&byte| byte == b'\n').count();
    let character = before[line_start..].encode_utf16().count();
    Position {
        line: u32::try_from(line).unwrap_or(u32::MAX),
        character: u32::try_from(character).unwrap_or(u32::MAX),
    }
}

/// Add one diagnostic per located pattern to `map`, on the config file that
/// declares the pattern. The severity is information: the pattern is
/// harmless, but it has no effect. The `unnecessary` tag fades the entry.
pub fn push_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    patterns: &[UnmatchedConfigPattern],
) {
    for pattern in patterns {
        let Some(location) = &pattern.location else {
            continue;
        };
        let Some(uri) = Uri::from_file_path(&location.path) else {
            continue;
        };
        let diagnostic = Diagnostic {
            range: location.range,
            severity: Some(DiagnosticSeverity::INFORMATION),
            source: Some("fallow".to_string()),
            code: Some(NumberOrString::String(pattern.code.to_string())),
            message: pattern.message.clone(),
            tags: Some(vec![DiagnosticTag::UNNECESSARY]),
            ..Default::default()
        };
        let diagnostics = map.entry(uri).or_default();
        if !diagnostics.contains(&diagnostic) {
            diagnostics.push(diagnostic);
        }
    }
}

/// The log lines for the patterns without a location, one per pattern.
pub fn unlocated_log_lines(patterns: &[UnmatchedConfigPattern]) -> Vec<String> {
    patterns
        .iter()
        .filter(|pattern| pattern.location.is_none())
        .map(|pattern| format!("{}: {}", pattern.project_root.display(), pattern.message))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_counts_utf16_units_on_the_line() {
        let content = "{\n  \"a\": \"\u{1F389}\", \"b\": [\"x\"]\n}";
        let offset = content.find("[\"x\"]").expect("list") + 1;

        let position = position_at(content, offset);

        assert_eq!(position.line, 1);
        let line = "  \"a\": \"\u{1F389}\", \"b\": [";
        assert_eq!(position.character as usize, line.encode_utf16().count());
    }

    #[test]
    fn byte_order_mark_is_not_a_column_of_the_first_line() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(".fallowrc.json");
        let content = "\u{feff}{\"ignoreFindings\": [\"a/**\"]}";
        std::fs::write(&path, content).expect("write config");
        let start = content.find("\"a/**\"").expect("entry");
        let span = ConfigEntrySpan {
            path,
            start,
            end: start + "\"a/**\"".len(),
        };

        let location = pattern_location(span).expect("location");

        assert_eq!(location.range.start, Position::new(0, 20));
        assert_eq!(location.range.end, Position::new(0, 26));
    }

    #[test]
    fn a_pattern_without_a_location_is_logged_and_not_published() {
        let pattern = UnmatchedConfigPattern {
            code: "ignore-findings-pattern-unmatched",
            message: "ignoreFindings pattern 'x' matched no finding".to_string(),
            project_root: PathBuf::from("/project"),
            location: None,
        };
        let mut map = FxHashMap::default();

        push_diagnostics(&mut map, std::slice::from_ref(&pattern));

        assert!(map.is_empty());
        assert_eq!(unlocated_log_lines(&[pattern]).len(), 1);
    }
}
