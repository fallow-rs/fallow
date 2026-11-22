//! Human sections for unused members and namespace escapes.

use super::*;

pub(super) fn build_unused_member_sections(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    max_grouped_files: usize,
) {
    let has_fixable_enum_member = results
        .unused_enum_members
        .iter()
        .flat_map(|finding| &finding.actions)
        .any(fallow_types::output::IssueAction::is_auto_fixable);
    build_human_grouped_section_with_fixability(
        GroupedSectionInput {
            lines,
            items: &results.unused_enum_members,
            title: "Unused enum members",
            level: severity_to_level(rules.unused_enum_members),
            root,
            max_files: max_grouped_files,
            get_path: |m| m.member.path.as_path(),
            format_detail: &|m: &UnusedEnumMemberFinding| {
                format_unused_member(&m.member, &m.reachability_caveats)
            },
        },
        has_fixable_enum_member,
    );

    let has_fixable_class_member = results
        .unused_class_members
        .iter()
        .flat_map(|finding| &finding.actions)
        .any(fallow_types::output::IssueAction::is_auto_fixable);
    build_human_grouped_section_with_fixability(
        GroupedSectionInput {
            lines,
            items: &results.unused_class_members,
            title: "Unused class members",
            level: severity_to_level(rules.unused_class_members),
            root,
            max_files: max_grouped_files,
            get_path: |m| m.member.path.as_path(),
            format_detail: &|m: &UnusedClassMemberFinding| {
                format_unused_member(&m.member, &m.reachability_caveats)
            },
        },
        has_fixable_class_member,
    );

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_store_members,
        title: "Unused store members",
        level: severity_to_level(rules.unused_store_members),
        root,
        max_files: max_grouped_files,
        get_path: |m| m.member.path.as_path(),
        format_detail: &|m: &UnusedStoreMemberFinding| {
            format_unused_member(&m.member, &m.reachability_caveats)
        },
    });
    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_namespace_members,
        title: "Unused namespace members",
        level: severity_to_level(rules.unused_namespace_members),
        root,
        max_files: max_grouped_files,
        get_path: |m| m.member.path.as_path(),
        format_detail: &|m: &UnusedNamespaceMemberFinding| {
            format_unused_member(&m.member, &m.reachability_caveats)
        },
    });
    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_protected_class_members,
        title: "Unused protected class members",
        level: severity_to_level(rules.unused_protected_class_members),
        root,
        max_files: max_grouped_files,
        get_path: |m| m.member.path.as_path(),
        format_detail: &|m: &UnusedProtectedClassMemberFinding| {
            format_unused_member(&m.member, &m.reachability_caveats)
        },
    });
    build_namespace_escapes_section(lines, results, root, rules);
}

fn build_namespace_escapes_section(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) {
    let items = &results.namespace_escapes;
    if items.is_empty() {
        return;
    }
    let title = "Namespace escapes";
    let level = severity_to_level(rules.namespace_escapes);
    lines.push(build_section_header(title, items.len(), level));
    for finding in items {
        let escape = &finding.escape;
        let location = format!(
            "{}:{}",
            format_display_path(&escape.path, root),
            escape.line
        );
        let module = format_display_path(&escape.module, root);
        lines.push(format!(
            "  {} {}",
            format_path(&location),
            escape.message(&module)
        ));
    }
    lines.push(String::new());
    push_section_footer_with_count(lines, title, items.len());
}
