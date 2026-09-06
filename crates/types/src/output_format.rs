/// Output format for fallow results.
///
/// This is command-line and integration metadata, not stored in config files.
/// Keeping it in `fallow-types` lets config, output, CLI, MCP, and API layers
/// agree on the same contract without creating a config-to-output dependency.
#[derive(Debug, Default, Clone, Copy)]
pub enum OutputFormat {
    /// Human-readable terminal output with source context.
    #[default]
    Human,
    /// Machine-readable JSON.
    Json,
    /// SARIF format for GitHub Code Scanning.
    Sarif,
    /// One issue per line (grep-friendly).
    Compact,
    /// Markdown for PR comments.
    Markdown,
    /// `CodeClimate` JSON for GitLab Code Quality.
    ///
    /// CLI aliases: `codeclimate`, `gitlab-codequality`, `gitlab-code-quality`.
    CodeClimate,
    /// GitHub-flavored sticky PR comment markdown.
    PrCommentGithub,
    /// GitLab-flavored sticky MR comment markdown.
    PrCommentGitlab,
    /// GitHub PR review JSON envelope.
    ReviewGithub,
    /// GitLab MR review JSON envelope.
    ReviewGitlab,
    /// Shields.io-compatible SVG badge (health command only).
    Badge,
    /// GitHub Actions workflow-command annotations (`::error` / `::warning` /
    /// `::notice` lines). Provider-prefixed name because workflow-command
    /// annotations are a GitHub-only concept with no GitLab twin.
    GithubAnnotations,
    /// GitHub Actions job-summary markdown (for `>> $GITHUB_STEP_SUMMARY`).
    /// Provider-prefixed for the same reason as `GithubAnnotations`.
    GithubSummary,
}

#[cfg(test)]
mod tests {
    use super::*;

    const VARIANTS: [OutputFormat; 13] = [
        OutputFormat::Human,
        OutputFormat::Json,
        OutputFormat::Sarif,
        OutputFormat::Compact,
        OutputFormat::Markdown,
        OutputFormat::CodeClimate,
        OutputFormat::PrCommentGithub,
        OutputFormat::PrCommentGitlab,
        OutputFormat::ReviewGithub,
        OutputFormat::ReviewGitlab,
        OutputFormat::Badge,
        OutputFormat::GithubAnnotations,
        OutputFormat::GithubSummary,
    ];

    #[test]
    fn default_is_human() {
        assert!(matches!(OutputFormat::default(), OutputFormat::Human));
    }

    #[test]
    fn debug_names_remain_stable() {
        let names: Vec<String> = VARIANTS
            .iter()
            .map(|variant| format!("{variant:?}"))
            .collect();
        assert_eq!(
            names,
            vec![
                "Human",
                "Json",
                "Sarif",
                "Compact",
                "Markdown",
                "CodeClimate",
                "PrCommentGithub",
                "PrCommentGitlab",
                "ReviewGithub",
                "ReviewGitlab",
                "Badge",
                "GithubAnnotations",
                "GithubSummary",
            ]
        );
    }
}
