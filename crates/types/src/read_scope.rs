//! The owner rule of the read scope base.
//!
//! The read scope base of a project (`fallow_config::project_scope_base`) can
//! be a directory above the analysis root, such as the Git work tree of a
//! package root. A directory above the root widens the scope only when the
//! current user owns it and other users cannot write to it. So a `/tmp/.git`
//! entry, or a workspace manifest in a shared directory, does not widen the
//! scope of a project below it.

use std::path::Path;

/// True when the current user owns `dir` and other users cannot write to it.
/// A metadata error counts as false. On Windows the rule does not apply, and
/// the result is true.
#[cfg(unix)]
#[must_use]
pub fn may_widen_scope(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    const WRITABLE_BY_ALL: u32 = 0o002;
    std::fs::metadata(dir).is_ok_and(|metadata| {
        metadata.uid() == rustix::process::geteuid().as_raw()
            && metadata.mode() & WRITABLE_BY_ALL == 0
    })
}

/// True when the current user owns `dir` and other users cannot write to it.
/// On Windows the rule does not apply, and the result is true.
#[cfg(not(unix))]
#[must_use]
pub fn may_widen_scope(_dir: &Path) -> bool {
    true
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_directory_that_all_users_can_write_does_not_widen_the_scope() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temp dir");
        assert!(may_widen_scope(dir.path()));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o777))
            .expect("chmod");
        assert!(!may_widen_scope(dir.path()));
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))
            .expect("chmod");
    }

    /// `/usr` belongs to the system user on each Unix host.
    #[test]
    fn a_directory_that_another_user_owns_does_not_widen_the_scope() {
        if rustix::process::geteuid().is_root() {
            return;
        }
        assert!(!may_widen_scope(Path::new("/usr")));
    }
}
