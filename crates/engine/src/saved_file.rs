//! Read back the files that fallow saves, such as baselines, snapshots and
//! setup state.
//!
//! A flag or a default names each file. A cloned repository can commit a
//! symlink at that path, so a path below the project root or the working
//! directory must resolve inside that directory. A path outside both is a
//! plain path that the user names, and it stays allowed. Each file must be a
//! regular file of at most [`DEFAULT_MAX_READ_BYTES`] bytes.
//!
//! A parse error names the position only, because the serde text can quote a
//! value of the file.

use std::path::{Path, PathBuf};

use fallow_types::path_guard::lexically_normalized;
use fallow_types::read_guard::{
    DEFAULT_MAX_READ_BYTES, ReadRefusal, is_inside_root, read_to_string_bounded,
    read_to_string_inside,
};

/// Read the saved file at `path`, with `root` and the working directory as
/// bases. A relative `path` is taken relative to the working directory.
///
/// # Errors
///
/// Returns a [`ReadRefusal`] when the path below `root` or the working
/// directory resolves outside it, the file is not a regular file, is too
/// large, is not UTF-8, or cannot be read. When the working directory cannot
/// be read, a relative path is refused.
pub fn read_saved_file(root: &Path, path: &Path) -> Result<String, ReadRefusal> {
    read_saved_text(Some(root), path, DEFAULT_MAX_READ_BYTES)
}

/// True when `path`, with `..` and `.` applied by text, is `base` or below
/// it. See [`base_in_path_spelling`].
#[must_use]
pub fn lies_below(base: &Path, path: &Path) -> bool {
    base_in_path_spelling(base, path).is_some()
}

/// The ancestor of `path`, with `..` and `.` applied by text, that is
/// `base`. A base can have another spelling through a link, such as `/tmp`
/// for `/private/tmp` on macOS, so an ancestor that resolves to `base` also
/// counts. A link below that ancestor does not move `path` out of `base`:
/// the caller then checks where that link resolves. `None` when `path` is
/// not below `base`.
#[must_use]
pub fn base_in_path_spelling(base: &Path, path: &Path) -> Option<PathBuf> {
    let normalized = lexically_normalized(path);
    if normalized.starts_with(base) {
        return Some(base.to_path_buf());
    }
    let canonical_base = std::fs::canonicalize(base).ok()?;
    normalized
        .ancestors()
        .find(|ancestor| {
            *ancestor == canonical_base
                || std::fs::canonicalize(ancestor).is_ok_and(|resolved| resolved == canonical_base)
        })
        .map(Path::to_path_buf)
}

/// [`read_saved_file`] for a caller that has no project root. Only the
/// working directory is a base.
///
/// # Errors
///
/// Returns a [`ReadRefusal`] as [`read_saved_file`] does.
pub fn read_saved_file_in_cwd(path: &Path) -> Result<String, ReadRefusal> {
    read_saved_text(None, path, DEFAULT_MAX_READ_BYTES)
}

/// [`read_saved_file`] with an optional `root` and the limit `max_bytes`.
///
/// # Errors
///
/// Returns a [`ReadRefusal`] as [`read_saved_file`] does.
pub fn read_saved_text(
    root: Option<&Path>,
    path: &Path,
    max_bytes: u64,
) -> Result<String, ReadRefusal> {
    let cwd = std::env::current_dir();
    let absolute = match (&cwd, path.is_absolute()) {
        (_, true) => path.to_path_buf(),
        (Ok(cwd), false) => cwd.join(path),
        (Err(error), false) => {
            return Err(ReadRefusal::Io {
                path: path.to_path_buf(),
                source: std::io::Error::new(error.kind(), error.to_string()),
            });
        }
    };
    read_saved_file_from(root, cwd.ok().as_deref(), &absolute, max_bytes)
}

fn read_saved_file_from(
    root: Option<&Path>,
    cwd: Option<&Path>,
    absolute: &Path,
    max_bytes: u64,
) -> Result<String, ReadRefusal> {
    let base = [root, cwd]
        .into_iter()
        .flatten()
        .find(|base| lies_below(base, absolute));
    match base {
        Some(base) => read_to_string_inside(base, absolute, max_bytes),
        None => read_to_string_bounded(absolute, max_bytes),
    }
}

