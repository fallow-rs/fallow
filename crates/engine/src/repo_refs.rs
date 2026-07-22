//! Engine-owned repository reference probes and temporary repo views.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
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
    let marker = PathBuf::from(marker.trim());
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
    run_git(destination, &["rev-parse", "--verify", "HEAD^{commit}"])
        .map(|commit| commit.trim().to_owned())
        .ok_or_else(|| {
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

fn materialize_committed_tree(
    repo_root: &Path,
    destination: &Path,
    commit: &str,
) -> EngineResult<()> {
    let entries = committed_tree_entries(repo_root, commit)?;
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
#[must_use]
pub fn base_analysis_root(current_root: &Path, base_worktree_root: &Path) -> PathBuf {
    let Some(git_root) = git_toplevel(current_root) else {
        return base_worktree_root.to_path_buf();
    };
    let current_root =
        dunce::canonicalize(current_root).unwrap_or_else(|_| current_root.to_path_buf());
    match current_root.strip_prefix(&git_root) {
        Ok(relative) => base_worktree_root.join(relative),
        Err(_) => base_worktree_root.to_path_buf(),
    }
}

/// Auto-detect the base ref used by changed-code audit.
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
    if let Some(reference) = run_git(
        root,
        &[
            "symbolic-ref",
            "--quiet",
            "--short",
            "refs/remotes/origin/HEAD",
        ],
    ) {
        let reference = reference.trim();
        if !reference.is_empty() {
            return Some(reference.to_owned());
        }
    }
    ["origin/main", "origin/master"]
        .into_iter()
        .find(|candidate| git_ref_exists(root, candidate))
        .map(str::to_owned)
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
    let value = run_git(root, &["config", "--get", key])?;
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

fn git_ref_exists(root: &Path, reference: &str) -> bool {
    run_git(root, &["rev-parse", "--verify", "--quiet", reference]).is_some()
}

fn git_toplevel(root: &Path) -> Option<PathBuf> {
    run_git(root, &["rev-parse", "--show-toplevel"]).map(PathBuf::from)
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
    Ok(std::env::temp_dir().join(format!("fallow-audit-base-{}-{nanos}", std::process::id())))
}

#[expect(
    clippy::disallowed_methods,
    reason = "canonical engine-owned git spawn wrapper for repository refs"
)]
fn git_command(root: &Path) -> Command {
    let mut command = Command::new("git");
    crate::changed_files::clear_ambient_git_env(&mut command);
    command.arg("-C").arg(root);
    command
}

fn run_git(root: &Path, args: &[&str]) -> Option<String> {
    let output = git_command(root).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    use super::*;

    fn git(root: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .args(args)
            .current_dir(root)
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
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
