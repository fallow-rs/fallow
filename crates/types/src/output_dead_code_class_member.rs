//! Actions of the unused class member findings.

use super::*;

impl UnusedClassMemberFinding {
    /// Build the wrapper from a raw [`UnusedMember`]. Class-member fixes
    /// are not auto-applied (members can be used via dependency injection
    /// or decorators), so `auto_fixable` is `false` and a context note is
    /// attached.
    #[must_use]
    pub fn with_actions(member: UnusedMember) -> Self {
        let actions = vec![
            IssueAction::Fix(FixAction {
                kind: FixActionType::RemoveClassMember,
                auto_fixable: false,
                description: "Remove this class member".to_string(),
                note: Some(
                    "Class member may be used via dependency injection or decorators".to_string(),
                ),
                available_in_catalogs: None,
                suggested_target: None,
            }),
            IssueAction::SuppressLine(SuppressLineAction {
                kind: SuppressLineKind::SuppressLine,
                auto_fixable: false,
                description: "Suppress with an inline comment above the line".to_string(),
                comment: "// fallow-ignore-next-line unused-class-member".to_string(),
                scope: None,
            }),
        ];
        Self {
            finding_id: None,
            member,
            actions,
            semantic: None,
            semantic_only_candidate: false,
            introduced: None,
            effective_severity: None,
            reachability_caveats: Vec::new(),
        }
    }

    /// Mark this finding as latent until semantic analysis proves that the
    /// framework contract does not apply and no static references exist.
    #[must_use]
    pub const fn semantic_only_candidate(mut self) -> Self {
        self.semantic_only_candidate = true;
        self
    }

    /// Attach the canonical semantic decision and expose the class-member fix
    /// only when the API policy granted closed-world eligibility AND this run
    /// holds the evidence for the mutation.
    ///
    /// This is the one code path that RAISES `auto_fixable` on a class member,
    /// and it runs in the API layer AFTER the analysis layer stamped the run's
    /// caveats, so it asks the gate for the same reason
    /// `set_export_semantic_action` does: a closed-world verdict computed
    /// over a program the run never fully read must not re-open a removal the
    /// incomplete run already withheld. The withheld note names the evidence
    /// gap rather than the semantic explanation, which stays readable on the
    /// finding's own `semantic` object.
    pub fn set_semantic_decision(&mut self, decision: SemanticCandidateDecision) {
        let evidence_complete = self.reachability_caveats.is_empty();
        if let Some(IssueAction::Fix(action)) = self.actions.first_mut() {
            action.auto_fixable = decision.closed_world_eligible && evidence_complete;
            action.note = Some(if evidence_complete {
                decision.explanation.clone()
            } else {
                INCOMPLETE_EVIDENCE_NOTE.to_string()
            });
        }
        self.semantic = Some(decision);
    }
}
