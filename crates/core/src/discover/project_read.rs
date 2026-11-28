//! Bounded reads of project files for the plugin, CI, hook and
//! infrastructure readers.
//!
//! A cloned repository can commit a symlink where one of these readers reads
//! a file or lists a directory. Each read here goes through the bounded read
//! helper, so a file that resolves outside the read scope of the project (see
//! [`fallow_config::project_scope_base`]), is not a regular file or is too large counts
//! as a missing file. No message names its content.

use std::path::{Path, PathBuf};

use fallow_config::PackageJson;
use fallow_types::read_guard::{DEFAULT_MAX_READ_BYTES, is_inside_root, read_to_string_inside};

/// The read scope base of `root` (see [`fallow_config::project_scope_base`]).
/// A link below the root may point at a shared file anywhere inside this
/// base, such as a config of the monorepo above a package root.
fn scope_base(root: &Path) -> PathBuf {
    fallow_config::project_scope_base(root)
}

std::thread_local! {
    static PROJECT_ROOT: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Run `resolve` with `project_root` as the project root of the config
/// readers on this thread (see [`config_project_root`]). The plugin registry
/// sets it around each config resolution, so a reader deep in a config walk
/// reads in the scope of the project and not of the config directory.
pub fn with_project_root<T>(project_root: &Path, resolve: impl FnOnce() -> T) -> T {
    let previous = PROJECT_ROOT.with(|root| root.replace(Some(project_root.to_path_buf())));
    let _restore = RestoreProjectRoot(previous);
    resolve()
}

/// Puts the previous project root back when it drops, also when `resolve`
/// of [`with_project_root`] panics.
struct RestoreProjectRoot(Option<PathBuf>);

impl Drop for RestoreProjectRoot {
    fn drop(&mut self) {
        let previous = self.0.take();
        // The thread-local can be gone while the thread ends.
        let _ = PROJECT_ROOT.try_with(|root| *root.borrow_mut() = previous);
    }
}

/// `run` with the project root of this thread (see [`with_project_root`]),
/// for a closure that runs on another thread, such as a bounded parse.
pub fn carry_project_root<T>(run: impl FnOnce() -> T + Send) -> impl FnOnce() -> T + Send {
    let project_root = PROJECT_ROOT.with(|root| root.borrow().clone());
    move || match project_root {
        Some(project_root) => with_project_root(&project_root, run),
        None => run(),
    }
}

/// The project root of a config reader: the root that
/// [`with_project_root`] set on this thread, or `config_dir` when a caller
/// outside a config resolution set none.
pub fn config_project_root(config_dir: &Path) -> PathBuf {
    PROJECT_ROOT
        .with(|root| root.borrow().clone())
        .unwrap_or_else(|| config_dir.to_path_buf())
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
    #[cfg(test)]
    GUARDED_DIR_READS.with(|reads| reads.set(reads.get() + 1));
    if !is_inside_root(&scope_base(root), dir) {
        return None;
    }
    std::fs::read_dir(dir).ok()
}

#[cfg(test)]
std::thread_local! {
    static GUARDED_DIR_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The number of [`read_project_dir`] calls on this thread, for tests that
/// check that a scan does not list a directory.
#[cfg(test)]
pub fn guarded_dir_reads_for_test() -> usize {
    GUARDED_DIR_READS.with(std::cell::Cell::get)
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

/// [`is_project_file`] for one candidate of a probe. A missing candidate
/// costs one `lstat` call. A candidate that resolves outside the scope
/// counts as missing, whether its target exists or not.
///
/// A candidate that is not a symlink resolves where its directory resolves,
/// so only the directory needs the scope check, and each directory gets it
/// once per run.
pub fn is_probed_project_file(root: &Path, path: &Path) -> bool {
    let Ok(meta) = path.symlink_metadata() else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return is_inside_scope(root, path) && path.is_file();
    }
    if !meta.is_file() {
        return false;
    }
    let Some(dir) = path.parent() else {
        return false;
    };
    let base = scope_base(root);
    fallow_config::dir_inside_scope_base(&base, dir, || is_inside_root(&base, dir))
}

/// The bounded reads of one scan of many candidate files under `root`, such
/// as the hook scan. The read scope base of `root` and its resolved form are
/// found once, so a missing candidate costs one `lstat` call. Each read gives
/// the same result as the matching free function with the same `root`.
pub struct ProjectReader {
    scope: fallow_types::read_guard::ReadRoot,
}

impl ProjectReader {
    /// A reader for the project root `root`.
    pub fn new(root: &Path) -> Self {
        Self {
            scope: fallow_types::read_guard::ReadRoot::new(&scope_base(root)),
        }
    }

    /// [`read_project_file`] with the root of this reader.
    pub fn read_file(&self, path: &Path) -> Option<String> {
        self.scope.read_to_string(path, DEFAULT_MAX_READ_BYTES).ok()
    }

    /// [`read_project_dir`] with the root of this reader.
    pub fn read_dir(&self, dir: &Path) -> Option<std::fs::ReadDir> {
        if !self.scope.is_inside(dir) {
            return None;
        }
        std::fs::read_dir(dir).ok()
    }

    /// [`is_project_file`] with the root of this reader.
    pub fn is_file(&self, path: &Path) -> bool {
        path.is_file() && self.scope.is_inside(path)
    }
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

    /// A panic in the resolution puts the previous project root back.
    #[test]
    fn a_panic_in_the_resolution_restores_the_previous_root() {
        let outer = Path::new("/outer");
        let inner = Path::new("/inner");
        with_project_root(outer, || {
            let result = std::panic::catch_unwind(|| {
                with_project_root(inner, || panic!("resolution failed"));
            });
            assert!(result.is_err());
            assert_eq!(config_project_root(Path::new("/config")), outer);
        });
        assert_eq!(
            config_project_root(Path::new("/config")),
            Path::new("/config")
        );
    }

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
        fallow_config::clear_project_scope_bases();
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

    /// A package of a workspace without Git reads a shared alias file of the
    /// workspace root, and a committed directory link next to the config is
    /// not read.
    #[test]
    #[cfg(unix)]
    fn config_siblings_read_in_the_scope_of_the_project_root() {
        use crate::plugins::config_parser::extract_config_aliases;
        let dir = tempfile::tempdir().expect("temp dir");
        let top = dunce::canonicalize(dir.path())
            .expect("canonical")
            .join("ws");
        let app = top.join("packages/app");
        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(&app).expect("mkdir");
        std::fs::create_dir_all(&outside).expect("mkdir");
        std::fs::write(
            top.join("pnpm-workspace.yaml"),
            "packages:\n  - 'packages/*'\n",
        )
        .expect("write");
        let shared = r#"export const aliases = [{ find: "@", replacement: "./src" }];"#;
        std::fs::write(top.join("vite.shared.ts"), shared).expect("write");
        std::fs::write(outside.join("aliases.ts"), shared).expect("write");
        std::os::unix::fs::symlink(&outside, app.join("linked")).expect("link");
        fallow_config::clear_project_scope_bases();
        let aliases_from = |config: &Path, specifier: &str| {
            let source = format!(
                "import {{ aliases }} from \"{specifier}\";\nexport default defineConfig({{ resolve: {{ alias: aliases }} }});"
            );
            with_project_root(&app, || {
                extract_config_aliases(&source, config, &["resolve", "alias"])
            })
        };
        let config = app.join("vite.config.ts");

        assert_eq!(
            aliases_from(&config, "../../vite.shared"),
            vec![("@".to_string(), "./src".to_string())]
        );
        assert!(aliases_from(&config, "./linked/aliases").is_empty());
        // A config inside the linked directory does not make the link
        // target the root of its sibling reads.
        assert!(aliases_from(&app.join("linked/vite.config.ts"), "./aliases").is_empty());
    }
}
