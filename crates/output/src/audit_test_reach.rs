//! Test reach of the changed exports in the review brief.

use serde::Serialize;

/// Maximum number of rows in [`TestReachFacts::exports`]. The rest is counted
/// in [`TestReachFacts::exports_omitted`].
pub const TEST_REACH_EXPORT_CAP: usize = 25;

/// Maximum number of files in [`ExportTestReachFact::tests`] and
/// [`ExportTestReachFact::mocked_targets`].
pub const TEST_REACH_FILE_CAP: usize = 3;

/// Whether the project has tests to measure against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TestReachStatus {
    /// The project has test files, and each changed export has a reach class.
    Measured,
    /// The project has no test files. Every changed export counts as
    /// `none-file`.
    NoTests,
}

/// Why mock detection is or is not active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TestReachMasking {
    /// At least one test replaces a module with a closed factory mock, and
    /// `mocked-only` can be reported.
    Active,
    /// No test declares a closed factory mock.
    NoMocks,
    /// Too many distinct mock sets: mock detection is off, so a mocked
    /// module counts as reached.
    Capped,
}

/// How tests reach one changed export.
///
/// Treat this as an open set: a later version can add a class. A consumer
/// must read an unknown value as "needs attention".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum TestReachClass {
    /// Only tests that replace a module on the import path with a mock
    /// reference the export. The real code does not run in those tests.
    MockedOnly,
    /// Tests load the declaring file, but no test-reachable module references
    /// the export.
    None,
    /// No test loads the declaring file.
    NoneFile,
    /// Tests reach the export only through other modules.
    Transitive,
    /// A test file references the export.
    Direct,
    /// A framework reads the export as configuration with a literal value, so
    /// it needs no test.
    Framework,
}

/// Whether the export existed before the change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ExportChange {
    /// The file exported this name at the base of the diff, and the diff
    /// changes its declaration. An edited, rewritten or moved declaration is
    /// `modified`.
    Modified,
    /// The file did not export this name at the base: a new export, a
    /// renamed export, or an export of a new file.
    New,
}

/// Number of changed exports per reach class. The keys are exactly the
/// [`TestReachClass`] values.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TestReachCounts {
    /// Exports that a test file references.
    pub direct: usize,
    /// Exports that tests reach only through other modules.
    pub transitive: usize,
    /// Exports that only mocking tests reference.
    #[serde(rename = "mocked-only")]
    pub mocked_only: usize,
    /// Exports in a test-reached file without a test-reachable reference.
    pub none: usize,
    /// Exports in a file that no test loads.
    #[serde(rename = "none-file")]
    pub none_file: usize,
    /// Framework configuration exports.
    pub framework: usize,
}

impl TestReachCounts {
    /// Count one export of `class`.
    pub const fn add(&mut self, class: TestReachClass) {
        match class {
            TestReachClass::Direct => self.direct += 1,
            TestReachClass::Transitive => self.transitive += 1,
            TestReachClass::MockedOnly => self.mocked_only += 1,
            TestReachClass::None => self.none += 1,
            TestReachClass::NoneFile => self.none_file += 1,
            TestReachClass::Framework => self.framework += 1,
        }
    }
}

/// One changed export that needs attention.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExportTestReachFact {
    /// Root-relative path of the file that declares the export.
    pub file: String,
    /// The export name, `default` for a default export.
    pub export_name: String,
    /// 1-based line of the export.
    pub line: u32,
    /// The reach class.
    pub reach: TestReachClass,
    /// Whether the diff adds or changes the declaration.
    pub change: ExportChange,
    /// Test files that name the export, sorted, at most
    /// [`TEST_REACH_FILE_CAP`]. On a `mocked-only` row these are the mocking
    /// tests. Absent when no test file names the export.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests: Option<Vec<String>>,
    /// Number of test files that name the export, before the cap. Present with
    /// `tests`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests_total: Option<usize>,
    /// True when one of the test files that name the export changed in this
    /// diff. Present with `tests`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_changed: Option<bool>,
    /// The mocked modules on the import paths, sorted, at most
    /// [`TEST_REACH_FILE_CAP`]. A mocked module can be a barrel in front of
    /// the declaring file. Present only on `mocked-only` rows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mocked_targets: Option<Vec<String>>,
}

/// How tests reach the value exports that the diff changes.
///
/// A changed export is a value export whose declaring statement overlaps an
/// added line. The section is advisory: it never changes the verdict or the
/// exit code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TestReachFacts {
    /// `measured`, or `no-tests` when the project has no test files.
    pub status: TestReachStatus,
    /// Whether mock detection is active.
    pub masking: TestReachMasking,
    /// Number of changed value exports.
    pub total: usize,
    /// Number of changed exports per reach class.
    pub counts: TestReachCounts,
    /// The exports that need attention (`mocked-only`, `none`, `none-file`,
    /// `transitive`), at most [`TEST_REACH_EXPORT_CAP`]. The order is reach
    /// class, then `modified` before `new`, then file, line and name. Empty
    /// for `no-tests`.
    pub exports: Vec<ExportTestReachFact>,
    /// Number of attention rows left out by the cap.
    pub exports_omitted: usize,
    /// True when the project uses an end-to-end test runner (Playwright or
    /// Cypress). End-to-end tests drive the running app and do not import
    /// source files, so the reach classes do not count them. Absent when no
    /// such runner is present.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub e2e_runner: bool,
}
