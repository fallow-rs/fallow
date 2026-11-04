use std::path::{Path, PathBuf};

use fallow_types::extract::MemberKind;
use fallow_types::output_dead_code::{
    ReachabilityCaveat, UnusedClassMemberFinding, UnusedDependencyFinding, UnusedEnumMemberFinding,
    UnusedExportFinding, UnusedFileFinding, UnusedStoreMemberFinding,
};
use fallow_types::results::{
    AnalysisResults, DependencyLocation, UnusedDependency, UnusedExport, UnusedFile, UnusedMember,
};

use super::build_markdown;

fn caveated_results(root: &Path) -> AnalysisResults {
    let caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    let mut results = AnalysisResults::default();

    let mut file = UnusedFileFinding::with_actions(UnusedFile {
        path: root.join("src/lib.ts"),
    });
    file.reachability_caveats.clone_from(&caveats);
    results.unused_files.push(file);

    let mut export = UnusedExportFinding::with_actions(UnusedExport {
        path: root.join("src/api.ts"),
        export_name: "needed".to_owned(),
        is_type_only: false,
        line: 3,
        col: 0,
        span_start: 0,
        is_re_export: false,
        deprecated: false,
        deprecated_reason: None,
    });
    export.reachability_caveats.clone_from(&caveats);
    results.unused_exports.push(export);

    let mut dep = UnusedDependencyFinding::with_actions(UnusedDependency {
        package_name: "left-pad".to_owned(),
        location: DependencyLocation::Dependencies,
        path: root.join("package.json"),
        line: 5,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    });
    dep.reachability_caveats.clone_from(&caveats);
    results.unused_dependencies.push(dep);

    let member = |parent: &str, name: &str, kind| UnusedMember {
        path: root.join("src/api.ts"),
        parent_name: parent.to_owned(),
        member_name: name.to_owned(),
        kind,
        line: 7,
        col: 2,
    };
    let mut enum_member =
        UnusedEnumMemberFinding::with_actions(member("Mode", "Legacy", MemberKind::EnumMember));
    enum_member.reachability_caveats.clone_from(&caveats);
    results.unused_enum_members.push(enum_member);

    let mut class_member =
        UnusedClassMemberFinding::with_actions(member("Widget", "render", MemberKind::ClassMethod));
    class_member.reachability_caveats.clone_from(&caveats);
    results.unused_class_members.push(class_member);

    let mut store_member = UnusedStoreMemberFinding::with_actions(member(
        "useCart",
        "subtotal",
        MemberKind::StoreMember,
    ));
    store_member.reachability_caveats.clone_from(&caveats);
    results.unused_store_members.push(store_member);

    results
}

/// The markdown document is the PR-comment surface, so a reader deletes
/// straight off this listing. Every caveated finding line has to hedge.
#[test]
fn caveated_findings_hedge_their_markdown_lines() {
    let root = PathBuf::from("/project");

    let out = build_markdown(&caveated_results(&root), &root);

    assert!(
        out.contains("- `src/lib.ts` *(caveat: incomplete import graph)*"),
        "{out}"
    );
    assert!(
        out.contains("- :3 `needed` *(caveat: incomplete import graph)*"),
        "{out}"
    );
    assert!(
        out.contains("- `left-pad` *(caveat: incomplete import graph)*"),
        "{out}"
    );
    for expected in [
        "- :7 `Mode.Legacy` *(caveat: incomplete import graph)*",
        "- :7 `Widget.render` *(caveat: incomplete import graph)*",
        "- :7 `useCart.subtotal` *(caveat: incomplete import graph)*",
    ] {
        assert!(out.contains(expected), "missing {expected}: {out}");
    }
}

/// A run that read every file it discovered renders exactly as before.
#[test]
fn a_clean_run_renders_no_caveat() {
    let root = PathBuf::from("/project");
    let mut results = caveated_results(&root);
    results.unused_files[0].reachability_caveats.clear();
    results.unused_exports[0].reachability_caveats.clear();
    results.unused_dependencies[0].reachability_caveats.clear();
    results.unused_enum_members[0].reachability_caveats.clear();
    results.unused_class_members[0].reachability_caveats.clear();
    results.unused_store_members[0].reachability_caveats.clear();

    let out = build_markdown(&results, &root);

    assert!(!out.contains("caveat"), "{out}");
}
