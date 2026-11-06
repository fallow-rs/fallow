//! The shared positional `PATH` scope resolver.
//!
//! `check`, `dupes`, `health`, `audit`, `security`, `fix`, `list`,
//! `similar-code`, and bare combined mode all accept an optional positional
//! `[PATH]`. The scope narrows reported findings to that file or directory;
//! the full project graph is still built first, so cross-file facts (unused
//! exports, reachability, clone families) stay sound. This module owns the
//! single resolution answer so a path accepted by one command behaves the
//! same on every other.

use std::path::{Path, PathBuf};

/// A resolved positional scope: a path guaranteed inside the root.
///
/// `absolute` keeps the root-joined spelling (simplified, not fully
/// canonicalized) so prefix matching agrees with finding paths.
#[derive(Debug, Clone)]
pub struct ScopePath {
    /// Root-joined absolute path of the scope target.
    pub absolute: PathBuf,
    /// True when the target is a directory (prefix scope), false for a file.
    pub is_dir: bool,
}

impl ScopePath {
    /// True when the scope is the project root itself, which covers
    /// everything and must behave exactly like no scope at all (in
    /// particular it must not clear dependency-level findings the way a
    /// narrower file scope does).
    #[must_use]
    pub fn covers_everything(&self, root: &Path) -> bool {
        let root = dunce::simplified(root);
        dunce::simplified(&self.absolute) == root
            || dunce::canonicalize(root).is_ok_and(|canon| self.absolute == canon)
    }
}

