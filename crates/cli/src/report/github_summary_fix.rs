//! Step summary of `fallow fix`.

use super::*;

/// Port of `summary-fix.jq`.
#[must_use]
pub fn render_fix_summary(env: &Value) -> String {
    let exports = fix_entries(env, "remove_export");
    let dependencies = fix_entries(env, "remove_dependency");
    let fix_attempts = arr(env, "fixes").filter(|fix| !b(fix, "skipped")).count();
    let content_changed = u(env, "skipped_content_changed") as usize;
    let mixed_eol = u(env, "skipped_mixed_line_endings") as usize;
    let low_confidence = u(env, "skipped_low_confidence_exports") as usize;
    let low_confidence_deps = u(env, "skipped_low_confidence_dependencies") as usize;
    let low_confidence_members = u(env, "skipped_low_confidence_members") as usize;
    let dry_run = b(env, "dry_run");

    if fix_attempts == 0
        && content_changed == 0
        && mixed_eol == 0
        && low_confidence == 0
        && low_confidence_deps == 0
        && low_confidence_members == 0
    {
        return "## Fallow - Auto-fix\n\nNo fixable issues found.".to_owned();
    }

    let mut out = String::from("## Fallow - Auto-fix\n\n");
    out.push_str(if dry_run {
        "**Dry run**: would apply"
    } else {
        "Applied"
    });
    let fix_noun = if fix_attempts == 1 { "fix" } else { "fixes" };
    let _ = write!(out, " **{fix_attempts} {fix_noun}**");
    if !dry_run {
        let _ = write!(out, " ({} succeeded)", num(env, "total_fixed"));
    }
    if content_changed > 0 {
        let _ = write!(
            out,
            ", skipped {content_changed} file(s) that changed since analysis"
        );
    }
    if mixed_eol > 0 {
        let _ = write!(out, ", skipped {mixed_eol} file(s) with mixed line endings");
    }
    if low_confidence > 0 {
        let _ = write!(
            out,
            ", kept exports in {low_confidence} file(s) where consumers may be hidden from static analysis"
        );
    }
    if low_confidence_deps > 0 {
        let _ = write!(
            out,
            ", kept {low_confidence_deps} declared package(s) whose only import may sit in a file this run did not fully read"
        );
    }
    if low_confidence_members > 0 {
        let _ = write!(
            out,
            ", kept {low_confidence_members} unused enum member(s) whose only reference may sit in a file this run did not fully analyze"
        );
    }
    out.push_str("\n\n| Type | Count |\n|------|-------|\n");
    if !exports.is_empty() {
        let _ = writeln!(out, "| Export removals | {} |", exports.len());
    }
    if !dependencies.is_empty() {
        let _ = writeln!(out, "| Dependency removals | {} |", dependencies.len());
    }
    out.push_str("\n<details>\n<summary>View details</summary>\n\n");
    if !exports.is_empty() {
        out.push_str(&fix_detail_block("Export removals", &exports, |it| {
            format!(
                "- {} - {}",
                markdown_code_span(&format!("{}:{}", s(it, "path"), num(it, "line"))),
                markdown_code_span(s(it, "name"))
            )
        }));
        out.push_str("\n\n");
    }
    if !dependencies.is_empty() {
        out.push_str(&fix_detail_block(
            "Dependency removals",
            &dependencies,
            |it| {
                format!(
                    "- {} from {} in {}",
                    markdown_code_span(s(it, "package")),
                    markdown_code_span(s(it, "location")),
                    markdown_code_span(s(it, "file")),
                )
            },
        ));
        out.push('\n');
    }
    out.push_str("\n\n</details>");
    out
}