/// List the saved-file directory `dir` below `root`, when it resolves inside
/// `root`. Read each entry with [`read_saved_file`].
///
/// # Errors
///
/// Returns a `PermissionDenied` error when `dir` resolves outside `root`, and
/// the error of `read_dir` otherwise.
pub(crate) fn read_saved_dir(root: &Path, dir: &Path) -> std::io::Result<std::fs::ReadDir> {
    let entries = std::fs::read_dir(dir)?;
    if is_inside_root(root, dir) {
        Ok(entries)
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            format!(
                "refusing to list {}: it resolves outside {}",
                dir.display(),
                root.display()
            ),
        ))
    }
}

/// The position of a JSON error, for a message that must not quote the file.
/// An error of a typed conversion has no position, so it gets a fixed text.
#[must_use]
pub fn json_error_position(error: &serde_json::Error) -> String {
    if error.line() == 0 {
        "the content does not have the expected structure".to_owned()
    } else {
        format!(
            "parse error at line {}, column {}",
            error.line(),
            error.column()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn refuses_a_link_below_the_root_or_the_working_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        let base = dunce::canonicalize(dir.path()).expect("canonical");
        let root = base.join("cwd").join("project");
        std::fs::create_dir_all(&root).expect("mkdir");
        let cwd = base.join("cwd");
        let outside = base.join("other.json");
        std::fs::write(&outside, r#"{"value": "outside"}"#).expect("write");
        std::os::unix::fs::symlink(&outside, root.join("baseline.json")).expect("link");
        std::os::unix::fs::symlink(&outside, cwd.join("baseline.json")).expect("link");
        std::fs::write(root.join("own.json"), "{}").expect("write");

        for linked in [root.join("baseline.json"), cwd.join("baseline.json")] {
            let refusal = read_saved_file_from(Some(&root), Some(&cwd), &linked, 1024).unwrap_err();
            assert!(
                matches!(refusal, ReadRefusal::OutsideRoot { .. }),
                "{refusal}"
            );
        }
        assert_eq!(
            read_saved_file_from(Some(&root), Some(&cwd), &root.join("own.json"), 1024)
                .expect("own"),
            "{}"
        );
        assert!(
            read_saved_file_from(Some(&root), Some(&cwd), &outside, 1024).is_ok(),
            "a plain path outside both directories stays allowed"
        );
        assert!(matches!(
            read_saved_file_from(Some(&root), Some(&cwd), &root, 1024).unwrap_err(),
            ReadRefusal::NotRegularFile { .. }
        ));
    }

    #[test]
    #[cfg(unix)]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn refuses_to_list_a_directory_link_out_of_the_root() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path().join("project");
        let outside = dir.path().join("elsewhere");
        std::fs::create_dir_all(&root).expect("mkdir");
        std::fs::create_dir_all(&outside).expect("mkdir");
        std::os::unix::fs::symlink(&outside, root.join("snapshots")).expect("link");
        std::fs::create_dir(root.join("own")).expect("mkdir");

        assert!(read_saved_dir(&root, &root.join("snapshots")).is_err());
        assert!(read_saved_dir(&root, &root.join("own")).is_ok());
    }

    #[test]
    #[cfg(unix)]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_base_spelled_through_a_link_still_counts() {
        let dir = tempfile::tempdir().expect("temp dir");
        let real = dunce::canonicalize(dir.path())
            .expect("canonical")
            .join("project");
        std::fs::create_dir_all(&real).expect("mkdir");
        let alias = real.parent().expect("parent").join("alias");
        std::os::unix::fs::symlink(&real, &alias).expect("link");

        assert_eq!(
            base_in_path_spelling(&real, &alias.join("a/b.json")),
            Some(alias.clone())
        );
        assert!(lies_below(&real, &real.join("a/../b.json")));
        assert!(!lies_below(&real, &real.join("../b.json")));
    }

    #[test]
    fn json_errors_name_the_position_only() {
        let error = serde_json::from_str::<u32>("\"hidden-value\"").unwrap_err();
        let message = json_error_position(&error);
        assert!(!message.contains("hidden-value"), "{message}");
        assert!(message.contains("line 1"), "{message}");
    }
}
