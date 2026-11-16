//! Helpers for result merges, ignore policies and catalog sorting.

use super::*;

/// Move each finding that `covered` selects from `findings` to `hidden`.
pub(super) fn move_covered<T>(
    findings: &mut Vec<T>,
    hidden: &mut Vec<T>,
    path_of: impl Fn(&T) -> &Path,
    covered: &dyn Fn(&Path) -> bool,
) {
    let (moved, kept): (Vec<T>, Vec<T>) = std::mem::take(findings)
        .into_iter()
        .partition(|finding| covered(path_of(finding)));
    *findings = kept;
    hidden.extend(moved);
}

pub(super) fn all_nonempty_paths_match<'a>(
    mut paths: impl Iterator<Item = &'a PathBuf>,
    predicate: &mut impl FnMut(&Path) -> bool,
) -> bool {
    let Some(first) = paths.next() else {
        return false;
    };
    predicate(first) && paths.all(|path| predicate(path))
}

/// Sort key for catalog names: the default catalog ("default") sorts before any named catalog.
pub(super) fn catalog_sort_key(name: &str) -> (u8, &str) {
    if name == "default" {
        (0, name)
    } else {
        (1, name)
    }
}
