//! Pure posting policy for sticky PR comments.

use crate::PrCommentEnvelope;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExistingPrComment {
    pub id: String,
    pub body: String,
}

pub struct PrCommentPostPlanInput<'a> {
    pub envelope: &'a PrCommentEnvelope,
    pub existing: Option<&'a ExistingPrComment>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrCommentPostAction {
    Create,
    Update,
    Skip,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrCommentPostSkipReason {
    CleanNoExistingComment,
    Unchanged,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct PrCommentPostPlan {
    pub action: PrCommentPostAction,
    pub marker_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub comment_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip_reason: Option<PrCommentPostSkipReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

#[must_use]
pub fn plan_pr_comment_post(input: &PrCommentPostPlanInput<'_>) -> PrCommentPostPlan {
    match input.existing {
        Some(existing) if existing.body == input.envelope.body => skip_unchanged(input.envelope),
        Some(existing) => update_existing(input.envelope, existing),
        None if input.envelope.is_clean => skip_clean(input.envelope),
        None => create_comment(input.envelope),
    }
}

fn create_comment(envelope: &PrCommentEnvelope) -> PrCommentPostPlan {
    PrCommentPostPlan {
        action: PrCommentPostAction::Create,
        marker_id: envelope.marker_id.clone(),
        comment_id: None,
        skip_reason: None,
        body: Some(envelope.body.clone()),
    }
}

fn update_existing(
    envelope: &PrCommentEnvelope,
    existing: &ExistingPrComment,
) -> PrCommentPostPlan {
    PrCommentPostPlan {
        action: PrCommentPostAction::Update,
        marker_id: envelope.marker_id.clone(),
        comment_id: Some(existing.id.clone()),
        skip_reason: None,
        body: Some(envelope.body.clone()),
    }
}

fn skip_clean(envelope: &PrCommentEnvelope) -> PrCommentPostPlan {
    PrCommentPostPlan {
        action: PrCommentPostAction::Skip,
        marker_id: envelope.marker_id.clone(),
        comment_id: None,
        skip_reason: Some(PrCommentPostSkipReason::CleanNoExistingComment),
        body: None,
    }
}

fn skip_unchanged(envelope: &PrCommentEnvelope) -> PrCommentPostPlan {
    PrCommentPostPlan {
        action: PrCommentPostAction::Skip,
        marker_id: envelope.marker_id.clone(),
        comment_id: None,
        skip_reason: Some(PrCommentPostSkipReason::Unchanged),
        body: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn envelope(is_clean: bool, body: &str) -> PrCommentEnvelope {
        PrCommentEnvelope {
            marker_id: "fallow-results".to_owned(),
            body: body.to_owned(),
            is_clean,
            details_url: None,
            check_summary: None,
            truncation: crate::PrCommentTruncation::default(),
        }
    }

    #[test]
    fn clean_without_existing_comment_skips_create() {
        let plan = plan_pr_comment_post(&PrCommentPostPlanInput {
            envelope: &envelope(true, "clean"),
            existing: None,
        });

        assert_eq!(plan.action, PrCommentPostAction::Skip);
        assert_eq!(
            plan.skip_reason,
            Some(PrCommentPostSkipReason::CleanNoExistingComment)
        );
        assert_eq!(plan.body, None);
    }

    #[test]
    fn clean_with_existing_comment_updates_existing_body() {
        let current = envelope(true, "clean");
        let existing = ExistingPrComment {
            id: "42".to_owned(),
            body: "old".to_owned(),
        };

        let plan = plan_pr_comment_post(&PrCommentPostPlanInput {
            envelope: &current,
            existing: Some(&existing),
        });

        assert_eq!(plan.action, PrCommentPostAction::Update);
        assert_eq!(plan.comment_id.as_deref(), Some("42"));
        assert_eq!(plan.body.as_deref(), Some("clean"));
    }

    #[test]
    fn dirty_without_existing_comment_creates_comment() {
        let current = envelope(false, "dirty");

        let plan = plan_pr_comment_post(&PrCommentPostPlanInput {
            envelope: &current,
            existing: None,
        });

        assert_eq!(plan.action, PrCommentPostAction::Create);
        assert_eq!(plan.body.as_deref(), Some("dirty"));
    }

    #[test]
    fn identical_existing_comment_skips_update() {
        let current = envelope(false, "same");
        let existing = ExistingPrComment {
            id: "42".to_owned(),
            body: "same".to_owned(),
        };

        let plan = plan_pr_comment_post(&PrCommentPostPlanInput {
            envelope: &current,
            existing: Some(&existing),
        });

        assert_eq!(plan.action, PrCommentPostAction::Skip);
        assert_eq!(plan.skip_reason, Some(PrCommentPostSkipReason::Unchanged));
    }
}
