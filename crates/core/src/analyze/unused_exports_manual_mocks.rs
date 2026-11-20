//! Manual mocks in duplicate-export groups.
//!
//! A manual mock at `<dir>/__mocks__/<rest>` stands in for the module at
//! `<dir>/<rest>` in tests. Jest, Vitest and Rstest all load it by this path
//! shape. The mock repeats the exports of its module on purpose, so the pair
//! is one origin, not a duplicate. The file extensions can differ, because a
//! test runner resolves the mock by its path without the extension.

use std::ffi::OsStr;
use std::path::{Component, Path, PathBuf};

use rustc_hash::FxHashSet;

use super::ExportEntry;

const MOCKS_DIR: &str = "__mocks__";

/// Drop each manual mock from `entries` when the module that it mocks is also
/// in `entries`. A mock without its own module in the group stays, so a mock
/// that repeats an export of an unrelated file is still reported. Entry order
/// is preserved.
pub(super) fn drop_manual_mocks_of_group_modules(entries: Vec<ExportEntry>) -> Vec<ExportEntry> {
    if !entries
        .iter()
        .any(|entry| mocked_module_key(&entry.path).is_some())
    {
        return entries;
    }
    let module_keys: FxHashSet<PathBuf> = entries
        .iter()
        .map(|entry| entry.path.with_extension(""))
        .collect();
    entries
        .into_iter()
        .filter(|entry| {
            mocked_module_key(&entry.path).is_none_or(|key| !module_keys.contains(&key))
        })
        .collect()
}

/// The path without extension of the module that the manual mock at `path`
/// stands in for, or `None` when `path` is not inside a `__mocks__`
/// directory. The last `__mocks__` component wins.
fn mocked_module_key(path: &Path) -> Option<PathBuf> {
    let components: Vec<Component<'_>> = path.components().collect();
    let mocks_index = components
        .iter()
        .rposition(|component| component.as_os_str() == OsStr::new(MOCKS_DIR))?;
    if mocks_index + 1 >= components.len() {
        return None;
    }
    let module: PathBuf = components[..mocks_index]
        .iter()
        .chain(&components[mocks_index + 1..])
        .collect();
    Some(module.with_extension(""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_a_mock_to_its_module_without_extension() {
        assert_eq!(
            mocked_module_key(Path::new("/p/src/lib/__mocks__/parse.js")),
            Some(PathBuf::from("/p/src/lib/parse"))
        );
        assert_eq!(
            mocked_module_key(Path::new("/p/src/__mocks__/nested/api.ts")),
            Some(PathBuf::from("/p/src/nested/api"))
        );
    }

    #[test]
    fn ignores_files_outside_a_mocks_directory() {
        assert_eq!(mocked_module_key(Path::new("/p/src/lib/parse.ts")), None);
        assert_eq!(mocked_module_key(Path::new("/p/src/__mocks__")), None);
        assert_eq!(
            mocked_module_key(Path::new("/p/src/not__mocks__/parse.ts")),
            None
        );
    }
}
