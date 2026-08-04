//! Shared test fixtures for the analyze detectors.

use fallow_types::discover::FileId;
use fallow_types::extract::ModuleInfo;

/// A fully-zeroed [`ModuleInfo`] with `file_id == FileId(1)` for detector unit
/// tests. Construct with struct-update syntax: `ModuleInfo { angular_inputs:
/// vec![..], ..empty_module() }`.
#[must_use]
pub fn empty_module() -> ModuleInfo {
    ModuleInfo::empty(FileId(1))
}
