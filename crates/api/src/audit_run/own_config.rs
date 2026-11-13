//! The evidence of a base pass under the config of the base commit.

use std::path::{Path, PathBuf};

use fallow_config::{ResolvedConfig, Severity};
use fallow_types::results::AnalysisResults;
use rustc_hash::FxHashSet;

use crate::audit_keys::collection_keys;

/// The path of the config file `config_path`, relative to `root`.
///
/// The function canonicalizes both paths. Thus a relative `-c` path, or a
/// path through a symlink such as the macOS `/tmp`, also resolves. `None` when
/// the config file does not exist or is not below `root`.
#[must_use]
pub fn config_path_under_root(config_path: &Path, root: &Path) -> Option<PathBuf> {
    let config_path = dunce::canonicalize(config_path).ok()?;
    let root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    config_path.strip_prefix(&root).ok().map(Path::to_path_buf)
}

/// The keys for [`super::AuditKeySnapshot::own_config_unused_code`].
///
/// `own` holds the results of the base pass under the config of the base
/// commit, and `own_config` is that config. `candidates` holds the results of
/// the base pass under the head config. The set has the unused files and
/// unused exports that `own` reports. It also has each candidate that
/// `own_config` keeps out of its report (an `ignorePatterns`,
/// `ignoreFindings` or `ignoreExports` entry, or an override that turns the
/// rule off), and each candidate export in a file that `own` reports as an
/// unused file. The base config cannot prove that such code was in use, so a
/// config change that only shows old unused code does not make it introduced.
#[must_use]
pub fn own_config_unused_code_keys(
    own: &AnalysisResults,
    own_config: &ResolvedConfig,
    candidates: &AnalysisResults,
    root: &Path,
) -> FxHashSet<String> {
    let mut keys: FxHashSet<String> = collection_keys(&own.unused_files, root)
        .into_iter()
        .chain(collection_keys(&own.unused_exports, root))
        .collect();
    let files = collection_keys(&candidates.unused_files, root);
    for (finding, key) in candidates.unused_files.iter().zip(files) {
        if hides_unused_file(own_config, relative(&finding.file.path, root)) {
            keys.insert(key);
        }
    }
    let unused_files: FxHashSet<&Path> = own
        .unused_files
        .iter()
        .map(|finding| relative(&finding.file.path, root))
        .collect();
    let exports = collection_keys(&candidates.unused_exports, root);
    for (finding, key) in candidates.unused_exports.iter().zip(exports) {
        let export = &finding.export;
        let path = relative(&export.path, root);
        if unused_files.contains(path) || hides_unused_export(own_config, path, &export.export_name)
        {
            keys.insert(key);
        }
    }
    keys
}

fn relative<'a>(path: &'a Path, root: &Path) -> &'a Path {
    path.strip_prefix(root).unwrap_or(path)
}

/// Whether `config` keeps every finding in the file at `relative` out of its
/// report.
fn hides_path(config: &ResolvedConfig, relative: &Path) -> bool {
    config.ignore_patterns.is_match(relative)
        || config
            .ignore_findings
            .is_ignored_path(&config.root, relative)
}

fn hides_unused_file(config: &ResolvedConfig, relative: &Path) -> bool {
    hides_path(config, relative)
        || config.resolve_rules_for_path(relative).unused_files == Severity::Off
}

fn hides_unused_export(config: &ResolvedConfig, relative: &Path, name: &str) -> bool {
    if hides_path(config, relative)
        || config.resolve_rules_for_path(relative).unused_exports == Severity::Off
    {
        return true;
    }
    let file = relative.to_string_lossy();
    config.compiled_ignore_exports.iter().any(|rule| {
        rule.matcher.is_match(file.as_ref())
            && rule
                .exports
                .iter()
                .any(|export| export == "*" || export == name)
    })
}
