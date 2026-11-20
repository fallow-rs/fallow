//! The command and the option that an issue type row of the manifests names.

use fallow_types::issue_meta::IssueKindMeta;

/// The command and the option that select one issue type.
pub(super) struct IssueTypeSelection<'a> {
    pub(super) command: &'a str,
    pub(super) filter_flag: Option<&'static str>,
    pub(super) deprecated_filter_flag: Option<&'static str>,
}

/// The current command and option of an issue type. An architecture issue
/// type of the dead-code rule registry names `fallow architecture` and its
/// selection flag, and keeps the deprecated `fallow dead-code` option in
/// `deprecated_filter_flag`.
pub(super) fn issue_type_selection<'a>(
    command: &'a str,
    filter_flag: Option<&'static str>,
    shared: Option<&IssueKindMeta>,
) -> IssueTypeSelection<'a> {
    match shared.and_then(|meta| meta.architecture_flag()) {
        Some(architecture_flag) if command == "dead-code" => IssueTypeSelection {
            command: "architecture",
            filter_flag: Some(architecture_flag),
            deprecated_filter_flag: filter_flag,
        },
        _ => IssueTypeSelection {
            command,
            filter_flag,
            deprecated_filter_flag: None,
        },
    }
}
