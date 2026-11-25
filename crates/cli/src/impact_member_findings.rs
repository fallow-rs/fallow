//! Impact records of member and namespace escape findings.

use super::*;

/// Push unused member, namespace escape and unprovided-inject findings.
pub(super) fn collect_member_findings(
    results: &AnalysisResults,
    push: &mut impl FnMut(&Path, &'static str, Option<String>),
) {
    for f in &results.unused_enum_members {
        push(
            &f.member.path,
            "unused-enum-member",
            Some(format!(
                "{}{ID_SEP}{}",
                f.member.parent_name, f.member.member_name
            )),
        );
    }
    for f in &results.unused_class_members {
        push(
            &f.member.path,
            "unused-class-member",
            Some(format!(
                "{}{ID_SEP}{}",
                f.member.parent_name, f.member.member_name
            )),
        );
    }
    for f in &results.unused_store_members {
        push(
            &f.member.path,
            "unused-store-member",
            Some(format!(
                "{}{ID_SEP}{}",
                f.member.parent_name, f.member.member_name
            )),
        );
    }
    for f in &results.unused_namespace_members {
        push(
            &f.member.path,
            "unused-namespace-member",
            Some(format!(
                "{}{ID_SEP}{}",
                f.member.parent_name, f.member.member_name
            )),
        );
    }
    for f in &results.unused_protected_class_members {
        push(
            &f.member.path,
            "unused-protected-class-member",
            Some(format!(
                "{}{ID_SEP}{}",
                f.member.parent_name, f.member.member_name
            )),
        );
    }
    for f in &results.unprovided_injects {
        push(
            &f.inject.path,
            "unprovided-inject",
            Some(f.inject.key_name.clone()),
        );
    }
    for f in &results.namespace_escapes {
        push(
            &f.escape.path,
            "namespace-escape",
            Some(f.escape.local_name.clone()),
        );
    }
    for f in &results.test_only_exports {
        push(
            &f.export.path,
            "test-only-export",
            Some(f.export.export_name.clone()),
        );
    }
}
