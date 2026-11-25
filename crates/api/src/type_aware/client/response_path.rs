//! Check the file paths that the type-aware companion returns.
//!
//! The companion reports each path relative to the analysis root, which is
//! the base of its read scope. A path may use `..` segments, but with those
//! segments applied it must stay inside the root.

use std::path::{Component, Path};

use super::TypeAwareError;

/// Fail when `path` is absolute or leads above the analysis root.
pub(super) fn validate_semantic_response_path(
    path: &Path,
    query_id: usize,
) -> Result<(), TypeAwareError> {
    if stays_inside_root(path) {
        return Ok(());
    }
    Err(TypeAwareError::from(format!(
        "type-aware query {query_id} returned a path outside the project root"
    )))
}

/// True when the root-relative `path`, with each `..` and `.` applied by
/// text, stays at or below the root.
fn stays_inside_root(path: &Path) -> bool {
    let mut depth: usize = 0;
    for component in path.components() {
        match component {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir => match depth.checked_sub(1) {
                Some(up) => depth = up,
                None => return false,
            },
            Component::RootDir | Component::Prefix(_) => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_paths_may_use_parent_segments_that_stay_inside_the_root() {
        for path in [
            "src/a.ts",
            "./src/a.ts",
            "src/../lib/a.ts",
            "packages/app/../lib/a.ts",
        ] {
            assert!(
                validate_semantic_response_path(Path::new(path), 0).is_ok(),
                "{path}"
            );
        }
        for path in ["../shared/consumer.ts", "src/../../a.ts", "/abs/a.ts"] {
            assert!(
                validate_semantic_response_path(Path::new(path), 0).is_err(),
                "{path}"
            );
        }
    }
}
