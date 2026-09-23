//! Engine-owned repository reference probes and temporary repo views.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use fallow_config::WorkspaceInfo;
use fallow_types::audit_cache::{
    AuditContextDirectoryFingerprint, AuditContextFileFingerprint, AuditContextPathState,
    AuditMaterializedContextFingerprint,
};
use fallow_types::source_fingerprint::SourceFingerprint;
use xxhash_rust::xxh3::xxh3_64;

use crate::{EngineError, EngineResult};

const RAW_MATERIALIZATION_MARKER: &str = "fallow-raw-materialized-v1";

/// Host directories shared with detached audit base views.
pub const AUDIT_MATERIALIZED_CONTEXT_DIRS: &[&str] = &["node_modules", ".nuxt", ".astro"];
const AUDIT_WORKSPACE_GENERATED_CONTEXT_DIRS: &[&str] = &[".nuxt", ".astro"];

const AUDIT_LOCKFILES: &[&str] = &[
    "package-lock.json",
    "npm-shrinkwrap.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
];

const NODE_MODULES_MARKERS: &[&str] = &[".package-lock.json", ".modules.yaml", ".yarn-state.yml"];
const NUXT_MARKERS: &[&str] = &[
    "tsconfig.json",
    "tsconfig.app.json",
    "imports.d.ts",
    "components.d.ts",
    "types/nitro-routes.d.ts",
    "types/nitro-imports.d.ts",
];
const ASTRO_MARKERS: &[&str] = &["types.d.ts", "content.d.ts", "env.d.ts"];
const AUDIT_CONTEXT_FILE_MAX_BYTES: u64 = 16 * 1024 * 1024;
const CONTEXT_SYMLINK_STATE: &str = "symlink";
const CONTEXT_SPECIAL_FILE_STATE: &str = "not_regular_file";
const CONTEXT_OVERSIZED_FILE_STATE: &str = "file_too_large";
const CONTEXT_PARENT_UNAVAILABLE_STATE: &str = "parent_context_unavailable";
const CONTEXT_CHANGED_DURING_READ_STATE: &str = "changed_during_read";

#[cfg(any(target_os = "linux", target_os = "android"))]
const UNIX_CONTEXT_OPEN_FLAGS: i32 = 0x0002_0800;
#[cfg(any(
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "openbsd",
    target_os = "netbsd"
))]
const UNIX_CONTEXT_OPEN_FLAGS: i32 = 0x0104;

#[cfg(windows)]
const WINDOWS_FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
#[cfg(windows)]
const WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;

/// Resolved base ref for changed-code audit.
#[derive(Debug, Clone)]
pub struct ResolvedAuditBase {
    /// Git ref or SHA used for comparison.
    pub git_ref: String,
    /// Human-readable source of the resolved ref.
    pub description: Option<String>,
}

/// Temporary detached worktree for comparing audit results against a base ref.
#[derive(Debug)]
pub struct TemporaryBaseWorktree {
    repo_root: PathBuf,
    path: PathBuf,
}

impl TemporaryBaseWorktree {
    /// Create a detached base worktree for `base_ref`.
    ///
    /// # Errors
    ///
    /// Returns an engine error when the temp path cannot be generated, `git`
    /// cannot be started, or the worktree cannot be created.
    pub fn create(repo_root: &Path, base_ref: &str) -> EngineResult<Self> {
        let path = base_worktree_path()?;
        create_detached_base_worktree(repo_root, &path, base_ref)?;
        materialize_base_dependency_context(repo_root, &path);
        Ok(Self {
            repo_root: repo_root.to_path_buf(),
            path,
        })
    }

    /// Path to the detached worktree.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Share dependency and generated context from the host checkout with a base view.
pub fn materialize_base_dependency_context(repo_root: &Path, worktree_path: &Path) {
    for slot in audit_materialized_context_slots(repo_root) {
        let Ok(source) = canonical_context_directory(&slot.source) else {
            continue;
        };

        if validate_materialized_path(&slot.relative).is_err()
            || create_safe_parent_directories(worktree_path, &slot.relative).is_err()
        {
            continue;
        }
        let destination = worktree_path.join(&slot.relative);
        match fs::symlink_metadata(&destination) {
            Ok(metadata) if metadata.file_type().is_dir() => continue,
            Ok(metadata) if metadata.file_type().is_symlink() => {
                if fs::remove_file(&destination).is_err() {
                    continue;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Ok(_) | Err(_) => continue,
        }

        let _ = symlink_dependency_dir(&source, &destination);
    }
}

/// Build a bounded fingerprint of the host context materialized into a base view.
#[must_use]
pub fn audit_materialized_context_fingerprint(root: &Path) -> AuditMaterializedContextFingerprint {
    let lockfiles = AUDIT_LOCKFILES
        .iter()
        .map(|name| fingerprint_context_file(root, &root.join(name)))
        .collect();
    let directories = audit_materialized_context_slots(root)
        .iter()
        .map(fingerprint_context_directory)
        .collect();
    AuditMaterializedContextFingerprint {
        lockfiles,
        directories,
    }
}

#[derive(Debug)]
struct AuditMaterializedContextSlot {
    kind: &'static str,
    relative: PathBuf,
    source: PathBuf,
}

fn audit_materialized_context_slots(root: &Path) -> Vec<AuditMaterializedContextSlot> {
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let mut slots = AUDIT_MATERIALIZED_CONTEXT_DIRS
        .iter()
        .map(|&kind| AuditMaterializedContextSlot {
            kind,
            relative: PathBuf::from(kind),
            source: canonical_root.join(kind),
        })
        .collect::<Vec<_>>();

    for workspace in crate::discover::discover_workspace_packages(root) {
        let Ok(canonical_workspace) = dunce::canonicalize(&workspace.root) else {
            continue;
        };
        let Ok(relative_workspace) = canonical_workspace.strip_prefix(&canonical_root) else {
            continue;
        };
        if relative_workspace.as_os_str().is_empty() {
            continue;
        }
        for &kind in AUDIT_WORKSPACE_GENERATED_CONTEXT_DIRS {
            slots.push(AuditMaterializedContextSlot {
                kind,
                relative: relative_workspace.join(kind),
                source: canonical_workspace.join(kind),
            });
        }
    }

    slots.sort_by(|left, right| left.relative.cmp(&right.relative));
    slots.dedup_by(|left, right| left.relative == right.relative);
    slots
}

fn fingerprint_context_directory(
    slot: &AuditMaterializedContextSlot,
) -> AuditContextDirectoryFingerprint {
    let path = &slot.source;
    let (state, canonical_path, source) = match canonical_context_directory(path) {
        Ok(canonical_path) => {
            let source = fs::symlink_metadata(&canonical_path)
                .ok()
                .as_ref()
                .map(SourceFingerprint::from_metadata);
            (AuditContextPathState::Present, Some(canonical_path), source)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (AuditContextPathState::Missing, None, None)
        }
        Err(error) => (
            AuditContextPathState::Unreadable(error.kind().to_string()),
            None,
            fs::symlink_metadata(path)
                .ok()
                .as_ref()
                .map(SourceFingerprint::from_metadata),
        ),
    };
    let canonical_path_display = canonical_path
        .as_ref()
        .map(|path| path.to_string_lossy().replace('\\', "/"));
    let markers = context_markers(slot.kind)
        .iter()
        .map(|marker| {
            let relative = slot
                .relative
                .join(marker)
                .to_string_lossy()
                .replace('\\', "/");
            canonical_path.as_ref().map_or_else(
                || unavailable_context_file(&relative, &state),
                |path| fingerprint_context_file_at(&path.join(marker), &relative),
            )
        })
        .collect();
    AuditContextDirectoryFingerprint {
        name: slot.relative.to_string_lossy().replace('\\', "/"),
        state,
        canonical_path: canonical_path_display,
        source,
        markers,
    }
}

fn canonical_context_directory(path: &Path) -> std::io::Result<PathBuf> {
    let canonical_path = dunce::canonicalize(path)?;
    match fs::symlink_metadata(&canonical_path) {
        Ok(metadata) if metadata.file_type().is_dir() => Ok(canonical_path),
        Ok(_) => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            CONTEXT_SPECIAL_FILE_STATE,
        )),
        Err(error) => Err(error),
    }
}

fn context_markers(name: &str) -> &'static [&'static str] {
    match name {
        "node_modules" => NODE_MODULES_MARKERS,
        ".nuxt" => NUXT_MARKERS,
        ".astro" => ASTRO_MARKERS,
        _ => &[],
    }
}

fn fingerprint_context_file(root: &Path, path: &Path) -> AuditContextFileFingerprint {
    let relative = path
        .strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/");
    fingerprint_context_file_at(path, &relative)
}

fn fingerprint_context_file_at(path: &Path, relative: &str) -> AuditContextFileFingerprint {
    fingerprint_context_file_at_with_hooks(path, relative, || {}, || {})
}

fn fingerprint_context_file_at_with_hooks(
    path: &Path,
    relative: &str,
    before_open: impl FnOnce(),
    after_read: impl FnOnce(),
) -> AuditContextFileFingerprint {
    before_open();
    let mut file = match open_context_file(path) {
        Ok(file) => file,
        Err(error) => return classify_unopened_context_file(path, relative, error.kind()),
    };
    let opened_metadata = match file.metadata() {
        Ok(metadata) if context_handle_is_regular_file(&metadata) => metadata,
        Ok(metadata) => {
            return unreadable_context_file(
                relative,
                CONTEXT_SPECIAL_FILE_STATE,
                Some(SourceFingerprint::from_metadata(&metadata)),
            );
        }
        Err(error) => return unreadable_context_file(relative, error.kind().to_string(), None),
    };
    let source = Some(SourceFingerprint::from_metadata(&opened_metadata));
    if opened_metadata.len() > AUDIT_CONTEXT_FILE_MAX_BYTES {
        return unreadable_context_file(relative, CONTEXT_OVERSIZED_FILE_STATE, source);
    }

    let capacity = usize::try_from(opened_metadata.len()).unwrap_or(0);
    let mut bytes = Vec::with_capacity(capacity);
    let read_limit = AUDIT_CONTEXT_FILE_MAX_BYTES.saturating_add(1);
    if let Err(error) = (&mut file).take(read_limit).read_to_end(&mut bytes) {
        return unreadable_context_file(relative, error.kind().to_string(), source);
    }
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > AUDIT_CONTEXT_FILE_MAX_BYTES {
        return unreadable_context_file(relative, CONTEXT_OVERSIZED_FILE_STATE, source);
    }
    after_read();
    let final_metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(error) => return unreadable_context_file(relative, error.kind().to_string(), source),
    };
    if SourceFingerprint::from_metadata(&opened_metadata)
        != SourceFingerprint::from_metadata(&final_metadata)
    {
        return unreadable_context_file(relative, CONTEXT_CHANGED_DURING_READ_STATE, source);
    }

    AuditContextFileFingerprint {
        path: relative.to_string(),
        state: AuditContextPathState::Present,
        source,
        content_hash: Some(format!("{:016x}", xxh3_64(&bytes))),
    }
}

