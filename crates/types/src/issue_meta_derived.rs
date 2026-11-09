//! Name lists derived from the issue metadata.

use super::*;

pub(super) fn known_issue_kind_names_from_meta() -> Vec<&'static str> {
    let mut names = Vec::new();
    for meta in ISSUE_KIND_META.iter().filter(|meta| meta.kind.is_some()) {
        push_unique(&mut names, meta.code);
        for alias in meta.aliases {
            push_unique(&mut names, *alias);
        }
    }
    names
}

pub(super) fn dead_code_filter_flags_from_meta() -> Vec<&'static str> {
    let mut flags = Vec::new();
    for meta in ISSUE_KIND_META {
        if let Some(flag) = meta.filter_flag {
            push_unique(&mut flags, flag);
        }
    }
    flags
}

pub(super) fn mcp_issue_type_flags_from_meta() -> Vec<(&'static str, &'static str)> {
    ISSUE_KIND_META
        .iter()
        .filter_map(|meta| meta.mcp_pair())
        .collect()
}

pub(super) fn codeclimate_result_codes_from_meta() -> Vec<&'static str> {
    ISSUE_RESULT_META
        .iter()
        .filter(|meta| meta.counts_in_total)
        .map(|meta| meta.code)
        .collect()
}

fn push_unique<T: Copy + PartialEq>(items: &mut Vec<T>, item: T) {
    if !items.contains(&item) {
        items.push(item);
    }
}
