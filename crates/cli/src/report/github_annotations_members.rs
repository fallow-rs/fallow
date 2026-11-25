//! GitHub annotations for unused members and namespace escapes.

use super::*;
use fallow_types::results::NamespaceEscapeReason;

pub(super) fn collect_check_members(env: &Value, out: &mut Vec<Annotation>) {
    push_each(
        out,
        env,
        "unused_enum_members",
        "Unused enum member",
        Anchor::line_col,
        |it| {
            format!(
                "Enum member '{}.{}' is never referenced in the codebase.\n\nRemove the member.{}",
                s(it, "parent_name"),
                s(it, "member_name"),
                caveat_note(it),
            )
        },
    );
    push_each(
        out,
        env,
        "unused_class_members",
        "Unused class member",
        Anchor::line_col,
        |it| {
            format!(
                "Class member '{}.{}' is never referenced.\n\nRemove the member, or make it private.{}",
                s(it, "parent_name"),
                s(it, "member_name"),
                caveat_note(it),
            )
        },
    );
    push_each(
        out,
        env,
        "unused_protected_class_members",
        "Unused protected class member",
        Anchor::line_col,
        |it| {
            format!(
                "Protected class member '{}.{}' is never referenced by its class or a subclass.\n\nRemove the member, or make it private.{}",
                s(it, "parent_name"),
                s(it, "member_name"),
                caveat_note(it),
            )
        },
    );
    push_each(
        out,
        env,
        "unused_namespace_members",
        "Unused namespace member",
        Anchor::line_col,
        |it| {
            format!(
                "Namespace member '{}.{}' is never referenced.\n\nRemove the member, or remove its export keyword when only the namespace body uses it.{}",
                s(it, "parent_name"),
                s(it, "member_name"),
                caveat_note(it),
            )
        },
    );
    push_each(
        out,
        env,
        "unused_store_members",
        "Unused store member",
        Anchor::line_col,
        |it| {
            format!(
                "Store member '{}.{}' is never accessed by any consumer.\n\nRemove the unused state, getter or action.{}",
                s(it, "parent_name"),
                s(it, "member_name"),
                caveat_note(it),
            )
        },
    );
    push_each(
        out,
        env,
        "namespace_escapes",
        "Namespace escape",
        Anchor::line_col,
        |it| {
            format!(
                "{}.\n\nRead the members by name, or suppress the import with // fallow-ignore-next-line namespace-escape.",
                namespace_escape_message(it)
            )
        },
    );
    collect_test_only_exports(env, out);
}

fn collect_test_only_exports(env: &Value, out: &mut Vec<Annotation>) {
    push_each(
        out,
        env,
        "test_only_exports",
        "Test-only export",
        Anchor::line_col,
        |it| {
            format!(
                "{}.\n\nRemove the export and its tests, or test through the public interface of the module. To keep it, suppress the line with // fallow-ignore-next-line test-only-export.",
                test_only_export_message(it)
            )
        },
    );
}

/// The text of `TestOnlyExport::message` for a JSON finding. The jq filter
/// `annotations-check.jq` builds the same text.
fn test_only_export_message(it: &Value) -> String {
    let importers = &it["test_importers"];
    let count = importers["files"].as_array().map_or(0, Vec::len)
        + usize::try_from(u(importers, "remaining")).unwrap_or(0);
    let noun = if count == 1 { "file" } else { "files" };
    format!(
        "export {} is used only by {count} test-side {noun}",
        s(it, "export_name")
    )
}

/// The text of `NamespaceEscape::message` for a JSON finding. The jq filter
/// `annotations-check.jq` builds the same text.
fn namespace_escape_message(it: &Value) -> String {
    let reason = it
        .get("reason")
        .cloned()
        .and_then(|value| serde_json::from_value::<NamespaceEscapeReason>(value).ok())
        .unwrap_or(NamespaceEscapeReason::WholeObjectUse);
    let count = u(it, "hidden_exports");
    let (noun, verb) = if count == 1 {
        ("export", "is")
    } else {
        ("exports", "are")
    };
    format!(
        "namespace {} {}; {count} {noun} of {} {verb} not checked",
        s(it, "local_name"),
        reason.description(),
        s(it, "module"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message_for(reason: &str, hidden_exports: u64) -> String {
        let env = serde_json::json!({
            "namespace_escapes": [{
                "path": "src/app.ts",
                "line": 3,
                "col": 7,
                "local_name": "Formats",
                "module": "src/formats.ts",
                "hidden_exports": hidden_exports,
                "reason": reason,
            }]
        });
        let mut out = Vec::new();
        collect_check_members(&env, &mut out);
        out.pop().expect("one annotation").message
    }

    #[test]
    fn namespace_escape_annotation_uses_singular_for_one_export() {
        assert!(
            message_for("whole-object-use", 1).starts_with(
                "namespace Formats is used as a whole value; 1 export of src/formats.ts is not checked.\n\n"
            )
        );
    }

    #[test]
    fn namespace_escape_annotation_names_the_reason() {
        assert!(message_for("re-exported-binding", 2).starts_with(
            "namespace Formats is re-exported as a whole object; 2 exports of src/formats.ts are not checked."
        ));
        assert!(message_for("no-member-read", 2).starts_with(
            "namespace Formats has no member read by name; 2 exports of src/formats.ts are not checked."
        ));
    }
}
