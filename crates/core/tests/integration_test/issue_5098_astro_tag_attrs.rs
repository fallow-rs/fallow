use crate::common::{create_config, fixture_path};

fn reported_unused_exports() -> Vec<String> {
    let root = fixture_path("issue-5098-astro-tag-attrs");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.clone())
        .collect()
}

#[test]
fn astro_style_and_script_attribute_expressions_credit_imports() {
    let reported = reported_unused_exports();

    for used in ["CSS", "JS", "STYLE_DATA", "SCRIPT_DATA"] {
        assert!(
            !reported.iter().any(|name| name == used),
            "{used} is referenced in a <style> or <script> opening tag attribute and must be credited: {reported:?}"
        );
    }

    for unused in ["STATIC_STR", "COMMENTED", "UNUSED"] {
        assert!(
            reported.iter().any(|name| name == unused),
            "{unused} is not evaluated as an attribute expression and should remain unused: {reported:?}"
        );
    }
}
