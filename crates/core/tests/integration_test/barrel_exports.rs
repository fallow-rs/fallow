use super::common::{create_config, fixture_path};

#[test]
fn barrel_exports_resolves_through_barrel() {
    let root = fixture_path("barrel-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"fooUnused"),
        "fooUnused should be unused, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"join"),
        "a consumed external named re-export must stay live: {unused_export_names:?}"
    );
}

#[test]
fn barrel_unused_re_exports_detected() {
    let root = fixture_path("barrel-unused-reexports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"UnusedComponent"),
        "UnusedComponent should be detected as unused re-export on barrel, found: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names.contains(&"UsedComponent"),
        "UsedComponent should NOT be detected as unused"
    );
}

#[test]
fn barrel_unused_type_re_exports_detected() {
    let root = fixture_path("barrel-unused-reexports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_type_names: Vec<&str> = results
        .unused_types
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_type_names.contains(&"UnusedType"),
        "UnusedType should be detected as unused type re-export on barrel, found: {unused_type_names:?}"
    );

    assert!(
        !unused_type_names.contains(&"UsedType"),
        "UsedType should NOT be detected as unused type"
    );
}

#[test]
fn barrel_re_export_propagates_to_source_module() {
    let root = fixture_path("barrel-unused-reexports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results
            .unused_exports
            .iter()
            .any(|e| e.export.export_name == "UsedComponent"),
        "source UsedComponent should not be unused since barrel re-export is consumed"
    );
}

#[test]
fn barrel_type_usage_credits_legal_declaration_merges_only() {
    let root = fixture_path("declaration-merge");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_exports: Vec<_> = results
        .unused_exports
        .iter()
        .map(|finding| finding.export.export_name.as_str())
        .collect();
    let unused_types: Vec<_> = results
        .unused_types
        .iter()
        .map(|finding| finding.export.export_name.as_str())
        .collect();

    assert!(!unused_exports.contains(&"Merged"));
    assert!(!unused_types.contains(&"Merged"));
    assert!(!unused_exports.contains(&"NamedMerged"));
    assert!(!unused_types.contains(&"NamedMerged"));
    assert!(unused_exports.contains(&"Independent"));
    assert!(!unused_types.contains(&"Independent"));
    assert!(unused_types.contains(&"UnusedControl"));
}

#[test]
fn renamed_exports_route_through_star_only_surfaces() {
    let root = fixture_path("renamed-star-surface");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    let unused_exports: Vec<_> = results
        .unused_exports
        .iter()
        .map(|finding| finding.export.export_name.as_str())
        .collect();
    let unused_types: Vec<_> = results
        .unused_types
        .iter()
        .map(|finding| finding.export.export_name.as_str())
        .collect();

    assert!(!unused_exports.contains(&"ValueMerged"));
    assert!(!unused_types.contains(&"TypeMerged"));
    assert!(unused_exports.contains(&"UnusedValueControl"));
    assert!(unused_types.contains(&"UnusedTypeControl"));
}

#[test]
fn source_order_independent_import_forwarding_is_re_export() {
    let root = fixture_path("source-order-re-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.duplicate_exports.is_empty(),
        "import-forwarding barrels should not emit duplicate exports when export appears before import: {:?}",
        results
            .duplicate_exports
            .iter()
            .map(|duplicate| duplicate.export.export_name.as_str())
            .collect::<Vec<_>>()
    );

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|export| export.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"used"),
        "used should propagate through the source-order-independent barrel, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"unused"),
        "genuinely unused source exports should still be reported, found: {unused_export_names:?}"
    );
}

#[test]
fn explicit_re_export_shadows_star_export_with_the_same_name() {
    let root = fixture_path("effective-export-explicit-shadow");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_exports: Vec<_> = results
        .unused_exports
        .iter()
        .map(|finding| {
            (
                finding.export.path.to_string_lossy().replace('\\', "/"),
                finding.export.export_name.as_str(),
            )
        })
        .collect();

    assert!(
        unused_exports
            .iter()
            .any(|(path, name)| { path.ends_with("src/star-source.ts") && *name == "foo" }),
        "the star source's shadowed foo must remain unused: {unused_exports:?}"
    );
    assert!(
        !unused_exports
            .iter()
            .any(|(path, name)| { path.ends_with("src/explicit-source.ts") && *name == "foo" }),
        "the explicit re-export is the effective foo binding: {unused_exports:?}"
    );
}

#[test]
fn shadowed_star_export_does_not_form_a_duplicate_export_group() {
    let root = fixture_path("effective-export-explicit-shadow");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.duplicate_exports.is_empty(),
        "the shadowed star export must not form a duplicate-export group: {:?}",
        results.duplicate_exports
    );
}

