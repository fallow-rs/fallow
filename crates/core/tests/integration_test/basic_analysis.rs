use super::common::{create_config, fixture_path};

#[test]
fn basic_project_detects_unused_files() {
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // orphan.ts should be detected as unused
    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be detected as unused file, found: {unused_file_names:?}"
    );
}

#[test]
fn basic_project_detects_unused_exports() {
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_export_names.contains(&"unusedFunction"),
        "unusedFunction should be detected as unused export, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"anotherUnused"),
        "anotherUnused should be detected as unused export, found: {unused_export_names:?}"
    );
    // usedFunction should NOT be in unused
    assert!(
        !unused_export_names.contains(&"usedFunction"),
        "usedFunction should NOT be detected as unused"
    );
}

#[test]
fn basic_project_detects_unused_types() {
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_type_names: Vec<&str> = results
        .unused_types
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        unused_type_names.contains(&"UnusedType"),
        "UnusedType should be detected as unused type, found: {unused_type_names:?}"
    );
    assert!(
        unused_type_names.contains(&"UnusedInterface"),
        "UnusedInterface should be detected as unused type, found: {unused_type_names:?}"
    );
    // UsedType should NOT be in unused
    assert!(
        !unused_type_names.contains(&"UsedType"),
        "UsedType should NOT be detected as unused"
    );
}

#[test]
fn basic_project_detects_unused_dependencies() {
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_dep_names: Vec<&str> = results
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();

    assert!(
        unused_dep_names.contains(&"unused-dep"),
        "unused-dep should be detected as unused dependency, found: {unused_dep_names:?}"
    );
}

#[test]
fn analysis_returns_correct_total_count() {
    let root = fixture_path("basic-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(results.has_issues(), "basic-project should have issues");
    assert!(results.total_issues() > 0, "total_issues should be > 0");
}

#[test]
fn analyze_project_convenience_function() {
    let root = fixture_path("basic-project");
    let results = fallow_core::analyze_project(&root).expect("analysis should succeed");
    assert!(results.has_issues());
}

#[test]
fn cjs_project_detects_orphan() {
    let root = fixture_path("cjs-project");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert!(
        unused_file_names.contains(&"orphan.js".to_string()),
        "orphan.js should be detected as unused, found: {unused_file_names:?}"
    );
}

// ── Namespace imports ─────────────────────────────────────────

#[test]
fn namespace_import_makes_all_exports_used() {
    let root = fixture_path("namespace-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // With import * as utils, only members accessed via utils.member are used.
    // In the fixture, only utils.foo is accessed; bar and baz are unused.
    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"foo"),
        "foo should be used via utils.foo member access"
    );
    assert!(
        unused_export_names.contains(&"bar"),
        "bar should be unused (not accessed via utils.bar)"
    );
    assert!(
        unused_export_names.contains(&"baz"),
        "baz should be unused (not accessed via utils.baz)"
    );
}

#[test]
fn namespace_import_used_through_object_alias_and_star_barrel() {
    let root = fixture_path("issue-269-namespace-object-alias");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"getMetaAssetsTeam"),
        "getMetaAssetsTeam should be used through API.motionNet.adEngine.getMetaAssetsTeam"
    );
    assert!(
        unused_export_names.contains(&"unusedQuery"),
        "unusedQuery should remain unused, found: {unused_export_names:?}"
    );
}

#[test]
fn namespace_import_used_through_object_alias_across_workspace_packages() {
    // Issue #303: `import * as foo from './bar'; export const API = { foo }`
    // in workspace package `@foo/bar`, then `import { API } from '@foo/bar';
    // API.foo.bar` in a different package, must credit `bar` on `./bar.ts`
    // without crediting unrelated exports of the same file.
    let root = fixture_path("issue-303-namespace-object-alias-cross-package");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"bar"),
        "bar should be credited through API.foo.bar across the @foo/bar package boundary, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"unusedBar"),
        "unusedBar must still be flagged as unused; the precise fix should not credit every export of the namespace target, found: {unused_export_names:?}"
    );
}

#[test]
fn namespace_import_used_through_object_alias_across_packages_via_star_barrel() {
    // Issue #303 follow-up: when the namespace target is a star barrel
    // (`./foo/index.ts` doing `export * from './bar'`), the cross-package
    // alias propagation must synthesize a stub export on the barrel for the
    // accessed member so Phase 4 chain resolution can carry the reference
    // through to the real defining file. Without that, the barrel has no
    // `bar` export symbol and the credit lands nowhere.
    let root = fixture_path("issue-303-namespace-object-alias-star-barrel");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"bar"),
        "bar should be credited through API.foo.bar even when ./foo is a star barrel, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"unusedBar"),
        "unusedBar must still be flagged as unused via the star barrel; the synthesis path should not credit every export, found: {unused_export_names:?}"
    );
}

