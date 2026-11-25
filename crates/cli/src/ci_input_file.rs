//! Read the input files of the `ci` commands, such as `--body`,
//! `--envelope` and the PR decision surface.
//!
//! A path below the working directory must resolve inside it, so a link
//! that the checkout commits cannot point the read at a file outside the
//! checkout. A path outside the working directory is a plain path that the
//! user names, and it stays allowed. Each input must be a regular file of at
//! most [`DEFAULT_MAX_READ_BYTES`] bytes.

use std::path::Path;

use fallow_engine::saved_file::lies_below;
use fallow_types::read_guard::{DEFAULT_MAX_READ_BYTES, is_inside_root, read_to_string_bounded};

/// Read the `ci` input file at `path`, with `label` in each error.
pub(super) fn read_text_file(path: &Path, label: &str) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| {
        format!(
            "failed to read {label} '{}': the working directory cannot be read ({e})",
            path.display()
        )
    })?;
    read_text_file_from(&cwd, path, label)
}

fn read_text_file_from(cwd: &Path, path: &Path, label: &str) -> Result<String, String> {
    let absolute = cwd.join(path);
    if lies_below(cwd, &absolute) && !is_inside_root(cwd, &absolute) {
        // A missing file below the working directory reads as missing.
        if absolute.symlink_metadata().is_ok() {
            return Err(format!(
                "refusing to read {label} '{}': it resolves to a location outside the working directory",
                path.display()
            ));
        }
    }
    read_to_string_bounded(&absolute, DEFAULT_MAX_READ_BYTES)
        .map_err(|e| format!("failed to read {label} '{}': {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn refuses_a_link_below_the_working_directory_that_leaves_it() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cwd = dunce::canonicalize(dir.path())
            .expect("canonical")
            .join("checkout");
        std::fs::create_dir_all(cwd.join("sub")).expect("mkdir");
        let outside = cwd.parent().expect("parent").join("other.md");
        std::fs::write(&outside, "outside text").expect("write");
        std::os::unix::fs::symlink(&outside, cwd.join("comment.md")).expect("link");
        std::fs::write(cwd.join("own.md"), "own text").expect("write");

        let err = read_text_file_from(&cwd, Path::new("comment.md"), "body").unwrap_err();
        assert!(!err.contains("outside text"), "{err}");
        assert!(read_text_file_from(&cwd, Path::new("sub/../comment.md"), "body").is_err());
        assert_eq!(
            read_text_file_from(&cwd, Path::new("own.md"), "body").as_deref(),
            Ok("own text")
        );
        assert_eq!(
            read_text_file_from(&cwd, &outside, "body").as_deref(),
            Ok("outside text"),
            "a plain path outside the working directory stays allowed"
        );
        assert!(read_text_file_from(&cwd, Path::new("sub"), "body").is_err());
        assert!(read_text_file_from(&cwd, Path::new("missing.md"), "body").is_err());

        // The same link, named through another spelling of the working directory.
        let alias = cwd.parent().expect("parent").join("alias");
        std::os::unix::fs::symlink(&cwd, &alias).expect("link");
        assert!(read_text_file_from(&cwd, &alias.join("comment.md"), "body").is_err());
    }
}