#[expect(
    clippy::filetype_is_file,
    reason = "failed atomic opens are classified conservatively without treating arbitrary non-directories as readable files"
)]
fn classify_unopened_context_file(
    path: &Path,
    relative: &str,
    open_error: std::io::ErrorKind,
) -> AuditContextFileFingerprint {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => unreadable_context_file(
            relative,
            CONTEXT_SYMLINK_STATE,
            Some(SourceFingerprint::from_metadata(&metadata)),
        ),
        Ok(metadata) if !metadata.file_type().is_file() => unreadable_context_file(
            relative,
            CONTEXT_SPECIAL_FILE_STATE,
            Some(SourceFingerprint::from_metadata(&metadata)),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            missing_context_file(relative)
        }
        Ok(metadata) => unreadable_context_file(
            relative,
            open_error.to_string(),
            Some(SourceFingerprint::from_metadata(&metadata)),
        ),
        Err(_) => unreadable_context_file(relative, open_error.to_string(), None),
    }
}

fn open_context_file(path: &Path) -> std::io::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);

    #[cfg(any(
        target_os = "linux",
        target_os = "android",
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "openbsd",
        target_os = "netbsd"
    ))]
    {
        use std::os::unix::fs::OpenOptionsExt as _;

        options.custom_flags(UNIX_CONTEXT_OPEN_FLAGS);
    }
    #[cfg(all(
        unix,
        not(any(
            target_os = "linux",
            target_os = "android",
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "dragonfly",
            target_os = "openbsd",
            target_os = "netbsd"
        ))
    ))]
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "atomic no-follow context reads are unavailable on this Unix target",
        ));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;

        options.custom_flags(WINDOWS_FILE_FLAG_OPEN_REPARSE_POINT);
    }

    options.open(path)
}

#[cfg(windows)]
#[expect(
    clippy::filetype_is_file,
    reason = "security boundary intentionally accepts regular files only and rejects reparse points, directories, and special files"
)]
fn context_handle_is_regular_file(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt as _;

    metadata.file_type().is_file()
        && metadata.file_attributes() & WINDOWS_FILE_ATTRIBUTE_REPARSE_POINT == 0
}

#[cfg(not(windows))]
#[expect(
    clippy::filetype_is_file,
    reason = "security boundary intentionally accepts regular files only and rejects directories, sockets, devices, and pipes"
)]
fn context_handle_is_regular_file(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_file()
}

fn missing_context_file(relative: &str) -> AuditContextFileFingerprint {
    AuditContextFileFingerprint {
        path: relative.to_string(),
        state: AuditContextPathState::Missing,
        source: None,
        content_hash: None,
    }
}

fn unreadable_context_file(
    relative: &str,
    reason: impl Into<String>,
    source: Option<SourceFingerprint>,
) -> AuditContextFileFingerprint {
    AuditContextFileFingerprint {
        path: relative.to_string(),
        state: AuditContextPathState::Unreadable(reason.into()),
        source,
        content_hash: None,
    }
}

fn unavailable_context_file(
    relative: &str,
    parent_state: &AuditContextPathState,
) -> AuditContextFileFingerprint {
    if matches!(parent_state, AuditContextPathState::Missing) {
        missing_context_file(relative)
    } else {
        unreadable_context_file(relative, CONTEXT_PARENT_UNAVAILABLE_STATE, None)
    }
}

#[cfg(unix)]
fn symlink_dependency_dir(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, destination)
}

#[cfg(windows)]
fn symlink_dependency_dir(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(source, destination)
}

/// Register a detached worktree without checking files out, then materialize
/// the committed tree directly from Git objects.
///
/// This deliberately avoids Git's checkout pipeline. No checkout hook,
/// smudge filter, process filter, line-ending conversion, or working-tree
/// encoding is invoked. Regular files contain the raw committed blob bytes.
///
/// # Errors
///
/// Returns an engine error when the destination is not absolute, Git cannot
/// create the administrative worktree, the tree contains an unsafe path, or
/// an object cannot be materialized. A failed materialization removes both
/// the worktree registration and its partial directory.
pub fn create_detached_base_worktree(
    repo_root: &Path,
    destination: &Path,
    base_ref: &str,
) -> EngineResult<()> {
    if !destination.is_absolute() {
        return Err(EngineError::new(format!(
            "base worktree destination must be absolute: {}",
            destination.display()
        )));
    }

    register_no_checkout_worktree(repo_root, destination, base_ref)?;
    let result = make_worktree_root_private(destination)
        .and_then(|()| resolve_registered_commit(destination, base_ref))
        .and_then(|commit| {
            populate_worktree_index(destination, &commit)?;
            materialize_committed_tree(repo_root, destination, &commit)
        })
        .and_then(|()| write_raw_materialization_marker(destination));
    if let Err(error) = result {
        remove_registered_worktree(repo_root, destination);
        let _ = fs::remove_dir_all(destination);
        return Err(error);
    }
    Ok(())
}

#[cfg(unix)]
fn make_worktree_root_private(destination: &Path) -> EngineResult<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(destination, fs::Permissions::from_mode(0o700)).map_err(|error| {
        EngineError::new(format!(
            "could not make base worktree private at `{}`: {error}",
            destination.display()
        ))
    })
}

#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "shared cross-platform signature; Unix applies privacy permissions"
)]
fn make_worktree_root_private(_destination: &Path) -> EngineResult<()> {
    Ok(())
}

/// Return whether a linked worktree was completely materialized by the raw
/// object path used by [`create_detached_base_worktree`].
///
/// Reusable audit caches created by older versions have no marker and must be
/// rebuilt once so smudged or checkout-generated contents are not reused.
#[must_use]
pub fn detached_base_worktree_is_raw_materialized(worktree_root: &Path) -> bool {
    raw_materialization_marker_path(worktree_root).is_ok_and(|path| path.is_file())
}

fn write_raw_materialization_marker(worktree_root: &Path) -> EngineResult<()> {
    let marker = raw_materialization_marker_path(worktree_root)?;
    fs::write(&marker, b"raw-v1\n").map_err(|error| {
        EngineError::new(format!(
            "could not record raw base-worktree materialization at `{}`: {error}",
            marker.display()
        ))
    })
}

fn raw_materialization_marker_path(worktree_root: &Path) -> EngineResult<PathBuf> {
    let marker = run_git(
        worktree_root,
        &["rev-parse", "--git-path", RAW_MATERIALIZATION_MARKER],
    )
    .ok_or_else(|| EngineError::new("could not resolve base-worktree materialization marker"))?;
    let marker = PathBuf::from(marker);
    if marker.is_absolute() {
        Ok(marker)
    } else {
        Ok(worktree_root.join(marker))
    }
}

