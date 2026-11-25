//! The owner rule of the read scope base.
//!
//! The read scope base of a project (`fallow_config::project_scope_base`) can
//! be a directory above the analysis root, such as the Git work tree of a
//! package root. A directory above the root widens the scope only when other
//! users cannot write to it and one of these users owns it:
//!
//! - the current user (the effective uid), or
//! - the user that owns the analysis root.
//!
//! So a CI job that runs as root on a checkout of another user still reads
//! the shared configs of that checkout. A `/tmp/.git` entry, or a workspace
//! manifest in a shared directory, does not widen the scope of a project
//! below it, because a different user owns it or all users can write to it.

use std::path::Path;

/// The mode bit that lets all users write to a directory.
#[cfg(unix)]
const WRITABLE_BY_ALL: u32 = 0o002;

/// True when the directory `dir` above the analysis root `root` may widen
/// the read scope: other users cannot write to `dir`, and the current user
/// or the owner of `root` owns `dir`. A metadata error counts as false. On
/// Windows the rule does not apply, and the result is true.
#[cfg(unix)]
#[must_use]
pub fn may_widen_scope(dir: &Path, root: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (Ok(dir_meta), Ok(root_meta)) = (std::fs::metadata(dir), std::fs::metadata(root)) else {
        return false;
    };
    owner_rule_allows(
        dir_meta.uid(),
        dir_meta.mode(),
        root_meta.uid(),
        rustix::process::geteuid().as_raw(),
    )
}

/// True when the directory `dir` above the analysis root `root` may widen
/// the read scope. On Windows the rule does not apply, and the result is
/// true.
#[cfg(not(unix))]
#[must_use]
pub fn may_widen_scope(_dir: &Path, _root: &Path) -> bool {
    true
}

/// The owner rule over the owner `dir_uid` and the mode `dir_mode` of a
/// directory, the owner `root_uid` of the analysis root, and the effective
/// uid `euid`.
#[cfg(unix)]
const fn owner_rule_allows(dir_uid: u32, dir_mode: u32, root_uid: u32, euid: u32) -> bool {
    dir_mode & WRITABLE_BY_ALL == 0 && (dir_uid == euid || dir_uid == root_uid)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    const PRIVATE_DIR: u32 = 0o040_755;
    const SHARED_DIR: u32 = 0o041_777;

    #[test]
    fn a_directory_of_the_current_user_widens_the_scope() {
        assert!(owner_rule_allows(501, PRIVATE_DIR, 1001, 501));
    }

    #[test]
    fn a_directory_of_the_root_owner_widens_the_scope_for_another_user() {
        assert!(owner_rule_allows(1001, PRIVATE_DIR, 1001, 0));
    }

    #[test]
    fn a_directory_of_a_third_user_does_not_widen_the_scope() {
        assert!(!owner_rule_allows(2002, PRIVATE_DIR, 1001, 0));
    }

    #[test]
    fn a_directory_that_all_users_can_write_never_widens_the_scope() {
        assert!(!owner_rule_allows(501, SHARED_DIR, 501, 501));
        assert!(!owner_rule_allows(1001, SHARED_DIR, 1001, 0));
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_directory_that_all_users_can_write_does_not_widen_the_scope() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("root");
        std::fs::create_dir(&root).expect("root dir");
        assert!(may_widen_scope(dir.path(), &root));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777))
            .expect("chmod");
        assert!(!may_widen_scope(dir.path(), &root));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("chmod");
    }

    /// `/usr` belongs to the system user on each Unix host.
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_directory_that_another_user_owns_does_not_widen_the_scope() {
        if rustix::process::geteuid().is_root() {
            return;
        }
        let root = tempfile::tempdir().expect("temp dir");
        assert!(!may_widen_scope(Path::new("/usr"), root.path()));
    }
}
