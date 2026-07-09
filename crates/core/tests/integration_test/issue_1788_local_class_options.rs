use super::common::{create_config, fixture_path};

#[test]
fn local_unexported_options_class_credits_imported_property_member() {
    // Issue #1788: an options object typed by a LOCAL, UNEXPORTED class whose
    // property type is imported (`class Opts { constructor(public c:
    // ImportedDep) {} }` + `this.opts.c.viaLocalOpts()`) must credit the
    // imported class's member. The unexported class never resolves through
    // `local_to_export_keys`, so the extract-side expansion has to hop
    // through the class's own typed-property bindings. A genuinely unused
    // method on the same class stays flagged.
    let root = fixture_path("issue-1788-local-class-options");
    let mut config = create_config(root);
    config.rules.unused_class_members = fallow_config::Severity::Error;
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|m| format!("{}.{}", m.member.parent_name, m.member.member_name))
        .collect();

    assert!(
        !unused.contains(&"ImportedDep.viaLocalOpts".to_string()),
        "ImportedDep.viaLocalOpts is reached through a local unexported options class and \
         must be credited (issue #1788), found: {unused:?}"
    );
    assert!(
        unused.contains(&"ImportedDep.deadOnImportedDep".to_string()),
        "ImportedDep.deadOnImportedDep has no call site and must stay flagged, found: {unused:?}"
    );
}
