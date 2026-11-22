//! Audit key fixtures and tests for protected members and namespace escapes.

use super::*;

pub(super) fn unused_protected_class_member_results(root: &Path) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    results
        .unused_protected_class_members
        .push(UnusedProtectedClassMemberFinding::with_actions(
            UnusedMember {
                path: root.join("src/shapes.ts"),
                parent_name: "Shape".to_string(),
                member_name: "unusedHelper".to_string(),
                kind: MemberKind::ClassMethod,
                line: 12,
                col: 2,
            },
        ));
    results
}

#[test]
fn dead_code_keys_cover_unused_protected_class_member() {
    let root = root();
    let keys = dead_code_keys(&unused_protected_class_member_results(&root), &root);
    assert!(
        keys.contains("unused-protected-class-member:src/shapes.ts:Shape:unusedHelper"),
        "{keys:?}"
    );
}

pub(super) fn namespace_escape_results(root: &Path) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    results.namespace_escapes.push(
        fallow_types::output_dead_code::NamespaceEscapeFinding::with_actions(
            fallow_types::results::NamespaceEscape {
                path: root.join("src/app.ts"),
                line: 3,
                col: 0,
                local_name: "Formats".to_string(),
                module: root.join("src/formats.ts"),
                hidden_exports: 2,
                reason: fallow_types::results::NamespaceEscapeReason::WholeObjectUse,
            },
        ),
    );
    results
}

#[test]
fn dead_code_keys_cover_namespace_escape() {
    let root = root();
    let keys = dead_code_keys(&namespace_escape_results(&root), &root);
    assert!(
        keys.iter()
            .any(|key| key.starts_with("namespace-escape:src/app.ts:")),
        "{keys:?}"
    );
}