/// Resolve a user-supplied positional path against the project root.
///
/// Lookup order is root-first for bare relative paths: `root.join(raw)`,
/// then the current working directory. An explicit `--root` declares the
/// project, so a bare `src` under it wins over a same-named directory where
/// the command happened to run (an agent's own working directory must not
/// hijack the scope). Absolute paths are used as given; `./` and `../`
/// prefixes are explicit current-directory claims and never consult the
/// root. The target must exist and must resolve inside the root; anything
/// else is an actionable exit-2 message, never a silent reinterpretation.
///
/// The returned path keeps the root-joined spelling (not the fully
/// canonicalized one) so prefix matching agrees with finding paths, which
/// are built as `root.join(...)` throughout the pipeline. Canonicalization
/// is used only for the inside-root check.
///
/// # Errors
///
/// Returns a message when the path is empty, carries control characters,
/// does not exist, or escapes the project root.
pub fn resolve_scope_path(root: &Path, raw: &Path) -> Result<ScopePath, String> {
    let raw_display = raw.display().to_string();
    if raw.as_os_str().is_empty() {
        return Err("PATH must not be empty".to_string());
    }
    if let Some(raw_str) = raw.to_str()
        && let Err(detail) = fallow_engine::validate::validate_no_control_chars(raw_str, "PATH")
    {
        return Err(detail);
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let explicit_cwd_claim = matches!(
        raw.components().next(),
        Some(std::path::Component::CurDir | std::path::Component::ParentDir)
    );
    let candidates: Vec<PathBuf> = if raw.is_absolute() {
        vec![raw.to_path_buf()]
    } else if explicit_cwd_claim {
        vec![cwd.join(raw)]
    } else {
        vec![root.join(raw), cwd.join(raw)]
    };
    let canonical_root = dunce::canonicalize(root)
        .map_err(|err| format!("invalid project root '{}': {err}", root.display()))?;
    let mut outside: Option<PathBuf> = None;
    for candidate in &candidates {
        if candidate.symlink_metadata().is_err() {
            continue;
        }
        let Ok(canonical) = dunce::canonicalize(candidate) else {
            continue;
        };
        if canonical != canonical_root && !canonical.starts_with(&canonical_root) {
            outside.get_or_insert_with(|| candidate.clone());
            continue;
        }
        let relative = match canonical.strip_prefix(&canonical_root) {
            Ok(relative) => relative.to_path_buf(),
            Err(_) => PathBuf::new(),
        };
        let absolute = if let Ok(lexical) = candidate.strip_prefix(root) {
            dunce::simplified(&root.join(lexical)).to_path_buf()
        } else {
            dunce::simplified(&canonical_root.join(relative)).to_path_buf()
        };
        return Ok(ScopePath {
            is_dir: canonical.is_dir(),
            absolute,
        });
    }
    if let Some(escaped) = outside {
        return Err(format!(
            "PATH '{raw_display}' (resolved to '{}') is outside the project root ('{}'). Pass --root <dir> for another project, or a path inside this root",
            escaped.display(),
            root.display()
        ));
    }
    let searched = if dunce::simplified(&cwd) == dunce::simplified(root) {
        format!("'{}'", root.display())
    } else {
        format!(
            "the current directory and the project root '{}'",
            root.display()
        )
    };
    Err(format!(
        "PATH '{raw_display}' does not exist (looked relative to {searched})"
    ))
}

/// True when a finding path falls inside the scope: the finding is the scope
/// target itself, or (for directory scopes) lives below it. Comparison is
/// component-wise, so scope `/repo/src` never matches `/repo/src-extra`.
#[must_use]
pub fn scope_covers(scope: &Path, path: &Path) -> bool {
    let simplified = dunce::simplified(path);
    simplified == scope || simplified.starts_with(scope)
}

/// Resolve an optional positional `PATH` for a command dispatch.
///
/// `None` (no positional) and a scope covering the whole project root both
/// yield `Ok(None)`, so `fallow dupes .` behaves exactly like `fallow dupes`.
/// Anything unresolvable is an actionable exit-2 error, never a silent
/// reinterpretation.
pub fn resolve_command_scope(
    root: &Path,
    output: fallow_config::OutputFormat,
    raw: Option<std::path::PathBuf>,
) -> Result<Option<ScopePath>, std::process::ExitCode> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    match resolve_scope_path(root, &raw) {
        Ok(scope) if scope.covers_everything(root) => Ok(None),
        Ok(scope) => Ok(Some(scope)),
        Err(message) => Err(crate::error::emit_input_error(&message, output)),
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn fixture_root() -> PathBuf {
        dunce::canonicalize(std::env::current_dir().expect("cwd")).expect("canonical cwd")
    }

    #[test]
    fn resolves_file_inside_root() {
        let root = fixture_root();
        let scope = resolve_scope_path(&root, Path::new("Cargo.toml")).expect("Cargo.toml exists");
        assert!(!scope.is_dir);
        assert!(scope.absolute.starts_with(&root));
    }

    #[test]
    fn rejects_missing_path() {
        let root = fixture_root();
        let err = resolve_scope_path(&root, Path::new("no-such-path-xyz-123")).unwrap_err();
        assert!(err.contains("does not exist"), "{err}");
    }

    #[test]
    fn rejects_outside_root() {
        let root = fixture_root();
        let outside = root.parent().expect("root has a parent").to_path_buf();
        if outside == root {
            return;
        }
        let err = resolve_scope_path(&root, &outside).unwrap_err();
        assert!(err.contains("outside the project root"), "{err}");
    }

    #[test]
    fn root_scope_covers_everything() {
        let root = fixture_root();
        let scope = resolve_scope_path(&root, &root).expect("root resolves");
        assert!(scope.covers_everything(&root));
    }

    #[test]
    fn dot_slash_prefix_claims_current_directory() {
        let root = fixture_root();
        let scope =
            resolve_scope_path(&root, Path::new("./Cargo.toml")).expect("./Cargo.toml exists");
        assert!(!scope.is_dir);
        assert!(scope.absolute.starts_with(&root));
    }

    #[test]
    fn bare_relative_prefers_root_over_cwd() {
        let root = tempfile::tempdir().expect("temporary scope root");
        let cwd_file = fixture_root().join("Cargo.toml");
        assert!(cwd_file.is_file(), "test cwd must hold Cargo.toml");
        fs::write(root.path().join("Cargo.toml"), "[package]\n").expect("scope file");
        let scope =
            resolve_scope_path(root.path(), Path::new("Cargo.toml")).expect("root file wins");
        assert!(scope.absolute.starts_with(dunce::simplified(root.path())));
        assert!(!scope.absolute.starts_with(fixture_root()));
    }

    #[test]
    fn scope_covers_file_and_children_only() {
        let scope = Path::new("/repo/src");
        assert!(scope_covers(scope, Path::new("/repo/src")));
        assert!(scope_covers(scope, Path::new("/repo/src/a.ts")));
        assert!(!scope_covers(scope, Path::new("/repo/src-extra/a.ts")));
        assert!(!scope_covers(scope, Path::new("/repo/other/a.ts")));
    }
}
