//! Bounded reads of project files for the plugin, CI, hook and
//! infrastructure readers.
//!
//! A cloned repository can commit a symlink where one of these readers reads
//! a file or lists a directory. Each read here goes through the bounded read
//! helper, so a file that resolves outside the read scope of the project (see
//! [`fallow_types::read_scope`]), is not a regular file or is too large counts
//! as a missing file. No message names its content.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use fallow_config::PackageJson;
use fallow_types::read_guard::{DEFAULT_MAX_READ_BYTES, is_inside_root, read_to_string_inside};
use fallow_types::read_scope::read_scope_base;
use rustc_hash::FxHashMap;

/// The read scope bases of the current analysis run, one per root.
fn scope_bases() -> &'static Mutex<FxHashMap<PathBuf, PathBuf>> {
    static BASES: OnceLock<Mutex<FxHashMap<PathBuf, PathBuf>>> = OnceLock::new();
    BASES.get_or_init(|| Mutex::new(FxHashMap::default()))
}

/// Start a new analysis run: drop the read scope bases of earlier runs. A
/// long-lived process (the language server, watch mode) then sees a `.git`
/// entry that appeared after an earlier run.
pub fn begin_read_scope_run() {
    scope_bases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// The read scope base of `root`, computed once per root and run. A link
/// below the root may point at a shared file anywhere inside this base, such
/// as a config of the monorepo above a package root.
fn scope_base(root: &Path) -> PathBuf {
    let mut bases = scope_bases()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    bases
        .entry(root.to_path_buf())
        .or_insert_with(|| {
            read_scope_base(root, fallow_process::HostPaths::from_env().home.as_deref())
        })
        .clone()
}

/// The text of the project file `path`, or `None` when it is missing or the
/// read is refused.
pub fn read_project_file(root: &Path, path: &Path) -> Option<String> {
    read_to_string_inside(&scope_base(root), path, DEFAULT_MAX_READ_BYTES).ok()
}

/// The bytes of the project file `path`, or `None` when it is missing or the
/// read is refused.
pub fn read_project_bytes(root: &Path, path: &Path) -> Option<Vec<u8>> {
    fallow_types::read_guard::read_bytes_inside(&scope_base(root), path, DEFAULT_MAX_READ_BYTES)
        .ok()
}

/// The entries of the project directory `dir`, or `None` when it is missing
/// or resolves outside the read scope of `root`.
pub fn read_project_dir(root: &Path, dir: &Path) -> Option<std::fs::ReadDir> {
    if !is_inside_root(&scope_base(root), dir) {
        return None;
    }
    std::fs::read_dir(dir).ok()
}

/// True when `path` resolves inside the read scope of `root`.
pub fn is_inside_scope(root: &Path, path: &Path) -> bool {
    is_inside_root(&scope_base(root), path)
}

/// True when `path` is a regular file that resolves inside the read scope of
/// `root`.
pub fn is_project_file(root: &Path, path: &Path) -> bool {
    path.is_file() && is_inside_scope(root, path)
}

/// The `package.json` of the project directory `dir`, or `None` when it is
/// missing or resolves outside the read scope of `root`.
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

    /// A package root inside a Git work tree may link a shared config of the
    /// work tree, but not a file outside it.
    #[test]
    #[cfg(unix)]
    fn a_link_to_a_shared_config_of_the_work_tree_is_read() {
        let dir = tempfile::tempdir().expect("temp dir");
        let top = dir.path().join("repo");
        let root = top.join("packages/app");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::create_dir(top.join(".git")).expect("mkdir");
        std::fs::write(top.join("jest.base.js"), "module.exports = {};").expect("write");
        std::fs::write(dir.path().join("other.js"), "module.exports = {};").expect("write");
        std::os::unix::fs::symlink("../../jest.base.js", root.join("jest.config.js"))
            .expect("link");
        std::os::unix::fs::symlink(dir.path().join("other.js"), root.join("other.config.js"))
            .expect("link");

        assert!(read_project_file(&root, &root.join("jest.config.js")).is_some());
        assert!(read_project_file(&root, &root.join("other.config.js")).is_none());
    }

    /// A `.git` entry that appears after a run widens the read scope of the
    /// next run.
    #[test]
    #[cfg(unix)]
    fn a_new_run_sees_a_work_tree_that_appeared_later() {
        let dir = tempfile::tempdir().expect("temp dir");
        let top = dir.path().join("repo");
        let root = top.join("packages/app");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::write(top.join("jest.base.js"), "module.exports = {};").expect("write");
        std::os::unix::fs::symlink("../../jest.base.js", root.join("jest.config.js"))
            .expect("link");
        assert!(read_project_file(&root, &root.join("jest.config.js")).is_none());

        std::fs::create_dir(top.join(".git")).expect("mkdir");
        begin_read_scope_run();
        assert!(read_project_file(&root, &root.join("jest.config.js")).is_some());
    }

    /// A sibling module of a config is read only when it is a regular file
    /// inside the read scope of the config directory.
    #[test]
    #[cfg(unix)]
    fn a_sibling_module_of_a_config_is_a_bounded_project_read() {
        use crate::plugins::config_parser::resolve_sibling_module;
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        std::fs::create_dir_all(root.join("config")).expect("mkdir");
        std::fs::create_dir_all(dir.path().join("outside")).expect("mkdir");
        std::fs::write(dir.path().join("outside/x.js"), "export default 1;").expect("write");
        std::fs::write(root.join("config/shared.js"), "export default 2;").expect("write");
        std::os::unix::fs::symlink("/dev/zero", root.join("config/zero.js")).expect("link");
        let config = root.join("config/vite.config.js");

        assert!(resolve_sibling_module(&config, "./shared.js").is_some());
        assert!(resolve_sibling_module(&config, "../../outside/x.js").is_none());
        assert!(resolve_sibling_module(&config, "/dev/zero").is_none());
        assert!(resolve_sibling_module(&config, "./zero.js").is_none());
    }

    /// A byte read refuses a link out of the read scope and a device, and
    /// reads a regular file inside it.
    #[test]
    #[cfg(unix)]
    fn byte_reads_refuse_links_out_of_the_scope_and_devices() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::write(dir.path().join("other.txt"), "other").expect("write");
        std::fs::write(root.join("own.txt"), "own").expect("write");
        std::os::unix::fs::symlink(dir.path().join("other.txt"), root.join("linked.txt"))
            .expect("link");
        std::os::unix::fs::symlink("/dev/zero", root.join("zero.txt")).expect("link");

        assert_eq!(
            read_project_bytes(&root, &root.join("own.txt")),
            Some(b"own".to_vec())
        );
        assert!(read_project_bytes(&root, &root.join("linked.txt")).is_none());
        assert!(read_project_bytes(&root, &root.join("zero.txt")).is_none());
    }
}
