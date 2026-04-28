/// Output format for results.
///
/// This is CLI-only (via `--format` flag), not stored in config files.
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
    /// Shields.io-compatible SVG badge (health command only).
    Badge,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_format_default_is_human() {
        let format = OutputFormat::default();
        assert!(matches!(format, OutputFormat::Human));
    }

    #[test]
    fn output_format_all_variants_constructible() {
        // Verify all variants can be constructed and pattern-matched
        assert!(matches!(OutputFormat::Human, OutputFormat::Human));
        assert!(matches!(OutputFormat::Json, OutputFormat::Json));
        assert!(matches!(OutputFormat::Sarif, OutputFormat::Sarif));
        assert!(matches!(OutputFormat::Compact, OutputFormat::Compact));
        assert!(matches!(OutputFormat::Markdown, OutputFormat::Markdown));
        assert!(matches!(
            OutputFormat::CodeClimate,
            OutputFormat::CodeClimate
        ));
        assert!(matches!(OutputFormat::Badge, OutputFormat::Badge));
    }

    #[test]
    fn output_format_debug_impl() {
        // Verify Debug is derived and produces reasonable output
        let human = format!("{:?}", OutputFormat::Human);
        assert_eq!(human, "Human");
        let json = format!("{:?}", OutputFormat::Json);
        assert_eq!(json, "Json");
        let sarif = format!("{:?}", OutputFormat::Sarif);
        assert_eq!(sarif, "Sarif");
        let compact = format!("{:?}", OutputFormat::Compact);
        assert_eq!(compact, "Compact");
        let markdown = format!("{:?}", OutputFormat::Markdown);
        assert_eq!(markdown, "Markdown");
        let codeclimate = format!("{:?}", OutputFormat::CodeClimate);
        assert_eq!(codeclimate, "CodeClimate");
        let badge = format!("{:?}", OutputFormat::Badge);
        assert_eq!(badge, "Badge");
    }

    #[test]
    fn output_format_copy() {
        let original = OutputFormat::Json;
        let copied = original;
        assert!(matches!(copied, OutputFormat::Json));
        // Original still usable (Copy)
        assert!(matches!(original, OutputFormat::Json));
    }

    #[test]
    #[expect(
        clippy::clone_on_copy,
        reason = "explicitly testing the Clone impl for coverage"
    )]
    fn output_format_clone_all_variants() {
        let variants = [
            OutputFormat::Human,
            OutputFormat::Json,
            OutputFormat::Sarif,
            OutputFormat::Compact,
            OutputFormat::Markdown,
            OutputFormat::CodeClimate,
            OutputFormat::Badge,
        ];
        for variant in variants {
            let cloned = variant.clone();
            // Debug output must match between original and clone
            assert_eq!(format!("{cloned:?}"), format!("{variant:?}"));
        }
    }

    #[test]
    #[expect(
        clippy::clone_on_copy,
        reason = "explicitly testing the Clone impl for coverage"
    )]
    fn output_format_clone_preserves_variant() {
        let badge = OutputFormat::Badge;
        let cloned = badge.clone();
        assert!(matches!(cloned, OutputFormat::Badge));

        let codeclimate = OutputFormat::CodeClimate;
        let cloned = codeclimate.clone();
        assert!(matches!(cloned, OutputFormat::CodeClimate));
    }

    #[test]
    fn output_format_default_matches_human_debug() {
        // Default variant should produce "Human" debug string
        assert_eq!(format!("{:?}", OutputFormat::default()), "Human");
    }

    #[test]
    fn output_format_variants_are_distinct() {
        // Verify each variant has a unique debug representation
        let debug_strings: Vec<String> = [
            OutputFormat::Human,
            OutputFormat::Json,
            OutputFormat::Sarif,
            OutputFormat::Compact,
            OutputFormat::Markdown,
            OutputFormat::CodeClimate,
            OutputFormat::Badge,
        ]
        .iter()
        .map(|v| format!("{v:?}"))
        .collect();

        for (i, a) in debug_strings.iter().enumerate() {
            for (j, b) in debug_strings.iter().enumerate() {
                if i != j {
                    assert_ne!(
                        a, b,
                        "variants at index {i} and {j} have the same debug output"
                    );
                }
            }
        }
    }
}
