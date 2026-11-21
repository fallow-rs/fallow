use super::*;

const EXPORT: &str = "fallow/unused-export";
const ENUM_MEMBER: &str = "fallow/unused-enum-member";
const CLASS_MEMBER: &str = "fallow/unused-class-member";
const DELETE_GITHUB: &str = "\n\n```suggestion\n\n```";

/// The suggestion for the finding of `rule_id` on the 1-based `line` of a
/// file `src/dead.ts` with `source`.
fn suggest(provider: Provider, rule_id: &str, source: &str, line: u32) -> Option<String> {
    suggestion_block_for_issue_line(provider, rule_id, Path::new("src/dead.ts"), source, line)
}

#[test]
fn no_suggestion_for_a_re_export() {
    for provider in [Provider::Github, Provider::Gitlab] {
        assert_eq!(
            suggest(provider, EXPORT, "export { a } from './x';\n", 1),
            None
        );
    }
}

#[test]
fn no_suggestion_for_a_method_over_several_lines() {
    let source = "class C {\n  legacy() {\n    return null;\n  }\n}\n";
    assert_eq!(suggest(Provider::Github, CLASS_MEMBER, source, 2), None);
    let one_line = "class C {\n  legacy() { return null; }\n}\n";
    assert_eq!(
        suggest(Provider::Github, CLASS_MEMBER, one_line, 2).as_deref(),
        Some(DELETE_GITHUB)
    );
}

#[test]
fn no_suggestion_for_a_default_export_without_a_name() {
    for source in ["export default function () {}\n", "export default thing;\n"] {
        assert_eq!(
            suggest(Provider::Github, EXPORT, source, 1),
            None,
            "{source}"
        );
    }
}

#[test]
fn no_suggestion_for_a_line_with_two_enum_members() {
    let source = "enum E {\n  A, B,\n  C\n}\n";
    assert_eq!(suggest(Provider::Github, ENUM_MEMBER, source, 2), None);
    assert_eq!(
        suggest(Provider::Github, ENUM_MEMBER, source, 3).as_deref(),
        Some(DELETE_GITHUB)
    );
}

#[test]
fn no_suggestion_for_a_line_with_two_exports() {
    for source in [
        "export const a = 1; export const b = 2;\n",
        "export const a = 1, b = 2;\n",
        "export const a = 1;\nconst = ;\n",
    ] {
        assert_eq!(
            suggest(Provider::Github, EXPORT, source, 1),
            None,
            "{source}"
        );
    }
}
