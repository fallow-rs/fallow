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

    // fooUnused should be detected as unused (it's not re-exported from barrel)
    assert!(
        unused_export_names.contains(&"fooUnused"),
        "fooUnused should be unused, found: {unused_export_names:?}"
    );
}

// ── Barrel re-export unused detection ──────────────────────────

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

    // UnusedComponent is re-exported from barrel but never imported by anyone
    assert!(
        unused_export_names.contains(&"UnusedComponent"),
        "UnusedComponent should be detected as unused re-export on barrel, found: {unused_export_names:?}"
    );

    // UsedComponent IS imported via barrel, so it should NOT be unused
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

    // UnusedType is re-exported as type from barrel but never imported
    assert!(
        unused_type_names.contains(&"UnusedType"),
        "UnusedType should be detected as unused type re-export on barrel, found: {unused_type_names:?}"
    );

    // UsedType IS imported via barrel, so it should NOT be unused
    assert!(
        !unused_type_names.contains(&"UsedType"),
        "UsedType should NOT be detected as unused type"
    );
}

#[test]
fn barrel_re_export_propagates_to_source_module() {
    // When a re-export on a barrel is unused, the source module's export
    // should also be flagged if only consumed through the (unused) barrel re-export.
    // Conversely, if the barrel re-export IS used, the source should NOT be flagged.
    let root = fixture_path("barrel-unused-reexports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // UsedComponent on the source module should NOT be flagged
    // (it's referenced through the barrel which is consumed)
    assert!(
        !results
            .unused_exports
            .iter()
            .any(|e| e.export.export_name == "UsedComponent"),
        "source UsedComponent should not be unused since barrel re-export is consumed"
    );
}

#[test]
fn barrel_exports_detects_unused_re_export_bar() {
    // In the existing barrel-exports fixture, `bar` is re-exported from barrel
    // but nobody imports `bar` from the barrel.
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

    // foo should not be flagged (it IS imported from barrel by index.ts)
    assert!(
        !unused_export_names.contains(&"foo"),
        "foo should NOT be unused since index.ts imports it from barrel"
    );
}

// ── Multi-hop barrel chains ────────────────────────────────────

#[test]
fn multi_hop_barrel_used_propagates() {
    let root = fixture_path("multi-hop-barrel");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // `used` is imported through barrel1 -> barrel2 -> source, so it should NOT be flagged
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

    // unused2 is only exported from source.ts and re-exported from barrel2
    // but NOT re-exported from barrel1, so it should be flagged
    assert!(
        unused_export_names.contains(&"unused2"),
        "unused2 should be detected as unused export, found: {unused_export_names:?}"
    );
}

// ── Star re-export chains ──────────────────────────────────────

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

    // `used` is imported through barrel1 (export *) -> barrel2 (export *) -> source
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

    // `unused` is exported from source.ts but never imported
    assert!(
        unused_export_names.contains(&"unused"),
        "unused should be detected as unused export, found: {unused_export_names:?}"
    );
}

// ── Multi-level barrel chain (3 levels) ──────────────────────

#[test]
fn multi_level_chain_used_exports_propagate() {
    // index.ts -> barrel-a -> barrel-b -> source (3-level named re-export chain)
    let root = fixture_path("multi-level-barrel-chain");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // alpha and beta are imported through 3 levels of barrels, should NOT be flagged
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
    // gamma is re-exported from barrel-b and barrel-a but never imported from barrel-a
    // delta is re-exported from barrel-b only, not from barrel-a
    // epsilon is not re-exported at all
    let root = fixture_path("multi-level-barrel-chain");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // gamma is re-exported through barrel-a but nobody imports it
    assert!(
        unused_export_names.contains(&"gamma"),
        "gamma should be unused (re-exported but never imported), found: {unused_export_names:?}"
    );

    // delta is only re-exported from barrel-b, not from barrel-a
    assert!(
        unused_export_names.contains(&"delta"),
        "delta should be unused (not re-exported from top-level barrel), found: {unused_export_names:?}"
    );

    // epsilon is not re-exported by any barrel
    assert!(
        unused_export_names.contains(&"epsilon"),
        "epsilon should be unused (not re-exported at all), found: {unused_export_names:?}"
    );
}

// ── Star re-export with selective usage ──────────────────────

