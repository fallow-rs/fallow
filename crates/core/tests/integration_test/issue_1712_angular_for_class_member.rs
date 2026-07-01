use super::common::{create_config, fixture_path};

/// Issue #1712: an Angular `@for` / `*ngFor` loop variable iterating over a
/// component field typed as an array of a class (`utils: Util[]`) must credit
/// member accesses on the loop item, so the class members are not falsely
/// reported as unused. The fixture's `app.component.ts` uses an INLINE template
/// with both `@for (util of utils; track util)` and `*ngFor="let util of utils"`
/// reading `util.getName()` / `util.getter` / `util.property`; `Util.unusedMethod`
/// is never accessed and is the non-vacuous control proving the detector still
/// fires.
#[test]
fn angular_for_loop_variable_credits_class_member_accesses() {
    let root = fixture_path("issue-1712-angular-for-class-member");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|member| {
            format!(
                "{}.{}",
                member.member.parent_name, member.member.member_name
            )
        })
        .collect();

    for member in ["getName", "getter", "property"] {
        assert!(
            !unused_members.contains(&format!("Util.{member}")),
            "Util.{member} is accessed via the @for / *ngFor loop item `util` and must be credited, found: {unused_members:?}"
        );
    }

    assert!(
        unused_members.contains(&"Util.unusedMethod".to_string()),
        "Util.unusedMethod is never accessed and must still be reported (the loop crediting must not blanket-credit the class), found: {unused_members:?}"
    );
}
