use std::collections::BTreeMap;

use fallow_types::envelope::{Meta, MetaMetric};

use crate::{ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION, ACTIONS_FIELD_DEFINITION};

/// Docs URL for the duplication command.
pub const DUPES_DOCS: &str = "https://docs.fallow.tools/cli/dupes";

/// Build the `_meta` object for `fallow dupes --format json --explain`.
#[must_use]
pub fn dupes_meta() -> Meta {
    Meta {
        docs: Some(DUPES_DOCS.to_string()),
        field_definitions: BTreeMap::from([
            (
                "actions[]".to_string(),
                ACTIONS_FIELD_DEFINITION.to_string(),
            ),
            (
                "actions[].auto_fixable".to_string(),
                ACTIONS_AUTO_FIXABLE_FIELD_DEFINITION.to_string(),
            ),
        ]),
        metrics: BTreeMap::from([
            (
                "duplication_percentage".to_string(),
                metric(
                    "Duplication Percentage",
                    "Fraction of total source tokens that appear in at least one clone group. Computed over the full analyzed file set.",
                    Some("[0, 100]"),
                    "lower is better",
                ),
            ),
            (
                "token_count".to_string(),
                metric(
                    "Token Count",
                    "Number of normalized source tokens in the clone group. Tokens are language-aware (keywords, identifiers, operators, punctuation). Higher token count = larger duplicate.",
                    Some("[1, ∞)"),
                    "larger clones have higher refactoring value",
                ),
            ),
            (
                "line_count".to_string(),
                metric(
                    "Line Count",
                    "Number of source lines spanned by the clone instance. Approximation of clone size for human readability.",
                    Some("[1, ∞)"),
                    "larger clones are more impactful to deduplicate",
                ),
            ),
            (
                "clone_groups".to_string(),
                metric(
                    "Clone Groups",
                    "A set of code fragments with identical or near-identical normalized token sequences. Each group has 2+ instances across different locations.",
                    None,
                    "each group is a single refactoring opportunity",
                ),
            ),
            (
                "clone_groups_below_min_occurrences".to_string(),
                metric(
                    "Clone Groups Below minOccurrences",
                    "Number of clone groups detected but hidden by the `duplicates.minOccurrences` filter. Always 0 (or absent) when the filter is at its default of 2. Pre-filter group count = `clone_groups + clone_groups_below_min_occurrences`.",
                    Some("[0, ∞)"),
                    "high values suggest noisy pair-only duplication; lower `minOccurrences` to inspect",
                ),
            ),
            (
                "clone_families".to_string(),
                metric(
                    "Clone Families",
                    "Groups of clone groups that share the same set of files. Indicates systematic duplication patterns (e.g., mirrored directory structures).",
                    None,
                    "families suggest extract-module refactoring opportunities",
                ),
            ),
        ]),
        ..Meta::default()
    }
}

fn metric(
    name: impl Into<String>,
    description: impl Into<String>,
    range: Option<&str>,
    interpretation: impl Into<String>,
) -> MetaMetric {
    MetaMetric {
        name: Some(name.into()),
        description: Some(description.into()),
        range: range.map(str::to_string),
        interpretation: Some(interpretation.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dupes_meta_uses_output_contract_shape() {
        let meta = dupes_meta();
        assert_eq!(meta.docs.as_deref(), Some(DUPES_DOCS));
        assert!(meta.field_definitions.contains_key("actions[]"));
        assert!(meta.metrics.contains_key("duplication_percentage"));
        assert!(
            meta.metrics
                .contains_key("clone_groups_below_min_occurrences")
        );
    }
}
