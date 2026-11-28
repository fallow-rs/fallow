//! The directory that holds the base snapshots of `fallow audit`.
//!
//! On Unix the temp dir is often shared by all users, for example `/tmp`.
//! Another user can create files there, also at the names that fallow uses
//! next to a snapshot. The snapshots therefore live in a directory of this
//! user inside the temp dir, with mode 0700. On other platforms the temp dir
//! is already private to the user, so the snapshots stay in it.

use std::path::{Path, PathBuf};

/// The directory for the audit snapshots of this user. This function does
/// not create or check the directory: use [`ensure_audit_snapshot_dir`]
/// before a snapshot is written.
#[must_use]
pub fn audit_snapshot_dir() -> PathBuf {
    audit_snapshot_dir_in(&std::env::temp_dir())
}

/// The name carries the effective user id, so users that share a temp dir
/// each have their own directory.
#[cfg(unix)]
fn audit_snapshot_dir_in(temp: &Path) -> PathBuf {
    temp.join(fallow_types::audit_snapshot::snapshot_dir_name())
}

#[cfg(not(unix))]
fn audit_snapshot_dir_in(temp: &Path) -> PathBuf {
    temp.to_path_buf()
}

/// Create the snapshot directory of this user when it is missing, check it,
/// and return its path.
///
/// # Errors
///
/// On Unix, returns an error when the directory cannot be created, or when
/// the path is a link, is not a directory, belongs to another user, or gives
/// access to other users. Fallow does not change or remove such a path.
pub fn ensure_audit_snapshot_dir() -> std::io::Result<PathBuf> {
    ensure_audit_snapshot_dir_in(&std::env::temp_dir())
}

/// The path of the snapshot `name` in the checked snapshot directory.
pub(crate) fn snapshot_path(name: &str) -> crate::EngineResult<PathBuf> {
    ensure_audit_snapshot_dir()
        .map(|dir| dir.join(name))
        .map_err(|error| crate::EngineError::new(error.to_string()))
}

#[cfg(unix)]
fn ensure_audit_snapshot_dir_in(temp: &Path) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt as _;

    let dir = audit_snapshot_dir_in(temp);
    // `mkdir` does not follow a link at the last component: a link there
    // gives `AlreadyExists`, and the check below refuses it.
    match std::fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    check_private_dir(&dir)?;
    Ok(dir)
}

#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "shared cross-platform signature; only the Unix directory can fail its check"
)]
fn ensure_audit_snapshot_dir_in(temp: &Path) -> std::io::Result<PathBuf> {
    Ok(audit_snapshot_dir_in(temp))
}

/// Refuse a path that is not a directory of this user with mode 0700 or
/// stricter. `symlink_metadata` does not follow a link, so a link at the
/// path is refused as a link.
#[cfg(unix)]
fn check_private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let metadata = std::fs::symlink_metadata(dir)?;
    let reason = if metadata.file_type().is_symlink() {
        Some("it is a link")
    } else if !metadata.file_type().is_dir() {
        Some("it is not a directory")
    } else if metadata.uid() != rustix::process::geteuid().as_raw() {
        Some("another user owns it")
    } else if metadata.permissions().mode() & 0o077 != 0 {
        Some("other users have access to it")
    } else {
        None
    };
    match reason {
        None => Ok(()),
        Some(reason) => Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "refusing to use the audit snapshot directory {} because {reason}; \
                 remove it, or set TMPDIR to a directory of your own",
                dir.display()
            ),
        )),
    }
}

/// The directories that a cache sweep scans for audit snapshots: the
/// snapshot directory of this user when it exists and passes the check, and
/// the temp dir, where earlier fallow versions kept the snapshots.
#[must_use]
pub fn audit_snapshot_scan_dirs() -> Vec<PathBuf> {
    audit_snapshot_scan_dirs_in(&std::env::temp_dir())
}

fn audit_snapshot_scan_dirs_in(temp: &Path) -> Vec<PathBuf> {
    let dir = audit_snapshot_dir_in(temp);
    let private = (dir != temp && private_dir_is_usable(&dir)).then_some(dir);
    private
        .into_iter()
        .chain(std::iter::once(temp.to_path_buf()))
        .collect()
}

#[cfg(unix)]
fn private_dir_is_usable(dir: &Path) -> bool {
    check_private_dir(dir).is_ok()
}

#[cfg(not(unix))]
const fn private_dir_is_usable(_dir: &Path) -> bool {
    false
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    #[test]
    fn ensure_creates_a_directory_that_only_this_user_can_open() {
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = ensure_audit_snapshot_dir_in(temp.path()).expect("directory is created");

        assert_eq!(dir.parent(), Some(temp.path()));
        let mode = std::fs::metadata(&dir)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o700);
        assert_eq!(
            ensure_audit_snapshot_dir_in(temp.path()).expect("an existing directory passes"),
            dir
        );
        assert_eq!(
            audit_snapshot_scan_dirs_in(temp.path()),
            vec![dir, temp.path().to_path_buf()]
        );
    }

    #[test]
    fn ensure_refuses_a_link_at_the_directory_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        let elsewhere = tempfile::tempdir().expect("link target");
        std::fs::set_permissions(elsewhere.path(), std::fs::Permissions::from_mode(0o700))
            .expect("chmod");
        let dir = audit_snapshot_dir_in(temp.path());
        std::os::unix::fs::symlink(elsewhere.path(), &dir).expect("link");

        let error = ensure_audit_snapshot_dir_in(temp.path()).expect_err("a link is refused");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(
            error.to_string().contains("because it is a link"),
            "{error}"
        );
        assert_eq!(
            audit_snapshot_scan_dirs_in(temp.path()),
            vec![temp.path().to_path_buf()]
        );
    }

    #[test]
    fn ensure_refuses_a_directory_that_other_users_can_open() {
        let temp = tempfile::tempdir().expect("temp dir");
        let dir = audit_snapshot_dir_in(temp.path());
        std::fs::create_dir(&dir).expect("pre-created directory");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).expect("chmod");

        let error = ensure_audit_snapshot_dir_in(temp.path()).expect_err("an open dir is refused");
        assert!(error.to_string().contains("other users"), "{error}");
        let mode = std::fs::metadata(&dir)
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o777, "fallow does not change the directory");
    }

    #[test]
    fn ensure_refuses_a_file_at_the_directory_path() {
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::write(audit_snapshot_dir_in(temp.path()), "").expect("file");

        assert!(ensure_audit_snapshot_dir_in(temp.path()).is_err());
    }
}
