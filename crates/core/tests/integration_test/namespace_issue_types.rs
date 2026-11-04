//! Pin how fallow reports unused members of a namespace.
//!
//! - `export * as ns from "./m"` with some members used: the other members
//!   are unused exports or unused types of `./m`.
//! - `import * as ns` where the namespace object escapes (a call argument):
//!   the consumer can read any export, so no export is reported.
//! - `export namespace N { export const a }` with `a` unused: fallow reports
//!   `N.a` as an unused namespace member, not as an unused export.

use super::common::{create_config, fixture_path};

#[cfg_attr(miri, ignore)]
#[test]
fn namespace_members_report_through_unused_exports_and_types() {
    let root = fixture_path("namespace-issue-types");
    let mut config = create_config(root);
    config.include_entry_exports = false;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let name_of = |path: &std::path::Path, name: &str| {
        let path = path.to_string_lossy().replace('\\', "/");
        let file = path.rsplit('/').next().unwrap_or_default().to_string();
        format!("{file}:{name}")
    };
    let mut exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|f| name_of(&f.export.path, &f.export.export_name))
        .collect();
    exports.sort();
    let mut types: Vec<String> = results
        .unused_types
        .iter()
        .map(|f| name_of(&f.export.path, &f.export.export_name))
        .collect();
    types.sort();

    assert_eq!(exports, ["members.ts:unusedValue"]);
    assert_eq!(types, ["members.ts:UnusedType"]);
    assert!(
        results.unused_class_members.is_empty() && results.unused_enum_members.is_empty(),
        "a TypeScript namespace member is not a class or enum member"
    );
    let namespace_members: Vec<String> = results
        .unused_namespace_members
        .iter()
        .map(|f| {
            name_of(
                &f.member.path,
                &format!("{}.{}", f.member.parent_name, f.member.member_name),
            )
        })
        .collect();
    assert_eq!(namespace_members, ["legacy.ts:Legacy.unusedInner"]);
}
