use std::path::Path;

use super::{PostEditViolation, SourceFacts};

/// Check the edit that replaces the first `old` of `source` with `new`.
fn check(file: &str, source: &str, old: &str, new: &str) -> Result<(), PostEditViolation> {
    let path = Path::new(file);
    let start = source.find(old).expect("the old text is in the source");
    let end = start + old.len();
    let after = format!("{}{new}{}", &source[..start], &source[end..]);
    let before = SourceFacts::collect(path, source).expect("the source parses");
    let deleted: Vec<(u32, u32)> = if new.is_empty() {
        vec![(
            u32::try_from(start).unwrap_or(u32::MAX),
            u32::try_from(end).unwrap_or(u32::MAX),
        )]
    } else {
        Vec::new()
    };
    before.check(path, &after, &deleted).map(|_| ())
}

#[test]
fn accepts_the_removal_of_an_unused_export_keyword() {
    let source = "export const a = 1;\nexport const b = 2;\nconsole.log(a, b);\n";
    assert_eq!(check("a.ts", source, "export const a", "const a"), Ok(()));
}

#[test]
fn refuses_a_text_that_does_not_parse() {
    let source = "export function a() {\n  return 1;\n}\n";
    assert_eq!(
        check("a.ts", source, "export function a() {", "function a("),
        Err(PostEditViolation::DoesNotParse)
    );
}

#[test]
fn refuses_a_deletion_that_shifts_a_kept_enum_value() {
    let source = "export enum Level {\n  Low,\n  Medium,\n  High,\n}\n";
    assert_eq!(
        check("a.ts", source, "  Low,\n", ""),
        Err(PostEditViolation::ValueChanged {
            enumeration: "Level".to_owned(),
            member: "Medium".to_owned(),
        })
    );
}

#[test]
fn accepts_a_deletion_with_a_pinned_value() {
    let source = "export enum Level {\n  Low,\n  Medium,\n  High,\n}\n";
    let offset = |text: &str| u32::try_from(source.find(text).expect("in source")).expect("small");
    let edits = [
        crate::source_edits::SpanEdit {
            start: offset("  Low,"),
            end: offset("  Medium,"),
            replacement: String::new(),
        },
        crate::source_edits::SpanEdit {
            start: offset("Medium"),
            end: offset("Medium") + 6,
            replacement: "Medium = 1".to_owned(),
        },
    ];
    assert_eq!(
        super::check_edits(Path::new("a.ts"), source, &edits, |_| false),
        Ok(())
    );
}

#[test]
fn evaluates_constant_enum_expressions() {
    let source = "enum Flag {\n  A = 1 << 0,\n  B = 1 << 1,\n  C = A | B,\n  D = Flag.C + 1,\n}\n";
    assert_eq!(
        check("a.ts", source, "  A = 1 << 0,\n", ""),
        Err(PostEditViolation::ValueChanged {
            enumeration: "Flag".to_owned(),
            member: "C".to_owned(),
        })
    );
    let strings = "enum Text {\n  A = 'a',\n  B = A + 'b',\n  C = `c`,\n}\n";
    assert_eq!(check("a.ts", strings, "  C = `c`,\n", ""), Ok(()));
}

#[test]
fn refuses_a_deletion_that_joins_two_class_members() {
    let source =
        "class Box {\n  count = 0\n  unused() {}\n  [Symbol.iterator]() {}\n}\nnew Box();\n";
    assert!(matches!(
        check("a.ts", source, "  unused() {}\n", ""),
        Err(PostEditViolation::MemberLost { .. } | PostEditViolation::DoesNotParse)
    ));
}

#[test]
fn accepts_the_removal_of_a_class_member() {
    let source = "class Box {\n  count = 0;\n  unused() {}\n  used() {}\n}\nnew Box().used();\n";
    assert_eq!(check("a.ts", source, "  unused() {}\n", ""), Ok(()));
}

#[test]
fn refuses_a_removed_declaration_that_code_still_uses() {
    let source = "import { helper } from './helper';\nhelper();\n";
    assert_eq!(
        check("a.ts", source, "import { helper } from './helper';\n", ""),
        Err(PostEditViolation::UnresolvedReference {
            name: "helper".to_owned()
        })
    );
}

#[test]
fn refuses_an_import_left_behind_by_a_removed_export_list_name() {
    let source =
        "import { a } from './a';\nimport { used } from './used';\n\nexport { a, used };\n";
    assert_eq!(
        check(
            "barrel.ts",
            source,
            "export { a, used };",
            "export { used };"
        ),
        Err(PostEditViolation::ImportLeftBehind {
            name: "a".to_owned()
        })
    );
}

#[test]
fn accepts_an_import_that_a_removed_member_used() {
    let source = "import { helper } from './helper';\nexport class Box {\n  unused() {\n    return helper();\n  }\n}\n";
    assert_eq!(
        check(
            "a.ts",
            source,
            "  unused() {\n    return helper();\n  }\n",
            ""
        ),
        Ok(())
    );
}

#[test]
fn reports_the_exports_of_a_text() {
    let source = "export const { a, b } = x;\nexport default 1;\nexport { c as d };\nconst c = 1;\nexport * as ns from './ns';\nexport { e } from './e';\n";
    let facts = SourceFacts::collect(Path::new("a.ts"), source).expect("parses");
    let mut names: Vec<&str> = facts.exports().iter().map(String::as_str).collect();
    names.sort_unstable();
    assert_eq!(names, ["a", "b", "d", "default", "e", "ns"]);
}

