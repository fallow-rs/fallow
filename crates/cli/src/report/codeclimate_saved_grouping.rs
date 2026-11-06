//! Owner and group annotations, and audit flags, for the CodeClimate issues
//! of a saved report.

use std::path::Path;

use fallow_output::{CodeClimateAnnotationField, CodeClimateIssue, annotate_codeclimate_issues};
use fallow_types::duplicates::DuplicationReport;

use super::super::github_annotations::EnvelopeKind;
use super::super::grouping::{self, OwnershipResolver};
use super::cc_path;

pub(super) fn annotate_dead_code_owner(
    issues: &mut [CodeClimateIssue],
    resolver: Option<&OwnershipResolver>,
) {
    let Some(resolver) = resolver else {
        return;
    };
    annotate_codeclimate_issues(issues, CodeClimateAnnotationField::Owner, |path| {
        grouping::resolve_owner(Path::new(path), Path::new(""), resolver)
    });
}

pub(super) fn annotate_health_groups(
    issues: &mut [CodeClimateIssue],
    resolver: Option<&OwnershipResolver>,
) {
    let Some(resolver) = resolver else {
        return;
    };
    annotate_codeclimate_issues(issues, CodeClimateAnnotationField::Group, |path| {
        grouping::resolve_owner(Path::new(path), Path::new(""), resolver)
    });
}

pub(super) fn annotate_duplication_groups(
    issues: &mut [CodeClimateIssue],
    report: &DuplicationReport,
    root: &Path,
    resolver: Option<&OwnershipResolver>,
) {
    let Some(resolver) = resolver else {
        return;
    };
    use rustc_hash::FxHashMap;
    let mut path_to_owner: FxHashMap<String, String> = FxHashMap::default();
    for group in &report.clone_groups {
        let owner = super::super::dupes_grouping::largest_owner(group, root, resolver);
        for instance in &group.instances {
            path_to_owner.insert(cc_path(&instance.file, root), owner.clone());
        }
    }
    annotate_codeclimate_issues(issues, CodeClimateAnnotationField::Group, |path| {
        path_to_owner
            .get(path)
            .cloned()
            .unwrap_or_else(|| crate::codeowners::UNOWNED_LABEL.to_string())
    });
}

pub(super) fn annotate_saved_fallback_grouping(
    issues: &mut [CodeClimateIssue],
    kind: EnvelopeKind,
    resolver: Option<&OwnershipResolver>,
) {
    match kind {
        EnvelopeKind::Health | EnvelopeKind::Dupes => annotate_health_groups(issues, resolver),
        _ => annotate_dead_code_owner(issues, resolver),
    }
}

/// The audit flag of each saved clone group, in report order. The typed
/// clone group has no `introduced` field, so the flags come from the JSON.
/// `None` when a group has no flag: the section is not from an audit with a
/// base snapshot.
pub(super) fn saved_clone_group_flags(
    envelope: &serde_json::Value,
    pointer: &str,
) -> Option<Vec<bool>> {
    envelope
        .pointer(pointer)?
        .get("clone_groups")?
        .as_array()?
        .iter()
        .map(|group| group.get("introduced")?.as_bool())
        .collect()
}
