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
    // members through both property chains. The chain `app.details.input`
    // also reads `details` on `AppPage` and `input` on `DetailsPage`.
    let unused = unused_class_members("playwright-fixture-property-chain");
    assert_credited(
        &unused,
        &[
            "AppPage.goto",
            "AppPage.details",
            "DetailsPage.getParameter",
            "DetailsPage.input",
            "InputPanel.select",
        ],
        "DetailsPage.unusedHelper",
    );
    assert!(
        unused.contains(&"InputPanel.unusedSelect".to_string()),
        "InputPanel.unusedSelect has no access and must stay flagged, found: {unused:?}"
    );
}

#[test]
fn imported_array_factory_element_credits_member() {
    // `make` in another module declares the return type `Sheet[]`. An element
    // of `make()` (an iteration callback parameter, a `for...of` variable, an
    // index), also through the local `sheets`, holds a `Sheet`. `makeSingle`
    // returns one `Single`, so an element of its result is not a `Single`.
    let unused = unused_class_members("class-member-imported-array-factory");
    assert_credited(
        &unused,
        &[
            "Sheet.viaCallback",
            "Sheet.viaLoop",
            "Sheet.viaMap",
            "Sheet.viaDirectLoop",
            "Sheet.viaIndex",
        ],
        "Sheet.unused",
    );
    assert!(
        unused.contains(&"Single.viaLoop".to_string()),
        "Single.viaLoop is read on an element of a single instance, found: {unused:?}"
    );
}

#[test]
fn utility_type_receiver_credits_member() {
    // A value typed `Pick<Coordinator, ...>`, `Partial<Coordinator>` or
    // `Readonly<Omit<Coordinator, ...>>` holds a `Coordinator`. No position
    // (parameter property, field, constructor parameter, interface field,
    // arrow parameter) gets a `new Coordinator()` argument at the call site.
    let unused = unused_class_members("class-member-utility-type-receivers");
    assert_credited(
        &unused,
        &[
            "Coordinator.viaParamProperty",
            "Coordinator.viaField",
            "Coordinator.viaCtorParam",
            "Coordinator.viaInterfaceField",
            "Coordinator.viaArrowParam",
            "Coordinator.viaPartial",
            "Coordinator.viaReadonlyOmit",
        ],
        "Coordinator.unused",
    );
}

#[test]
fn durable_object_stub_credits_member() {
    // `env.QUOTA` is a `DurableObjectNamespace<DailyQuota>`. The result of
    // `get()` and `getByName()` on it is an RPC stub of `DailyQuota`, as is a
    // value typed `DurableObjectStub<DailyQuota>`. The `get()` on the class
    // `Repo` takes no Durable Object id, so its result is not a `Repo`.
    let unused = unused_class_members("class-member-durable-object-stub");
    assert_credited(
        &unused,
        &[
            "DailyQuota.viaGet",
            "DailyQuota.viaGetByName",
            "DailyQuota.viaIdLocal",
            "DailyQuota.viaAnnotatedStub",
            "Repo.get",
        ],
        "DailyQuota.unused",
    );
    assert!(
        unused.contains(&"Repo.unusedOnRepo".to_string()),
        "Repo.unusedOnRepo is read on the result of `Repo.get`, found: {unused:?}"
    );
}
