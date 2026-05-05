//! Lit framework / Web Components plugin.
//!
//! Activates on `lit`, `lit-element`, or `@lit/reactive-element` dependencies.
//! Tracks Lit's runtime tooling deps (so they are not flagged as unused) and
//! whitelists the lifecycle methods that Lit invokes at runtime, scoped to
//! classes that extend `LitElement` or `ReactiveElement`. Native Custom
//! Elements lifecycle methods on `HTMLElement` subclasses are handled by the
//! member analyzer so they work without a Lit dependency.
//!
//! The decorator-form (`@customElement`) and `customElements.define(...)` form
//! both flow through `crates/extract/src/visitor` which marks the registered
//! class export as side-effect-used so unused-export detection ignores it.
//! That detection is independent of this plugin; this plugin only handles the
//! lifecycle members that the framework calls reflectively.

use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};

use super::Plugin;

const ENABLERS: &[&str] = &["lit", "lit-element", "@lit/reactive-element"];

const TOOLING_DEPENDENCIES: &[&str] = &[
    "lit",
    "lit-element",
    "lit-html",
    "@lit/reactive-element",
    "@lit/context",
    "@lit/task",
    "@lit/localize",
    "@lit-labs/ssr",
    "@lit-labs/router",
    "@lit-labs/observers",
    "@lit-labs/motion",
    "@lit-labs/preact-signals",
    "@lit-labs/signals",
    "@lit-labs/virtualizer",
];

/// Lit lifecycle and reactive members called by the framework at runtime,
/// in addition to the native Web Components lifecycle.
const LIT_LIFECYCLE_MEMBERS: &[&str] = &[
    "render",
    "update",
    "updated",
    "willUpdate",
    "firstUpdated",
    "shouldUpdate",
    "performUpdate",
    "scheduleUpdate",
    "getUpdateComplete",
    "requestUpdate",
    "createRenderRoot",
    "properties",
    "styles",
    "elementProperties",
    "finalize",
    "finalized",
    // Native lifecycle is also called on Lit elements via super.connectedCallback() etc.
    "connectedCallback",
    "disconnectedCallback",
    "attributeChangedCallback",
    "adoptedCallback",
    "connectedMoveCallback",
    "observedAttributes",
];

fn scoped_rule(extends: &str, members: &[&str]) -> UsedClassMemberRule {
    UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: Some(extends.to_string()),
        implements: None,
        members: members.iter().map(|s| (*s).to_string()).collect(),
    })
}

pub struct LitPlugin;

impl Plugin for LitPlugin {
    fn name(&self) -> &'static str {
        "lit"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn tooling_dependencies(&self) -> &'static [&'static str] {
        TOOLING_DEPENDENCIES
    }

    fn used_class_member_rules(&self) -> Vec<UsedClassMemberRule> {
        vec![
            scoped_rule("LitElement", LIT_LIFECYCLE_MEMBERS),
            scoped_rule("ReactiveElement", LIT_LIFECYCLE_MEMBERS),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enablers_cover_lit_3_and_legacy() {
        let plugin = LitPlugin;
        assert!(plugin.enablers().contains(&"lit"));
        assert!(plugin.enablers().contains(&"lit-element"));
        assert!(plugin.enablers().contains(&"@lit/reactive-element"));
    }

    #[test]
    fn tooling_dependencies_cover_runtime_packages() {
        let plugin = LitPlugin;
        let deps = plugin.tooling_dependencies();
        assert!(deps.contains(&"lit"));
        assert!(deps.contains(&"lit-html"));
        assert!(deps.contains(&"@lit/reactive-element"));
        assert!(deps.contains(&"@lit/context"));
    }

    #[test]
    fn lifecycle_rules_scope_lit_members_to_lit_element_subclasses() {
        let rules = LitPlugin.used_class_member_rules();
        let lit_rule = rules.iter().find_map(|r| match r {
            UsedClassMemberRule::Scoped(s) if s.extends.as_deref() == Some("LitElement") => Some(s),
            _ => None,
        });
        let lit_rule = lit_rule.expect("LitElement-scoped rule missing");
        assert!(lit_rule.members.iter().any(|m| m == "render"));
        assert!(lit_rule.members.iter().any(|m| m == "firstUpdated"));
        assert!(lit_rule.members.iter().any(|m| m == "connectedCallback"));
    }

    #[test]
    fn lifecycle_rules_do_not_depend_on_native_html_element_scope() {
        let rules = LitPlugin.used_class_member_rules();
        assert!(rules.iter().all(|r| match r {
            UsedClassMemberRule::Scoped(s) => s.extends.as_deref() != Some("HTMLElement"),
            UsedClassMemberRule::Name(_) => true,
        }));
    }

    #[test]
    fn unrelated_classes_get_no_lifecycle_rule_match() {
        let rules = LitPlugin.used_class_member_rules();
        for r in &rules {
            let UsedClassMemberRule::Scoped(s) = r else {
                continue;
            };
            assert!(!s.matches_heritage(Some("UserService"), &[]));
        }
    }

    #[test]
    fn scoped_rule_matches_only_declared_super() {
        let rules = LitPlugin.used_class_member_rules();
        let lit_rule = rules
            .iter()
            .find_map(|r| match r {
                UsedClassMemberRule::Scoped(s) if s.extends.as_deref() == Some("LitElement") => {
                    Some(s)
                }
                _ => None,
            })
            .expect("LitElement rule");
        assert!(lit_rule.matches_heritage(Some("LitElement"), &[]));
        assert!(!lit_rule.matches_heritage(Some("HTMLElement"), &[]));
    }
}
