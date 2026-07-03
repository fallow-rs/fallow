use super::common::{create_config, fixture_path};

#[test]
fn basic_project_detects_unused_files() {
    let root = fixture_path("basic-project");
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

#[test]
fn namespace_import_makes_all_exports_used() {
    let root = fixture_path("namespace-imports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

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
    assert!(
        unused_export_names.contains(&"unusedQuery"),
        "unusedQuery must still be flagged as unused; the BFS-walked credit path should not credit every export, found: {unused_export_names:?}"
    );
}

#[test]
fn namespace_re_export_via_named_import_credits_target_members() {
    let root = fixture_path("issue-324-namespace-re-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

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

    assert!(
        !unused_export_names.contains(&"deepUsed"),
        "deepUsed must be credited through the two-hop named-re-export chain, found: {unused_export_names:?}"
    );
    assert!(
        unused_export_names.contains(&"deepUnused"),
        "deepUnused must remain flagged across the chain (no over-credit), found: {unused_export_names:?}"
    );

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
    let root = fixture_path("issue-328-namespace-object-alias-through-ns-re-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_export_names: Vec<&str> = results
        .unused_exports
        .iter()
        .map(|e| e.export.export_name.as_str())
        .collect();

    assert!(
        !unused_export_names.contains(&"used"),
        "used should be credited through the object-alias + single namespace-re-export, found: {unused_export_names:?}"
    );

    assert!(
        !unused_export_names.contains(&"deepUsed"),
        "deepUsed should be credited through the two-hop namespace-re-export chain, found: {unused_export_names:?}"
    );

    assert!(
        unused_export_names.contains(&"stillUnused"),
        "stillUnused on leaf.ts must remain flagged; the chain walker must be per-member, found: {unused_export_names:?}"
    );

    assert!(
        unused_export_names.contains(&"deepUnused"),
        "deepUnused on deeper-leaf.ts must remain flagged across the two-hop chain, found: {unused_export_names:?}"
    );

    assert!(
        unused_export_names.contains(&"siblingLeaf"),
        "siblingLeaf on sibling.ts must remain flagged; the walker must key on the touched re-export name, found: {unused_export_names:?}"
    );

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

#[test]
fn namespace_export_members_not_reported_as_unused() {
    let root = fixture_path("namespace-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

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

#[test]
fn duplicate_exports_detected() {
    let root = fixture_path("duplicate-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let dup_names: Vec<&str> = results
        .duplicate_exports
        .iter()
        .map(|d| d.export.export_name.as_str())
        .collect();

    assert!(
        dup_names.contains(&"shared"),
        "shared should be detected as duplicate export, found: {dup_names:?}"
    );
}

#[test]
fn default_export_flagged_when_not_imported() {
    let root = fixture_path("default-export");
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
        unused_file_names.contains(&"unused-default.ts".to_string()),
        "unused-default.ts should be detected as unused file, found: {unused_file_names:?}"
    );
}

#[test]
fn default_export_flagged_when_only_named_imported() {
    let root = fixture_path("default-export");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

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

    assert!(
        !results
            .unused_exports
            .iter()
            .any(|e| e.export.export_name == "usedNamed"),
        "usedNamed should NOT be detected as unused"
    );
}

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

    assert!(
        !unused_file_names.contains(&"setup.ts".to_string()),
        "setup.ts should be reachable via side-effect import, unused files: {unused_file_names:?}"
    );

    assert!(
        unused_file_names.contains(&"orphan.ts".to_string()),
        "orphan.ts should be detected as unused file, found: {unused_file_names:?}"
    );
}

#[test]
fn circular_import_does_not_crash() {
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

    // b.ts has a type-only import above the runtime import to the same target.
    // The per-file anchor below must use the runtime import line, not the first
    // symbol on the grouped graph edge.
    std::fs::write(
        temp_dir.join("src/b.ts"),
        "// b depends on a\nimport type { a as AValue } from './a';\nimport { a } from './a';\nexport const b = a + 1;\n",
    )
    .unwrap();

    let config = create_config(temp_dir);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");
    assert!(
        !results.circular_dependencies.is_empty(),
        "should detect circular dependency between a.ts and b.ts"
    );
    let cycle = &results.circular_dependencies[0].cycle;
    assert_eq!(cycle.length, 2);

    // Per-file anchors: exactly one edge per hop, in lockstep with `files`,
    // regardless of how the LSP later renders them. This invariant is what
    // lets a consumer index `edges[i]` against `files[i]` without desync.
    assert_eq!(
        cycle.edges.len(),
        cycle.files.len(),
        "edges must carry one entry per file in the cycle"
    );
    for (edge, file) in cycle.edges.iter().zip(&cycle.files) {
        assert_eq!(&edge.path, file, "edge[i].path must equal files[i]");
    }

    // Span lookup must resolve the runtime import line per file: a.ts imports
    // on line 1, b.ts has a type-only import on line 2 and runtime import on
    // line 3.
    let edge_line = |needle: &str| {
        cycle
            .edges
            .iter()
            .find(|e| e.path.ends_with(needle))
            .unwrap_or_else(|| panic!("no edge for {needle}"))
            .line
    };
    assert_eq!(edge_line("a.ts"), 1, "a.ts import is on line 1");
    assert_eq!(
        edge_line("b.ts"),
        3,
        "b.ts runtime import is on line 3, below the type-only import"
    );

    // Top-level line/col mirror the first hop for backward compatibility.
    assert_eq!(cycle.line, cycle.edges[0].line);
    assert_eq!(cycle.col, cycle.edges[0].col);
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
