use super::common::{create_config, fixture_path};

fn unused_members(fixture: &str) -> Vec<String> {
    let root = fixture_path(fixture);
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .unused_class_members
        .iter()
        .map(|member| {
            format!(
                "{}.{}",
                member.member.parent_name, member.member.member_name
            )
        })
        .collect()
}

/// Issue #1713 (#1707 follow-up): a `.map()` callback in the Astro TEMPLATE body
/// (`{utils.map((util) => <li>{util.getter}</li>)}`) whose receiver is a
/// frontmatter `Util[]` binding credits the element-class members. Previously the
/// template `{...}` region was only scanned for bare identifiers, so `util.getter`
/// / `util.property` / `util.hello` were never credited and false-reported as
/// `unused-class-member`. The control `unusedMethod` (accessed nowhere) must still
/// report, proving the fix is over-credit-only.
#[test]
fn astro_template_map_credits_element_class_members() {
    let unused = unused_members("issue-1713-astro-template-map-class-member");

    for member in ["getter", "property", "hello"] {
        assert!(
            !unused.contains(&format!("Util.{member}")),
            "Util.{member} is accessed via the Astro template .map callback and must be credited, found: {unused:?}"
        );
    }
    assert!(
        unused.contains(&"Util.unusedMethod".to_string()),
        "Util.unusedMethod is never accessed and must still report, found: {unused:?}"
    );
}
