//! Confine the files that `--save-baseline`, `--save-regression-baseline` and
//! `--save-snapshot` write to the project.
//!
//! A relative save path resolves against the working directory, as it always
//! did, so the matching read path (`--baseline`, `--regression-baseline`) still
//! finds the file. The resolved path must then lie inside the project root, or
//! inside the Git work tree that contains the root when the working directory
//! is inside that tree too, or inside a temp directory (`RUNNER_TEMP` when it
//! is set, and the system temp directory). The default destinations of a bare
//! `--save-snapshot` and a bare `--save-regression-baseline` are checked the
//! same way.
//! The work tree keeps the monorepo form working, where a job runs from the
//! repository root with `--root packages/app` and saves to a
//! repository-relative path. The temp directories keep the CI form working,
//! where a job saves a baseline outside the checkout between steps.
//!
//! Symlinks are resolved on both sides: on the part of the save path that
//! exists, and on each allowed directory. So a link inside the root that
//! points outside it does not open a way out, and `/var` and `/private/var`
//! on macOS compare equal.

use std::path::{Component, Path, PathBuf};

use crate::{Cli, Command};

/// Return an error message when a file that a save flag writes resolves
/// outside the allowed directories.
///
/// Only the commands that write these files are checked. A command that
/// rejects the flags, such as `audit`, keeps its own error about the flag.
/// The check fails closed: when the working directory or the root cannot be
/// resolved, a save is rejected.
pub fn save_path_error(cli: &Cli, root: &Path) -> Option<String> {
    if !matches!(
        cli.command,
        None | Some(Command::Check { .. } | Command::Dupes { .. } | Command::Health { .. })
    ) {
        return None;
    }
    let targets = save_targets(cli, root);
    let (first_flag, _) = targets.first()?;
    let cwd = match std::env::current_dir() {
        Ok(cwd) => cwd,
        Err(err) => {
            return Some(format!(
                "{first_flag} cannot be checked, because the working directory cannot be read ({err}). Run fallow from an existing directory."
            ));
        }
    };
    let scope = match WriteScope::new(root, &cwd, temp_dirs()) {
        Ok(scope) => scope,
        Err(message) => return Some(format!("{first_flag} cannot be checked: {message}")),
    };
    targets
        .iter()
        .find_map(|(flag, path)| scope.check(flag, path, &cwd))
}

/// Every file on the command line that a save flag writes, with the flag
/// that asked for it.
///
/// An explicit path is checked as given. A bare `--save-snapshot` writes into
/// `<root>/.fallow/snapshots`, and a bare `--save-regression-baseline`
/// rewrites the config file, so their default destinations are checked too:
/// a committed `.fallow` or config symlink must not carry the write out of
/// the project.
fn save_targets(cli: &Cli, root: &Path) -> Vec<(&'static str, PathBuf)> {
    let mut targets = Vec::new();
    if let Some(path) = cli.save_baseline.as_deref() {
        targets.push(("--save-baseline", path.to_path_buf()));
    }
    if let Some(value) = cli.save_regression_baseline.as_ref() {
        match value.as_deref().filter(|path| !path.is_empty()) {
            Some(path) => targets.push(("--save-regression-baseline", PathBuf::from(path))),
            None => targets.push((
                "--save-regression-baseline",
                crate::regression::regression_config_target(cli.config.as_deref(), root),
            )),
        }
    }
    let health_snapshot = match cli.command.as_ref() {
        Some(Command::Health { save_snapshot, .. }) => save_snapshot.as_ref(),
        _ => None,
    };
    for snapshot in [cli.save_snapshot.as_ref(), health_snapshot]
        .into_iter()
        .flatten()
    {
        match snapshot.as_deref().filter(|path| !path.is_empty()) {
            Some(path) => targets.push(("--save-snapshot", PathBuf::from(path))),
            None => targets.push((
                "--save-snapshot",
                root.join(".fallow").join("snapshots").join("snapshot.json"),
            )),
        }
    }
    targets.retain(|(_, path)| !path.as_os_str().is_empty());
    targets
}

/// The temp directories a save may write into: `RUNNER_TEMP` when it is set
/// and not empty, and the system temp directory. A directory that does not
/// exist is skipped, because it cannot be resolved.
fn temp_dirs() -> Vec<PathBuf> {
    let runner_temp = std::env::var_os("RUNNER_TEMP")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from);
    runner_temp
        .into_iter()
        .chain(std::iter::once(std::env::temp_dir()))
        .filter_map(|dir| dir.canonicalize().ok())
        .collect()
}

/// The directories a save may write into.
struct WriteScope {
    root: PathBuf,
    work_tree: Option<PathBuf>,
    temp_dirs: Vec<PathBuf>,
}

impl WriteScope {
    /// The Git work tree that contains the root counts only when the working
    /// directory is inside it too. That is the monorepo case the allowance
    /// exists for. It keeps a Git repository at `$HOME` (a dotfiles setup)
    /// from allowing every home path to a run started outside that tree.
    fn new(root: &Path, cwd: &Path, temp_dirs: Vec<PathBuf>) -> Result<Self, String> {
        let root = root.canonicalize().map_err(|err| {
            format!(
                "the project root {} cannot be resolved ({err})",
                root.display()
            )
        })?;
        let cwd = cwd.canonicalize().unwrap_or_else(|_| cwd.to_path_buf());
        let work_tree = root
            .ancestors()
            .find(|dir| dir.join(".git").exists())
            .filter(|tree| cwd.starts_with(tree))
            .map(Path::to_path_buf);
        Ok(Self {
            root,
            work_tree,
            temp_dirs,
        })
    }

