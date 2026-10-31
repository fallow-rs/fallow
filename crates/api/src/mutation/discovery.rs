//! JavaScript runtime discovery for the companion: `node` or
//! `bun` from `PATH`, or an explicit runtime name or path.

use std::ffi::{OsStr, OsString};
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

/// The Windows file extensions that start a program.
const WINDOWS_EXECUTABLE_EXTENSIONS: [&str; 2] = ["exe", "cmd"];

fn executable_names(base: &str) -> Vec<OsString> {
    if cfg!(windows) {
        WINDOWS_EXECUTABLE_EXTENSIONS
            .iter()
            .map(|extension| OsString::from(format!("{base}.{extension}")))
            .collect()
    } else {
        vec![OsString::from(base)]
    }
}

/// Finds the first of `kinds`, in that order, in the directories of
/// `search_path`.
pub fn find_kinds_on_path(
    search_path: &OsStr,
    kinds: &[RuntimeKind],
) -> Option<(RuntimeKind, PathBuf)> {
    kinds.iter().find_map(|&kind| {
        find_on_path(search_path, &executable_names(kind.name())).map(|path| (kind, path))
    })
}

/// Finds the first file of `names` in the absolute directories of
/// `search_path`, directory by directory.
fn find_on_path(search_path: &OsStr, names: &[OsString]) -> Option<PathBuf> {
    std::env::split_paths(search_path)
        .filter(|directory| directory.is_absolute())
        .find_map(|directory| {
            names
                .iter()
                .map(|name| directory.join(name))
                .find(|candidate| candidate.is_file())
        })
}

/// Whether `path` is a bare command name such as `node`: one component and
/// no path separator, also no trailing one. A shell looks up such a name on
/// `PATH`.
fn is_bare_name(path: &Path) -> bool {
    path.file_name() == Some(path.as_os_str())
}