#[test]
fn star_selective_usage_used_propagates() {
    // export * from './source' but only usedOne and usedTwo are imported
    let root = fixture_path("star-selective-usage");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // usedOne and usedTwo are selectively imported through star re-export barrel
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

    // unusedThree and unusedFour are star re-exported but nobody imports them
    assert!(
        unused_export_names.contains(&"unusedThree"),
        "unusedThree should be unused (star re-exported but not imported), found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"unusedFour"),
        "unusedFour should be unused (star re-exported but not imported), found: {unused_export_names:?}"
    );
}

// ── Mixed named + star re-exports ────────────────────────────

#[test]
fn mixed_named_star_used_propagates() {
    // Barrel has both `export { namedUsed } from` and `export * from`
    let root = fixture_path("mixed-named-star-reexports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // namedUsed via named re-export, starUsed via star re-export — both should propagate
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

    // namedUnused is named-re-exported but nobody imports it
    assert!(
        unused_export_names.contains(&"namedUnused"),
        "namedUnused should be unused (named re-exported but not imported), found: {unused_export_names:?}"
    );

    // starUnused is star-re-exported but nobody imports it
    assert!(
        unused_export_names.contains(&"starUnused"),
        "starUnused should be unused (star re-exported but not imported), found: {unused_export_names:?}"
    );
}

// ── Re-export chain with aliases ─────────────────────────────

#[test]
fn alias_chain_used_exports_propagate() {
    // original -> aliasB -> aliasC (2 alias hops), consumed as aliasC
    // renamed -> renamedOnce -> doubleAlias (2 alias hops), consumed as doubleAlias
    let root = fixture_path("re-export-alias-chain");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // original is aliased as aliasB then aliasC, consumed by index.ts as aliasC
    assert!(
        !unused_export_names.contains(&"original"),
        "original should NOT be flagged (used through alias chain as aliasC), found: {unused_export_names:?}"
    );

    // renamed is aliased as renamedOnce then doubleAlias, consumed by index.ts as doubleAlias
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

    // unusedOriginal -> unusedAliasB -> unusedAliasC: aliased but never consumed
    assert!(
        unused_export_names.contains(&"unusedOriginal"),
        "unusedOriginal should be unused (aliased but never imported), found: {unused_export_names:?}"
    );

    // neverExported is not re-exported by any barrel
    assert!(
        unused_export_names.contains(&"neverExported"),
        "neverExported should be unused (not re-exported at all), found: {unused_export_names:?}"
    );
}

// ── Circular re-export detection ─────────────────────────────

#[test]
fn circular_re_export_completes_without_infinite_loop() {
    // module-a re-exports from module-b, module-b re-exports from module-a
    // Analysis should complete (not infinite loop) thanks to the iteration limit
    let root = fixture_path("circular-re-export");
    let config = create_config(root);
    let results =
        fallow_core::analyze(&config).expect("analysis should succeed with circular re-exports");

    // The key assertion is that analysis completes at all (no hang/infinite loop).
    // Additionally, the directly-defined exports should be correctly resolved.
    // The re-export copies (module-a re-exporting fromB, module-b re-exporting fromA)
    // are correctly flagged as unused since index.ts imports directly from each module.
    // Original definitions should NOT be flagged.
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

    // The re-export copies ARE unused (nobody imports fromB from module-a,
    // nobody imports fromA from module-b)
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
    // All files in the circular re-export fixture should be reachable
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

// ── Default re-export through barrel ────────────────────────

#[test]
fn barrel_default_reexport_unused_detected() {
    // Barrel re-exports default exports as named: `export { default as Card } from './Card'`
    // Only Button is imported from the barrel, so Card should be flagged as unused.
    let root = fixture_path("barrel-default-reexport");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // Card is re-exported from barrel but never imported by anyone
    assert!(
        unused_export_names.contains(&"Card"),
        "Card should be detected as unused re-export on barrel, found: {unused_export_names:?}"
    );

    // Button IS imported via barrel, so it should NOT be unused
    assert!(
        !unused_export_names.contains(&"Button"),
        "Button should NOT be detected as unused (imported by index.ts)"
    );
}

#[test]
fn barrel_default_reexport_no_unused_files() {
    // All files should be reachable (barrel is imported, Card/Button source files are re-exported from it)
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
