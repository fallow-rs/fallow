use serde::{Deserialize, Serialize};

/// Schema discriminator serialized into [`PrDetailsArtifact::schema`].
pub const PR_DETAILS_SCHEMA: &str = "fallow-pr-details/v1";

/// Full-findings report artifact backing the PR summary comment's details
/// link, grouped into per-area sections.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDetailsArtifact {
    /// Schema discriminator; always [`PR_DETAILS_SCHEMA`].
    pub schema: String,
    /// Display title of the report.
    pub title: String,
    /// Per-area finding sections.
    pub sections: Vec<PrDetailsSection>,
}

/// One findings section inside [`PrDetailsArtifact::sections`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDetailsSection {
    /// Stable section identifier, e.g. `findings`.
    pub id: String,
    /// Display title of the section.
    pub title: String,
    /// Finding rows in the section.
    pub rows: Vec<PrDetailsRow>,
}

/// One finding row inside [`PrDetailsSection::rows`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDetailsRow {
    /// `path:line` location text.
    pub location: String,
    /// Rule identifier the finding belongs to.
    pub rule: String,
    /// Human-readable finding description.
    pub description: String,
    /// Suggested fix text, when one is known.
    pub fix: Option<String>,
    /// Stable finding fingerprint for cross-run tracking, when available.
    pub fingerprint: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn details_artifact_serializes_stable_schema() {
        let artifact = PrDetailsArtifact {
            schema: PR_DETAILS_SCHEMA.to_owned(),
            title: "Fallow".to_owned(),
            sections: vec![PrDetailsSection {
                id: "findings".to_owned(),
                title: "Findings".to_owned(),
                rows: vec![PrDetailsRow {
                    location: "src/app.ts:12".to_owned(),
                    rule: "fallow/high-crap-score".to_owned(),
                    description: "Function is hard to safely change.".to_owned(),
                    fix: Some("Extract smaller units.".to_owned()),
                    fingerprint: Some("abc123".to_owned()),
                }],
            }],
        };

        let json = serde_json::to_value(artifact).expect("serializes");

        assert_eq!(json["schema"], PR_DETAILS_SCHEMA);
        assert_eq!(json["sections"][0]["rows"][0]["location"], "src/app.ts:12");
    }
}
