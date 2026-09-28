use std::path::PathBuf;

use fallow_output::{CiIssue, CiProvider as Provider};
use fallow_types::output_dead_code::description_carries_caveat;

#[must_use]
pub fn suggestion_block(provider: Provider, issue: &CiIssue) -> Option<String> {
    let mut block = fix_intent(issue).map(fix_intent_block).unwrap_or_default();
    if issue.rule_id.contains("unused-file") {
        block.push_str(&unused_file_hint());
        return Some(block);
    }
    if issue.line == 0 {
        return (!block.is_empty()).then_some(block);
    }

    let root = std::env::var_os("FALLOW_ROOT").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let path = root.join(&issue.path);
    let Some(source) = std::fs::read_to_string(path).ok() else {
        return (!block.is_empty()).then_some(block);
    };
    let Some(line) = source.lines().nth(issue.line.saturating_sub(1) as usize) else {
        return (!block.is_empty()).then_some(block);
    };
    if let Some(suggestion) = suggestion_or_withheld_hint(provider, issue, line) {
        block.push_str(&suggestion);
    }
    (!block.is_empty()).then_some(block)
}

/// The edit block for this finding's line, or the note that says why it was
/// withheld.
///
/// The gate is asked here, on the block this finding would actually have
/// produced, rather than up front on the caveat alone. A dependency finding
/// carries a caveat on every incomplete run and never had a suggestion block;
/// announcing a withheld one-click fix there would invent a fix that never
/// existed, which is the same overclaiming in the other direction.
#[must_use]
fn suggestion_or_withheld_hint(provider: Provider, issue: &CiIssue, line: &str) -> Option<String> {
    let suggestion = suggestion_block_for_issue_line(provider, &issue.rule_id, line)?;
    Some(if description_carries_caveat(&issue.description) {
        withheld_suggestion_hint()
    } else {
        suggestion
    })
}

#[must_use]
pub fn fix_intent(issue: &CiIssue) -> Option<&'static str> {
    match issue.rule_id.as_str() {
        "fallow/unused-file" => Some("Delete the file or add a real entry-point reference."),
        "fallow/unused-export" => {
            Some("Remove the export or mark it public if it is part of the API.")
        }
        "fallow/code-duplication" => Some("Extract the repeated block or centralize shared logic."),
        "fallow/high-crap-score" | "fallow/high-complexity" => {
            Some("Split branches or add focused tests around the risky path.")
        }
        "fallow/unresolved-import" => Some("Fix the import path or install the missing package."),
        _ => None,
    }
}

fn fix_intent_block(intent: &str) -> String {
    format!("\n\n> Fix intent: {intent}")
}

#[must_use]
pub fn suggestion_block_for_issue_line(
    provider: Provider,
    rule_id: &str,
    line: &str,
) -> Option<String> {
    if rule_id.contains("unused-import") {
        return unused_import_suggestion(provider, line);
    }
    if rule_id.contains("unused-enum-member") || rule_id.contains("unused-class-member") {
        return delete_line_suggestion(provider, line);
    }
    if rule_id.contains("unused-export") || rule_id.contains("unused-type") {
        return unused_export_suggestion(provider, line);
    }
    None
}

/// Text hint for `unused-file` findings. Neither GitHub nor GitLab supports
/// file-scope deletion suggestions through the review-comment API.
///
/// It names no command because there is none: `delete-file` ships
/// `auto_fixable: false` on every run, and `fallow fix` has no file-deletion
/// path at all. The previous text pointed at `fallow fix --files`, a flag that
/// has never existed and which the CLI rejects with `unexpected argument`.
#[must_use]
fn unused_file_hint() -> String {
    "\n\n> No automatic fix: `fallow fix` never deletes files. Delete it by hand once you have \
     confirmed nothing loads it at runtime, or keep it and add `// fallow-ignore-file \
     unused-file` at the top."
        .to_owned()
}

/// Replacement for the suggestion block on a finding whose run flagged its own
/// evidence as incomplete.
///
/// A `suggestion` block is a MUTATION SURFACE, not prose: on GitHub it is one
/// click from a commit on the contributor's branch, and on GitLab it is one
/// click from an applied change. Every other mutation surface asks
/// `MutationEvidence::may_auto_apply_mutation` before offering the write, and
/// this one never did: it dispatched on `rule_id` alone, so a caveated
/// unused-export and a caveated enum member both shipped a committable edit
/// with only a parenthetical several lines up saying the evidence was
/// incomplete. A caveat inside a sentence is a disclosure; withholding the
/// block is the gate.
///
/// The finding itself is untouched: same comment, same location, same
/// description carrying its own caveat text. Only the one-click edit is gone,
/// and this says why and what restores it.
#[must_use]
fn withheld_suggestion_hint() -> String {
    "\n\n> No one-click fix offered: this run did not fully analyze every file, so a reference \
     that would credit this finding may never have been seen. Resolve the files named in \
     `workspace_diagnostics[]` and re-run, or apply the change by hand."
        .to_owned()
}

