//! GitHub annotations for unused members and namespace escapes.

use super::*;

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
                "Namespace '{}' is used as a whole value, so {} exports of {} are not checked.\n\nRead the members by name, or suppress the import with // fallow-ignore-next-line namespace-escape.",
                s(it, "local_name"),
                u(it, "hidden_exports"),
                s(it, "module"),
            )
        },
    );
}
