use fallow_config::Severity;

use super::common::{create_config, create_production_config, fixture_path};

fn unused_members(fixture: &str, rule_on: bool, production: bool) -> (Vec<String>, Vec<String>) {
    let root = fixture_path(fixture);
    let mut config = if production {
        create_production_config(root.clone())
    } else {
        create_config(root.clone())
    };
    if rule_on {
        config.rules.unused_protected_class_members = Severity::Error;
    }
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let render = |member: &fallow_core::results::UnusedMember| {
        let path = member.path.strip_prefix(&root).unwrap_or(&member.path);
        format!(
            "{}:{}.{}",
            path.to_string_lossy().replace('\\', "/"),
            member.parent_name,
            member.member_name
        )
    };
    let mut protected: Vec<String> = results
        .unused_protected_class_members
        .iter()
        .map(|finding| render(&finding.member))
        .collect();
    protected.sort();
    let mut public: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|finding| render(&finding.member))
        .collect();
    public.sort();
    (protected, public)
}

/// A protected member is used when its class or a subclass reads it through
/// `this` or `super`, or when the base class calls a member that a subclass
/// overrides. A class of the package public API stays silent, and so does a
/// suppression comment. `unused-class-members` keeps its public findings.
#[test]
fn reports_unread_protected_members_when_rule_is_on() {
    let (protected, public) = unused_members("unused-protected-class-members", true, false);
    assert_eq!(
        protected,
        [
            "src/base.ts:Base.unusedBase",
            "src/shapes.ts:Shape.unusedProtected",
            "src/shapes.ts:Shape.unusedProtectedField",
            "src/shapes.ts:Square.unusedHelper",
        ]
    );
    assert_eq!(public, ["src/shapes.ts:Shape.unusedPublic"]);
}

/// The rule is off by default, so the default output does not change.
#[test]
fn reports_no_protected_members_by_default() {
    let (protected, public) = unused_members("unused-protected-class-members", false, false);
    assert!(protected.is_empty(), "{protected:?}");
    assert_eq!(public, ["src/shapes.ts:Shape.unusedPublic"]);
}

/// Production mode treats protected members like public members: a member
/// that only test code reads is unused.
#[test]
fn production_mode_reports_protected_members_that_only_tests_read() {
    let (protected, _) = unused_members("unused-protected-class-members", true, true);
    assert!(
        protected.contains(&"src/shapes.ts:Square.testOnlyHelper".to_string()),
        "{protected:?}"
    );
}

/// `Renderer.draw` in one file calls `this.paint()`, and `FancyRenderer` in
/// another file overrides `paint`. The base-class call credits the override,
/// so neither `paint` is unused.
#[test]
fn override_in_another_file_is_used_when_the_base_class_calls_it() {
    let (protected, _) = unused_members("unused-protected-class-members", true, false);
    assert!(
        !protected.contains(&"src/fancy-renderer.ts:FancyRenderer.paint".to_string()),
        "{protected:?}"
    );
    assert!(
        !protected.contains(&"src/renderer.ts:Renderer.paint".to_string()),
        "{protected:?}"
    );
}