/// The file names to look up on `PATH` for the bare name `name`, in order.
/// On Windows, each name also matches its `.exe` and `.cmd` files, before
/// the literal name, unless it already ends in such an extension.
fn bare_name_candidates(name: &OsStr, windows: bool) -> Vec<OsString> {
    if !windows {
        return vec![name.to_owned()];
    }
    let executable = Path::new(name).extension().is_some_and(|extension| {
        WINDOWS_EXECUTABLE_EXTENSIONS
            .iter()
            .any(|known| extension.eq_ignore_ascii_case(known))
    });
    if executable {
        return vec![name.to_owned()];
    }
    let mut names: Vec<OsString> = WINDOWS_EXECUTABLE_EXTENSIONS
        .iter()
        .map(|extension| {
            let mut candidate = name.to_owned();
            candidate.push(".");
            candidate.push(extension);
            candidate
        })
        .collect();
    names.push(name.to_owned());
    names
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
/// `kinds` from `PATH`. A bare explicit name such as `node` is a `PATH`
/// lookup, as in a shell. An explicit path with a separator is a file path.
/// An explicit runtime of a kind outside `kinds` is an error, because the
/// runner cannot start on it.
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
    let search_path = std::env::var_os("PATH").unwrap_or_default();
    discover_runtime_in(explicit, kinds, &cwd, &search_path)
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
/// `cwd` and a bare explicit name looked up on `search_path`. The companion
/// starts in the project root, so the stored path must not depend on the
/// working directory.
fn discover_runtime_in(
    explicit: Option<&Path>,
    kinds: &[RuntimeKind],
    cwd: &Path,
    search_path: &OsStr,
) -> Result<JsRuntime, String> {
    let (kind, path) = if let Some(path) = explicit {
        let path = if is_bare_name(path) {
            find_on_path(
                search_path,
                &bare_name_candidates(path.as_os_str(), cfg!(windows)),
            ).ok_or_else(|| {
                format!(
                    "no `{}` executable on PATH; pass a path such as ./{0} for a file in the working directory",
                    path.display()
                )
            })?
        } else {
            cwd.join(path)
        };
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
        find_kinds_on_path(search_path, kinds)
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
        let runtime = discover_runtime_in(
            Some(Path::new("bin/node")),
            BOTH,
            cwd.path(),
            OsStr::new(""),
        )
        .expect("runtime");
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
    fn a_bare_explicit_runtime_name_resolves_on_path() {
        let cwd = tempfile::tempdir().expect("dir");
        let bin = tempfile::tempdir().expect("dir");
        let node = fake_runtime(bin.path(), "node", "v24.20.0");
        let bun = fake_runtime(bin.path(), "bun", "1.4.2");
        let search = std::env::join_paths([bin.path()]).expect("join");
        let runtime = discover_runtime_in(Some(Path::new("node")), BOTH, cwd.path(), &search)
            .expect("a bare name is a PATH lookup");
        assert_eq!(runtime.path, node);
        assert_eq!(runtime.kind, RuntimeKind::Node);
        assert_eq!(
            discover_runtime_in(Some(Path::new("bun")), BOTH, cwd.path(), &search)
                .expect("runtime")
                .path,
            bun
        );

        // A file of that name in the working directory does not shadow PATH,
        // as in a shell.
        fake_runtime(cwd.path(), "node", "v0.0.0-local");
        let runtime = discover_runtime_in(Some(Path::new("node")), BOTH, cwd.path(), &search)
            .expect("runtime");
        assert_eq!(runtime.path, node);
        let local = discover_runtime_in(Some(Path::new("./node")), BOTH, cwd.path(), &search)
            .expect("a path with a separator stays relative");
        assert_eq!(local.version, "v0.0.0-local");

        let empty = std::env::join_paths([cwd.path().join("none")]).expect("join");
        let error = discover_runtime_in(Some(Path::new("node")), BOTH, cwd.path(), &empty)
            .expect_err("no node on PATH");
        assert!(error.contains("no `node` executable on PATH"), "{error}");
    }

    #[test]
    fn only_a_name_without_any_separator_is_bare() {
        for bare in ["node", "node24.1", "bun-canary", "~"] {
            assert!(is_bare_name(Path::new(bare)), "{bare}");
        }
        for path in [
            "node/",
            "node/.",
            "./node",
            "bin/node",
            "/usr/bin/node",
            ".",
            "..",
            "",
        ] {
            assert!(!is_bare_name(Path::new(path)), "{path}");
        }
    }

    #[test]
    fn windows_bare_names_try_program_extensions_first() {
        let names = |name: &str| {
            bare_name_candidates(OsStr::new(name), true)
                .into_iter()
                .map(|candidate| candidate.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        assert_eq!(names("node"), ["node.exe", "node.cmd", "node"]);
        assert_eq!(
            names("node24.1"),
            ["node24.1.exe", "node24.1.cmd", "node24.1"]
        );
        assert_eq!(names("node.EXE"), ["node.EXE"]);
        assert_eq!(names("bun.cmd"), ["bun.cmd"]);
        assert_eq!(
            bare_name_candidates(OsStr::new("node"), false),
            [OsString::from("node")]
        );
    }

    #[test]
    fn a_non_utf8_bare_name_keeps_its_bytes() {
        use std::os::unix::ffi::OsStrExt;
        let name = OsStr::from_bytes(b"node\xff");
        assert_eq!(bare_name_candidates(name, false), [name.to_owned()]);
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

        let error = discover_runtime_in(Some(&node), &[RuntimeKind::Bun], first.path(), &search)
            .expect_err("bun test cannot start on node");
        assert!(error.contains("needs `bun`"), "{error}");
        assert_eq!(
            discover_runtime_in(Some(&bun), &[RuntimeKind::Bun], first.path(), &search)
                .expect("runtime")
                .kind,
            RuntimeKind::Bun
        );
    }
}
