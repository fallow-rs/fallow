use std::path::PathBuf;

use super::pr_comment::{CiIssue, Provider};

#[must_use]
pub fn suggestion_block(provider: Provider, issue: &CiIssue) -> Option<String> {
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
    if !rule_id.contains("unused-export") {
        return None;
    }
    unused_export_suggestion(provider, line)
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
}
