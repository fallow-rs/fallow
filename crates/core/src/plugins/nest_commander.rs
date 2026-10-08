//! `nest-commander` CLI plugin.
//!
//! A command class extends `CommandRunner` from `nest-commander`, and the
//! command factory calls its `run` method when the command executes. Project
//! code does not call `run`, so it would otherwise surface as an
//! `unused-class-member` false positive.
//!
//! The rule is scoped to `CommandRunner` through `UsedClassMemberRule::Scoped`,
//! as the `ag-ui` plugin does. Other methods on a command class are still
//! reported. Heritage matching uses the direct superclass name, so a subclass
//! of a local intermediate base is not covered.

use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};

use super::Plugin;

const ENABLERS: &[&str] = &["nest-commander"];

/// The base class that command classes extend.
const COMMAND_BASE_CLASS: &str = "CommandRunner";

/// The abstract member of `CommandRunner` that the command factory calls.
const COMMAND_RUNTIME_MEMBERS: &[&str] = &["run"];

pub struct NestCommanderPlugin;

impl Plugin for NestCommanderPlugin {
    fn name(&self) -> &'static str {
        "nest-commander"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn used_class_member_rules(&self) -> Vec<UsedClassMemberRule> {
        vec![UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
            extends: Some(COMMAND_BASE_CLASS.to_string()),
            implements: None,
            members: COMMAND_RUNTIME_MEMBERS
                .iter()
                .map(|member| (*member).to_string())
                .collect(),
        })]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enablers_cover_the_package() {
        assert_eq!(NestCommanderPlugin.enablers(), &["nest-commander"]);
    }

    #[test]
    fn run_is_credited_only_on_command_runner_subclasses() {
        let rules = NestCommanderPlugin.used_class_member_rules();
        let [UsedClassMemberRule::Scoped(rule)] = rules.as_slice() else {
            panic!("expected one scoped rule, got {rules:?}");
        };
        assert_eq!(rule.extends.as_deref(), Some("CommandRunner"));
        assert_eq!(rule.implements, None);
        assert_eq!(rule.members, vec!["run".to_string()]);
    }
}
