//! Exports that only test-side files use.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::output_dead_code::TestImporters;
use crate::serde_path;

/// An exported value that production code does not use, while at least one
/// reachable test-side file does.
///
/// Test-side files are the files that production mode leaves out: test,
/// story, fixture and bench files and the `production.ignore` globs, plus
/// the files that only tests reach, such as a test helper.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TestOnlyExport {
    /// File that declares the export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Name of the export.
    pub export_name: String,
    /// 1-based line number of the export.
    pub line: u32,
    /// 0-based byte column offset.
    pub col: u32,
    /// The test-side files that use the export.
    pub test_importers: TestImporters,
    /// True when each test-side importer is in the directory of the
    /// declaring file or in a `__tests__` directory directly below it. A
    /// colocated test can use the export on purpose as an internal seam of
    /// the module.
    pub colocated_only: bool,
}

impl TestOnlyExport {
    /// The message of the finding, for example `export testOnly is used only
    /// by 2 test-side files`.
    #[must_use]
    pub fn message(&self) -> String {
        let count = self.test_importers.files.len() + self.test_importers.remaining;
        let noun = if count == 1 { "file" } else { "files" };
        format!(
            "export {} is used only by {count} test-side {noun}",
            self.export_name
        )
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn export(files: usize) -> TestOnlyExport {
        TestOnlyExport {
            path: PathBuf::from("src/lib.ts"),
            export_name: "seam".to_string(),
            line: 3,
            col: 13,
            test_importers: TestImporters::from_files(
                (0..files)
                    .map(|index| PathBuf::from(format!("src/{index}.test.ts")))
                    .collect(),
            )
            .expect("at least one importer"),
            colocated_only: true,
        }
    }

    #[test]
    fn message_counts_every_importer() {
        assert_eq!(
            export(1).message(),
            "export seam is used only by 1 test-side file"
        );
        assert_eq!(
            export(7).message(),
            "export seam is used only by 7 test-side files",
            "the count includes the importers past the cap"
        );
    }
}
