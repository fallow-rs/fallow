//! Engine-owned repository reference probes and temporary repo views.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::SystemTime;

use fallow_config::WorkspaceInfo;

use crate::{EngineError, EngineResult};

const RAW_MATERIALIZATION_MARKER: &str = "fallow-raw-materialized-v1";

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
        if terminator != [b'\n'] {
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
}
