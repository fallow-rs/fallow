use std::path::{Path, PathBuf};

use fallow_engine::source_edits::SourceEditIndex;
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
    if let Some(suggestion) = suggestion_or_withheld_hint(
        provider,
        issue,
        Path::new(&issue.path),
        &source,
        u32::try_from(issue.line).unwrap_or(u32::MAX),
    ) {
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
fn suggestion_or_withheld_hint(
    provider: Provider,
    issue: &CiIssue,
    path: &Path,
    source: &str,
    line: u32,
) -> Option<String> {
    let suggestion = suggestion_block_for_issue_line(provider, &issue.rule_id, path, source, line)?;
    Some(if description_carries_caveat(&issue.description) {
        withheld_suggestion_hint()
    } else {
        suggestion
    })
}

#[must_use]
pub fn fix_intent(issue: &CiIssue) -> Option<&'static str> {
    match issue.rule_id.as_str() {
        "fallow/unused-file" => Some("Delete the file, or import it from an entry point."),
        "fallow/unused-export" => Some("Remove the export, or keep it when it is public API."),
        "fallow/code-duplication" => Some("Move the repeated code into one shared function."),
        "fallow/high-crap-score" | "fallow/high-complexity" => {
            Some("Split the function, or add tests for the risky branches.")
        }
        "fallow/unresolved-import" => Some("Fix the import path, or install the missing package."),
        "fallow/long-file" => Some("Move groups of related exports to their own modules."),
        _ => None,
    }
}

fn fix_intent_block(intent: &str) -> String {
    format!("\n\n**Fix:** {intent}")
}

/// The `suggestion` block for the finding of `rule_id` on the 1-based `line`
/// of the file at `path` with the text `source`.
///
/// The export and member rules parse the file. They give a block only when
/// the parser confirms that the edit of the one line changes exactly the
/// statement or the member of the finding.
#[must_use]
pub fn suggestion_block_for_issue_line(
    provider: Provider,
    rule_id: &str,
    path: &Path,
    source: &str,
    line: u32,
) -> Option<String> {
    let line_text = source.lines().nth(line.checked_sub(1)? as usize)?;
    if rule_id.contains("unused-import") {
        return unused_import_suggestion(provider, line_text);
    }
    let is_member_rule =
        rule_id.contains("unused-enum-member") || rule_id.contains("unused-class-member");
    let is_export_rule = rule_id.contains("unused-export") || rule_id.contains("unused-type");
    if !is_member_rule && !is_export_rule {
        return None;
    }
    let index = SourceEditIndex::parse(path, source)?;
    let line_start = line_start(source, line)?;
    if is_member_rule {
        let deletable = member_line_is_deletable(&index, source, line, line_start, line_text)
            && deletion_keeps_other_members(path, &index, source, line_start, line_text);
        return deletable.then(|| delete_line_suggestion(provider));
    }
    let fixed = unused_export_line(&index, line, line_start, line_text)?;
    Some(match provider {
        Provider::Github => format!("\n\n```suggestion\n{fixed}\n```"),
        Provider::Gitlab => format!("\n\n```suggestion:-0+0\n{fixed}\n```"),
    })
}

/// The byte offset of the start of the 1-based `line` of `source`.
fn line_start(source: &str, line: u32) -> Option<usize> {
    let index = line.checked_sub(1)? as usize;
    if index == 0 {
        return Some(0);
    }
    source
        .match_indices('\n')
        .nth(index - 1)
        .map(|(offset, _)| offset + 1)
}

/// The text of `line` without the `export` of its one statement, when the
/// line holds one complete edit: one export statement starts at the line
/// start, no other statement covers the line, the statement exports one
/// name, and its keyword edit ends on the line.
fn unused_export_line<'a>(
    index: &SourceEditIndex,
    line: u32,
    line_start: usize,
    line_text: &'a str,
) -> Option<&'a str> {
    let mut on_line = index.export_statements_on_line(line);
    let (Some(statement), None) = (on_line.next(), on_line.next()) else {
        return None;
    };
    let line_idx = line.saturating_sub(1) as usize;
    let covered_by_another = index.statements().iter().any(|other| {
        let (first, last) = index.line_range(other.start, other.end);
        other.start != statement.start && (first..=last).contains(&line_idx)
    });
    // An overload or a merged declaration of the same name must keep its
    // `export` together with the other declarations of the name.
    let shares_its_name = index.statements().iter().any(|other| {
        other.start != statement.start
            && other
                .names
                .iter()
                .any(|name| statement.names.contains(name))
    });
    if covered_by_another
        || shares_its_name
        || statement.start as usize != line_start
        || statement.names.len() != 1
    {
        return None;
    }
    let edit = statement.keyword_edit()?;
    let removed = (edit.end as usize).checked_sub(line_start)?;
    line_text.get(removed..)
}

