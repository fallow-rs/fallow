use super::common::{create_config, fixture_path};

fn unused_class_members(fixture: &str) -> Vec<String> {
    let mut config = create_config(fixture_path(fixture));
    config.rules.unused_class_members = fallow_config::Severity::Error;
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

fn assert_credited(unused: &[String], used: &[&str], dead: &str) {
    for member in used {
        assert!(
            !unused.contains(&(*member).to_string()),
            "{member} is read through an inferred receiver, found: {unused:?}"
        );
    }
    assert!(
        unused.contains(&dead.to_string()),
        "{dead} has no access and must stay flagged, found: {unused:?}"
    );
}

#[test]
fn awaited_static_factory_result_credits_member() {
    // `const awaited = await Service.create()` with
    // `static async create(): Promise<Service>` holds a `Service`, as
    // `Service.build()` with `static build(): Service` does.
    let unused = unused_class_members("class-member-awaited-static-factory");
    assert_credited(
        &unused,
        &["Service.viaAwait", "Service.viaSync"],
        "Service.unused",
    );
}

#[test]
fn array_field_element_credits_member() {
    // `Month.days` is typed `Day[]`. An element of `m.days` (an iteration
    // callback parameter, a `for...of` variable, an index), an element of
    // `this.days`, and an element of the getter `sorted` hold a `Day`.
    let unused = unused_class_members("class-member-array-field-element");
    assert_credited(
        &unused,
        &[
            "Day.viaIndex",
            "Day.viaThisIndex",
            "Day.viaCallback",
            "Day.viaLoop",
            "Day.viaGetter",
        ],
        "Day.unused",
    );
}

#[test]
fn playwright_fixture_property_chain_credits_member() {
    // The `app` fixture is an `AppPage`, whose property `details` is a
    // `DetailsPage`, whose property `input` is an `InputPanel`. The spec reads
    // members through both property chains.
    let unused = unused_class_members("playwright-fixture-property-chain");
    assert_credited(
        &unused,
        &[
            "AppPage.goto",
            "DetailsPage.getParameter",
            "InputPanel.select",
        ],
        "DetailsPage.unusedHelper",
    );
    assert!(
        unused.contains(&"InputPanel.unusedSelect".to_string()),
        "InputPanel.unusedSelect has no access and must stay flagged, found: {unused:?}"
    );
}
