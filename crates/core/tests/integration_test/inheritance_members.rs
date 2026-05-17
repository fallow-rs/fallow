use super::common::{create_config, fixture_path};

#[test]
fn inheritance_propagates_this_accesses_to_children() {
    let root = fixture_path("inheritance-project");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // Child class members that are accessed via `this.*` in the parent class
    // should NOT be reported as unused.
    // BaseShape.describe() calls this.kind, this.getArea(), this.getPerimeter()
    // which should propagate to Circle and Rectangle.
    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    // Circle's members should be credited via inheritance propagation
    assert!(
        !unused_members.contains(&"Circle.kind".to_string()),
        "Circle.kind should be used via this.kind in BaseShape: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"Circle.getArea".to_string()),
        "Circle.getArea should be used via this.getArea() in BaseShape: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"Circle.getPerimeter".to_string()),
        "Circle.getPerimeter should be used via this.getPerimeter() in BaseShape: {unused_members:?}"
    );

    // Rectangle's members should also be credited
    assert!(
        !unused_members.contains(&"Rectangle.kind".to_string()),
        "Rectangle.kind should be used via this.kind in BaseShape: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"Rectangle.getArea".to_string()),
        "Rectangle.getArea should be used via this.getArea() in BaseShape: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"Rectangle.getPerimeter".to_string()),
        "Rectangle.getPerimeter should be used via this.getPerimeter() in BaseShape: {unused_members:?}"
    );

    // Default export class extends: `export default class extends BaseShape`
    // Members should also be credited via inheritance propagation
    assert!(
        !unused_members.contains(&"default.kind".to_string()),
        "default export class kind should be used via this.kind in BaseShape: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"default.getArea".to_string()),
        "default export class getArea should be used via this.getArea() in BaseShape: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"default.getPerimeter".to_string()),
        "default export class getPerimeter should be used via this.getPerimeter() in BaseShape: {unused_members:?}"
    );
}