#[test]
fn checks_the_script_blocks_of_a_vue_file() {
    let source = "<script setup lang=\"ts\">\nenum Level {\n  Low,\n  High,\n}\nconsole.log(Level.High);\n</script>\n";
    assert!(matches!(
        check("a.vue", source, "  Low,\n", ""),
        Err(PostEditViolation::ValueChanged { .. })
    ));
}

#[test]
fn accepts_the_removal_of_a_member_that_holds_an_anonymous_class() {
    let source =
        "export class K {\n  u = class {\n    a() {}\n  };\n  v = class {\n    b() {}\n  };\n}\n";
    assert_eq!(
        check("a.ts", source, "  u = class {\n    a() {}\n  };\n", ""),
        Ok(())
    );
}

#[test]
fn accepts_the_removal_of_one_overload_signature() {
    let source = "export class K {\n  run(a: string): void;\n  run(a: number): void;\n  run(a: unknown) {}\n}\n";
    assert_eq!(
        check("a.ts", source, "  run(a: number): void;\n", ""),
        Ok(())
    );
}

#[test]
fn refuses_a_replacement_that_removes_a_kept_member() {
    let source = "enum Level {\n  Low = 0,\n  Medium = 1,\n}\nconsole.log(Level);\n";
    assert!(matches!(
        check("a.ts", source, "  Medium = 1,", "  Other = 1,"),
        Err(PostEditViolation::MemberLost { .. })
    ));
}

#[test]
fn check_edits_refuses_the_removal_of_an_export_that_is_not_removable() {
    let source = "export const { a, b } = { a: 1, b: 2 };\n";
    let edit = crate::source_edits::SpanEdit {
        start: 0,
        end: 7,
        replacement: String::new(),
    };
    assert_eq!(
        super::check_edits(
            Path::new("a.ts"),
            source,
            std::slice::from_ref(&edit),
            |name| name == "a"
        ),
        Err(PostEditViolation::ExportInUse {
            name: "b".to_owned()
        })
    );
    assert_eq!(
        super::check_edits(Path::new("a.ts"), source, &[edit], |_| true),
        Ok(())
    );
}

#[test]
fn refuses_an_edit_that_leaves_an_overload_half_exported() {
    let source = "export function f(a: string): void;\nexport function f(a: unknown) {}\n";
    assert_eq!(
        check(
            "a.ts",
            source,
            "export function f(a: unknown)",
            "function f(a: unknown)"
        ),
        Err(PostEditViolation::MixedExport {
            name: "f".to_owned()
        })
    );
    let both = "function f(a: string): void;\nfunction f(a: unknown) {}\nf('');\n";
    let path = Path::new("a.ts");
    let before = SourceFacts::collect(path, source).expect("parses");
    assert!(before.check(path, both, &[]).is_ok());
}

#[test]
fn accepts_a_value_and_a_type_of_one_name_with_different_exports() {
    let source = "export const Foo = 1;\nexport type Foo = number;\n";
    assert_eq!(
        check("a.ts", source, "export const Foo", "const Foo"),
        Ok(())
    );
    let interface = "export interface g {}\nexport function g() {}\n";
    assert_eq!(
        check("a.ts", interface, "export interface g", "interface g"),
        Ok(())
    );
}

#[test]
fn refuses_a_split_of_merged_declarations() {
    let class = "export interface Box {}\nexport class Box {}\n";
    assert!(matches!(
        check("a.ts", class, "export class Box", "class Box"),
        Err(PostEditViolation::MixedExport { .. })
    ));
    let namespace = "export function f() {}\nexport namespace f {}\n";
    assert!(matches!(
        check("a.ts", namespace, "export function f()", "function f()"),
        Err(PostEditViolation::MixedExport { .. })
    ));
}

#[test]
fn refuses_an_overload_split_from_a_default_export() {
    let source = "export function f(a: string): void;\nexport default function f(a: unknown) {}\n";
    assert!(matches!(
        check(
            "a.ts",
            source,
            "export function f(a: string)",
            "function f(a: string)"
        ),
        Err(PostEditViolation::MixedExport { .. })
    ));
}

/// Oxc puts the optional `;` after an index signature into its span. A `;`
/// that a member removal writes after the signature keeps the signature.
#[test]
fn a_semicolon_after_an_index_signature_keeps_the_signature() {
    let path = Path::new("a.ts");
    let source = "export class C {\n  [key: string]: unknown\n  rm(): void {}\n  [k](): void {}\n}\nconst k = Symbol();\n";
    let removed = "  rm(): void {}\n  ";
    let start = source.find(removed).expect("the member is in the source");
    let end = start + removed.len();
    let after = format!("{}  ;{}", &source[..start], &source[end..]);
    let before = SourceFacts::collect(path, source).expect("the source parses");
    let deleted = [(
        u32::try_from(start).unwrap_or(u32::MAX),
        u32::try_from(end).unwrap_or(u32::MAX),
    )];
    assert_eq!(before.check(path, &after, &deleted).map(|_| ()), Ok(()));
}
