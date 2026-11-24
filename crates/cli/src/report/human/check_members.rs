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
    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.namespace_escapes,
        title: "Namespace escapes",
        level: severity_to_level(rules.namespace_escapes),
        root,
        max_files: max_grouped_files,
        get_path: |e| e.escape.path.as_path(),
        format_detail: &|e: &NamespaceEscapeFinding| {
            let escape = &e.escape;
            let module = format_display_path(&escape.module, root);
            format!(
                "{} {}",
                format!(":{}", escape.line).dimmed(),
                escape.message(&module)
            )
        },
    });
}

#[cfg(test)]
mod tests {
    use super::super::super::plain;
    use super::*;
    use fallow_types::results::{NamespaceEscape, NamespaceEscapeReason};
    use std::path::PathBuf;

    fn escape(root: &Path, file: &str, line: u32, hidden_exports: u32) -> NamespaceEscapeFinding {
        NamespaceEscapeFinding::with_actions(NamespaceEscape {
            path: root.join(file),
            line,
            col: 7,
            local_name: "Formats".to_string(),
            module: root.join("src/formats.ts"),
            hidden_exports,
            reason: NamespaceEscapeReason::WholeObjectUse,
        })
    }

    #[test]
    fn namespace_escapes_group_by_file_under_the_file_cap() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .namespace_escapes
            .push(escape(&root, "src/a.ts", 3, 1));
        results
            .namespace_escapes
            .push(escape(&root, "src/a.ts", 9, 2));
        results
            .namespace_escapes
            .push(escape(&root, "src/b.ts", 4, 2));
        let rules = RulesConfig::default();

        let text = plain(&build_human_lines(&results, &root, &rules, Some(1)));

        assert!(text.contains("src/a.ts"), "{text}");
        assert!(
            text.contains(
                ":3 namespace Formats is used as a whole value; 1 export of src/formats.ts is not checked"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                ":9 namespace Formats is used as a whole value; 2 exports of src/formats.ts are not checked"
            ),
            "{text}"
        );
        assert!(!text.contains(":4 namespace Formats"), "{text}");
    }
}
