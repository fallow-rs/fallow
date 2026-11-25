//! Absolute launchers for user-level configuration.
//!
//! A user-level gate and a user-level MCP entry apply in every project. They
//! name a launcher by its absolute path, and that path must not belong to a
//! project. `npx` and `npm exec` put the `node_modules/.bin` directories of
//! the current project and its parents first on `PATH`. A candidate is
//! refused when the path as found has a `node_modules` component, or when the
//! path or its target is inside the project root or the git work tree of the
//! project. The search then goes on with the next `PATH` entry. A relative or
//! empty `PATH` entry never counts. A version-manager shim (a Volta, mise or
//! asdf shim directory, or a link to `volta-shim`) never counts either,
//! because it runs the fallow that the project selects. On macOS and Windows, where the default
//! file systems ignore letter case, the comparisons ignore letter case too.

use std::path::{Path, PathBuf};

/// True on a platform whose default file system ignores letter case.
const CASE_INSENSITIVE_PATHS: bool = cfg!(any(target_os = "macos", windows));

/// The text of `path` for a comparison: lower case where letter case does
/// not count.
fn comparable(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    if CASE_INSENSITIVE_PATHS {
        text.to_lowercase()
    } else {
        text
    }
}

/// True when `path` is `dir` or below it, by whole components.
fn is_within(path: &Path, dir: &Path) -> bool {
    let path = PathBuf::from(comparable(path));
    path.starts_with(comparable(dir))
}

/// True when a component of `path` is `node_modules`.
fn has_node_modules(path: &Path) -> bool {
    path.components().any(|part| {
        let part = part.as_os_str().to_string_lossy();
        if CASE_INSENSITIVE_PATHS {
            part.eq_ignore_ascii_case("node_modules")
        } else {
            part == "node_modules"
        }
    })
}

/// Directories whose files never become a user-level launcher: the project
/// root and the git work tree that holds it, each as given and canonical.
/// The home directory itself and the file system root never count, because
/// user-level installs such as `~/.cargo/bin` live below them.
pub fn project_dirs(root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![root.to_path_buf()];
    dirs.extend(dunce::canonicalize(root).ok());
    if let Some(top) = crate::base_worktree::git_toplevel(root) {
        dirs.extend(dunce::canonicalize(&top).ok());
        dirs.push(top);
    }
    let home = fallow_process::HostPaths::from_env().home;
    let home_real = home
        .as_deref()
        .and_then(|home| dunce::canonicalize(home).ok());
    let is_home = |dir: &Path| {
        [home.as_deref(), home_real.as_deref()]
            .into_iter()
            .flatten()
            .any(|home| comparable(home) == comparable(dir))
    };
    dirs.retain(|dir| dir.is_absolute() && dir.parent().is_some() && !is_home(dir));
    dirs
}

/// The directory suffixes of version-manager shims, in lower case. A shim
/// runs the fallow that the current project selects.
const SHIM_DIRS: &[(&str, &str)] = &[(".volta", "bin"), ("mise", "shims"), (".asdf", "shims")];

/// True when the directory of `file` is a known version-manager shim
/// directory (in any letter case), or when `file` is a `volta-shim`.
fn is_version_manager_shim(file: &Path) -> bool {
    let in_shim_dir = |path: &Path| {
        let mut parts = path
            .parent()
            .into_iter()
            .flat_map(Path::components)
            .rev()
            .map(|part| part.as_os_str().to_string_lossy().to_lowercase());
        let (Some(last), Some(before)) = (parts.next(), parts.next()) else {
            return false;
        };
        SHIM_DIRS
            .iter()
            .any(|(dir, sub)| before == *dir && last == *sub)
    };
    let is_volta_shim = |path: &Path| {
        path.file_name()
            .map(|name| name.to_string_lossy().to_lowercase())
            .is_some_and(|name| name == "volta-shim" || name == "volta-shim.exe")
    };
    in_shim_dir(file)
        || dunce::canonicalize(file).is_ok_and(|real| in_shim_dir(&real) || is_volta_shim(&real))
}

/// True when `candidate` can be a user-level launcher.
pub fn is_user_launcher(candidate: &Path, project_dirs: &[PathBuf]) -> bool {
    if !candidate.is_absolute() || has_node_modules(candidate) || is_version_manager_shim(candidate)
    {
        return false;
    }
    let canonical = dunce::canonicalize(candidate).ok();
    !project_dirs.iter().any(|dir| {
        is_within(candidate, dir) || canonical.as_ref().is_some_and(|real| is_within(real, dir))
    })
}

