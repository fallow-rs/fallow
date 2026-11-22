//! Markdown sections for unused members and namespace escapes.

use fallow_types::results::NamespaceEscape;

use super::*;

pub(super) fn push_markdown_member_sections(
    out: &mut String,
    results: &AnalysisResults,
    root: &Path,
) {
    markdown_grouped_section(
        out,
        &results.unused_enum_members,
        "Unused enum members",
        root,
        |m| m.member.path.as_path(),
        |m: &UnusedEnumMemberFinding| format_member(&m.member, &m.reachability_caveats),
    );
    markdown_grouped_section(
        out,
        &results.unused_class_members,
        "Unused class members",
        root,
        |m| m.member.path.as_path(),
        |m: &UnusedClassMemberFinding| format_member(&m.member, &m.reachability_caveats),
    );
    markdown_grouped_section(
        out,
        &results.unused_store_members,
        "Unused store members",
        root,
        |m| m.member.path.as_path(),
        |m: &UnusedStoreMemberFinding| format_member(&m.member, &m.reachability_caveats),
    );
    markdown_grouped_section(
        out,
        &results.unused_namespace_members,
        "Unused namespace members",
        root,
        |m| m.member.path.as_path(),
        |m: &UnusedNamespaceMemberFinding| format_member(&m.member, &m.reachability_caveats),
    );
    markdown_grouped_section(
        out,
        &results.unused_protected_class_members,
        "Unused protected class members",
        root,
        |m| m.member.path.as_path(),
        |m: &UnusedProtectedClassMemberFinding| format_member(&m.member, &m.reachability_caveats),
    );
    markdown_grouped_section(
        out,
        &results.namespace_escapes,
        "Namespace escapes",
        root,
        |e| e.escape.path.as_path(),
        |e: &NamespaceEscapeFinding| format_namespace_escape(&e.escape, root),
    );
}

fn format_namespace_escape(escape: &NamespaceEscape, root: &Path) -> String {
    let module = normalize_uri(&relative_path(&escape.module, root).display().to_string());
    format!(
        ":{} {}",
        escape.line,
        escape_markdown_inline(&escape.message(&module))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_types::results::NamespaceEscapeReason;

    /// A module path can hold markdown syntax, such as `[` in a route folder.
    /// The line must render it as plain text.
    #[test]
    fn namespace_escape_line_escapes_markdown_syntax() {
        let root = Path::new("/project");
        let escape = NamespaceEscape {
            path: root.join("src/app.ts"),
            line: 3,
            col: 0,
            local_name: "Lib".to_string(),
            module: root.join("src/[id]/<lib>.ts"),
            hidden_exports: 2,
            reason: NamespaceEscapeReason::WholeObjectUse,
        };
        assert_eq!(
            format_namespace_escape(&escape, root),
            r":3 namespace Lib is used as a whole value; 2 exports of src/%5Bid%5D/\<lib\>.ts are not checked"
        );
    }
}