fn unused_export_suggestion(provider: Provider, line: &str) -> Option<String> {
    let fixed = line
        .strip_prefix("export default ")
        .or_else(|| line.strip_prefix("export "))?;
    if fixed == line {
        return None;
    }

    match provider {
        Provider::Github => Some(format!("\n\n```suggestion\n{fixed}\n```")),
        Provider::Gitlab => Some(format!("\n\n```suggestion:-0+0\n{fixed}\n```")),
    }
}

/// Delete the matched line entirely for unused enum/class members.
fn delete_line_suggestion(provider: Provider, line: &str) -> Option<String> {
    if line.trim().is_empty() {
        return None;
    }
    match provider {
        Provider::Github => Some("\n\n```suggestion\n\n```".to_owned()),
        Provider::Gitlab => Some("\n\n```suggestion:-0+0\n\n```".to_owned()),
    }
}

fn unused_import_suggestion(provider: Provider, line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with("import ") {
        return None;
    }

    let import_target = trimmed.strip_prefix("import ")?.trim_start();
    if import_target.starts_with('"') || import_target.starts_with('\'') {
        return None;
    }

    let (clause, _) = import_target.split_once(" from ")?;
    let clause = clause
        .trim()
        .strip_prefix("type ")
        .unwrap_or_else(|| clause.trim())
        .trim();
    if clause.contains(',') {
        return None;
    }
    if let Some(named) = clause
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
    {
        let named = named.trim();
        if named.is_empty() || named.contains(',') {
            return None;
        }
    }

    match provider {
        Provider::Github => Some("\n\n```suggestion\n\n```".to_string()),
        Provider::Gitlab => Some("\n\n```suggestion:-0+0\n\n```".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_github_suggestion() {
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Github,
                "fallow/unused-export",
                "export const value = 1;"
            )
            .as_deref(),
            Some("\n\n```suggestion\nconst value = 1;\n```")
        );
    }

    #[test]
    fn renders_gitlab_suggestion() {
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Gitlab,
                "fallow/unused-export",
                "export default thing;"
            )
            .as_deref(),
            Some("\n\n```suggestion:-0+0\nthing;\n```")
        );
    }

    #[test]
    fn renders_unused_type_export_suggestion() {
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Github,
                "fallow/unused-type",
                "export type Legacy = { id: string };"
            )
            .as_deref(),
            Some("\n\n```suggestion\ntype Legacy = { id: string };\n```")
        );
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Gitlab,
                "fallow/unused-type",
                "export interface Legacy { id: string }"
            )
            .as_deref(),
            Some("\n\n```suggestion:-0+0\ninterface Legacy { id: string }\n```")
        );
    }

    #[test]
    fn fix_intent_names_common_review_actions() {
        let issue = CiIssue {
            rule_id: "fallow/code-duplication".to_owned(),
            description: "clone".to_owned(),
            severity: "minor".to_owned(),
            path: "src/a.ts".to_owned(),
            line: 1,
            end_line: None,
            other_locations: Vec::new(),
            fingerprint: "abc".to_owned(),
            legacy_fingerprint: None,
        };

        assert_eq!(
            fix_intent(&issue),
            Some("Extract the repeated block or centralize shared logic.")
        );
    }

    #[test]
    fn unused_type_suggestion_is_conservative() {
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Github,
                "fallow/unused-type",
                "  export type Indented = string;"
            ),
            None
        );
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Github,
                "fallow/unused-type",
                "type Local = string;"
            ),
            None
        );
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Github,
                "fallow/unused-type",
                "const used = 1; export type Legacy = string;"
            ),
            None
        );
    }

    #[test]
    fn renders_unused_import_delete_suggestion() {
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Github,
                "fallow/unused-import",
                "import { unused } from './module';"
            )
            .as_deref(),
            Some("\n\n```suggestion\n\n```")
        );
    }

    #[test]
    fn skips_side_effect_imports() {
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Github,
                "fallow/unused-import",
                "import './setup';"
            ),
            None
        );
    }

    #[test]
    fn skips_mixed_import_bindings() {
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Github,
                "fallow/unused-import",
                "import { used, unused } from './module';"
            ),
            None
        );
    }

    #[test]
    fn renders_unused_enum_member_delete_suggestion() {
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Github,
                "fallow/unused-enum-member",
                "  Deprecated,"
            )
            .as_deref(),
            Some("\n\n```suggestion\n\n```")
        );
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Gitlab,
                "fallow/unused-enum-member",
                "  Deprecated,"
            )
            .as_deref(),
            Some("\n\n```suggestion:-0+0\n\n```")
        );
    }

    #[test]
    fn store_members_never_offer_unverified_line_deletions() {
        for provider in [Provider::Github, Provider::Gitlab] {
            assert!(
                suggestion_block_for_issue_line(
                    provider,
                    "fallow/unused-store-member",
                    "  reset: () => set({ count: 0 }),"
                )
                .is_none()
            );
        }
    }

    #[test]
    fn renders_unused_class_member_delete_suggestion() {
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Github,
                "fallow/unused-class-member",
                "  legacyMethod() { return null; }"
            )
            .as_deref(),
            Some("\n\n```suggestion\n\n```")
        );
    }

    fn issue(rule_id: &str, description: &str) -> CiIssue {
        CiIssue {
            rule_id: rule_id.to_owned(),
            description: description.to_owned(),
            severity: "major".to_owned(),
            path: "src/dead.ts".to_owned(),
            line: 1,
            end_line: None,
            other_locations: Vec::new(),
            fingerprint: "abc".to_owned(),
            legacy_fingerprint: None,
        }
    }

    #[test]
    fn unused_file_hint_uses_text_not_suggestion_block() {
        let body = suggestion_block(
            Provider::Github,
            &issue("fallow/unused-file", "File is not reachable"),
        )
        .expect("hint");
        assert!(!body.contains("```suggestion"), "must not be a code block");
        assert!(body.contains("`fallow fix` never deletes files"));
        assert!(body.contains("fallow-ignore-file unused-file"));
    }

    /// The hint used to name `fallow fix --files`, which the CLI rejects with
    /// `unexpected argument`. Advice a reader cannot run is worse than no
    /// advice, so no flag that does not parse may appear here.
    #[test]
    fn no_hint_names_a_flag_the_cli_does_not_accept() {
        let body = suggestion_block(
            Provider::Github,
            &issue("fallow/unused-file", "File is not reachable"),
        )
        .expect("hint");
        assert!(
            !body.contains("fallow fix --"),
            "the unused-file hint must not invent a `fallow fix` flag: {body}"
        );
    }

    /// The blocker: a `suggestion` block is one click from a commit, so it is a
    /// mutation surface and answers to the same gate every other one does. A
    /// caveated finding keeps its comment and its caveat text and loses only
    /// the committable edit.
    #[test]
    fn a_caveated_finding_gets_no_committable_suggestion() {
        for (rule, line) in [
            (
                "fallow/unused-class-member",
                "  legacyMethod() { return null; }",
            ),
            ("fallow/unused-enum-member", "  Deprecated,"),
            ("fallow/unused-export", "export const value = 1;"),
        ] {
            for provider in [Provider::Github, Provider::Gitlab] {
                let body = suggestion_or_withheld_hint(
                    provider,
                    &issue(
                        rule,
                        "Something is never referenced (caveat: incomplete import graph)",
                    ),
                    line,
                )
                .expect("a finding that had a block still renders something");
                assert!(
                    !body.contains("```suggestion"),
                    "{rule}: a caveated finding must ship no one-click edit: {body}"
                );
                assert!(
                    body.contains("No one-click fix offered"),
                    "{rule}: and must say why: {body}"
                );
            }
        }
    }

    /// The other half: the gate must not withhold the suggestion from a run
    /// that read every file it discovered.
    #[test]
    fn an_uncaveated_finding_keeps_its_suggestion() {
        assert_eq!(
            suggestion_or_withheld_hint(
                Provider::Github,
                &issue(
                    "fallow/unused-class-member",
                    "Class member 'Widget.legacyMethod' is never referenced"
                ),
                "  legacyMethod() { return null; }"
            )
            .as_deref(),
            Some("\n\n```suggestion\n\n```"),
            "a clean run keeps exactly the behavior it had"
        );
    }

    /// A caveat on a finding that never had a block must not announce a
    /// withheld fix. A dependency finding carries a caveat on every incomplete
    /// run, and there was never a one-click removal for it.
    #[test]
    fn a_caveat_alone_does_not_invent_a_withheld_fix() {
        assert_eq!(
            suggestion_or_withheld_hint(
                Provider::Github,
                &issue(
                    "fallow/unused-dependency",
                    "Dependency 'lodash' is never imported (caveat: incomplete import graph)"
                ),
                "  \"lodash\": \"^4.17.21\","
            ),
            None
        );
    }

    #[test]
    fn delete_line_suggestion_skips_blank_lines() {
        assert_eq!(
            suggestion_block_for_issue_line(Provider::Github, "fallow/unused-enum-member", "   "),
            None
        );
    }
}
