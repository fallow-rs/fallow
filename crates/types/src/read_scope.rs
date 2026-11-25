//! The directory that the reads of a project may reach.
//!
//! A project can link a shared file from above its root, for example a
//! `jest.config.js` in a package that links to `../../jest.base.js` of the
//! monorepo. Such a target is still part of the checkout. The read scope base
//! is therefore the Git work tree that contains the root, with these
//! exceptions, where the base is the root itself:
//!
//! - The work tree is the home directory or a directory above it, for
//!   example a dotfiles repository or a `/.git` entry. With no home
//!   directory, a work tree at the file system root counts the same.
//! - The work tree directory is writable by all users, or another user owns
//!   it, for example a `/tmp/.git` entry.

use std::path::{Path, PathBuf};

/// The read scope base of `root`: the nearest directory at or above the
/// resolved root that holds a `.git` entry, or the resolved root itself when
/// there is none or when that directory may not widen the scope (see the
/// module documentation). The caller reads `home` from the host paths of the
/// process.
#[must_use]
pub fn read_scope_base(root: &Path, home: Option<&Path>) -> PathBuf {
    let home = home.map(|home| dunce::canonicalize(home).unwrap_or_else(|_| home.to_path_buf()));
    let Ok(root) = dunce::canonicalize(root) else {
        return root.to_path_buf();
    };
    let Some(top) = root
        .ancestors()
        .find(|dir| dir.join(".git").symlink_metadata().is_ok())
    else {
        return root;
    };
    if top == root || widens_scope(top, home.as_deref()) {
        return top.to_path_buf();
    }
    root
}

/// True when the work tree directory `top`, above the root, may widen the
/// read scope.
fn widens_scope(top: &Path, home: Option<&Path>) -> bool {
    let above_home = match home {
        Some(home) => home.starts_with(top),
        None => top.parent().is_none(),
    };
    !above_home && owned_by_this_user(top)
}

/// True when the current user owns `dir` and other users cannot write to it.
/// A metadata error counts as false.
#[cfg(unix)]
fn owned_by_this_user(dir: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    const WRITABLE_BY_ALL: u32 = 0o002;
    std::fs::metadata(dir).is_ok_and(|metadata| {
        metadata.uid() == rustix::process::geteuid().as_raw()
            && metadata.mode() & WRITABLE_BY_ALL == 0
    })
}

/// Windows has no owner check here: the scope base is the work tree.
#[cfg(not(unix))]
fn owned_by_this_user(_dir: &Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn the_base_is_the_work_tree_except_a_home_work_tree() {
        let dir = tempfile::tempdir().expect("temp dir");
        let top = dunce::canonicalize(dir.path()).expect("canonical");
        let root = top.join("packages/app");
        std::fs::create_dir_all(&root).expect("mkdir");
        assert_eq!(read_scope_base(&root, None), root, "no work tree");

        std::fs::create_dir(top.join(".git")).expect("mkdir");
        assert_eq!(read_scope_base(&root, None), top);
        assert_eq!(read_scope_base(&root, Some(&top)), root, "home work tree");
        assert_eq!(read_scope_base(&top, Some(&top)), top, "root at home");
        let home = top.join("home");
        assert_eq!(read_scope_base(&root, Some(&home)), root, "above home");
    }

    #[test]
    #[cfg(unix)]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_work_tree_that_all_users_can_write_does_not_widen_the_scope() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("temp dir");
        let top = dunce::canonicalize(dir.path()).expect("canonical");
        let root = top.join("app");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::create_dir(top.join(".git")).expect("mkdir");
        assert_eq!(read_scope_base(&root, None), top);

        std::fs::set_permissions(&top, std::fs::Permissions::from_mode(0o777)).expect("chmod");
        assert_eq!(read_scope_base(&root, None), root);
        std::fs::set_permissions(&top, std::fs::Permissions::from_mode(0o700)).expect("chmod");
    }

    /// A directory that another user owns does not widen the scope. `/usr`
    /// belongs to the system user on each Unix host.
    #[test]
    #[cfg(unix)]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_work_tree_that_another_user_owns_does_not_widen_the_scope() {
        if rustix::process::geteuid().is_root() {
            return;
        }
        let other = Path::new("/usr");
        assert!(!owned_by_this_user(other));
        assert!(!widens_scope(other, Some(Path::new("/home/user"))));
        let own = tempfile::tempdir().expect("temp dir");
        assert!(owned_by_this_user(own.path()));
    }

    #[test]
    fn a_work_tree_at_the_file_system_root_does_not_widen_the_scope() {
        let top = Path::new("/");
        assert!(!widens_scope(top, None));
        assert!(!widens_scope(top, Some(Path::new("/home/user"))));
    }

    /// The companion compares the base with plain resolved paths, so the
    /// base has no Windows verbatim prefix.
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn the_base_has_no_verbatim_prefix() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = read_scope_base(dir.path(), None);
        assert!(
            !base.to_string_lossy().starts_with(r"\\?\"),
            "{}",
            base.display()
        );
    }
}
