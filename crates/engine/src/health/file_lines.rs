//! File-length findings (`health.maxFileLines`).
//!
//! A file with more physical lines than its ceiling becomes a complexity
//! finding with `exceeded: "file_lines"`. The finding reuses the complexity
//! finding shape, so the `complexity:<path>:<file>` identity, the audit
//! attribution, the baseline, and every output format treat it like a
//! function finding. The ceiling is off (`0`) by default.

use fallow_output::{ComplexityViolation, ExceededThreshold, FindingSeverity};
use fallow_types::discover::{DiscoveredFile, FileId};

use super::threshold_overrides::{
    FileLinesContext, ThresholdOverrideResolver, ThresholdOverrideStateTracker,
};

pub(super) struct FileLinesInput<'a> {
    pub(super) files: &'a [DiscoveredFile],
    pub(super) modules: &'a [crate::source::ModuleInfo],
    pub(super) file_paths: &'a rustc_hash::FxHashMap<FileId, &'a std::path::PathBuf>,
    pub(super) config_root: &'a std::path::Path,
    pub(super) ignore_set: &'a globset::GlobSet,
    pub(super) changed_files: Option<&'a rustc_hash::FxHashSet<std::path::PathBuf>>,
    pub(super) ws_roots: Option<&'a [std::path::PathBuf]>,
    pub(super) threshold_resolver: &'a ThresholdOverrideResolver,
}

/// Append a file-length finding for each file over its ceiling.
///
/// The filters match the function findings: `health.ignore`, the changed-file
/// scope, and the workspace scope. A `fallow-ignore-file complexity` comment
/// suppresses the finding.
pub(super) fn append_file_lines_findings(
    input: &FileLinesInput<'_>,
    tracker: &mut ThresholdOverrideStateTracker,
    findings: &mut Vec<ComplexityViolation>,
) {
    let resolver = input.threshold_resolver;
    if resolver.max_file_lines() == 0 && !resolver.has_file_lines_overrides() {
        return;
    }
    for module in input.modules {
        let Some(&path) = input.file_paths.get(&module.file_id) else {
            continue;
        };
        let relative = path.strip_prefix(input.config_root).unwrap_or(path);
        if input.ignore_set.is_match(relative)
            || input
                .changed_files
                .is_some_and(|changed| !changed.contains(path))
            || input
                .ws_roots
                .is_some_and(|roots| !roots.iter().any(|root| path.starts_with(root)))
        {
            continue;
        }
        let (max_file_lines, matches) = resolver.resolve_max_file_lines(relative);
        let line_count = physical_line_count(module, size_of(input.files, module.file_id), || {
            file_starts_with_bom(path)
        });
        let suppressed = crate::suppress::is_file_suppressed(
            &module.suppressions,
            crate::suppress::IssueKind::Complexity,
        );
        if !matches.is_empty() {
            let (applied, _) = resolver.resolve(relative, fallow_output::FILE_LINES_FINDING_NAME);
            tracker.record_file_lines(
                FileLinesContext {
                    path,
                    line_count,
                    suppressed,
                },
                &matches,
                resolver.max_file_lines(),
                max_file_lines,
                applied.effective,
            );
        }
        if suppressed || max_file_lines == 0 || line_count <= max_file_lines {
            continue;
        }
        findings.push(ComplexityViolation {
            path: path.clone(),
            name: fallow_output::FILE_LINES_FINDING_NAME.to_string(),
            line: 1,
            col: 0,
            line_count,
            exceeded: ExceededThreshold::FileLines,
            severity: FindingSeverity::Moderate,
            threshold_source: (!matches.is_empty())
                .then_some(fallow_output::ThresholdSource::Override),
            max_file_lines: Some(max_file_lines),
            ..ComplexityViolation::default()
        });
    }
}

fn file_starts_with_bom(path: &std::path::Path) -> bool {
    use std::io::Read as _;
    let mut head = [0_u8; 3];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok_and(|()| head == [0xEF, 0xBB, 0xBF])
}

fn size_of(files: &[DiscoveredFile], file_id: FileId) -> Option<u64> {
    files
        .get(file_id.0 as usize)
        .filter(|file| file.id == file_id)
        .or_else(|| files.iter().find(|file| file.id == file_id))
        .map(|file| file.size_bytes)
}

/// Length in bytes of the UTF-8 byte order mark.
const UTF8_BOM_LEN: u64 = 3;

/// The physical line count of a module, as an editor shows it without the
/// empty line after a final newline.
///
/// `line_offsets` holds one entry per line start, so a final newline adds an
/// entry for an empty last line. That entry is dropped when its offset is the
/// end of the file. Every other line counts, also blank lines and comments.
///
/// The offsets come from the source after the extractor removed a BOM, while
/// `size_bytes` is the size on disk. `has_bom` reads the file only when the
/// last offset is exactly a BOM length short of the size, so the common case
/// does no extra I/O.
fn physical_line_count(
    module: &crate::source::ModuleInfo,
    size_bytes: Option<u64>,
    has_bom: impl FnOnce() -> bool,
) -> u32 {
    let starts = u32::try_from(module.line_offsets.len()).unwrap_or(u32::MAX);
    let ends_with_newline = starts > 1
        && module
            .line_offsets
            .last()
            .zip(size_bytes)
            .is_some_and(|(&last, size)| {
                let last = u64::from(last);
                last == size || (last + UTF8_BOM_LEN == size && has_bom())
            });
    if ends_with_newline {
        starts - 1
    } else {
        starts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn module_with(source: &str) -> crate::source::ModuleInfo {
        crate::source::ModuleInfo {
            line_offsets: fallow_types::extract::compute_line_offsets(source),
            ..crate::source::ModuleInfo::empty(FileId(0))
        }
    }

    fn count(source: &str) -> u32 {
        physical_line_count(&module_with(source), Some(source.len() as u64), || false)
    }

    #[test]
    fn final_newline_adds_no_line() {
        assert_eq!(count("a\nb\n"), 2);
        assert_eq!(count("a\nb"), 2);
        assert_eq!(count("a\n\n// comment\n\nb\n"), 5);
        assert_eq!(count("\n"), 1);
        assert_eq!(count(""), 1);
    }

    #[test]
    fn unknown_size_counts_every_line_start() {
        assert_eq!(
            physical_line_count(&module_with("a\nb\n"), None, || false),
            3
        );
    }

    #[test]
    fn bom_does_not_add_a_line() {
        let source = "a\nb\n";
        let on_disk = source.len() as u64 + UTF8_BOM_LEN;
        assert_eq!(
            physical_line_count(&module_with(source), Some(on_disk), || true),
            2
        );
        // A last line of three bytes without a final newline is not a BOM.
        let source = "a\nfoo";
        assert_eq!(
            physical_line_count(&module_with(source), Some(source.len() as u64), || false),
            2
        );
        assert_eq!(
            physical_line_count(&module_with("a\n"), Some(5), || false),
            2,
            "without a BOM, a size three bytes past the last offset is a last line"
        );
    }
}
