//! The brief-local security scan of `fallow audit --brief` and
//! `fallow review`. It reads the base and the head text of each changed file
//! and gives `security-check-removed` signals: removed security steps in CI
//! files, and security sink substitution in non-test source files.

use std::path::{Path, PathBuf};

use fallow_api::audit_run::{BaseFileReader, BaseRead};
use rustc_hash::FxHashSet;

use super::weakening;
use crate::base_worktree::git_toplevel;

/// What the security scan looks for in one changed file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SecurityScan {
    /// A CI workflow file: removed security steps.
    CiSteps,
    /// A non-test source file: security sinks swapped for another sink of the
    /// same class, or moved out of the static sink view.
    SinkSubstitution,
}

/// Pick the security scan of one changed file and the path that its signals
/// carry: repository-relative for CI files, root-relative for source files.
fn security_scan_for(root: &Path, git_root: &Path, abs: &Path) -> Option<(SecurityScan, String)> {
    let git_relative = abs.strip_prefix(git_root).ok()?;
    let git_rel = git_relative.to_string_lossy().replace('\\', "/");
    if weakening::is_ci_file(&git_rel) {
        return Some((SecurityScan::CiSteps, git_rel));
    }
    let root_rel = abs
        .strip_prefix(root)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    (fallow_engine::sink_substitution::is_sink_source_path(&root_rel)
        && !fallow_engine::test_paths::is_test_code_path_str(root, &root_rel))
    .then_some((SecurityScan::SinkSubstitution, root_rel))
}

/// Scan the changed CI files for removed security steps, and the changed
/// non-test source files for security sink substitution: read each file's
/// base content via [`BaseFileReader`] and compare it with the head content
/// on disk. Best-effort, but a read failure never reads as empty content: a
/// file deleted at head or absent at base scans against `""`, an unreadable
/// head file is skipped, and a base-reader error stops the scan for the
/// remaining files (the batch pipe is no longer trustworthy).
pub(super) fn compute_security_step_signals(
    root: &Path,
    base_ref: &str,
    changed_files: &FxHashSet<PathBuf>,
) -> Vec<weakening::WeakeningSignal> {
    let Some(git_root) = git_toplevel(root) else {
        return Vec::new();
    };
    let mut files: Vec<(SecurityScan, String, &PathBuf)> = changed_files
        .iter()
        .filter_map(|abs| {
            let (scan, rel_str) = security_scan_for(root, &git_root, abs)?;
            Some((scan, rel_str, abs))
        })
        .collect();
    if files.is_empty() {
        return Vec::new();
    }
    files.sort();
    let Some(mut reader) = BaseFileReader::spawn(root) else {
        return Vec::new();
    };

    let mut signals = Vec::new();
    for (scan, rel_str, abs) in files {
        let Ok(relative) = abs.strip_prefix(&git_root) else {
            continue;
        };
        let head = match std::fs::read(abs) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(_) => continue,
        };
        let base = match reader.read(base_ref, relative) {
            BaseRead::Content(base) => base,
            BaseRead::Missing => String::new(),
            BaseRead::Error => break,
        };
        let evidence = match scan {
            SecurityScan::CiSteps => weakening::detect_removed_security_steps(&base, &head),
            SecurityScan::SinkSubstitution => {
                fallow_engine::sink_substitution::detect_sink_substitutions(abs, &base, &head)
            }
        };
        signals.extend(
            evidence
                .into_iter()
                .map(|evidence| weakening::WeakeningSignal {
                    kind: weakening::WeakeningKind::SecurityCheckRemoved,
                    file: rel_str.clone(),
                    evidence,
                }),
        );
    }
    signals
}
