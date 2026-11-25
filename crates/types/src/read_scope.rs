//! The directory that the reads of a project may reach.
//!
//! A project can link a shared file from above its root, for example a
//! `jest.config.js` in a package that links to `../../jest.base.js` of the
//! monorepo. Such a target is still part of the checkout. The read scope base
//! is therefore the Git work tree that contains the root, with one exception:
//! a work tree at the home directory (a dotfiles repository) does not widen
//! the scope of a root below it.

use std::path::{Path, PathBuf};

/// The read scope base of `root`: the nearest directory at or above the
/// resolved root that holds a `.git` entry, or the resolved root itself when
/// there is none or when that directory is `home` and the root is not. The
/// caller reads `home` from the host paths of the process.
#[must_use]
pub fn read_scope_base(root: &Path, home: Option<&Path>) -> PathBuf {
    let home = home.and_then(|home| std::fs::canonicalize(home).ok());
    let Ok(root) = std::fs::canonicalize(root) else {
        return root.to_path_buf();
    };
    let Some(top) = root
        .ancestors()
        .find(|dir| dir.join(".git").symlink_metadata().is_ok())
    else {
        return root;
    };
    if home.as_deref() == Some(top) && top != root {
        return root;
    }
    top.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn the_base_is_the_work_tree_except_a_home_work_tree() {
        let dir = tempfile::tempdir().expect("temp dir");
        let top = std::fs::canonicalize(dir.path()).expect("canonical");
        let root = top.join("packages/app");
        std::fs::create_dir_all(&root).expect("mkdir");
        assert_eq!(read_scope_base(&root, None), root, "no work tree");

        std::fs::create_dir(top.join(".git")).expect("mkdir");
        assert_eq!(read_scope_base(&root, None), top);
        assert_eq!(read_scope_base(&root, Some(&top)), root, "home work tree");
        assert_eq!(read_scope_base(&top, Some(&top)), top, "root at home");
    }
}
