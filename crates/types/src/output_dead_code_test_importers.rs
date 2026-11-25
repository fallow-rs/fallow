//! The test-side importers of an unused export of a production run
//! (issue #5251).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The most files that one [`TestImporters`] lists.
pub const TEST_IMPORTERS_CAP: usize = 5;

/// Text of the `note` on the `remove-export` action of an unused export
/// that a test-side file imports.
pub const TEST_IMPORTERS_NOTE: &str = "Production code does not use this export, but a test, story, fixture or other file that production mode excludes imports it. A removal of the export breaks that import. Change or remove the import first, or keep the export.";

/// The reachable files outside the production scope that import an export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TestImporters {
    /// The importing files, sorted by path, at most 5.
    #[serde(serialize_with = "crate::serde_path::serialize_vec")]
    pub files: Vec<PathBuf>,
    /// The number of importing files that `files` does not list.
    pub remaining: usize,
}

impl TestImporters {
    /// Build the capped list from all importing files. Returns `None` when
    /// `files` is empty.
    #[must_use]
    pub fn from_files(mut files: Vec<PathBuf>) -> Option<Self> {
        if files.is_empty() {
            return None;
        }
        files.sort_unstable();
        files.dedup();
        let remaining = files.len().saturating_sub(TEST_IMPORTERS_CAP);
        files.truncate(TEST_IMPORTERS_CAP);
        Some(Self { files, remaining })
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{TEST_IMPORTERS_CAP, TestImporters};

    #[test]
    fn no_files_gives_no_test_importers() {
        assert_eq!(TestImporters::from_files(Vec::new()), None);
    }

    #[test]
    fn files_are_sorted_deduplicated_and_capped() {
        let mut files: Vec<PathBuf> = (0..TEST_IMPORTERS_CAP + 2)
            .rev()
            .map(|index| PathBuf::from(format!("/p/test/{index}.test.ts")))
            .collect();
        files.push(PathBuf::from("/p/test/0.test.ts"));
        let importers = TestImporters::from_files(files).unwrap();
        assert_eq!(importers.files.len(), TEST_IMPORTERS_CAP);
        assert_eq!(importers.files[0], PathBuf::from("/p/test/0.test.ts"));
        assert!(importers.files.is_sorted());
        assert_eq!(importers.remaining, 2);
    }
}