/// The first file named `name` in `search_dirs` that [`is_user_launcher`]
/// accepts, as found (not canonical). Relative and empty entries are skipped.
pub fn find_user_launcher(
    name: &str,
    search_dirs: impl IntoIterator<Item = PathBuf>,
    project_dirs: &[PathBuf],
) -> Option<PathBuf> {
    let extensions: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd"]
    } else {
        &[""]
    };
    search_dirs
        .into_iter()
        .filter(|dir| dir.is_absolute())
        .flat_map(|dir| {
            extensions
                .iter()
                .map(move |ext| dir.join(format!("{name}{ext}")))
        })
        .find(|candidate| candidate.is_file() && is_user_launcher(candidate, project_dirs))
}

/// The `PATH` directories of this process.
pub fn search_dirs() -> Vec<PathBuf> {
    fallow_process::HostPaths::from_env()
        .search_dirs()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\n").unwrap();
    }

    /// `npm exec` puts the `node_modules/.bin` of the project and of its
    /// parents first on `PATH`. The search skips them, skips relative
    /// entries, and returns the next launcher as found.
    #[test]
    fn find_user_launcher_skips_project_and_package_directories() {
        let tmp = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(tmp.path()).unwrap();
        let project = base.join("work/app");
        let global = base.join("global/bin");
        touch(&project.join("node_modules/.bin/fallow"));
        touch(&base.join("work/node_modules/.bin/fallow"));
        touch(&project.join("tools/fallow"));
        touch(&global.join("fallow"));
        let dirs = vec![
            project.join("node_modules/.bin"),
            base.join("work/node_modules/.bin"),
            project.join("tools"),
            PathBuf::from("tools"),
            PathBuf::new(),
            global.clone(),
        ];

        let found = find_user_launcher("fallow", dirs.clone(), &project_dirs(&project));

        assert_eq!(found, Some(global.join("fallow")));
        assert_eq!(
            find_user_launcher("fallow", dirs[..5].to_vec(), &project_dirs(&project)),
            None
        );
    }

    /// A link outside the project that points into the project is refused
    /// too. A link to a file outside the project is kept as found.
    #[cfg(unix)]
    #[test]
    fn is_user_launcher_checks_the_link_target() {
        let tmp = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(tmp.path()).unwrap();
        let project = base.join("app");
        touch(&project.join("bin/fallow"));
        touch(&base.join("cellar/1.0/bin/fallow"));
        std::fs::create_dir_all(base.join("bin")).unwrap();
        std::os::unix::fs::symlink(project.join("bin/fallow"), base.join("bin/into")).unwrap();
        std::os::unix::fs::symlink(base.join("cellar/1.0/bin/fallow"), base.join("bin/fallow"))
            .unwrap();
        let dirs = project_dirs(&project);

        assert!(!is_user_launcher(&base.join("bin/into"), &dirs));
        assert!(is_user_launcher(&base.join("bin/fallow"), &dirs));
        assert_eq!(
            find_user_launcher("fallow", vec![base.join("bin")], &dirs),
            Some(base.join("bin/fallow"))
        );
    }

    /// A version-manager shim picks the fallow of the project that it runs
    /// in, so the search skips a shim directory (in any letter case) and a
    /// link to `volta-shim`.
    #[cfg(unix)]
    #[test]
    fn find_user_launcher_skips_version_manager_shims() {
        let tmp = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(tmp.path()).unwrap();
        let project = base.join("work/app");
        let shim_dirs = [
            base.join(".volta/bin"),
            base.join(".local/share/mise/shims"),
            base.join("mise/shims"),
            base.join(".asdf/shims"),
            base.join(".Volta/Bin"),
        ];
        for dir in &shim_dirs {
            touch(&dir.join("fallow"));
        }
        touch(&base.join("volta/volta-shim"));
        std::fs::create_dir_all(base.join("links")).unwrap();
        std::os::unix::fs::symlink(base.join("volta/volta-shim"), base.join("links/fallow"))
            .unwrap();
        let global = base.join("global/bin");
        touch(&global.join("fallow"));
        let mut dirs = shim_dirs.to_vec();
        dirs.push(base.join("links"));
        dirs.push(global.clone());

        let found = find_user_launcher("fallow", dirs, &project_dirs(&project));

        assert_eq!(found, Some(global.join("fallow")));
        assert!(!is_user_launcher(
            &base.join(".asdf/shims/fallow"),
            &project_dirs(&project)
        ));
    }

    /// On macOS and Windows a `Node_Modules` directory or a project path in
    /// another letter case is the same directory.
    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn is_user_launcher_ignores_letter_case_where_paths_do() {
        let tmp = tempfile::tempdir().unwrap();
        let base = dunce::canonicalize(tmp.path()).unwrap();
        let project = base.join("App");
        touch(&project.join("bin/fallow"));
        touch(&base.join("Node_Modules/.bin/fallow"));
        let dirs = project_dirs(&project);
        let other_case = PathBuf::from(project.to_string_lossy().to_uppercase()).join("bin/fallow");

        assert!(!is_user_launcher(
            &base.join("Node_Modules/.bin/fallow"),
            &dirs
        ));
        assert!(!is_user_launcher(&other_case, &dirs));
    }
}
