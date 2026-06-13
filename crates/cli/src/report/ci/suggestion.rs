use std::path::PathBuf;

use super::pr_comment::{CiIssue, Provider};

#[must_use]
pub fn suggestion_block(provider: Provider, issue: &CiIssue) -> Option<String> {
    if issue.rule_id.contains("unused-file") {
        return Some(unused_file_hint());
    }
    if issue.line == 0 {
        return None;
    }

    let root = std::env::var_os("FALLOW_ROOT").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let path = root.join(&issue.path);
    let source = std::fs::read_to_string(path).ok()?;
    let line = source.lines().nth(issue.line.saturating_sub(1) as usize)?;
    suggestion_block_for_issue_line(provider, &issue.rule_id, line)
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
    if rule_id.contains("unused-enum-member")
        || rule_id.contains("unused-class-member")
        || rule_id.contains("unused-store-member")
    {
        return delete_line_suggestion(provider, line);
    }
    if rule_id.contains("unused-export") || rule_id.contains("unused-type") {
        return unused_export_suggestion(provider, line);
    }
    None
}

/// Text hint for `unused-file` findings. Neither GitHub nor GitLab supports
/// file-scope deletion suggestions through the review-comment API.
#[must_use]
fn unused_file_hint() -> String {
    "\n\n> Run `fallow fix --files` or delete this file.".to_owned()
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

    #[test]
    fn unused_file_hint_uses_text_not_suggestion_block() {
        let issue = CiIssue {
            rule_id: "fallow/unused-file".to_owned(),
            description: "File is not reachable".to_owned(),
            severity: "major".to_owned(),
            path: "src/dead.ts".to_owned(),
            line: 1,
            fingerprint: "abc".to_owned(),
        };
        let body = suggestion_block(Provider::Github, &issue).expect("hint");
        assert!(!body.contains("```suggestion"), "must not be a code block");
        assert!(body.contains("fallow fix --files"));
    }

    #[test]
    fn delete_line_suggestion_skips_blank_lines() {
        assert_eq!(
            suggestion_block_for_issue_line(Provider::Github, "fallow/unused-enum-member", "   "),
            None
        );
    }
}
