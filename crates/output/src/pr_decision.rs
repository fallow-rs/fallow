use serde::{Deserialize, Serialize};

pub const PR_DECISION_SCHEMA: &str = "fallow-pr-decision/v1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDecisionSurface {
    pub schema: String,
    pub title: String,
    pub conclusion: PrDecisionConclusion,
    pub gates: Vec<PrDecisionGate>,
    pub annotations: Vec<PrDecisionAnnotation>,
    pub details: PrDecisionDetails,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrDecisionConclusion {
    Success,
    Failure,
    Neutral,
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDecisionGate {
    pub id: String,
    pub label: String,
    pub status: PrDecisionConclusion,
    pub observed: String,
    pub threshold: Option<String>,
    pub scope: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDecisionAnnotation {
    pub path: String,
    pub line: u32,
    pub level: PrDecisionAnnotationLevel,
    pub title: String,
    pub message: String,
    pub raw_details: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrDecisionAnnotationLevel {
    Notice,
    Warning,
    Failure,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrDecisionDetails {
    pub summary_markdown: String,
    pub full_report_path: Option<String>,
    pub details_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_surface_serializes_stable_schema() {
        let surface = PrDecisionSurface {
            schema: PR_DECISION_SCHEMA.to_owned(),
            title: "Fallow".to_owned(),
            conclusion: PrDecisionConclusion::Failure,
            gates: vec![PrDecisionGate {
                id: "duplication".to_owned(),
                label: "Duplication".to_owned(),
                status: PrDecisionConclusion::Failure,
                observed: "9.1% on changed code".to_owned(),
                threshold: Some("<= 3%".to_owned()),
                scope: "new code".to_owned(),
            }],
            annotations: vec![PrDecisionAnnotation {
                path: "src/app.ts".to_owned(),
                line: 42,
                level: PrDecisionAnnotationLevel::Warning,
                title: "Duplication".to_owned(),
                message: "Clone group found".to_owned(),
                raw_details: Some("fallow/code-duplication".to_owned()),
            }],
            details: PrDecisionDetails {
                summary_markdown: "Quality gate failed".to_owned(),
                full_report_path: None,
                details_url: None,
            },
        };

        let json = serde_json::to_value(surface).expect("serializes");
        assert_eq!(json["schema"], PR_DECISION_SCHEMA);
        assert_eq!(json["conclusion"], "failure");
        assert_eq!(json["annotations"][0]["level"], "warning");
    }
}