#[test]
fn namespace_import_used_through_object_alias_across_multi_hop_barrel_chain() {
    // Issue #310: real-world consumers reach the alias-defining file through
    // multiple named-re-export hops. The #303 fix only matched consumers whose
    // import target was the alias-defining file directly; consumers landing at
    // an intermediate barrel were missed.
    //
    //   consumer.ts: import { API } from '@foo/bar'  →  api/src/index.ts
    //   api/src/index.ts:           export { API } from './methods'
    //   api/src/methods/index.ts:   export { API } from './methods'
    //   api/src/methods/methods.ts: import * as bar from './bar'; export const API = { bar }
    //   api/src/methods/bar/index.ts: export * from './queries'
    //   api/src/methods/bar/queries.ts: export const searchFoo = ...
    //
    // The fix walks re-export edges forward from the alias-defining file to
    // enumerate every (barrel, exported_name) pair the alias is reachable
    // through, then matches consumer imports against the full set.
    let root = fixture_path("issue-310-namespace-object-alias-multi-hop-barrel");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"searchFoo"),
        "searchFoo should be credited through API.bar.searchFoo across two named-re-export hops, found: {unused_export_names:?}"
    );
    // Negative control: the multi-hop walk must not over-credit unrelated
    // exports of the same star-barrel target file.
    assert!(
        unused_export_names.contains(&"unusedQuery"),
        "unusedQuery must still be flagged as unused; the BFS-walked credit path should not credit every export, found: {unused_export_names:?}"
    );
}

