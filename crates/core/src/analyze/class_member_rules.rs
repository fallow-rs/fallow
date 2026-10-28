//! Split the `usedClassMembers` rules of a run by the pass that applies them.

use fallow_config::ResolvedConfig;

/// The `usedClassMembers` rules of a run, split by the pass that applies them.
pub(super) struct ClassMemberRules {
    /// User rules and plugin rules that suppress a class member syntactically.
    pub(super) user: Vec<fallow_config::UsedClassMemberRule>,
    /// Plugin rules that the semantic pass validates against the exact
    /// framework package.
    pub(super) semantic_framework_candidates: Vec<fallow_config::UsedClassMemberRule>,
}

impl ClassMemberRules {
    pub(super) fn new(
        config: &ResolvedConfig,
        plugin_result: Option<&crate::plugins::AggregatedPluginResult>,
    ) -> Self {
        let mut user = config.used_class_members.clone();
        let mut semantic_framework_candidates = Vec::new();
        if let Some(plugin_result) = plugin_result {
            for rule in &plugin_result.used_class_members {
                if config.type_aware.enabled
                    && plugin_result
                        .framework_class_member_contracts
                        .iter()
                        .any(|contract| framework_contract_covers_rule(contract, rule))
                {
                    semantic_framework_candidates.push(rule.clone());
                } else {
                    user.push(rule.clone());
                }
            }
        }
        Self {
            user,
            semantic_framework_candidates,
        }
    }
}

pub(super) fn framework_contract_covers_rule(
    contract: &fallow_types::semantic::SemanticFrameworkContract,
    rule: &fallow_config::UsedClassMemberRule,
) -> bool {
    use fallow_types::semantic::SemanticFrameworkRelation;

    let fallow_config::UsedClassMemberRule::Scoped(rule) = rule else {
        return false;
    };
    let heritage_matches =
        match contract.relation {
            SemanticFrameworkRelation::Extends => {
                rule.implements.is_none()
                    && rule.extends.as_ref().is_some_and(|name| {
                        contract.heritage_names.iter().any(|known| known == name)
                    })
            }
            SemanticFrameworkRelation::Implements => {
                rule.extends.is_none()
                    && rule.implements.as_ref().is_some_and(|name| {
                        contract.heritage_names.iter().any(|known| known == name)
                    })
            }
        };
    heritage_matches
        && rule
            .members
            .iter()
            .all(|member| contract.members.contains(member))
}
