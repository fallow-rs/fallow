//! The contract relation that decides a class-member result, and its text.

use fallow_types::semantic::{SemanticContractEvidence, SemanticContractRelation};

/// The first contract that keeps the declaration: a required member, or an
/// optional hook that a dependency declares. An optional member of a project
/// contract does not decide, because its callers are in the program.
pub(super) fn deciding_contract(
    relations: &[SemanticContractEvidence],
) -> Option<&SemanticContractEvidence> {
    relations.iter().find(|contract| {
        !contract.optional || contract.relation == SemanticContractRelation::DependencyHook
    })
}

pub(super) const fn contract_relation_phrase(relation: SemanticContractRelation) -> &'static str {
    match relation {
        SemanticContractRelation::InterfaceImplementation => "implements",
        SemanticContractRelation::AbstractImplementation => "implements the abstract member",
        SemanticContractRelation::Override => "overrides",
        SemanticContractRelation::OptionalContract => "matches the optional contract",
        SemanticContractRelation::DependencyHook => "implements the optional hook",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_aware::client::tests::symbol;

    #[test]
    fn dependency_hook_decides_but_project_optional_contract_does_not() {
        let contract = |relation, optional| SemanticContractEvidence {
            relation,
            declaration: symbol(),
            optional,
        };
        let project_optional = contract(SemanticContractRelation::OptionalContract, true);
        let hook = contract(SemanticContractRelation::DependencyHook, true);
        assert_eq!(
            deciding_contract(std::slice::from_ref(&project_optional)),
            None
        );
        assert_eq!(
            deciding_contract(&[project_optional, hook.clone()]),
            Some(&hook)
        );
    }
}
