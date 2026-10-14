//! JavaScript runtime discovery for the companion: `node` or
//! `bun` from `PATH`, or an explicit runtime path.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The kind of a JavaScript runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeKind {
    Node,
    Bun,
}

impl RuntimeKind {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Bun => "bun",
        }
    }

    /// The kind of an executable, from its file name. Each name that does
    /// not start with `bun` is Node.
    fn from_executable(path: &Path) -> Self {
        let name = path
            .file_stem()
            .map(|stem| stem.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if name.starts_with("bun") {
            Self::Bun
        } else {
            Self::Node
        }
    }
}

/// A runtime that can start the companion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsRuntime {
    pub kind: RuntimeKind,
    pub path: PathBuf,
    pub version: String,
}

fn executable_names(base: &str) -> Vec<String> {
    if cfg!(windows) {
        vec![format!("{base}.exe"), format!("{base}.cmd")]
    } else {
        vec![base.to_owned()]
    }
}

/// Finds the first of `kinds`, in that order, in the directories of
/// `search_path`.
pub fn find_kinds_on_path(
    search_path: &OsStr,
    kinds: &[RuntimeKind],
) -> Option<(RuntimeKind, PathBuf)> {
    for &kind in kinds {
        for directory in std::env::split_paths(search_path) {
            if directory.as_os_str().is_empty() || !directory.is_absolute() {
                continue;
            }
            for name in executable_names(kind.name()) {
                let candidate = directory.join(name);
                if candidate.is_file() {
                    return Some((kind, candidate));
                }
            }
        }
    }
    None
}