#[test]
fn conflicting_star_exports_do_not_resolve_an_effective_binding() {
    let root = fixture_path("effective-export-ambiguous-star");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_foo_paths: Vec<_> = results
        .unused_exports
        .iter()
        .filter(|finding| finding.export.export_name == "foo")
        .map(|finding| finding.export.path.to_string_lossy().replace('\\', "/"))
        .collect();

    assert_eq!(
        unused_foo_paths.len(),
        2,
        "an ambiguous barrel import must credit neither foo binding: {unused_foo_paths:?}"
    );
    assert!(
        unused_foo_paths
            .iter()
            .any(|path| path.ends_with("src/left.ts")),
        "left.foo must remain unused: {unused_foo_paths:?}"
    );
    assert!(
        unused_foo_paths
            .iter()
            .any(|path| path.ends_with("src/right.ts")),
        "right.foo must remain unused: {unused_foo_paths:?}"
    );
}

#[test]
fn convergent_star_exports_resolve_the_shared_original_binding() {
    let root = fixture_path("effective-export-convergent-star");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results
            .unused_exports
            .iter()
            .any(|finding| finding.export.export_name == "foo"),
        "two star paths to one original binding are not ambiguous: {:?}",
        results.unused_exports
    );
}

#[test]
fn barrel_exports_detects_unused_re_export_bar() {
    let root = fixture_path("barrel-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"bar"),
        "bar should be detected as unused re-export on barrel (nobody imports it), found: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names.contains(&"foo"),
        "foo should NOT be unused since index.ts imports it from barrel"
    );
}

#[test]
fn multi_hop_barrel_used_propagates() {
    let root = fixture_path("multi-hop-barrel");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results
            .unused_exports
            .iter()
            .any(|e| e.export.export_name == "used"),
        "used should propagate through barrel chain and NOT be flagged"
    );
}

#[test]
fn multi_hop_barrel_unused_detected() {
    let root = fixture_path("multi-hop-barrel");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"unused2"),
        "unused2 should be detected as unused export, found: {unused_export_names:?}"
    );
}

#[test]
fn star_re_export_chain_used_propagates() {
    let root = fixture_path("star-re-export-chain");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"used"),
        "used should propagate through star re-export chain and NOT be flagged, found: {unused_export_names:?}"
    );
}

#[test]
fn star_re_export_chain_unused_detected() {
    let root = fixture_path("star-re-export-chain");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"unused"),
        "unused should be detected as unused export, found: {unused_export_names:?}"
    );
}

#[test]
fn multi_level_chain_used_exports_propagate() {
    let root = fixture_path("multi-level-barrel-chain");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"alpha"),
        "alpha should propagate through 3-level chain and NOT be flagged, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"beta"),
        "beta should propagate through 3-level chain and NOT be flagged, found: {unused_export_names:?}"
    );
}

#[test]
fn multi_level_chain_partially_re_exported_detected() {
    let root = fixture_path("multi-level-barrel-chain");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"gamma"),
        "gamma should be unused (re-exported but never imported), found: {unused_export_names:?}"
    );

    assert!(
        unused_export_names.contains(&"delta"),
        "delta should be unused (not re-exported from top-level barrel), found: {unused_export_names:?}"
    );

    assert!(
        unused_export_names.contains(&"epsilon"),
        "epsilon should be unused (not re-exported at all), found: {unused_export_names:?}"
    );
}

#[test]
fn star_selective_usage_used_propagates() {
    let root = fixture_path("star-selective-usage");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"usedOne"),
        "usedOne should NOT be flagged (imported via star barrel), found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"usedTwo"),
        "usedTwo should NOT be flagged (imported via star barrel), found: {unused_export_names:?}"
    );
}

#[test]
fn star_selective_usage_unused_detected() {
    let root = fixture_path("star-selective-usage");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"unusedThree"),
        "unusedThree should be unused (star re-exported but not imported), found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"unusedFour"),
        "unusedFour should be unused (star re-exported but not imported), found: {unused_export_names:?}"
    );
}

#[test]
fn mixed_named_star_used_propagates() {
    let root = fixture_path("mixed-named-star-reexports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"namedUsed"),
        "namedUsed should NOT be flagged (imported via named barrel re-export), found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"starUsed"),
        "starUsed should NOT be flagged (imported via star barrel re-export), found: {unused_export_names:?}"
    );
}