/// True when the line holds exactly one member, from its first byte to its
/// last, with only white space before it and only an optional `,` or `;`,
/// white space and an optional `//` comment after it.
fn member_line_is_deletable(
    index: &SourceEditIndex,
    source: &str,
    line: u32,
    line_start: usize,
    line_text: &str,
) -> bool {
    let mut on_line = index.members_on_line(line);
    let (Some(member), None) = (on_line.next(), on_line.next()) else {
        return false;
    };
    let (first, last) = index.line_range(member.start, member.end);
    if first != last || !member.deletion_keeps_values || member.shares_its_key {
        return false;
    }
    let Some(before) = source.get(line_start..member.start as usize) else {
        return false;
    };
    let Some(after) = line_text.get(member.end as usize - line_start..) else {
        return false;
    };
    let after = after.trim_start();
    let after = after
        .strip_prefix(',')
        .or_else(|| after.strip_prefix(';'))
        .unwrap_or(after)
        .trim();
    before.trim().is_empty() && (after.is_empty() || after.starts_with("//"))
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

/// True when the text without the line still parses, and each other member
/// keeps its text. Without a semicolon after the member above, the deletion
/// can join that member with the member below, as in `count = 0` followed by
/// `[Symbol.iterator]() {}`.
fn deletion_keeps_other_members(
    path: &Path,
    index: &SourceEditIndex,
    source: &str,
    line_start: usize,
    line_text: &str,
) -> bool {
    let line_end = source[line_start..]
        .find('\n')
        .map_or(source.len(), |newline| line_start + newline + 1);
    let mut rest = String::with_capacity(source.len());
    rest.push_str(&source[..line_start]);
    rest.push_str(&source[line_end..]);
    let Some(after) = SourceEditIndex::parse(path, &rest) else {
        return false;
    };
    let Ok(removed) = u32::try_from(line_end - line_start) else {
        return false;
    };
    let text_of = |start: u32, end: u32| rest.get(start as usize..end as usize);
    let shift = |offset: u32| {
        if offset as usize >= line_end {
            offset - removed
        } else {
            offset
        }
    };
    let deleted_line = line_start..line_start + line_text.len();
    let kept: Option<Vec<&str>> = index
        .members()
        .iter()
        .filter(|member| !deleted_line.contains(&(member.start as usize)))
        .map(|member| text_of(shift(member.start), shift(member.end)))
        .collect();
    let now: Option<Vec<&str>> = after
        .members()
        .iter()
        .map(|member| text_of(member.start, member.end))
        .collect();
    kept.is_some() && kept == now
}

/// The block that deletes the line of the finding.
fn delete_line_suggestion(provider: Provider) -> String {
    match provider {
        Provider::Github => "\n\n```suggestion\n\n```".to_owned(),
        Provider::Gitlab => "\n\n```suggestion:-0+0\n\n```".to_owned(),
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

    Some(delete_line_suggestion(provider))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The suggestion for the member on line 2 of a file `src/dead.ts`.
    fn member_suggestion(provider: Provider, rule_id: &str, source: &str) -> Option<String> {
        super::suggestion_block_for_issue_line(
            provider,
            rule_id,
            Path::new("src/dead.ts"),
            source,
            2,
        )
    }

    /// The suggestion for a file `src/dead.ts` whose whole text is `line`.
    fn suggestion_block_for_issue_line(
        provider: Provider,
        rule_id: &str,
        line: &str,
    ) -> Option<String> {
        super::suggestion_block_for_issue_line(provider, rule_id, Path::new("src/dead.ts"), line, 1)
    }

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
            ),
            None,
            "an edit to `thing;` changes what the module does"
        );
        assert_eq!(
            suggestion_block_for_issue_line(
                Provider::Gitlab,
                "fallow/unused-export",
                "export default function thing() {}"
            )
            .as_deref(),
            Some("\n\n```suggestion:-0+0\nfunction thing() {}\n```")
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
            audit: None,
        };

        assert_eq!(
            fix_intent(&issue),
            Some("Move the repeated code into one shared function.")
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
            member_suggestion(
                Provider::Github,
                "fallow/unused-enum-member",
                "enum E {\n  Deprecated,\n  Kept = 2,\n}\n"
            )
            .as_deref(),
            Some("\n\n```suggestion\n\n```")
        );
        assert_eq!(
            member_suggestion(
                Provider::Gitlab,
                "fallow/unused-enum-member",
                "enum E {\n  Deprecated,\n  Kept = 2,\n}\n"
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
    fn namespace_members_never_offer_unverified_line_deletions() {
        for provider in [Provider::Github, Provider::Gitlab] {
            assert!(
                suggestion_block_for_issue_line(
                    provider,
                    "fallow/unused-namespace-member",
                    "  export const unused = 2;"
                )
                .is_none()
            );
        }
    }

    #[test]
    fn renders_unused_class_member_delete_suggestion() {
        assert_eq!(
            member_suggestion(
                Provider::Github,
                "fallow/unused-class-member",
                "class Widget {\n  legacyMethod() { return null; }\n}\n"
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
            audit: None,
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

    /// The unused-file hint must not advertise unsupported `fallow fix` flags.
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
        for (rule, source, line) in [
            (
                "fallow/unused-class-member",
                "class Widget {\n  legacyMethod() { return null; }\n}\n",
                2,
            ),
            (
                "fallow/unused-enum-member",
                "enum E {\n  Deprecated,\n  Kept = 2,\n}\n",
                2,
            ),
            ("fallow/unused-export", "export const value = 1;\n", 1),
        ] {
            for provider in [Provider::Github, Provider::Gitlab] {
                let body = suggestion_or_withheld_hint(
                    provider,
                    &issue(
                        rule,
                        "Something is never referenced (caveat: incomplete import graph)",
                    ),
                    Path::new("src/dead.ts"),
                    source,
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
                Path::new("src/dead.ts"),
                "class Widget {\n  legacyMethod() { return null; }\n}\n",
                2,
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
                Path::new("package.json"),
                "{\n  \"lodash\": \"^4.17.21\",\n}\n",
                2,
            ),
            None
        );
    }

    /// A survived mutant names a test gap, not an edit of the line, so its
    /// review comment never offers a one-click change. The source line here
    /// gives a block for an unused export, which shows that the rule id alone
    /// withholds it.
    #[test]
    fn a_survived_mutant_gets_no_suggestion_block() {
        let dir = tempfile::tempdir().expect("temp dir");
        let source = dir.path().join("age.ts");
        std::fs::write(&source, "export const isAdult = (age) => age >= 18;\n").expect("write");
        let finding = |rule_id: &str| CiIssue {
            rule_id: rule_id.to_owned(),
            description: "Changing >= to > in isAdult made no test fail. Mutant 9f3c2a1b."
                .to_owned(),
            severity: "minor".to_owned(),
            path: source.to_string_lossy().into_owned(),
            line: 1,
            end_line: None,
            other_locations: Vec::new(),
            fingerprint: "abc".to_owned(),
            legacy_fingerprint: None,
            audit: None,
        };
        let survived = finding(crate::report::mutation_evidence::MUTATION_SURVIVED_RULE);
        for provider in [Provider::Github, Provider::Gitlab] {
            assert!(
                suggestion_block(provider, &finding("fallow/unused-export")).is_some(),
                "the control finding reads the line"
            );
            assert_eq!(suggestion_block(provider, &survived), None);
        }
        assert_eq!(fix_intent(&survived), None);
    }

    #[test]
    fn delete_line_suggestion_skips_blank_lines() {
        assert_eq!(
            suggestion_block_for_issue_line(Provider::Github, "fallow/unused-enum-member", "   "),
            None
        );
    }
}

#[cfg(test)]
#[path = "suggestion_syntax_tests.rs"]
mod syntax_tests;