fn register_no_checkout_worktree(
    repo_root: &Path,
    destination: &Path,
    base_ref: &str,
) -> EngineResult<()> {
    let mut command = git_command(repo_root);
    command.args([
        "worktree",
        "add",
        "--detach",
        "--quiet",
        "--no-checkout",
        "--",
    ]);
    command.arg(destination).arg(base_ref);
    let output = command.output().map_err(|error| {
        EngineError::new(format!(
            "could not create a temporary worktree for base ref `{base_ref}`: {error}"
        ))
    })?;
    if !output.status.success() {
        return Err(EngineError::new(format!(
            "could not create a temporary worktree for base ref `{base_ref}`: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

fn resolve_registered_commit(destination: &Path, base_ref: &str) -> EngineResult<String> {
    run_git(destination, &["rev-parse", "--verify", "HEAD^{commit}"]).ok_or_else(|| {
        EngineError::new(format!(
            "could not resolve the commit for base ref `{base_ref}` after creating the worktree"
        ))
    })
}

fn populate_worktree_index(destination: &Path, commit: &str) -> EngineResult<()> {
    let disabled_hooks_path = destination.join(".fallow-disabled-git-hooks");
    let output = git_command(destination)
        .env("GIT_CONFIG_COUNT", "2")
        .env("GIT_CONFIG_KEY_0", "core.hooksPath")
        .env("GIT_CONFIG_VALUE_0", disabled_hooks_path)
        .env("GIT_CONFIG_KEY_1", "core.fsmonitor")
        .env("GIT_CONFIG_VALUE_1", "false")
        .args(["read-tree", "--reset", commit])
        .output()
        .map_err(|error| {
            EngineError::new(format!("could not populate base worktree index: {error}"))
        })?;
    if !output.status.success() {
        return Err(EngineError::new(format!(
            "could not populate base worktree index: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TreeEntryKind {
    Regular,
    Executable,
    Symlink,
    Gitlink,
}

#[derive(Debug)]
struct TreeEntry {
    kind: TreeEntryKind,
    object_id: String,
    path: PathBuf,
}

/// Which committed-tree paths a base worktree actually needs on disk.
///
/// The raw object materialization deliberately bypasses git's checkout
/// pipeline (no hooks, smudge filters, or line-ending conversion). Before
/// that change the worktree checkout honored the host's sparse-checkout cone
/// and, for a subdirectory analysis root, only the cone was ever read. The
/// unscoped materialization reads EVERY blob in the commit instead: on a
/// blobless partial clone (`actions/checkout` sets `--filter=blob:none`
/// whenever `sparse-checkout` is set) each out-of-cone blob triggers a lazy
/// promisor fetch via `git-remote-https`. For a large monorepo checked out
/// sparsely to one subdirectory that turns a seconds-long snapshot into a
/// fetch of the whole monorepo, which presents as `fallow audit` hanging to
/// the CI timeout with `git` / `git-remote-https` orphans (issue #2615).
///
/// The scope restores the old working set without reintroducing checkout:
/// - a subdirectory analysis root materializes only that subtree (plus
///   top-level files and ancestor ignore files, so gitignore parity holds),
/// - a repository-root run on a sparse checkout materializes the sparse cone
///   (top-level files plus the listed cone directories),
/// - otherwise everything is materialized as before.
///
/// Both probes fail open to full materialization: a probe error is at worst a
/// slower snapshot, never a missing-file misattribution.
struct MaterializationScope {
    /// Forward-slash repo-relative analysis subdir (e.g. `apps/web`), or
    /// `None` when the requested root is the repository top level.
    subdir_prefix: Option<String>,
    /// Cone-mode sparse directories (forward-slash, no trailing slash), or
    /// `None` when sparse-checkout is off, non-cone, or unreadable.
    sparse_dirs: Option<Vec<String>>,
}

impl MaterializationScope {
    fn should_materialize(&self, path: &Path) -> bool {
        let Some(relative) = forward_slash_path(path) else {
            return true;
        };
        if let Some(prefix) = self.subdir_prefix.as_deref() {
            if relative == prefix || relative.starts_with(&format!("{prefix}/")) {
                return true;
            }
            // Top-level files and ancestor ignore files shape discovery of the
            // subtree (root `.gitignore` applies hierarchically). They are few
            // and already present in a sparse checkout, so keeping them is
            // free and preserves ignore parity with a full snapshot.
            if !relative.contains('/') {
                return true;
            }
            return is_ancestor_ignore_file(&relative, prefix);
        }
        if let Some(dirs) = self.sparse_dirs.as_deref() {
            if !relative.contains('/') {
                return true;
            }
            return dirs
                .iter()
                .any(|dir| relative == *dir || relative.starts_with(&format!("{dir}/")));
        }
        true
    }
}

/// Forward-slash repo-relative path for scope matching, or `None` when the
/// path is not valid UTF-8. Non-UTF-8 tree paths are rare; failing open keeps
/// them materialized rather than risking a misattributed base snapshot.
fn forward_slash_path(path: &Path) -> Option<String> {
    let raw = path.to_str()?;
    Some(raw.replace('\\', "/"))
}

/// True for an ignore/attributes file that governs `prefix` from an ancestor
/// directory (including the repository root), e.g. `.gitignore` or
/// `apps/.gitignore` for prefix `apps/web`.
fn is_ancestor_ignore_file(relative: &str, prefix: &str) -> bool {
    const IGNORE_FILES: &[&str] = &[".gitignore", ".gitattributes"];
    let Some(file_name) = relative.rsplit('/').next() else {
        return false;
    };
    if !IGNORE_FILES.contains(&file_name) {
        return false;
    }
    let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
    parent.is_empty() || prefix == parent || prefix.starts_with(&format!("{parent}/"))
}

fn materialization_scope(repo_root: &Path) -> MaterializationScope {
    MaterializationScope {
        subdir_prefix: analysis_subdir_prefix(repo_root),
        sparse_dirs: sparse_cone_dirs(repo_root),
    }
}

/// Repo-relative forward-slash subdir of the requested analysis root, or
/// `None` when it is the repository top level (or the top level cannot be
/// resolved, which fails open to full materialization).
fn analysis_subdir_prefix(repo_root: &Path) -> Option<String> {
    let toplevel = run_git(repo_root, &["rev-parse", "--show-toplevel"])?;
    let toplevel = PathBuf::from(toplevel);
    let canonical_toplevel = dunce::canonicalize(&toplevel).unwrap_or(toplevel);
    let canonical_root = dunce::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    let relative = canonical_root.strip_prefix(&canonical_toplevel).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    let prefix = forward_slash_path(relative)?;
    if prefix.is_empty() {
        return None;
    }
    Some(prefix)
}

/// Cone-mode sparse-checkout directories of the host checkout, or `None` when
/// sparse-checkout is off, non-cone, or unreadable (fail open to full).
///
/// `git sparse-checkout list` exits non-zero on a non-sparse worktree, which
/// is the common full-clone case. Non-cone mode uses glob patterns that this
/// matcher does not implement, so it also falls back to full materialization.
fn sparse_cone_dirs(repo_root: &Path) -> Option<Vec<String>> {
    if run_git(repo_root, &["config", "--get", "core.sparseCheckout"])? != "true" {
        return None;
    }
    if run_git(repo_root, &["config", "--get", "core.sparseCheckoutCone"])? != "true" {
        return None;
    }
    let output = git_command(repo_root)
        .args(["sparse-checkout", "list"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let list = String::from_utf8(output.stdout).ok()?;
    let mut dirs = Vec::new();
    for line in list.lines() {
        let pattern = line.trim().trim_matches('/');
        if pattern.is_empty() {
            continue;
        }
        // Cone mode lists directories; a stray glob (non-cone residue) cannot
        // be matched exactly, so fail open rather than under-materialize.
        if pattern.contains(['*', '?', '[', '!']) {
            return None;
        }
        dirs.push(pattern.replace('\\', "/"));
    }
    Some(dirs)
}

fn materialize_committed_tree(
    repo_root: &Path,
    destination: &Path,
    commit: &str,
) -> EngineResult<()> {
    let entries = committed_tree_entries(repo_root, commit)?;
    let scope = materialization_scope(repo_root);
    let entries: Vec<TreeEntry> = entries
        .into_iter()
        .filter(|entry| scope.should_materialize(&entry.path))
        .collect();
    let mut blobs = BatchBlobReader::spawn(repo_root)?;
    let mut symlinks = Vec::new();

    for entry in entries {
        create_safe_parent_directories(destination, &entry.path)?;
        let output_path = destination.join(&entry.path);
        match entry.kind {
            TreeEntryKind::Regular | TreeEntryKind::Executable => {
                let mut file = OpenOptions::new()
                    .create_new(true)
                    .write(true)
                    .open(&output_path)
                    .map_err(|error| materialization_error(&entry.path, error))?;
                blobs.copy_blob(&entry.object_id, &entry.path, &mut file)?;
                set_regular_file_mode(&output_path, entry.kind == TreeEntryKind::Executable)?;
            }
            TreeEntryKind::Symlink => {
                let target = blobs.read_blob(&entry.object_id, &entry.path)?;
                symlinks.push((entry.path, target));
            }
            TreeEntryKind::Gitlink => {
                fs::create_dir(&output_path)
                    .map_err(|error| materialization_error(&entry.path, error))?;
            }
        }
    }

    blobs.finish()?;
    for (path, target) in symlinks {
        create_safe_parent_directories(destination, &path)?;
        create_materialized_symlink(&destination.join(&path), &target)
            .map_err(|error| materialization_error(&path, error))?;
    }
    Ok(())
}

fn committed_tree_entries(repo_root: &Path, commit: &str) -> EngineResult<Vec<TreeEntry>> {
    let output = git_command(repo_root)
        .args(["ls-tree", "-r", "-z", "--full-tree", commit])
        .output()
        .map_err(|error| EngineError::new(format!("could not read base commit tree: {error}")))?;
    if !output.status.success() {
        return Err(EngineError::new(format!(
            "could not read base commit tree: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }

    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(parse_tree_entry)
        .collect()
}

fn parse_tree_entry(record: &[u8]) -> EngineResult<TreeEntry> {
    let tab = record
        .iter()
        .position(|byte| *byte == b'\t')
        .ok_or_else(|| EngineError::new("could not parse base commit tree entry without a path"))?;
    let header = std::str::from_utf8(&record[..tab])
        .map_err(|error| EngineError::new(format!("invalid Git tree header: {error}")))?;
    let mut fields = header.split_ascii_whitespace();
    let mode = fields.next().unwrap_or_default();
    let object_type = fields.next().unwrap_or_default();
    let object_id = fields.next().unwrap_or_default();
    if fields.next().is_some() || object_id.is_empty() {
        return Err(EngineError::new(format!(
            "could not parse Git tree header `{header}`"
        )));
    }
    let kind = match (mode, object_type) {
        ("100644", "blob") => TreeEntryKind::Regular,
        ("100755", "blob") => TreeEntryKind::Executable,
        ("120000", "blob") => TreeEntryKind::Symlink,
        ("160000", "commit") => TreeEntryKind::Gitlink,
        _ => {
            return Err(EngineError::new(format!(
                "unsupported Git tree entry mode `{mode}` and type `{object_type}`"
            )));
        }
    };
    let path = git_path_from_bytes(&record[tab + 1..])?;
    validate_materialized_path(&path)?;
    Ok(TreeEntry {
        kind,
        object_id: object_id.to_owned(),
        path,
    })
}

#[cfg(unix)]
#[expect(
    clippy::unnecessary_wraps,
    reason = "shared cross-platform signature; non-Unix path decoding is fallible"
)]
fn git_path_from_bytes(bytes: &[u8]) -> EngineResult<PathBuf> {
    use std::os::unix::ffi::OsStringExt as _;

    Ok(std::ffi::OsString::from_vec(bytes.to_vec()).into())
}

#[cfg(not(unix))]
fn git_path_from_bytes(bytes: &[u8]) -> EngineResult<PathBuf> {
    String::from_utf8(bytes.to_vec())
        .map(PathBuf::from)
        .map_err(|error| EngineError::new(format!("Git tree path is not valid UTF-8: {error}")))
}

fn validate_materialized_path(path: &Path) -> EngineResult<()> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err(unsafe_tree_path(path));
    }

    let mut saw_component = false;
    for component in path.components() {
        let Component::Normal(segment) = component else {
            return Err(unsafe_tree_path(path));
        };
        saw_component = true;
        if segment.to_str().is_some_and(is_git_admin_alias) {
            return Err(unsafe_tree_path(path));
        }
    }
    if !saw_component {
        return Err(unsafe_tree_path(path));
    }
    Ok(())
}

fn is_git_admin_alias(segment: &str) -> bool {
    let normalized = segment.trim_end_matches([' ', '.']).to_ascii_lowercase();
    normalized == ".git" || normalized == "git~1"
}

fn unsafe_tree_path(path: &Path) -> EngineError {
    EngineError::new(format!(
        "refusing to materialize unsafe Git tree path `{}`",
        path.display()
    ))
}

fn create_safe_parent_directories(root: &Path, relative: &Path) -> EngineResult<()> {
    let Some(parent) = relative.parent() else {
        return Ok(());
    };
    let mut current = root.to_path_buf();
    for component in parent.components() {
        let Component::Normal(segment) = component else {
            return Err(unsafe_tree_path(relative));
        };
        current.push(segment);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_dir() => {}
            Ok(_) => {
                return Err(EngineError::new(format!(
                    "refusing to materialize through non-directory path `{}`",
                    current.display()
                )));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                fs::create_dir(&current).map_err(|error| materialization_error(relative, error))?;
            }
            Err(error) => return Err(materialization_error(relative, error)),
        }
    }
    Ok(())
}

fn materialization_error(path: &Path, error: impl std::fmt::Display) -> EngineError {
    EngineError::new(format!(
        "could not materialize base commit path `{}`: {error}",
        path.display()
    ))
}

#[cfg(unix)]
fn set_regular_file_mode(path: &Path, executable: bool) -> EngineResult<()> {
    use std::os::unix::fs::PermissionsExt as _;

    let mode = if executable { 0o755 } else { 0o644 };
    let permissions = fs::Permissions::from_mode(mode);
    fs::set_permissions(path, permissions).map_err(|error| materialization_error(path, error))
}

#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "shared cross-platform signature; Unix permission updates are fallible"
)]
fn set_regular_file_mode(_path: &Path, _executable: bool) -> EngineResult<()> {
    Ok(())
}

#[cfg(unix)]
fn create_materialized_symlink(path: &Path, target: &[u8]) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStringExt as _;

    std::os::unix::fs::symlink(std::ffi::OsString::from_vec(target.to_vec()), path)
}

#[cfg(windows)]
fn create_materialized_symlink(path: &Path, target: &[u8]) -> std::io::Result<()> {
    let target_path = PathBuf::from(String::from_utf8_lossy(target).into_owned());
    let resolved_target = path
        .parent()
        .map_or_else(|| target_path.clone(), |parent| parent.join(&target_path));
    let result = if resolved_target.is_dir() {
        std::os::windows::fs::symlink_dir(&target_path, path)
    } else {
        std::os::windows::fs::symlink_file(&target_path, path)
    };
    result.or_else(|_| fs::write(path, target))
}

#[cfg(not(any(unix, windows)))]
fn create_materialized_symlink(path: &Path, target: &[u8]) -> std::io::Result<()> {
    fs::write(path, target)
}

struct BatchBlobReader {
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl BatchBlobReader {
    fn spawn(repo_root: &Path) -> EngineResult<Self> {
        let mut command = git_command(repo_root);
        command
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command.spawn().map_err(|error| {
            EngineError::new(format!("could not start Git object reader: {error}"))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| EngineError::new("Git object reader has no stdin pipe"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| EngineError::new("Git object reader has no stdout pipe"))?;
        Ok(Self {
            child: Some(child),
            stdin: Some(stdin),
            stdout: BufReader::new(stdout),
        })
    }

    fn copy_blob(&mut self, object_id: &str, path: &Path, target: &mut File) -> EngineResult<()> {
        let size = self.request_blob(object_id, path)?;
        let copied = std::io::copy(&mut self.stdout.by_ref().take(size), target)
            .map_err(|error| materialization_error(path, error))?;
        if copied != size {
            return Err(EngineError::new(format!(
                "Git object reader returned {copied} of {size} bytes for `{}`",
                path.display()
            )));
        }
        self.consume_blob_terminator(path)
    }

    fn read_blob(&mut self, object_id: &str, path: &Path) -> EngineResult<Vec<u8>> {
        let size = self.request_blob(object_id, path)?;
        let size = usize::try_from(size).map_err(|error| materialization_error(path, error))?;
        let mut bytes = vec![0; size];
        self.stdout
            .read_exact(&mut bytes)
            .map_err(|error| materialization_error(path, error))?;
        self.consume_blob_terminator(path)?;
        Ok(bytes)
    }

    fn request_blob(&mut self, object_id: &str, path: &Path) -> EngineResult<u64> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or_else(|| EngineError::new("Git object reader stdin is closed"))?;
        writeln!(stdin, "{object_id}").map_err(|error| materialization_error(path, error))?;
        stdin
            .flush()
            .map_err(|error| materialization_error(path, error))?;

        let mut header = Vec::new();
        self.stdout
            .read_until(b'\n', &mut header)
            .map_err(|error| materialization_error(path, error))?;
        let header = std::str::from_utf8(&header)
            .map_err(|error| materialization_error(path, error))?
            .trim_end();
        let mut fields = header.split_ascii_whitespace();
        let returned_id = fields.next().unwrap_or_default();
        let object_type = fields.next().unwrap_or_default();
        let size = fields.next().unwrap_or_default();
        if returned_id != object_id || object_type != "blob" || fields.next().is_some() {
            return Err(EngineError::new(format!(
                "unexpected Git object response `{header}` for `{}`",
                path.display()
            )));
        }
        size.parse::<u64>()
            .map_err(|error| materialization_error(path, error))
    }

    fn consume_blob_terminator(&mut self, path: &Path) -> EngineResult<()> {
        let mut terminator = [0; 1];
        self.stdout
            .read_exact(&mut terminator)
            .map_err(|error| materialization_error(path, error))?;
        if terminator != *b"\n" {
            return Err(EngineError::new(format!(
                "Git object response for `{}` had no terminator",
                path.display()
            )));
        }
        Ok(())
    }

    fn finish(mut self) -> EngineResult<()> {
        self.stdin.take();
        let status = self
            .child
            .take()
            .ok_or_else(|| EngineError::new("Git object reader is already closed"))?
            .wait()
            .map_err(|error| {
                EngineError::new(format!("could not wait for Git object reader: {error}"))
            })?;
        if !status.success() {
            return Err(EngineError::new(format!(
                "Git object reader exited with status {status}"
            )));
        }
        Ok(())
    }
}

impl Drop for BatchBlobReader {
    fn drop(&mut self) {
        self.stdin.take();
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn remove_registered_worktree(repo_root: &Path, destination: &Path) {
    let _ = git_command(repo_root)
        .args(["worktree", "remove", "--force"])
        .arg(destination)
        .output();
}

impl Drop for TemporaryBaseWorktree {
    fn drop(&mut self) {
        let mut command = git_command(&self.repo_root);
        command
            .arg("worktree")
            .arg("remove")
            .arg("--force")
            .arg(&self.path);
        let _ = command.output();
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Resolve the analysis root inside a detached base worktree.
///
/// This is the one implementation for `fallow audit`, `fallow security --base`
/// and the typed routes. Both sides of the prefix comparison are real paths,
/// because a caller can spell the root through a symbolic link (`/tmp` on
/// macOS resolves to `/private/tmp`) while git reports the resolved top level.
/// A comparison across the two path spaces fails, and the base snapshot then
/// covers the whole base worktree while the head snapshot stays scoped. Only
/// the relative remainder joins `base_worktree_root`, so no canonical spelling
/// reaches the result.
#[must_use]
pub fn base_analysis_root(current_root: &Path, base_worktree_root: &Path) -> PathBuf {
    let Some(git_root) = git_toplevel(current_root) else {
        return base_worktree_root.to_path_buf();
    };
    let current_root =
        dunce::canonicalize(current_root).unwrap_or_else(|_| current_root.to_path_buf());
    match current_root.strip_prefix(&git_root) {
        Ok(relative) => base_worktree_root.join(relative),
        Err(error) => {
            tracing::warn!(
                current_root = %current_root.display(),
                git_root = %git_root.display(),
                error = %error,
                "Could not remap the analysis root into the base worktree; falling back to the worktree root"
            );
            base_worktree_root.to_path_buf()
        }
    }
}

/// Analysis root for a detached base worktree, and whether the base commit
/// contains it at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseAnalysisRoot {
    /// The head analysis root maps onto a directory that the base commit
    /// contains, so the base snapshot is analyzed there.
    Present(PathBuf),
    /// The head analysis root maps onto a directory the base commit does not
    /// contain, such as a package added on the branch. Everything under it is
    /// new, so the base snapshot for that root is empty.
    NewInHead(PathBuf),
}

/// Resolve the analysis root inside a detached base worktree and report
/// whether the base commit contains it.
///
/// A root that the base commit does not contain is the ordinary shape of
/// auditing a package added on the branch. Analyzing the whole base worktree
/// instead would compare a subdirectory head snapshot against a
/// whole-repository base snapshot, whose key spaces do not intersect, and
/// refusing the call would blame a `root` the caller spelled correctly.
#[must_use]
pub fn resolve_base_analysis_root(
    current_root: &Path,
    base_worktree_root: &Path,
) -> BaseAnalysisRoot {
    let root = base_analysis_root(current_root, base_worktree_root);
    if root.is_dir() {
        BaseAnalysisRoot::Present(root)
    } else {
        BaseAnalysisRoot::NewInHead(root)
    }
}

/// Auto-detect the base ref used by changed-code audit when no explicit base
/// or environment override is set.
///
/// The base is the `git merge-base` (fork point) against the branch's upstream
/// or the remote default, mirroring the `fallow hooks install --target git`
/// pre-commit hook (issue #242). Resolving to the merge-base SHA, rather than a
/// bare branch name, fixes the long-standing bug where the default branch was
/// discovered via `origin/HEAD` but returned as the bare name `main` (issue
/// #1168): git resolves a bare `main` to the LOCAL `refs/heads/main`, which is
/// stale on worktree checkouts cut from `origin/main`, so the audit diffed
/// every branch against an ancient base and false-failed the gate.
///
/// Resolution order:
/// 1. `@{upstream}` merge-base, so a branch forked off a non-default
///    integration branch compares against where it actually forked.
/// 2. Remote default (`origin/HEAD` -> `origin/main` -> `origin/master`)
///    merge-base. The remote-tracking ref refreshes on fetch, unlike a
///    long-stale local branch; the merge-base is also immune to an unfetched
///    `origin/main` in the false-fail direction.
/// 3. Local `main` / `master` when there is no `origin` remote, preserving the
///    historical behavior for air-gapped and local-only repositories.
///
/// A branch with no common ancestor with its base (a shallow clone, unrelated
/// history) falls back to the remote-tracking tip rather than failing the
/// detection outright.
#[must_use]
pub fn auto_detect_audit_base_ref(root: &Path) -> Option<ResolvedAuditBase> {
    if let Some(upstream) = git_upstream_ref(root) {
        if let Some(sha) = git_merge_base(root, &upstream, "HEAD") {
            return Some(ResolvedAuditBase {
                git_ref: sha,
                description: Some(format!("merge-base with {upstream}")),
            });
        }
        return Some(ResolvedAuditBase {
            description: Some(format!("{upstream} (tip)")),
            git_ref: upstream,
        });
    }

    if let Some(remote_ref) = detect_remote_default_ref(root) {
        if let Some(sha) = git_merge_base(root, &remote_ref, "HEAD") {
            return Some(ResolvedAuditBase {
                git_ref: sha,
                description: Some(format!("merge-base with {remote_ref}")),
            });
        }
        return Some(ResolvedAuditBase {
            description: Some(format!("{remote_ref} (tip)")),
            git_ref: remote_ref,
        });
    }

    for candidate in ["main", "master"] {
        if git_ref_exists(root, candidate) {
            return Some(ResolvedAuditBase {
                git_ref: candidate.to_string(),
                description: Some(format!("local {candidate}")),
            });
        }
    }

    None
}

/// Short SHA for the current HEAD.
#[must_use]
pub fn short_head_sha(root: &Path) -> Option<String> {
    run_git(root, &["rev-parse", "--short", "HEAD"])
}

/// Resolve a concrete `--changed-workspaces` ref for project-level next steps.
///
/// Returns `None` when the project has no workspaces, is not a git repository,
/// or has no resolvable remote default branch.
#[must_use]
pub fn default_workspace_ref(root: &Path) -> Option<String> {
    let workspaces = crate::discover::discover_workspace_packages(root);
    default_workspace_ref_for_workspaces(root, &workspaces)
}

/// Resolve a concrete `--changed-workspaces` ref using existing workspace data.
#[must_use]
pub fn default_workspace_ref_for_workspaces(
    root: &Path,
    workspaces: &[WorkspaceInfo],
) -> Option<String> {
    if workspaces.is_empty() || !crate::churn::is_git_repo(root) {
        return None;
    }
    run_git(
        root,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    )
    .or_else(|| {
        ["origin/main", "origin/master"]
            .into_iter()
            .find(|candidate| git_ref_exists(root, candidate))
            .map(str::to_owned)
    })
}

/// Git identities for the current user in forms useful for self-routing.
///
/// Includes `user.email`, its local-part handle, a GitHub no-reply unwrapped
/// handle when applicable, and `user.name`. Missing config values are ignored.
#[must_use]
pub fn current_user_identities(root: &Path) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(email) = read_git_config(root, "user.email") {
        if let Some((local, _)) = email.split_once('@') {
            ids.push(local.rsplit('+').next().unwrap_or(local).to_owned());
        }
        ids.push(email);
    }
    if let Some(name) = read_git_config(root, "user.name") {
        ids.push(name);
    }
    ids
}

fn read_git_config(root: &Path, key: &str) -> Option<String> {
    run_git(root, &["config", "--get", key])
}

fn git_ref_exists(root: &Path, reference: &str) -> bool {
    run_git(root, &["rev-parse", "--verify", "--quiet", reference]).is_some()
}

/// The repository top level as a real path.
///
/// Git resolves symbolic links in the toplevel it reports on every host
/// checked, so the extra canonicalization is a by-construction guard rather
/// than a behavior change. It keeps both sides of the prefix comparison in
/// `base_analysis_root` in one path space.
fn git_toplevel(root: &Path) -> Option<PathBuf> {
    let toplevel = PathBuf::from(run_git(root, &["rev-parse", "--show-toplevel"])?);
    Some(dunce::canonicalize(&toplevel).unwrap_or(toplevel))
}

fn git_upstream_ref(root: &Path) -> Option<String> {
    run_git(
        root,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
    )
}

fn git_merge_base(root: &Path, a: &str, b: &str) -> Option<String> {
    run_git(root, &["merge-base", a, b])
}

fn detect_remote_default_ref(root: &Path) -> Option<String> {
    if let Some(full_ref) = run_git(root, &["symbolic-ref", "refs/remotes/origin/HEAD"])
        && let Some(branch) = full_ref.strip_prefix("refs/remotes/origin/")
    {
        return Some(format!("origin/{branch}"));
    }
    ["origin/main", "origin/master"]
        .into_iter()
        .find(|candidate| git_ref_exists(root, candidate))
        .map(str::to_string)
}

fn base_worktree_path() -> EngineResult<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|err| EngineError::new(format!("system clock before unix epoch: {err}")))?
        .as_nanos();
    Ok(std::env::temp_dir().join(base_worktree_name(nanos)))
}

/// Compose the directory name for a base worktree taken at clock read `nanos`.
///
/// The pid stays the FIRST `-`-separated segment so the CLI orphan sweep keeps
/// parsing it. A process-global monotonic counter is the final segment: `nanos`
/// is NOT monotonic and repeats across threads, so two audits running
/// concurrently in one process could otherwise compose the same name and the
/// loser's `git worktree add` fails with "already exists". `nanos` is a
/// parameter so that collision is reproducible in a test without depending on
/// the host clock resolution.
fn base_worktree_name(nanos: u128) -> String {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("fallow-audit-base-{}-{nanos}-{seq}", std::process::id())
}

#[expect(
    clippy::disallowed_methods,
    reason = "canonical engine-owned git spawn wrapper for repository refs"
)]
fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    crate::changed_files::clear_ambient_git_env(&mut command);
    // Repository probes never consume input and must not retain an embedder's protocol stdin.
    command.stdin(Stdio::null()).arg("-C").arg(root);
    command
}

/// Run `git <args>` in `root` and return trimmed, non-empty stdout, or `None`
/// on a non-zero exit, empty output, or non-UTF-8 output.
///
/// Trimming belongs to this contract: git terminates every line it prints, and
/// callers feed these values straight back to git as refs and compare them as
/// paths, where a trailing newline is rejected or silently mismatches. Non-UTF-8
/// output stays `None` rather than becoming a mangled ref or path.
fn run_git(root: &Path, args: &[&str]) -> Option<String> {
    let output = git_command(root).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    use super::*;

    fn git(root: &Path, args: &[&str]) -> String {
        let output = git_command(root)
            .args(args)
            .output()
            .expect("git command starts");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn init_repo(root: &Path) {
        fs::create_dir_all(root).expect("create repo");
        git(root, &["init", "-b", "main"]);
        git(root, &["config", "user.name", "Test User"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "commit.gpgsign", "false"]);
    }

    fn commit_all(root: &Path, message: &str) {
        git(root, &["add", "."]);
        git(root, &["commit", "-m", message]);
    }

    /// A repository on `main` with one seed commit and no remote.
    fn seeded_repo(parent: &Path) -> PathBuf {
        let root = parent.join("repo");
        init_repo(&root);
        fs::write(root.join("README.md"), "seed\n").expect("write seed");
        commit_all(&root, "initial");
        root
    }

    /// Add a tracked file, commit it, and return the new HEAD SHA.
    fn commit_file(repo: &Path, name: &str, body: &str) -> String {
        fs::write(repo.join(name), body).expect("write file");
        commit_all(repo, name);
        git(repo, &["rev-parse", "HEAD"])
    }

    #[cfg(unix)]
    fn write_executable(path: &Path, source: &str) {
        use std::os::unix::fs::PermissionsExt as _;

        fs::write(path, source).expect("write executable");
        let mut permissions = fs::metadata(path)
            .expect("executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("set executable mode");
    }

    /// Concurrent callers whose clock reads land in the same tick must still
    /// each get a distinct name. Before the monotonic counter they composed the
    /// identical name, so the second `git worktree add` failed with "already
    /// exists" and the audit aborted with `FALLOW_AUDIT_BASE_WORKTREE_FAILED`.
    ///
    /// The tick is pinned rather than sampled: a real `SystemTime` read is fine
    /// enough on most hosts that the collision would surface only as a rare
    /// flake, which is exactly the failure this guards.
    #[test]
    fn base_worktree_names_are_unique_when_the_clock_read_repeats() {
        const N: usize = 64;
        const SAME_TICK: u128 = 1_788_187_156_297_209_000;

        let barrier = std::sync::Barrier::new(N);
        let names = std::sync::Mutex::new(Vec::with_capacity(N));
        std::thread::scope(|scope| {
            for _ in 0..N {
                let barrier = &barrier;
                let names = &names;
                scope.spawn(move || {
                    barrier.wait();
                    names
                        .lock()
                        .expect("names lock")
                        .push(base_worktree_name(SAME_TICK));
                });
            }
        });

        let mut names = names.into_inner().expect("names lock");
        assert_eq!(names.len(), N);
        names.sort();
        names.dedup();
        assert_eq!(names.len(), N, "base worktree names collided");
    }

    /// The pid stays the first segment so the CLI orphan sweep keeps parsing it.
    #[test]
    fn base_worktree_path_keeps_the_pid_as_the_first_segment() {
        let path = base_worktree_path().expect("path should build");
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .expect("worktree name should be utf-8");
        let pid = name
            .strip_prefix("fallow-audit-base-")
            .and_then(|rest| rest.split('-').next())
            .expect("pid segment should be present");
        assert_eq!(pid, std::process::id().to_string());
    }

    /// A subdirectory analysis root only materializes its own subtree (plus
    /// top-level files). Without this, a sparse checkout of one subdirectory
    /// of a large monorepo materializes the whole monorepo, and on a blobless
    /// partial clone each out-of-cone blob triggers a lazy promisor fetch that
    /// presents as `fallow audit` hanging to the CI timeout (issue #2615).
    #[test]
    fn detached_worktree_from_a_subdir_skips_sibling_subtrees() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::create_dir_all(repo.join("sub")).expect("create sub dir");
        fs::create_dir_all(repo.join("big")).expect("create big dir");
        fs::write(repo.join("sub/a.ts"), "export const a = 1;\n").expect("write sub file");
        fs::write(repo.join("big/b.ts"), "export const b = 1;\n").expect("write big file");
        fs::write(repo.join("top.ts"), "export const top = 1;\n").expect("write top file");
        commit_all(&repo, "initial");

        let destination = temp.path().join("base");
        create_detached_base_worktree(&repo.join("sub"), &destination, "HEAD")
            .expect("base worktree should be created");

        assert!(
            destination.join("sub/a.ts").is_file(),
            "the requested subtree must be materialized"
        );
        assert!(
            destination.join("top.ts").is_file(),
            "top-level files shape subdir discovery and stay materialized"
        );
        assert!(
            !destination.join("big/b.ts").exists(),
            "sibling subtrees must not be materialized: {}",
            destination.join("big/b.ts").display()
        );

        remove_registered_worktree(&repo, &destination);
        let _ = fs::remove_dir_all(&destination);
    }

    /// A repository-root run on a sparse checkout materializes the cone, not
    /// the whole monorepo. This is the `actions/checkout` sparse-checkout
    /// shape from issue #2615: cone mode lists the sparse directory, and the
    /// blobless partial clone has no out-of-cone blobs locally.
    #[test]
    fn detached_worktree_at_the_root_respects_the_sparse_cone() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::create_dir_all(repo.join("sub")).expect("create sub dir");
        fs::create_dir_all(repo.join("big")).expect("create big dir");
        fs::write(repo.join("sub/a.ts"), "export const a = 1;\n").expect("write sub file");
        fs::write(repo.join("big/b.ts"), "export const b = 1;\n").expect("write big file");
        commit_all(&repo, "initial");
        git(&repo, &["sparse-checkout", "init", "--cone"]);
        git(&repo, &["sparse-checkout", "set", "sub"]);

        let destination = temp.path().join("base");
        create_detached_base_worktree(&repo, &destination, "HEAD")
            .expect("base worktree should be created");

        assert!(
            destination.join("sub/a.ts").is_file(),
            "the sparse cone must be materialized"
        );
        assert!(
            !destination.join("big/b.ts").exists(),
            "paths outside the sparse cone must not be materialized: {}",
            destination.join("big/b.ts").display()
        );

        remove_registered_worktree(&repo, &destination);
        let _ = fs::remove_dir_all(&destination);
    }

    /// Pure scope unit coverage: subdir runs keep their subtree plus top-level
    /// and ancestor ignore files; root sparse runs keep the cone; full clones
    /// keep everything.
    #[test]
    fn materialization_scope_filters_to_the_needed_working_set() {
        let subdir = MaterializationScope {
            subdir_prefix: Some("apps/web".to_string()),
            sparse_dirs: None,
        };
        assert!(subdir.should_materialize(Path::new("apps/web/a.ts")));
        assert!(subdir.should_materialize(Path::new("top.ts")));
        assert!(subdir.should_materialize(Path::new(".gitignore")));
        assert!(subdir.should_materialize(Path::new("apps/.gitignore")));
        assert!(!subdir.should_materialize(Path::new("apps/other/b.ts")));
        assert!(!subdir.should_materialize(Path::new("apps/.gitignore.bak")));

        let sparse = MaterializationScope {
            subdir_prefix: None,
            sparse_dirs: Some(vec!["apps/web".to_string()]),
        };
        assert!(sparse.should_materialize(Path::new("apps/web/a.ts")));
        assert!(sparse.should_materialize(Path::new("top.ts")));
        assert!(!sparse.should_materialize(Path::new("apps/other/b.ts")));

        let full = MaterializationScope {
            subdir_prefix: None,
            sparse_dirs: None,
        };
        assert!(full.should_materialize(Path::new("apps/other/b.ts")));
    }

    #[test]
    fn default_workspace_ref_skips_projects_without_workspaces() {
        assert!(default_workspace_ref_for_workspaces(Path::new("/repo"), &[]).is_none());
    }

    #[test]
    fn default_workspace_ref_skips_non_git_workspace_projects() {
        let workspace = WorkspaceInfo {
            root: PathBuf::from("/repo/packages/app"),
            name: "app".to_owned(),
            is_internal_dependency: false,
        };

        assert!(default_workspace_ref_for_workspaces(Path::new("/repo"), &[workspace]).is_none());
    }

    #[test]
    fn current_user_identities_empty_when_git_config_is_unavailable() {
        assert!(current_user_identities(Path::new("/repo")).is_empty());
    }

    #[test]
    fn short_head_sha_omits_git_line_ending() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::write(repo.join("tracked.txt"), "committed\n").expect("write tracked file");
        commit_all(&repo, "initial");

        let sha = short_head_sha(&repo).expect("HEAD sha");
        assert_eq!(sha, sha.trim());
        assert!(!sha.is_empty());
    }

    #[test]
    fn short_head_sha_is_absent_outside_a_git_repo() {
        let temp = tempfile::tempdir().expect("temp dir");

        assert_eq!(short_head_sha(temp.path()), None);
    }

    /// Regression for issue #2699: the detected ref is handed straight back to
    /// git as a diff target, so it must carry no line ending. Without the
    /// trimmed probe contract the upstream is `origin/main\n`, the merge-base
    /// call against it fails, and the detection degrades to the tip branch with
    /// an unusable ref.
    #[test]
    fn auto_detect_audit_base_ref_omits_git_line_endings_for_the_upstream_merge_base() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::write(repo.join("tracked.txt"), "committed\n").expect("write tracked file");
        commit_all(&repo, "initial");
        let fork_point = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["remote", "add", "origin", &repo.to_string_lossy()]);
        git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
        git(&repo, &["checkout", "-b", "feature"]);
        git(
            &repo,
            &["branch", "--set-upstream-to=origin/main", "feature"],
        );
        fs::write(repo.join("feature.txt"), "my change\n").expect("write feature file");
        commit_all(&repo, "feature");

        let detected = auto_detect_audit_base_ref(&repo).expect("base is detected");

        assert_eq!(detected.git_ref, fork_point);
        assert_eq!(
            detected.description.as_deref(),
            Some("merge-base with origin/main")
        );
        assert!(crate::validate::validate_git_ref(&detected.git_ref).is_ok());
    }

    /// Regression for issue #2699 on the remote-default branch of the
    /// detection, where the line ending survives `strip_prefix` and reappears
    /// inside the composed `origin/<branch>` ref.
    #[test]
    fn auto_detect_audit_base_ref_omits_git_line_endings_for_the_remote_default() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::write(repo.join("tracked.txt"), "committed\n").expect("write tracked file");
        commit_all(&repo, "initial");
        let fork_point = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
        git(
            &repo,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );

        let detected = auto_detect_audit_base_ref(&repo).expect("base is detected");

        assert_eq!(detected.git_ref, fork_point);
        assert_eq!(
            detected.description.as_deref(),
            Some("merge-base with origin/main")
        );
        assert!(crate::validate::validate_git_ref(&detected.git_ref).is_ok());
    }

    #[test]
    fn auto_detect_audit_base_ref_resolves_origin_default_to_merge_base() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = seeded_repo(temp.path());
        let head = git(&repo, &["rev-parse", "HEAD"]);
        git(&repo, &["branch", "trunk"]);
        git(&repo, &["update-ref", "refs/remotes/origin/trunk", "trunk"]);
        git(
            &repo,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/trunk",
            ],
        );

        let detected = auto_detect_audit_base_ref(&repo).expect("base is detected");

        // trunk == HEAD, so the merge-base is HEAD's own SHA. The bare branch
        // name `trunk` is never returned: it would resolve to a local ref.
        assert_eq!(detected.git_ref, head);
        assert_eq!(
            detected.description.as_deref(),
            Some("merge-base with origin/trunk")
        );
    }

    /// Regression for issue #1168: a worktree checkout whose local `main` is
    /// stale relative to a fresh `origin/main`. The base must be the fork point
    /// (merge-base with `origin/main`), NOT the stale local-`main` commit that
    /// the old bare-name resolution diffed against.
    #[test]
    fn auto_detect_audit_base_ref_ignores_stale_local_main() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = seeded_repo(temp.path());
        let stale = git(&repo, &["rev-parse", "HEAD"]);

        git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
        git(
            &repo,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        let fork_point = commit_file(&repo, "teammate.txt", "merged work\n");
        git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);

        // Cut a feature branch from the fresh origin tip using the raw SHA (no
        // upstream tracking), then leave local `main` behind at the stale commit.
        git(&repo, &["checkout", "-b", "feature", &fork_point]);
        commit_file(&repo, "feature.txt", "my change\n");
        git(&repo, &["branch", "-f", "main", &stale]);

        let detected = auto_detect_audit_base_ref(&repo).expect("base is detected");

        assert_eq!(
            detected.git_ref, fork_point,
            "base must be the fork point (origin/main), not stale local main"
        );
        assert_eq!(
            detected.description.as_deref(),
            Some("merge-base with origin/main")
        );
    }

    #[test]
    fn auto_detect_audit_base_ref_prefers_configured_upstream() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = seeded_repo(temp.path());
        let fork_point = git(&repo, &["rev-parse", "HEAD"]);
        // Configure `origin` so refs/remotes/origin/* are recognized as
        // tracking refs and `--set-upstream-to` is accepted.
        git(&repo, &["remote", "add", "origin", &repo.to_string_lossy()]);
        git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
        git(&repo, &["checkout", "-b", "feature"]);
        git(
            &repo,
            &["branch", "--set-upstream-to=origin/main", "feature"],
        );
        commit_file(&repo, "feature.txt", "my change\n");

        let detected = auto_detect_audit_base_ref(&repo).expect("base is detected");

        assert_eq!(detected.git_ref, fork_point);
        assert_eq!(
            detected.description.as_deref(),
            Some("merge-base with origin/main")
        );
    }

    #[test]
    fn auto_detect_audit_base_ref_falls_back_to_local_main_without_remote() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = seeded_repo(temp.path());

        let detected = auto_detect_audit_base_ref(&repo).expect("base is detected");

        assert_eq!(detected.git_ref, "main");
        assert_eq!(detected.description.as_deref(), Some("local main"));
    }

    #[test]
    fn auto_detect_audit_base_ref_falls_back_to_local_master_without_remote() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        fs::create_dir_all(&repo).expect("create repo");
        git(&repo, &["init", "-b", "master"]);
        git(&repo, &["config", "user.name", "Test User"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        fs::write(repo.join("README.md"), "seed\n").expect("write seed");
        commit_all(&repo, "initial");

        let detected = auto_detect_audit_base_ref(&repo).expect("base is detected");

        assert_eq!(detected.git_ref, "master");
        assert_eq!(detected.description.as_deref(), Some("local master"));
    }

    #[test]
    fn auto_detect_audit_base_ref_returns_none_outside_git_repo() {
        let temp = tempfile::tempdir().expect("temp dir");

        assert!(auto_detect_audit_base_ref(temp.path()).is_none());
    }

    /// When the remote default shares no history with HEAD (the merge-base
    /// failure a shallow clone also hits), auto-detect falls back to the
    /// remote-tracking ref tip rather than failing the detection. That tip is
    /// the only branch that returns a ref git composed rather than printed, so
    /// it also pins the trimming for issue #2699.
    #[test]
    fn auto_detect_audit_base_ref_falls_back_to_remote_tip_without_common_ancestor() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = seeded_repo(temp.path());
        git(&repo, &["checkout", "--orphan", "unrelated"]);
        let unrelated = commit_file(&repo, "unrelated.txt", "no shared history\n");
        git(
            &repo,
            &["update-ref", "refs/remotes/origin/main", &unrelated],
        );
        git(
            &repo,
            &[
                "symbolic-ref",
                "refs/remotes/origin/HEAD",
                "refs/remotes/origin/main",
            ],
        );
        git(&repo, &["checkout", "main"]);

        let detected = auto_detect_audit_base_ref(&repo).expect("base is detected");

        assert_eq!(detected.git_ref, "origin/main");
        assert_eq!(detected.description.as_deref(), Some("origin/main (tip)"));
    }

    /// The repository top level is compared as a path prefix, so a line ending
    /// on it makes every subdirectory root fall back to the whole base
    /// worktree instead of the matching subdirectory (issue #2699).
    #[test]
    fn base_analysis_root_preserves_repo_subdirectory_roots() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        let app_root = repo.join("apps").join("mobile");
        fs::create_dir_all(&app_root).expect("create app root");
        let base_worktree = temp.path().join("base-worktree");

        assert_eq!(
            base_analysis_root(&app_root, &base_worktree),
            base_worktree.join("apps").join("mobile")
        );
    }

    /// A caller can spell the analysis root through a symbolic link, and git
    /// reports the resolved top level. The two path spaces must meet, or the
    /// prefix comparison fails and the base snapshot covers the whole base
    /// worktree while the head snapshot stays scoped (issue #2740).
    #[cfg(unix)]
    #[test]
    fn base_analysis_root_maps_a_symlinked_root_spelling() {
        let temp = tempfile::tempdir().expect("temp dir");
        let real_parent = temp.path().join("real");
        let repo = real_parent.join("repo");
        init_repo(&repo);
        let app_root = repo.join("apps").join("mobile");
        fs::create_dir_all(&app_root).expect("create app root");
        let linked_parent = temp.path().join("linked");
        std::os::unix::fs::symlink(&real_parent, &linked_parent).expect("link the parent");
        let base_worktree = temp.path().join("base-worktree");

        let linked_app_root = linked_parent.join("repo").join("apps").join("mobile");
        assert_eq!(
            base_analysis_root(&linked_app_root, &base_worktree),
            base_worktree.join("apps").join("mobile")
        );
    }

    /// Auditing a package added on the branch is the ordinary case where the
    /// remapped root is absent from the base worktree. Consumers need that
    /// reported rather than validating the joined path and refusing the call
    /// (issue #2699).
    #[test]
    fn resolve_base_analysis_root_reports_a_root_absent_from_the_base() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        let existing_root = repo.join("apps").join("mobile");
        fs::create_dir_all(&existing_root).expect("create existing root");
        let new_root = repo.join("apps").join("new");
        fs::create_dir_all(&new_root).expect("create new root");

        let base_worktree = temp.path().join("base-worktree");
        fs::create_dir_all(base_worktree.join("apps").join("mobile"))
            .expect("create base subdirectory");

        assert_eq!(
            resolve_base_analysis_root(&existing_root, &base_worktree),
            BaseAnalysisRoot::Present(base_worktree.join("apps").join("mobile"))
        );
        assert_eq!(
            resolve_base_analysis_root(&new_root, &base_worktree),
            BaseAnalysisRoot::NewInHead(base_worktree.join("apps").join("new"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn temporary_base_worktree_does_not_run_post_checkout_hook() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::write(repo.join("tracked.txt"), "committed\n").expect("write tracked file");
        commit_all(&repo, "initial");

        let sentinel = temp.path().join("post-checkout-ran");
        write_executable(
            &repo.join(".git/hooks/post-checkout"),
            &format!("#!/bin/sh\nprintf ran > '{}'\n", sentinel.display()),
        );

        let worktree = TemporaryBaseWorktree::create(&repo, "HEAD")
            .expect("temporary worktree should be created");

        assert_eq!(
            fs::read_to_string(worktree.path().join("tracked.txt")).expect("read tracked file"),
            "committed\n"
        );
        assert!(
            !sentinel.exists(),
            "creating a base view must not execute post-checkout hooks"
        );
    }

    #[cfg(unix)]
    #[test]
    fn temporary_base_worktree_does_not_run_post_index_change_hook() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::write(repo.join("tracked.txt"), "committed\n").expect("write tracked file");
        commit_all(&repo, "initial");

        let sentinel = temp.path().join("post-index-change-ran");
        write_executable(
            &repo.join(".git/hooks/post-index-change"),
            &format!("#!/bin/sh\nprintf ran > '{}'\n", sentinel.display()),
        );

        let worktree = TemporaryBaseWorktree::create(&repo, "HEAD")
            .expect("temporary worktree should be created");

        assert_eq!(
            fs::read_to_string(worktree.path().join("tracked.txt")).expect("read tracked file"),
            "committed\n"
        );
        assert!(
            !sentinel.exists(),
            "creating a base view must not execute post-index-change hooks"
        );
    }

    #[cfg(unix)]
    #[test]
    fn temporary_base_worktree_does_not_run_smudge_filter() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::write(
            repo.join(".gitattributes"),
            "filtered.txt filter=sentinel\n",
        )
        .expect("write attributes");
        fs::write(repo.join("filtered.txt"), "committed raw bytes\n").expect("write filtered file");
        commit_all(&repo, "initial");

        let sentinel = temp.path().join("smudge-ran");
        let filter = temp.path().join("smudge-filter.sh");
        write_executable(
            &filter,
            &format!(
                "#!/bin/sh\nprintf ran > '{}'\ncat >/dev/null\nprintf 'smudged bytes\\n'\n",
                sentinel.display()
            ),
        );
        git(
            &repo,
            &[
                "config",
                "filter.sentinel.smudge",
                filter.to_str().expect("filter path is UTF-8"),
            ],
        );

        let worktree = TemporaryBaseWorktree::create(&repo, "HEAD")
            .expect("temporary worktree should be created");

        assert_eq!(
            fs::read(worktree.path().join("filtered.txt")).expect("read filtered file"),
            b"committed raw bytes\n"
        );
        assert!(
            !sentinel.exists(),
            "creating a base view must not execute smudge filters"
        );
    }

    #[cfg(unix)]
    #[test]
    fn temporary_base_worktree_does_not_start_process_filter() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::write(
            repo.join(".gitattributes"),
            "filtered.txt filter=sentinel\n",
        )
        .expect("write attributes");
        fs::write(repo.join("filtered.txt"), "committed raw bytes\n").expect("write filtered file");
        commit_all(&repo, "initial");

        let sentinel = temp.path().join("process-filter-ran");
        let filter = temp.path().join("process-filter.sh");
        write_executable(
            &filter,
            &format!("#!/bin/sh\nprintf ran > '{}'\nexit 1\n", sentinel.display()),
        );
        git(
            &repo,
            &[
                "config",
                "filter.sentinel.process",
                filter.to_str().expect("filter path is UTF-8"),
            ],
        );

        let worktree = TemporaryBaseWorktree::create(&repo, "HEAD")
            .expect("temporary worktree should be created");

        assert_eq!(
            fs::read(worktree.path().join("filtered.txt")).expect("read filtered file"),
            b"committed raw bytes\n"
        );
        assert!(
            !sentinel.exists(),
            "creating a base view must not start process filters"
        );
    }

    #[test]
    fn failed_registration_does_not_remove_existing_worktree() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::write(repo.join("tracked.txt"), "committed\n").expect("write tracked file");
        commit_all(&repo, "initial");
        let destination = temp.path().join("base");

        create_detached_base_worktree(&repo, &destination, "HEAD")
            .expect("first worktree should be created");
        let second = create_detached_base_worktree(&repo, &destination, "HEAD");

        assert!(second.is_err(), "duplicate destination must fail");
        assert!(
            destination.join("tracked.txt").is_file(),
            "failed registration must not remove the existing worktree"
        );
        assert_eq!(git(&destination, &["rev-parse", "HEAD"]).len(), 40);

        remove_registered_worktree(&repo, &destination);
        let _ = fs::remove_dir_all(destination);
    }

    #[cfg(unix)]
    #[test]
    fn temporary_base_worktree_preserves_modes_symlinks_and_gitlinks() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::write(repo.join("regular.txt"), "regular\n").expect("write regular file");
        let executable = repo.join("run.sh");
        fs::write(&executable, "#!/bin/sh\nexit 0\n").expect("write executable");
        let mut permissions = fs::metadata(&executable)
            .expect("executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&executable, permissions).expect("set executable mode");
        std::os::unix::fs::symlink("regular.txt", repo.join("regular-link"))
            .expect("create symlink");
        commit_all(&repo, "files");

        let gitlink_commit = git(&repo, &["rev-parse", "HEAD"]);
        git(
            &repo,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("160000,{gitlink_commit},vendor/submodule"),
            ],
        );
        git(&repo, &["commit", "-m", "gitlink"]);

        let worktree = TemporaryBaseWorktree::create(&repo, "HEAD")
            .expect("temporary worktree should be created");
        let regular_mode = fs::metadata(worktree.path().join("regular.txt"))
            .expect("regular metadata")
            .mode();
        let executable_mode = fs::metadata(worktree.path().join("run.sh"))
            .expect("executable metadata")
            .mode();

        assert_eq!(regular_mode & 0o111, 0);
        assert_ne!(executable_mode & 0o111, 0);
        assert_eq!(
            fs::read_link(worktree.path().join("regular-link")).expect("read symlink"),
            PathBuf::from("regular.txt")
        );
        let gitlink = worktree.path().join("vendor/submodule");
        assert!(gitlink.is_dir(), "gitlink must materialize as a directory");
        assert!(
            fs::read_dir(gitlink)
                .expect("read gitlink directory")
                .next()
                .is_none(),
            "an uninitialized gitlink directory must remain empty"
        );
        assert!(
            git(
                worktree.path(),
                &["ls-files", "--stage", "vendor/submodule"]
            )
            .starts_with(&format!("160000 {gitlink_commit} 0\t")),
            "the linked worktree index must retain the gitlink object id"
        );

        let path = worktree.path().to_path_buf();
        drop(worktree);
        assert!(!path.exists(), "temporary worktree must clean up on drop");
    }

    #[test]
    fn materialized_tree_paths_reject_traversal_and_git_admin_aliases() {
        for path in [
            Path::new("../escape"),
            Path::new("/absolute"),
            Path::new(".git/config"),
            Path::new("nested/.GIT/config"),
            Path::new("nested/.git. /config"),
            Path::new("nested/git~1/config"),
        ] {
            assert!(
                validate_materialized_path(path).is_err(),
                "unsafe path should be rejected: {}",
                path.display()
            );
        }
        assert!(validate_materialized_path(Path::new("src/.github/file.ts")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn parent_directory_creation_refuses_symlink_traversal() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path().join("root");
        let outside = temp.path().join("outside");
        fs::create_dir(&root).expect("create root");
        fs::create_dir(&outside).expect("create outside");
        std::os::unix::fs::symlink(&outside, root.join("link")).expect("create parent symlink");

        let result = create_safe_parent_directories(&root, Path::new("link/escaped.txt"));

        assert!(result.is_err(), "symlink parent must be rejected");
        assert!(!outside.join("escaped.txt").exists());
    }

    #[test]
    fn audit_context_fingerprint_tracks_bounded_lockfiles_and_markers() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path();
        fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: 9\n").expect("lockfile");
        fs::create_dir(root.join("node_modules")).expect("node_modules");
        fs::write(
            root.join("node_modules/.modules.yaml"),
            "layoutVersion: 5\n",
        )
        .expect("node marker");

        let first = audit_materialized_context_fingerprint(root);
        let unchanged = audit_materialized_context_fingerprint(root);
        assert_eq!(
            first, unchanged,
            "unchanged context must preserve a warm key"
        );

        fs::write(root.join("pnpm-lock.yaml"), "lockfileVersion: 10\n").expect("mutate lockfile");
        let lock_changed = audit_materialized_context_fingerprint(root);
        assert_ne!(
            first, lock_changed,
            "lockfile content must invalidate the key"
        );

        fs::write(
            root.join("node_modules/.modules.yaml"),
            "layoutVersion: 6\n",
        )
        .expect("mutate node marker");
        let marker_changed = audit_materialized_context_fingerprint(root);
        assert_ne!(
            lock_changed, marker_changed,
            "bounded dependency markers must invalidate the key"
        );

        fs::create_dir(root.join(".nuxt")).expect("nuxt context");
        fs::write(root.join(".nuxt/imports.d.ts"), "export {}\n").expect("nuxt marker");
        assert_ne!(
            marker_changed,
            audit_materialized_context_fingerprint(root),
            "missing and materialized generated context must differ"
        );
    }

    #[test]
    fn audit_context_fingerprint_tracks_nested_workspace_generated_roots() {
        let temp = tempfile::tempdir().expect("temp dir");
        let root = temp.path();
        fs::write(
            root.join("package.json"),
            r#"{"private":true,"workspaces":["packages/*"]}"#,
        )
        .expect("root package");
        let nuxt = root.join("packages/nuxt-app");
        let astro = root.join("packages/astro-app");
        fs::create_dir_all(nuxt.join(".nuxt")).expect("nested nuxt context");
        fs::create_dir_all(astro.join(".astro")).expect("nested astro context");
        fs::write(nuxt.join("package.json"), r#"{"name":"nuxt-app"}"#).expect("nuxt package");
        fs::write(astro.join("package.json"), r#"{"name":"astro-app"}"#).expect("astro package");
        fs::write(nuxt.join(".nuxt/imports.d.ts"), "export {};\n").expect("nuxt marker");
        fs::write(astro.join(".astro/types.d.ts"), "export {};\n").expect("astro marker");

        let first = audit_materialized_context_fingerprint(root);
        assert!(
            first
                .directories
                .iter()
                .any(|directory| directory.name == "packages/nuxt-app/.nuxt")
        );
        assert!(
            first
                .directories
                .iter()
                .any(|directory| directory.name == "packages/astro-app/.astro")
        );

        fs::write(
            nuxt.join(".nuxt/imports.d.ts"),
            "export type Changed = true;\n",
        )
        .expect("mutate nuxt marker");
        assert_ne!(
            first,
            audit_materialized_context_fingerprint(root),
            "nested workspace marker changes must invalidate the audit context"
        );
    }

    #[cfg(unix)]
    #[test]
    fn materialize_base_context_symlinks_nested_workspace_generated_roots() {
        let host = tempfile::tempdir().expect("host");
        let worktree = tempfile::tempdir().expect("worktree");
        fs::write(
            host.path().join("package.json"),
            r#"{"private":true,"workspaces":["packages/*"]}"#,
        )
        .expect("root package");

        for (workspace, generated, marker) in [
            ("nuxt-app", ".nuxt", "imports.d.ts"),
            ("astro-app", ".astro", "types.d.ts"),
        ] {
            let host_workspace = host.path().join("packages").join(workspace);
            let worktree_workspace = worktree.path().join("packages").join(workspace);
            fs::create_dir_all(host_workspace.join(generated)).expect("host generated context");
            fs::create_dir_all(&worktree_workspace).expect("worktree workspace");
            fs::write(
                host_workspace.join("package.json"),
                format!(r#"{{"name":"{workspace}"}}"#),
            )
            .expect("workspace package");
            fs::write(host_workspace.join(generated).join(marker), "export {};\n")
                .expect("generated marker");
        }

        materialize_base_dependency_context(host.path(), worktree.path());

        for (workspace, generated, marker) in [
            ("nuxt-app", ".nuxt", "imports.d.ts"),
            ("astro-app", ".astro", "types.d.ts"),
        ] {
            let mirrored = worktree
                .path()
                .join("packages")
                .join(workspace)
                .join(generated);
            assert!(
                fs::symlink_metadata(&mirrored)
                    .expect("mirrored generated root")
                    .file_type()
                    .is_symlink(),
                "{workspace}/{generated} must reuse the host generated root"
            );
            assert!(mirrored.join(marker).is_file());
        }
    }

    #[cfg(unix)]
    #[test]
    fn materialize_base_context_resolves_symlinked_source_directories() {
        let host = tempfile::tempdir().expect("host");
        let targets = tempfile::tempdir().expect("targets");
        let worktree = tempfile::tempdir().expect("worktree");

        for (kind, marker) in [
            ("node_modules", ".modules.yaml"),
            (".nuxt", "imports.d.ts"),
            (".astro", "types.d.ts"),
        ] {
            let target = targets.path().join(kind);
            fs::create_dir(&target).expect("source target");
            fs::write(target.join(marker), "generated context\n").expect("context marker");
            std::os::unix::fs::symlink(&target, host.path().join(kind))
                .expect("source directory symlink");
        }

        materialize_base_dependency_context(host.path(), worktree.path());

        let fingerprint = audit_materialized_context_fingerprint(host.path());
        for kind in AUDIT_MATERIALIZED_CONTEXT_DIRS {
            let target = dunce::canonicalize(targets.path().join(kind)).expect("canonical target");
            let mirrored = worktree.path().join(kind);
            assert_eq!(
                fs::read_link(&mirrored).expect("materialized symlink"),
                target,
                "{kind} must link directly to the validated canonical target"
            );
            let directory = fingerprint
                .directories
                .iter()
                .find(|directory| directory.name == *kind)
                .expect("fingerprinted context directory");
            assert_eq!(directory.state, AuditContextPathState::Present);
            assert!(directory.markers.iter().any(|marker| {
                matches!(marker.state, AuditContextPathState::Present)
                    && marker.content_hash.is_some()
            }));
        }
    }

    #[cfg(unix)]
    #[test]
    fn materialize_base_context_refuses_symlinked_workspace_parent() {
        let host = tempfile::tempdir().expect("host");
        let worktree = tempfile::tempdir().expect("worktree");
        let outside = tempfile::tempdir().expect("outside");
        fs::write(
            host.path().join("package.json"),
            r#"{"private":true,"workspaces":["packages/*"]}"#,
        )
        .expect("root package");
        let host_workspace = host.path().join("packages/app");
        fs::create_dir_all(host_workspace.join(".nuxt")).expect("host generated context");
        fs::write(host_workspace.join("package.json"), r#"{"name":"app"}"#)
            .expect("workspace package");
        fs::write(host_workspace.join(".nuxt/imports.d.ts"), "export {};\n")
            .expect("generated marker");

        let outside_workspace = outside.path().join("app");
        fs::create_dir_all(&outside_workspace).expect("outside workspace");
        let outside_generated = outside_workspace.join(".nuxt");
        std::os::unix::fs::symlink("missing-target", &outside_generated)
            .expect("outside sentinel symlink");
        std::os::unix::fs::symlink(outside.path(), worktree.path().join("packages"))
            .expect("hostile workspace parent symlink");

        materialize_base_dependency_context(host.path(), worktree.path());

        assert_eq!(
            fs::read_link(&outside_generated).expect("sentinel symlink must survive"),
            PathBuf::from("missing-target")
        );
        assert!(
            !outside.path().join(".nuxt").exists(),
            "materialization must not create generated context outside the worktree"
        );
    }

    #[test]
    fn audit_context_fingerprint_rejects_oversized_files_without_reading_them() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("pnpm-lock.yaml");
        let file = File::create(&path).expect("oversized file");
        file.set_len(AUDIT_CONTEXT_FILE_MAX_BYTES.saturating_add(1))
            .expect("set oversized length");

        let fingerprint = fingerprint_context_file_at(&path, "pnpm-lock.yaml");

        assert_eq!(
            fingerprint.state,
            AuditContextPathState::Unreadable(CONTEXT_OVERSIZED_FILE_STATE.to_string())
        );
        assert!(fingerprint.source.is_some());
        assert!(fingerprint.content_hash.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn audit_context_fingerprint_rejects_symlinked_files_without_following_them() {
        let temp = tempfile::tempdir().expect("temp dir");
        let target = temp.path().join("target-lock.yaml");
        let link = temp.path().join("pnpm-lock.yaml");
        fs::write(&target, "secret target contents\n").expect("target file");
        std::os::unix::fs::symlink(&target, &link).expect("lockfile symlink");

        let fingerprint = fingerprint_context_file_at(&link, "pnpm-lock.yaml");

        assert_eq!(
            fingerprint.state,
            AuditContextPathState::Unreadable(CONTEXT_SYMLINK_STATE.to_string())
        );
        assert!(fingerprint.content_hash.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn audit_context_fingerprint_does_not_follow_symlink_swapped_before_open() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("pnpm-lock.yaml");
        let target = temp.path().join("target-lock.yaml");
        fs::write(&path, "original contents\n").expect("original file");
        fs::write(&target, "secret target contents\n").expect("target file");

        let fingerprint = fingerprint_context_file_at_with_hooks(
            &path,
            "pnpm-lock.yaml",
            || {
                fs::remove_file(&path).expect("remove original");
                std::os::unix::fs::symlink(&target, &path).expect("replacement symlink");
            },
            || {},
        );

        assert_eq!(
            fingerprint.state,
            AuditContextPathState::Unreadable(CONTEXT_SYMLINK_STATE.to_string())
        );
        assert!(fingerprint.content_hash.is_none());
    }

    #[test]
    fn audit_context_fingerprint_rejects_file_changed_during_read() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("pnpm-lock.yaml");
        fs::write(&path, "original contents\n").expect("original file");

        let fingerprint = fingerprint_context_file_at_with_hooks(
            &path,
            "pnpm-lock.yaml",
            || {},
            || {
                OpenOptions::new()
                    .write(true)
                    .open(&path)
                    .expect("open replacement")
                    .set_len(1)
                    .expect("truncate replacement");
            },
        );

        assert_eq!(
            fingerprint.state,
            AuditContextPathState::Unreadable(CONTEXT_CHANGED_DURING_READ_STATE.to_string())
        );
        assert!(fingerprint.content_hash.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn unix_context_open_does_not_block_on_fifo() {
        let temp = tempfile::tempdir().expect("temp dir");
        let fifo = temp.path().join("pnpm-lock.yaml");
        let status = Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("run mkfifo");
        assert!(status.success(), "mkfifo must create the test pipe");

        let fallback_fifo = fifo.clone();
        let fallback_writer = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(1));
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(fallback_fifo)
                .expect("open fallback FIFO writer")
        });
        let started = std::time::Instant::now();
        let fingerprint = fingerprint_context_file_at(&fifo, "pnpm-lock.yaml");
        let elapsed = started.elapsed();

        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "nonblocking FIFO open took {elapsed:?}"
        );
        assert_eq!(
            fingerprint.state,
            AuditContextPathState::Unreadable(CONTEXT_SPECIAL_FILE_STATE.to_string())
        );
        assert!(fingerprint.content_hash.is_none());
        drop(fallback_writer.join().expect("fallback writer"));
    }

    #[cfg(unix)]
    #[test]
    fn audit_context_fingerprint_rejects_special_files_without_opening_them() {
        use std::os::unix::net::UnixListener;

        let temp = tempfile::tempdir().expect("temp dir");
        let socket = temp.path().join("pnpm-lock.yaml");
        let _listener = UnixListener::bind(&socket).expect("unix socket");

        let fingerprint = fingerprint_context_file_at(&socket, "pnpm-lock.yaml");

        assert_eq!(
            fingerprint.state,
            AuditContextPathState::Unreadable(CONTEXT_SPECIAL_FILE_STATE.to_string())
        );
        assert!(fingerprint.content_hash.is_none());
    }
}