#[test]
fn mixed_named_star_unused_detected() {
    let root = fixture_path("mixed-named-star-reexports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"namedUnused"),
        "namedUnused should be unused (named re-exported but not imported), found: {unused_export_names:?}"
    );

    assert!(
        unused_export_names.contains(&"starUnused"),
        "starUnused should be unused (star re-exported but not imported), found: {unused_export_names:?}"
    );
}

#[test]
fn alias_chain_used_exports_propagate() {
    let root = fixture_path("re-export-alias-chain");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"original"),
        "original should NOT be flagged (used through alias chain as aliasC), found: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names.contains(&"renamed"),
        "renamed should NOT be flagged (used through alias chain as doubleAlias), found: {unused_export_names:?}"
    );
}

#[test]
fn alias_chain_unused_detected() {
    let root = fixture_path("re-export-alias-chain");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"unusedOriginal"),
        "unusedOriginal should be unused (aliased but never imported), found: {unused_export_names:?}"
    );

    assert!(
        unused_export_names.contains(&"neverExported"),
        "neverExported should be unused (not re-exported at all), found: {unused_export_names:?}"
    );
}

#[test]
fn circular_re_export_completes_without_infinite_loop() {
    let root = fixture_path("circular-re-export");
    let config = create_config(root);
    let results =
        fallow_core::analyze(&config).expect("analysis should succeed with circular re-exports");

    assert!(
        !results
            .unused_exports
            .iter()
            .any(|e| e.export.export_name == "fromA" && !e.export.is_re_export),
        "original fromA definition should NOT be flagged (imported directly by index.ts)"
    );
    assert!(
        !results
            .unused_exports
            .iter()
            .any(|e| e.export.export_name == "fromB" && !e.export.is_re_export),
        "original fromB definition should NOT be flagged (imported directly by index.ts)"
    );

    assert!(
        results
            .unused_exports
            .iter()
            .any(|e| e.export.export_name == "fromB" && e.export.is_re_export),
        "fromB re-export on module-a should be flagged as unused"
    );
    assert!(
        results
            .unused_exports
            .iter()
            .any(|e| e.export.export_name == "fromA" && e.export.is_re_export),
        "fromA re-export on module-b should be flagged as unused"
    );
}

#[test]
fn circular_re_export_no_unused_files() {
    let root = fixture_path("circular-re-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_files.is_empty(),
        "no files should be unused in circular re-export fixture, found: {:?}",
        results
            .unused_files
            .iter()
            .map(|f| &f.file.path)
            .collect::<Vec<_>>()
    );
}

#[test]
fn barrel_default_reexport_unused_detected() {
    let root = fixture_path("barrel-default-reexport");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"Card"),
        "Card should be detected as unused re-export on barrel, found: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names.contains(&"Button"),
        "Button should NOT be detected as unused (imported by index.ts)"
    );
}

#[test]
fn barrel_default_reexport_no_unused_files() {
    let root = fixture_path("barrel-default-reexport");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_paths: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().replace('\\', "/"))
        .collect();

    assert!(
        !unused_file_paths.iter().any(|p| p.contains("Button.ts")),
        "Button.ts should NOT be unused (re-exported and imported), found: {unused_file_paths:?}"
    );

    assert!(
        !unused_file_paths
            .iter()
            .any(|p| p.contains("components/index.ts")),
        "components/index.ts barrel should NOT be unused, found: {unused_file_paths:?}"
    );
}

#[test]
fn star_barrel_does_not_forward_or_credit_default_export() {
    let root = fixture_path("star-barrel-default-isolation");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_exports.iter().any(|finding| {
            finding.export.export_name == "default"
                && finding
                    .export
                    .path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .ends_with("src/source.ts")
        }),
        "a default import from an export-star barrel must not credit the source default: {:?}",
        results
            .unused_exports
            .iter()
            .map(|finding| (&finding.export.path, &finding.export.export_name))
            .collect::<Vec<_>>()
    );
}

#[test]
fn star_barrel_namespace_default_access_does_not_credit_source_default() {
    let root = fixture_path("star-barrel-namespace-default-isolation");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unused_exports.iter().any(|finding| {
            finding.export.export_name == "default"
                && finding
                    .export
                    .path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .ends_with("src/source.ts")
        }),
        "ns.default on an export-star barrel must not credit the source default"
    );
}

#[test]
fn explicit_default_reexport_alongside_star_still_credits_source_default() {
    let root = fixture_path("explicit-default-with-star");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        !results.unused_exports.iter().any(|finding| {
            finding.export.export_name == "default"
                && finding
                    .export
                    .path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .ends_with("src/source.ts")
        }),
        "an explicit default re-export must still credit the source default"
    );
}