/// Runs `<runtime> --version`.
fn version_of(path: &Path) -> Result<String, String> {
    let mut command = Command::new(path);
    command.arg("--version");
    let output = fallow_process::output(&mut command)
        .map_err(|error| format!("cannot run {} --version: {error}", path.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{} --version failed: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if version.is_empty() {
        return Err(format!("{} --version printed nothing", path.display()));
    }
    Ok(version)
}

/// Resolves the runtime: `explicit` when given, otherwise the first of
/// `kinds` from `PATH`. An explicit runtime of a kind outside `kinds` is an
/// error, because the runner cannot start on it.
///
/// # Errors
///
/// Returns an error when no runtime is found, the explicit runtime has the
/// wrong kind, or the runtime does not report a version.
pub fn discover_runtime(
    explicit: Option<&Path>,
    kinds: &[RuntimeKind],
) -> Result<JsRuntime, String> {
    let cwd = std::env::current_dir()
        .map_err(|error| format!("cannot read the working directory: {error}"))?;
    discover_runtime_in(explicit, kinds, &cwd)
}

/// The names of `kinds`, as `` `node` or `bun` ``.
fn kind_names(kinds: &[RuntimeKind]) -> String {
    kinds
        .iter()
        .map(|kind| format!("`{}`", kind.name()))
        .collect::<Vec<_>>()
        .join(" or ")
}

/// [`discover_runtime`] with a relative explicit path resolved against
/// `cwd`. The companion starts in the project root, so the stored path must
/// not depend on the working directory.
fn discover_runtime_in(
    explicit: Option<&Path>,
    kinds: &[RuntimeKind],
    cwd: &Path,
) -> Result<JsRuntime, String> {
    let (kind, path) = if let Some(path) = explicit {
        let path = cwd.join(path);
        if !path.is_file() {
            return Err(format!("the runtime {} is not a file", path.display()));
        }
        let kind = RuntimeKind::from_executable(&path);
        if !kinds.contains(&kind) {
            return Err(format!(
                "the runtime {} is `{}`, and the test runner needs {}",
                path.display(),
                kind.name(),
                kind_names(kinds)
            ));
        }
        (kind, path)
    } else {
        let search_path = std::env::var_os("PATH").unwrap_or_default();
        find_kinds_on_path(&search_path, kinds)
            .ok_or_else(|| format!("no {} executable on PATH", kind_names(kinds)))?
    };
    let version = version_of(&path)?;
    Ok(JsRuntime {
        kind,
        path,
        version,
    })
}

#[expect(clippy::unwrap_used, reason = "tests unwrap to keep assertions short")]
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    const BOTH: &[RuntimeKind] = &[RuntimeKind::Node, RuntimeKind::Bun];

    fn fake_runtime(dir: &Path, name: &str, version: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\necho '{version}'\n")).expect("write");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        path
    }

    #[test]
    fn node_wins_over_bun_in_any_path_order() {
        let first = tempfile::tempdir().expect("dir");
        let second = tempfile::tempdir().expect("dir");
        let bun = fake_runtime(first.path(), "bun", "1.4.2");
        let node = fake_runtime(second.path(), "node", "v24.20.0");
        let search = std::env::join_paths([first.path(), second.path()]).expect("join");
        assert_eq!(
            find_kinds_on_path(&search, BOTH),
            Some((RuntimeKind::Node, node))
        );
        let only_bun = std::env::join_paths([first.path()]).expect("join");
        assert_eq!(
            find_kinds_on_path(&only_bun, BOTH),
            Some((RuntimeKind::Bun, bun))
        );
        let relative = std::env::join_paths(["relative/dir"]).expect("join");
        assert_eq!(find_kinds_on_path(&relative, BOTH), None);
    }

    #[test]
    fn a_relative_explicit_runtime_resolves_against_the_working_directory() {
        let cwd = tempfile::tempdir().expect("dir");
        std::fs::create_dir(cwd.path().join("bin")).expect("mkdir");
        let node = fake_runtime(&cwd.path().join("bin"), "node", "v24.20.0");
        let runtime =
            discover_runtime_in(Some(Path::new("bin/node")), BOTH, cwd.path()).expect("runtime");
        assert!(runtime.path.is_absolute(), "{}", runtime.path.display());
        assert_eq!(runtime.path, node);
        assert_eq!(runtime.version, "v24.20.0");
        let output = Command::new(&runtime.path)
            .current_dir("/")
            .output()
            .expect("the stored path starts from another directory");
        assert!(output.status.success());
    }

    #[test]
    fn an_explicit_runtime_reports_its_kind_and_version() {
        let dir = tempfile::tempdir().expect("dir");
        let bun = fake_runtime(dir.path(), "bun-canary", "1.4.2");
        let runtime = discover_runtime(Some(&bun), BOTH).expect("runtime");
        assert_eq!(runtime.kind, RuntimeKind::Bun);
        assert_eq!(runtime.version, "1.4.2");
        let node = fake_runtime(dir.path(), "node24", "v24.20.0");
        assert_eq!(
            discover_runtime(Some(&node), BOTH).expect("runtime").kind,
            RuntimeKind::Node
        );
        let missing = dir.path().join("nope");
        assert!(
            discover_runtime(Some(&missing), BOTH)
                .unwrap_err()
                .contains("not a file")
        );
    }

    #[test]
    fn a_runner_that_needs_one_runtime_finds_only_that_runtime() {
        let first = tempfile::tempdir().expect("dir");
        let second = tempfile::tempdir().expect("dir");
        let node = fake_runtime(first.path(), "node", "v24.20.0");
        let bun = fake_runtime(second.path(), "bun", "1.4.2");
        let search = std::env::join_paths([first.path(), second.path()]).expect("join");
        assert_eq!(
            find_kinds_on_path(&search, &[RuntimeKind::Bun]),
            Some((RuntimeKind::Bun, bun.clone()))
        );
        assert_eq!(
            find_kinds_on_path(&search, &[RuntimeKind::Node]),
            Some((RuntimeKind::Node, node.clone()))
        );
        let only_node = std::env::join_paths([first.path()]).expect("join");
        assert_eq!(find_kinds_on_path(&only_node, &[RuntimeKind::Bun]), None);

        let error = discover_runtime_in(Some(&node), &[RuntimeKind::Bun], first.path())
            .expect_err("bun test cannot start on node");
        assert!(error.contains("needs `bun`"), "{error}");
        assert_eq!(
            discover_runtime_in(Some(&bun), &[RuntimeKind::Bun], first.path())
                .expect("runtime")
                .kind,
            RuntimeKind::Bun
        );
    }
}
