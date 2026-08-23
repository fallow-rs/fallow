use crate::common::{create_config, fixture_path};

/// Issue #2377: a namespace import handed over whole must credit every export
/// of its target, whatever position hands it over. Before the fix the visitor
/// recorded a whole-object use for an allow-list of positions only
/// (`Object.keys(NS)`, spread, `for ... in`, computed non-string access, rest
/// destructuring), so a consumer that also wrote one dotted access narrowed to
/// that member and reported every sibling the receiver can still reach.
///
/// The fixture pins one consumer per handover shape, each against its own
/// namespace target so a miscredit cannot be masked by a sibling shape:
/// - a JSX attribute value (`<Callout icons={AttributeIcons} />`),
/// - a call argument (`register(ArgumentIcons)`),
/// - an alias (`const alias = AliasIcons`),
/// - an array literal element (`[ArrayIcons]`),
/// - an object literal value (`{ icons: ObjectIcons }`),
/// - a return value (`return ReturnIcons`),
///
/// plus the precision control: a namespace used only through dotted accesses
/// still narrows, so its genuinely unused sibling keeps reporting.
#[test]
fn whole_object_namespace_pass_credits_every_export() {
    let root = fixture_path("issue-2377-whole-object-use");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    for (shape, sibling) in [
        ("a JSX attribute value", "AttributeMoon"),
        ("a call argument", "ArgumentMoon"),
        ("an alias", "AliasMoon"),
        ("an array literal element", "ArrayMoon"),
        ("an object literal value", "ObjectMoon"),
        ("a return value", "ReturnMoon"),
    ] {
        assert!(
            !unused_export_names.contains(&sibling),
            "a namespace handed over as {shape} must credit every export: {unused_export_names:?}"
        );
    }

    assert!(
        unused_export_names.contains(&"DottedMoon"),
        "a namespace used only through dotted accesses must still narrow: {unused_export_names:?}"
    );
}
