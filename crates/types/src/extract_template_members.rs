//! Angular template member names from typed semantic facts.

use super::*;

/// Iterate Angular template member names from typed semantic facts.
pub(super) fn angular_template_member_names_from_parts(
    semantic_facts: &[SemanticFact],
) -> impl Iterator<Item = &str> {
    semantic_facts.iter().filter_map(|fact| {
        if let SemanticFact::AngularTemplateMemberAccess(access) = fact {
            Some(access.member.as_str())
        } else {
            None
        }
    })
}