#[test]
fn namespace_re_export_via_named_import_credits_target_members() {
    // Issue #324: `export * as MyNamespace from './source-module'` in a
    // barrel, then `import { MyNamespace } from './barrel'; MyNamespace.X(...)`
    // in a consumer. The barrel produces a synthesised ExportSymbol named
    // `MyNamespace` AND a ReExportEdge with imported_name="*", exported_name="MyNamespace".
    // Phase 2 attaches a reference to the barrel's stub, but Phase 4 looks for
    // a source export matching `"*"` (never matches), so the source file's
    // real exports stay unreferenced. Phase 2c (namespace_re_exports) closes
    // the gap by walking consumer member accesses and crediting them on the
    // namespace target directly.
    //
    // The fixture exercises three orthogonal shapes:
    //   - Variant A (bug-report): direct barrel + named import + `.X` access.
    //   - Variant B (multi-hop):  outer named-re-export barrel between
    //                              consumer and the `export * as` barrel.
    //   - Variant C (whole-object): consumer uses `Object.keys(Whole)` so
    //                                every target export must be credited.
    let root = fixture_path("issue-324-namespace-re-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // Variant A: accessed members credited, stillUnused stays flagged.
    assert!(
        !unused_export_names.contains(&"someExportedSymbol"),
        "someExportedSymbol must be credited via MyNamespace.someExportedSymbol, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"anotherSymbol"),
        "anotherSymbol must be credited via MyNamespace.anotherSymbol, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"stillUnused"),
        "stillUnused must remain flagged (precise narrowing, not blanket credit), found: {unused_export_names:?}"
    );

    // Variant B: deepUsed credited through outer-barrel -> inner-barrel chain.
    assert!(
        !unused_export_names.contains(&"deepUsed"),
        "deepUsed must be credited through the two-hop named-re-export chain, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"deepUnused"),
        "deepUnused must remain flagged across the chain (no over-credit), found: {unused_export_names:?}"
    );

    // Variant C: every export credited under whole-object use.
    assert!(
        !unused_export_names.contains(&"wholeA"),
        "wholeA must be credited under Object.keys(Whole) whole-object use, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"wholeB"),
        "wholeB must be credited under whole-object use, found: {unused_export_names:?}"
    );
    assert!(
        !unused_export_names.contains(&"wholeC"),
        "wholeC must be credited under whole-object use, found: {unused_export_names:?}"
    );
}

#[test]
fn namespace_object_alias_chains_through_namespace_re_export_target() {
    // Issue #328: the namespace target of a Phase 2b namespace-object alias
    // can itself contain `export * as N from './S'` namespace re-exports.
    // Real-world chain (filipw01's repro):
    //
    //   consumer.ts:               import { API } from '@foo/bar'
    //                              API.foo.inner.used          (one re-export hop)
    //                              API.foo.outer.deep.deepUsed (two re-export hops)
    //   api/index.ts:              import * as foo from './nested/barrel'
    //                              export const API = { foo }
    //   api/nested/barrel.ts:      export * as inner from './leaf'
    //                              export * as outer from './deeper-barrel'
    //                              export * as untouched from './sibling'   (negative)
    //   api/nested/leaf.ts:        export const used = ...
    //                              export const stillUnused = ...           (negative)
    //   api/nested/deeper-barrel.ts: export * as deep from './deeper-leaf'
    //   api/nested/deeper-leaf.ts: export const deepUsed = ...
    //                              export const deepUnused = ...            (negative)
    //   api/nested/sibling.ts:     export const siblingLeaf = ...           (negative)
    //
    // Before the fix, Phase 2b credited `inner` / `outer` on barrel.ts but
    // stopped there; `used` and `deepUsed` were never reached because the
    // re-export edge `imported_name="*", exported_name="<name>"` is not
    // followed by Phase 4 chain resolution. The fix walks chained namespace
    // re-exports on the alias-target side: when a Phase 2b credit lands on a
    // name namespace-re-exported elsewhere, the consumer's deeper accesses
    // propagate to the underlying source, recursively for multi-hop chains.
    let root = fixture_path("issue-328-namespace-object-alias-through-ns-re-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    // Positive case 1 (single hop): `used` must be credited through
    // API.foo.inner.used (one namespace re-export between barrel and leaf).
    assert!(
        !unused_export_names.contains(&"used"),
        "used should be credited through the object-alias + single namespace-re-export, found: {unused_export_names:?}"
    );

    // Positive case 2 (multi hop): `deepUsed` must be credited through
    // API.foo.outer.deep.deepUsed (two consecutive namespace re-exports).
    // Confirms the chain walker recurses past the first hop.
    assert!(
        !unused_export_names.contains(&"deepUsed"),
        "deepUsed should be credited through the two-hop namespace-re-export chain, found: {unused_export_names:?}"
    );

    // Negative case 1: same-file unused export on leaf.ts must stay flagged.
    // Catches over-crediting (whole-target instead of per-member) on the
    // single-hop side.
    assert!(
        unused_export_names.contains(&"stillUnused"),
        "stillUnused on leaf.ts must remain flagged; the chain walker must be per-member, found: {unused_export_names:?}"
    );

    // Negative case 2: same-file unused export on deeper-leaf.ts must stay
    // flagged even after two re-export hops. Catches over-crediting on the
    // multi-hop side.
    assert!(
        unused_export_names.contains(&"deepUnused"),
        "deepUnused on deeper-leaf.ts must remain flagged across the two-hop chain, found: {unused_export_names:?}"
    );

    // Negative case 3: sibling.ts's export must stay flagged because the
    // consumer never accesses `API.foo.untouched.*`. Catches the case where
    // the walker credits all namespace re-exports on the target instead of
    // the specific one the consumer touched.
    assert!(
        unused_export_names.contains(&"siblingLeaf"),
        "siblingLeaf on sibling.ts must remain flagged; the walker must key on the touched re-export name, found: {unused_export_names:?}"
    );

    // Files must still be reachable (the chain credits exports, not files,
    // but reachability propagates through the re-export edges already).
    let unused_file_paths: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.file.path.display().to_string())
        .collect();
    for required in ["leaf.ts", "barrel.ts", "deeper-barrel.ts", "deeper-leaf.ts"] {
        assert!(
            !unused_file_paths.iter().any(|p| p.ends_with(required)),
            "{required} must stay reachable, found unused files: {unused_file_paths:?}"
        );
    }
}

// ── Namespace exports (issue #52) ────────────────────────────

#[test]
fn namespace_export_members_not_reported_as_unused() {
    let root = fixture_path("namespace-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // The namespace export `BusinessHelper` is imported and its members
    // accessed via `BusinessHelper.inviteSupplier()` etc. Neither the
    // namespace nor its inner functions should be reported as unused.
    assert!(
        results.unused_exports.is_empty(),
        "No unused exports expected, got: {:?}",
        results
            .unused_exports
            .iter()
            .map(|e| e.export.export_name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(
        results.unused_types.is_empty(),
        "No unused types expected, got: {:?}",
        results
            .unused_types
            .iter()
            .map(|e| e.export.export_name.as_str())
            .collect::<Vec<_>>()
    );
    assert!(results.unused_files.is_empty(), "No unused files expected");
}

// ── Duplicate exports ─────────────────────────────────────────

#[test]
fn duplicate_exports_detected() {
    let root = fixture_path("duplicate-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let dup_names: Vec<&str> = results
        .duplicate_exports
        .iter()
        .map(|d| d.export_name.as_str())
        .collect();

    assert!(
        dup_names.contains(&"shared"),
        "shared should be detected as duplicate export, found: {dup_names:?}"
    );
}

// ── Default export detection ───────────────────────────────────

#[test]
fn default_export_flagged_when_not_imported() {
    let root = fixture_path("default-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // unused-default.ts is never imported, so it should be an unused file
    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    assert!(
        unused_file_names.contains(&"unused-default.ts".to_string()),
        "unused-default.ts should be detected as unused file, found: {unused_file_names:?}"
    );
}

#[test]
fn default_export_flagged_when_only_named_imported() {
    let root = fixture_path("default-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // component.ts is imported for { usedNamed } only, so its default export
    // should be flagged as unused
    let unused_export_entries: Vec<(&str, String)> = results
        .unused_exports
        .iter()
        .map(|e| {
            (
                e.export.export_name.as_str(),
                e.export
                    .path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
            )
        })
        .collect();

    assert!(
        unused_export_entries
            .iter()
            .any(|(name, file)| *name == "default" && file == "component.ts"),
        "default export on component.ts should be flagged as unused, found: {unused_export_entries:?}"
    );

    // usedNamed should NOT be flagged
    assert!(
        !results
            .unused_exports
            .iter()
            .any(|e| e.export.export_name == "usedNamed"),
        "usedNamed should NOT be detected as unused"
    );
}

// ── Side-effect imports ────────────────────────────────────────

#[test]
fn side_effect_import_makes_file_reachable() {
    let root = fixture_path("side-effect-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| {
            f.file
                .path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .to_string()
        })
        .collect();

    // setup.ts is imported via side-effect import, so it should be reachable
    assert!(
        !unused_file_names.contains(&"setup.ts".to_string()),
        "setup.ts should be reachable via side-effect import, unused files: {unused_file_names:?}"
    );

    // orphan.ts is never imported, so it should be unused
    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be detected as unused file, found: {unused_file_names:?}"
    );
}

#[test]
fn circular_import_does_not_crash() {
    // Create temporary fixture with circular imports
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let temp_dir = tmp.path().to_path_buf();
    std::fs::create_dir_all(temp_dir.join("src")).unwrap();

    std::fs::write(
        temp_dir.join("package.json"),
        r#"{"name": "circular", "main": "src/a.ts"}"#,
    )
    .unwrap();

    std::fs::write(
        temp_dir.join("src/a.ts"),
        "import { b } from './b';\nexport const a = b + 1;\n",
    )
    .unwrap();

    std::fs::write(
        temp_dir.join("src/b.ts"),
        "import { a } from './a';\nexport const b = a + 1;\n",
    )
    .unwrap();

    let config = create_config(temp_dir);
    // This should not crash or infinite loop
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        !results.circular_dependencies.is_empty(),
        "should detect circular dependency between a.ts and b.ts"
    );
    assert_eq!(results.circular_dependencies[0].cycle.length, 2);
}

#[test]
fn circular_import_next_line_suppression_hides_cycle() {
    let tmp = tempfile::tempdir().expect("failed to create temp dir");
    let temp_dir = tmp.path().to_path_buf();
    std::fs::create_dir_all(temp_dir.join("src")).unwrap();

    std::fs::write(
        temp_dir.join("package.json"),
        r#"{"name": "circular", "main": "src/a.ts"}"#,
    )
    .unwrap();

    std::fs::write(
        temp_dir.join("src/a.ts"),
        "// fallow-ignore-next-line circular-dependency\nimport { b } from './b';\nexport const a = b + 1;\n",
    )
    .unwrap();

    std::fs::write(
        temp_dir.join("src/b.ts"),
        "// fallow-ignore-next-line circular-dependency\nimport { a } from './a';\nexport const b = a + 1;\n",
    )
    .unwrap();

    let config = create_config(temp_dir);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        results.circular_dependencies.is_empty(),
        "line-level circular-dependency suppression should hide the cycle, got: {:?}",
        results.circular_dependencies
    );
    assert!(
        results.stale_suppressions.is_empty(),
        "consumed circular-dependency suppressions should not be stale, got: {:?}",
        results.stale_suppressions
    );
}
