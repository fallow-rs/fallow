//! Bounded reads of project files for the plugin, CI, hook and
//! infrastructure readers.
//!
//! A cloned repository can commit a symlink where one of these readers reads
//! a file or lists a directory. Each read here goes through the bounded read
//! helper, so a file that resolves outside the project root, is not a regular
//! file or is too large counts as a missing file. No message names its
//! content.

use std::path::Path;

use fallow_config::PackageJson;
use fallow_types::read_guard::{DEFAULT_MAX_READ_BYTES, is_inside_root, read_to_string_inside};

/// The text of the project file `path`, or `None` when it is missing or the
/// read is refused.
pub fn read_project_file(root: &Path, path: &Path) -> Option<String> {
    read_to_string_inside(root, path, DEFAULT_MAX_READ_BYTES).ok()
}

/// The entries of the project directory `dir`, or `None` when it is missing
/// or resolves outside `root`.
pub fn read_project_dir(root: &Path, dir: &Path) -> Option<std::fs::ReadDir> {
    if !is_inside_root(root, dir) {
        return None;
    }
    std::fs::read_dir(dir).ok()
}

/// True when `path` is a regular file that resolves inside `root`.
pub fn is_project_file(root: &Path, path: &Path) -> bool {
    path.is_file() && is_inside_root(root, path)
}

/// The `package.json` of the project directory `dir`, or `None` when it is
/// missing or resolves outside `root`.
pub fn package_json(root: &Path, dir: &Path) -> Option<PackageJson> {
    is_project_file(root, &dir.join("package.json"))
        .then(|| fallow_config::load_dir_package_json(dir))
        .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn refuses_files_and_directories_that_link_out_of_the_root() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(root.join("own")).expect("mkdir");
        std::fs::create_dir_all(&outside).expect("mkdir");
        std::fs::write(outside.join("package.json"), r#"{"name":"other"}"#).expect("write");
        std::os::unix::fs::symlink(&outside, root.join("linked")).expect("link");
        std::os::unix::fs::symlink(outside.join("package.json"), root.join("own/package.json"))
            .expect("link");

        assert!(read_project_file(&root, &root.join("linked/package.json")).is_none());
        assert!(read_project_dir(&root, &root.join("linked")).is_none());
        assert!(package_json(&root, &root.join("own")).is_none());
        assert!(read_project_dir(&root, &root.join("own")).is_some());
    }
}
