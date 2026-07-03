use serde::{Deserialize, Serialize};

pub const PR_DETAILS_SCHEMA: &str = "fallow-pr-details/v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDetailsArtifact {
    pub schema: String,
    pub title: String,
    pub sections: Vec<PrDetailsSection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDetailsSection {
    pub id: String,
    pub title: String,
    pub rows: Vec<PrDetailsRow>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDetailsRow {
    pub location: String,
    pub rule: String,
    pub description: String,
    pub fix: Option<String>,
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
