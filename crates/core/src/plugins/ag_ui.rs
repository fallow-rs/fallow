//! AG-UI agent plugin.
//!
//! Activates on any `@ag-ui/` scoped package. A custom agent extends
//! `AbstractAgent` from `@ag-ui/client` and overrides hooks that the agent
//! runtime calls: `run` produces the event stream, `clone` copies the agent
//! for each run, and the `on*` hooks follow the run lifecycle. Project code
//! does not call these hooks, so they would otherwise surface as
//! `unused-class-member` false positives.
//!
//! The rule is scoped to `AbstractAgent` through `UsedClassMemberRule::Scoped`,
//! as the `lexical` and `lit` plugins do. Other methods on an agent subclass
//! are still reported. Heritage matching uses the direct superclass name, so
//! a subclass of a local intermediate base is not covered.

use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};

use super::Plugin;

const ENABLERS: &[&str] = &["@ag-ui/"];

/// The base class that custom agents extend.
const AGENT_BASE_CLASS: &str = "AbstractAgent";

/// Members of `AbstractAgent` that the agent runtime calls and that a subclass
/// can override. Verified against the `AbstractAgent` declaration in
/// `@ag-ui/client` 0.0.59 (`dist/index.d.ts`).
const AGENT_RUNTIME_MEMBERS: &[&str] = &[
    "run",
    "clone",
    "connect",
    "getCapabilities",
    "apply",
    "processApplyEvents",
    "prepareRunAgentInput",
    "onInitialize",
    "onError",
    "onFinalize",
    "abortRun",
    "detachActiveRun",
];

pub struct AgUiPlugin;

impl Plugin for AgUiPlugin {
    fn name(&self) -> &'static str {
        "ag-ui"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn used_class_member_rules(&self) -> Vec<UsedClassMemberRule> {
        vec![UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
            extends: Some(AGENT_BASE_CLASS.to_string()),
            implements: None,
            members: AGENT_RUNTIME_MEMBERS
                .iter()
                .map(|member| (*member).to_string())
                .collect(),
        })]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent_rule() -> ScopedUsedClassMemberRule {
        let rules = AgUiPlugin.used_class_member_rules();
        assert_eq!(
            rules.len(),
            1,
            "expected one scoped rule; rules = {rules:?}"
        );
        match rules.into_iter().next() {
            Some(UsedClassMemberRule::Scoped(rule)) => rule,
            other => panic!("expected a scoped rule, got {other:?}"),
        }
    }

    #[test]
    fn enablers_cover_the_scoped_packages() {
        assert!(AgUiPlugin.enablers().contains(&"@ag-ui/"));
    }

    #[test]
    fn rule_is_scoped_to_the_agent_base_class() {
        let rule = agent_rule();
        assert_eq!(rule.extends.as_deref(), Some("AbstractAgent"));
        assert!(rule.implements.is_none());
    }

    #[test]
    fn rule_credits_the_runtime_hooks() {
        let rule = agent_rule();
        for member in [
            "run",
            "clone",
            "connect",
            "onInitialize",
            "onError",
            "onFinalize",
        ] {
            assert!(
                rule.members.iter().any(|m| m == member),
                "AbstractAgent rule should credit {member}; members = {:?}",
                rule.members
            );
        }
    }

    #[test]
    fn rule_does_not_credit_caller_side_methods() {
        let rule = agent_rule();
        for member in ["runAgent", "addMessage", "setState", "subscribe"] {
            assert!(
                !rule.members.iter().any(|m| m == member),
                "{member} is called by project code, not overridden for the runtime; \
                 members = {:?}",
                rule.members
            );
        }
    }
}