    fn contains(&self, path: &Path) -> bool {
        path.starts_with(&self.root)
            || self
                .work_tree
                .as_deref()
                .is_some_and(|tree| path.starts_with(tree))
            || self.temp_dirs.iter().any(|dir| path.starts_with(dir))
    }

    fn check(&self, flag: &str, path: &Path, cwd: &Path) -> Option<String> {
        let resolved = resolve(&cwd.join(path));
        if self.contains(&resolved) {
            return None;
        }
        Some(format!(
            "{flag} {} resolves to {}, which is outside the project root {}. Choose a path inside the project root{}, or inside the temp directory (RUNNER_TEMP or the system temp directory).",
            path.display(),
            resolved.display(),
            self.root.display(),
            self.work_tree
                .as_deref()
                .filter(|tree| *tree != self.root)
                .map(|tree| format!(" or its Git work tree {}", tree.display()))
                .unwrap_or_default(),
        ))
    }
}

/// How many dangling symlinks [`resolve`] follows before it gives up.
const MAX_LINK_HOPS: usize = 40;

/// Resolve `path` the way a write would reach it: symlinks and `..` in the
/// part that exists are resolved by the file system, and the rest, which a
/// write would create, is normalised lexically. A dangling symlink is followed
/// to its target, because a write through it lands there.
fn resolve(path: &Path) -> PathBuf {
    resolve_with_hops(path, 0)
}

fn resolve_with_hops(path: &Path, hops: usize) -> PathBuf {
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        if let Ok(real) = existing.canonicalize() {
            return append_missing(real, &missing);
        }
        if hops < MAX_LINK_HOPS
            && let Ok(target) = std::fs::read_link(existing)
        {
            let base = existing.parent().unwrap_or_else(|| Path::new(""));
            let followed = resolve_with_hops(&base.join(target), hops + 1);
            return append_missing(followed, &missing);
        }
        let (Some(parent), Some(last)) = (existing.parent(), existing.components().next_back())
        else {
            return append_missing(PathBuf::new(), &missing);
        };
        missing.push(last);
        existing = parent;
    }
}

/// Append the components that do not exist yet, innermost last.
fn append_missing(mut resolved: PathBuf, missing: &[Component<'_>]) -> PathBuf {
    for component in missing.iter().rev() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => resolved.push(other.as_os_str()),
        }
    }
    resolved
}

#[cfg(test)]
mod tests {
    use super::{WriteScope, resolve};

    #[test]
    fn resolve_normalises_the_missing_part() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dir.path().canonicalize().unwrap();
        assert_eq!(
            resolve(&base.join("a/b/../c.json")),
            base.join("a").join("c.json")
        );
        assert_eq!(
            resolve(&base.join("a/../../x.json")),
            base.parent().unwrap().join("x.json")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_dangling_link_resolves_to_its_target() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dir.path().canonicalize().unwrap();
        let root = base.join("project");
        std::fs::create_dir_all(&root).unwrap();
        std::os::unix::fs::symlink(base.join("elsewhere.json"), root.join("b.json")).unwrap();
        assert_eq!(resolve(&root.join("b.json")), base.join("elsewhere.json"));
    }

    #[test]
    fn scope_accepts_a_temp_dir_through_a_symlinked_spelling() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let temp = dir.path().join("temp");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&temp).unwrap();
        let scope =
            WriteScope::new(&root, &root, vec![temp.canonicalize().unwrap()]).expect("scope");
        // `dir.path()` is not canonical on macOS (`/var` links to
        // `/private/var`), so this also checks that both sides are resolved.
        assert!(
            scope
                .check("--save-baseline", &temp.join("b.json"), &root)
                .is_none()
        );
        assert!(
            scope
                .check("--save-baseline", &dir.path().join("b.json"), &root)
                .is_some()
        );
    }

    #[test]
    fn a_root_that_cannot_be_resolved_fails_closed() {
        let dir = tempfile::tempdir().expect("temp dir");
        let missing = dir.path().join("missing");
        assert!(WriteScope::new(&missing, dir.path(), Vec::new()).is_err());
    }

    #[test]
    fn the_work_tree_needs_the_working_directory_inside_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let home = dir.path().join("home");
        let root = home.join("project");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::create_dir_all(home.join(".git")).unwrap();
        let target = home.join(".config").join("x.json");
        let from_outside = WriteScope::new(&root, &elsewhere, Vec::new()).expect("scope");
        assert!(
            from_outside
                .check("--save-baseline", &target, &elsewhere)
                .is_some()
        );
        let from_inside = WriteScope::new(&root, &home, Vec::new()).expect("scope");
        assert!(
            from_inside
                .check("--save-baseline", &target, &home)
                .is_none()
        );
    }

    #[test]
    fn scope_accepts_the_root_and_rejects_a_sibling() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        std::fs::create_dir_all(&root).unwrap();
        let scope = WriteScope::new(&root, &root, Vec::new()).expect("scope");
        assert!(
            scope
                .check("--save-baseline", "nested/b.json".as_ref(), &root)
                .is_none()
        );
        let message = scope
            .check("--save-baseline", "../b.json".as_ref(), &root)
            .expect("outside");
        assert!(message.contains("outside the project root"), "{message}");
    }

    #[test]
    fn scope_accepts_the_git_work_tree_of_the_root() {
        let dir = tempfile::tempdir().expect("temp dir");
        let repo = dir.path().join("repo");
        let root = repo.join("packages/app");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let scope = WriteScope::new(&root, &repo, Vec::new()).expect("scope");
        assert!(
            scope
                .check("--save-baseline", "baselines/b.json".as_ref(), &repo)
                .is_none()
        );
        let message = scope
            .check("--save-baseline", "../b.json".as_ref(), &repo)
            .expect("outside");
        assert!(message.contains("Git work tree"), "{message}");
    }
}
