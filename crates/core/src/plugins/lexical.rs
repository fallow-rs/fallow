//! Lexical editor framework plugin.
//!
//! Activates on the `lexical` core package (or any `@lexical/` scoped package).
//! Lexical reconstructs, clones, serializes, reconciles, and decorates custom
//! nodes through a fixed set of class methods that the editor calls
//! reflectively at runtime. Local project code never calls these directly, so
//! they would otherwise surface as `unused-class-member` false positives.
//!
//! The allowlist is heritage-scoped to the three documented extendable node
//! bases (`DecoratorNode`, `ElementNode`, `TextNode`) via
//! `UsedClassMemberRule::Scoped`, mirroring how the `lit` plugin scopes Lit
//! lifecycle members to `LitElement` / `ReactiveElement`. Non-lifecycle methods
//! on a node subclass are still reported; the rule credits only the named
//! framework hooks, not the whole class.
//!
//! Heritage matching is on the direct superclass name, so an intermediate base
//! (`class MyBase extends DecoratorNode {}` then `class Foo extends MyBase {}`)
//! is not covered. This matches the `lit` / `ember` plugins. Custom nodes
//! extend a Lexical base directly in practice.
//!
//! Custom nodes are registered through the editor config `nodes: [...]` array
//! rather than a module-load side effect, so unused-export detection is not in
//! scope for this plugin; it only handles the reflectively-invoked members.

use fallow_config::{ScopedUsedClassMemberRule, UsedClassMemberRule};

use super::Plugin;

const ENABLERS: &[&str] = &["lexical", "@lexical/"];

/// Node lifecycle, serialization, and DOM-reconciliation members that Lexical
/// invokes at runtime on every custom node, regardless of which base it
/// extends (`DecoratorNode`, `ElementNode`, `TextNode`). `getType`, `clone`,
/// `importJSON`, and `importDOM` are static; the rest are instance methods.
/// Verified against the Lexical custom-node docs
/// (lexical.dev/docs/concepts/nodes).
const LEXICAL_NODE_LIFECYCLE_MEMBERS: &[&str] = &[
    "getType",
    "clone",
    "importJSON",
    "importDOM",
    "exportJSON",
    "exportDOM",
    "createDOM",
    "updateDOM",
    "updateFromJSON",
    "getTextContent",
];

fn scoped_rule(extends: &str, members: &[&str]) -> UsedClassMemberRule {
    UsedClassMemberRule::Scoped(ScopedUsedClassMemberRule {
        extends: Some(extends.to_string()),
        implements: None,
        members: members.iter().map(|s| (*s).to_string()).collect(),
    })
}

pub struct LexicalPlugin;

impl Plugin for LexicalPlugin {
    fn name(&self) -> &'static str {
        "lexical"
    }

    fn enablers(&self) -> &'static [&'static str] {
        ENABLERS
    }

    fn used_class_member_rules(&self) -> Vec<UsedClassMemberRule> {
        // `isInline` is an inline-vs-block layout hook on ElementNode and
        // DecoratorNode only; TextNode is inherently inline and has no
        // isInline(). `decorate` is a DecoratorNode-specific render hook. Both
        // are layered on top of the shared lifecycle set per base so a
        // genuinely-dead isInline / decorate on the wrong node kind still
        // surfaces.
        let element_members: Vec<&str> = LEXICAL_NODE_LIFECYCLE_MEMBERS
            .iter()
            .copied()
            .chain(["isInline"])
            .collect();
        let decorator_members: Vec<&str> = element_members
            .iter()
            .copied()
            .chain(["decorate"])
            .collect();
        vec![
            scoped_rule("DecoratorNode", &decorator_members),
            scoped_rule("ElementNode", &element_members),
            scoped_rule("TextNode", LEXICAL_NODE_LIFECYCLE_MEMBERS),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enablers_cover_core_and_scoped_packages() {
        let plugin = LexicalPlugin;
        assert!(plugin.enablers().contains(&"lexical"));
        assert!(plugin.enablers().contains(&"@lexical/"));
    }

    fn rule_for<'a>(
        rules: &'a [UsedClassMemberRule],
        extends: &str,
    ) -> &'a ScopedUsedClassMemberRule {
        rules
            .iter()
            .find_map(|r| match r {
                UsedClassMemberRule::Scoped(s) if s.extends.as_deref() == Some(extends) => Some(s),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{extends}-scoped rule missing"))
    }

    #[test]
    fn rules_scope_lifecycle_members_to_the_three_node_bases() {
        let rules = LexicalPlugin.used_class_member_rules();
        for base in ["DecoratorNode", "ElementNode", "TextNode"] {
            let rule = rule_for(&rules, base);
            for member in ["getType", "clone", "createDOM", "updateDOM", "exportJSON"] {
                assert!(
                    rule.members.iter().any(|m| m == member),
                    "{base} rule should credit {member}; members = {:?}",
                    rule.members
                );
            }
        }
    }

    #[test]
    fn decorate_is_scoped_to_decorator_node_only() {
        let rules = LexicalPlugin.used_class_member_rules();
        assert!(
            rule_for(&rules, "DecoratorNode")
                .members
                .iter()
                .any(|m| m == "decorate"),
            "DecoratorNode rule must credit decorate"
        );
        for base in ["ElementNode", "TextNode"] {
            assert!(
                !rule_for(&rules, base)
                    .members
                    .iter()
                    .any(|m| m == "decorate"),
                "{base} rule must not credit decorate (DecoratorNode-only hook)"
            );
        }
    }

    #[test]
    fn is_inline_is_scoped_to_element_and_decorator_nodes_only() {
        let rules = LexicalPlugin.used_class_member_rules();
        for base in ["DecoratorNode", "ElementNode"] {
            assert!(
                rule_for(&rules, base)
                    .members
                    .iter()
                    .any(|m| m == "isInline"),
                "{base} rule must credit isInline"
            );
        }
        assert!(
            !rule_for(&rules, "TextNode")
                .members
                .iter()
                .any(|m| m == "isInline"),
            "TextNode rule must not credit isInline (TextNode is inherently inline; no isInline hook)"
        );
    }

    #[test]
    fn rules_match_only_the_declared_super_class() {
        let rules = LexicalPlugin.used_class_member_rules();
        let decorator_rule = rule_for(&rules, "DecoratorNode");
        assert!(decorator_rule.matches_heritage(Some("DecoratorNode"), &[]));
        assert!(!decorator_rule.matches_heritage(Some("UserService"), &[]));
        assert!(!decorator_rule.matches_heritage(Some("ElementNode"), &[]));
    }

    #[test]
    fn unrelated_classes_get_no_lifecycle_rule_match() {
        let rules = LexicalPlugin.used_class_member_rules();
        for r in &rules {
            let UsedClassMemberRule::Scoped(s) = r else {
                continue;
            };
            assert!(!s.matches_heritage(Some("HTMLElement"), &[]));
            assert!(!s.matches_heritage(None, &[]));
        }
    }
}
