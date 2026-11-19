//! Step summary of `fallow fix`.

use super::*;

/// The withholding counters of the fix envelope, with the clause each one
/// adds to the headline: `, {before}{count}{after}`.
const SKIP_CLAUSES: [(&str, &str, &str); 7] = [
    (
        "skipped_content_changed",
        "skipped ",
        " file(s) that changed since analysis",
    ),
    (
        "skipped_mixed_line_endings",
        "skipped ",
        " file(s) with mixed line endings",
    ),
    (
        "skipped_low_confidence_exports",
        "kept exports in ",
        " file(s) where consumers may be hidden from static analysis",
    ),
    (
        "skipped_low_confidence_dependencies",
        "kept ",
        " declared package(s) whose only import may sit in a file this run did not fully read",
    ),
    (
        "skipped_low_confidence_members",
        "kept ",
        " unused enum member(s) whose only reference may sit in a file this run did not fully analyze",
    ),
    (
        "skipped_kept_re_exports",
        "kept ",
        " unused export(s) that a kept re-export still names",
    ),
    (
        "skipped_shared_declarations",
        "kept ",
        " unused export(s) whose `export` keyword also exports a name that is in use",
    ),
];

/// Port of `summary-fix.jq`.
#[must_use]
pub fn render_fix_summary(env: &Value) -> String {
    let exports = fix_entries(env, "remove_export");
    let dependencies = fix_entries(env, "remove_dependency");
    let fix_attempts = arr(env, "fixes").filter(|fix| !b(fix, "skipped")).count();
    let skip_counts = SKIP_CLAUSES.map(|(key, _, _)| u(env, key) as usize);
    let dry_run = b(env, "dry_run");

    if fix_attempts == 0 && skip_counts.iter().all(|count| *count == 0) {
        return "## Fallow fix report\n\nNo fixable issues found.".to_owned();
    }

    let mut out = String::from("## Fallow fix report\n\n");
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
    for ((_, before, after), count) in SKIP_CLAUSES.iter().zip(skip_counts) {
        if count > 0 {
            let _ = write!(out, ", {before}{count}{after}");
        }
    }
    out.push_str("\n\n| Type | Count |\n|------|-------|\n");
    if !exports.is_empty() {
        let _ = writeln!(out, "| Export removals | {} |", exports.len());
    }
    if !dependencies.is_empty() {
        let _ = writeln!(out, "| Dependency removals | {} |", dependencies.len());
    }
    push_fix_details(&mut out, &exports, &dependencies);
    out
}

fn push_fix_details(out: &mut String, exports: &[&Value], dependencies: &[&Value]) {
    out.push_str("\n<details>\n<summary>View details</summary>\n\n");
    if !exports.is_empty() {
        out.push_str(&fix_detail_block("Export removals", exports, |it| {
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
            dependencies,
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
}
