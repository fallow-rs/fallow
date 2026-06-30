use super::common::{create_config, fixture_path};
use fallow_core::discover::{DiscoveredFile, FileId};
use rustc_hash::FxHashSet;

#[test]
fn duplicate_code_detects_exact_clones() {
    let root = fixture_path("duplicate-code");
    let config = create_config(root.clone());
    let files = fallow_core::discover::discover_files(&config);

    let dupes_config = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 20,
        min_lines: 3,
        ..fallow_core::duplicates::DuplicatesConfig::default()
    };

    let report = fallow_core::duplicates::find_duplicates(&root, &files, &dupes_config);

    assert!(
        !report.clone_groups.is_empty(),
        "Should detect clones in duplicate-code fixture"
    );
    assert!(
        report.stats.files_with_clones >= 2,
        "At least 2 files should have clones"
    );
    assert!(
        report.stats.duplication_percentage > 0.0,
        "Duplication percentage should be > 0"
    );
}

#[test]
fn duplicate_code_semantic_mode_detects_type2_clones() {
    let root = fixture_path("duplicate-code");
    let config = create_config(root.clone());
    let files = fallow_core::discover::discover_files(&config);

    let dupes_config = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 20,
        min_lines: 3,
        mode: fallow_core::duplicates::DetectionMode::Semantic,
        ..fallow_core::duplicates::DuplicatesConfig::default()
    };

    let report = fallow_core::duplicates::find_duplicates(&root, &files, &dupes_config);

    let files_with_clones: rustc_hash::FxHashSet<_> = report
        .clone_groups
        .iter()
        .flat_map(|g| g.instances.iter())
        .map(|inst| inst.file.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    assert!(
        files_with_clones.contains("copy2.ts"),
        "Semantic mode should detect copy2.ts with renamed variables, files found: {files_with_clones:?}"
    );
}

#[test]
fn duplicate_code_unique_file_has_no_clones() {
    let root = fixture_path("duplicate-code");
    let config = create_config(root.clone());
    let files = fallow_core::discover::discover_files(&config);

    let dupes_config = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 20,
        min_lines: 3,
        ..fallow_core::duplicates::DuplicatesConfig::default()
    };

    let report = fallow_core::duplicates::find_duplicates(&root, &files, &dupes_config);

    let all_clone_files: Vec<String> = report
        .clone_groups
        .iter()
        .flat_map(|g| g.instances.iter())
        .map(|inst| inst.file.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    assert!(
        !all_clone_files.contains(&"unique.ts".to_string()),
        "unique.ts should not appear in any clone group, found in: {all_clone_files:?}"
    );
}

#[test]
fn duplicate_code_json_output_serializable() {
    let root = fixture_path("duplicate-code");
    let config = create_config(root.clone());
    let files = fallow_core::discover::discover_files(&config);

    let dupes_config = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 20,
        min_lines: 3,
        ..fallow_core::duplicates::DuplicatesConfig::default()
    };

    let report = fallow_core::duplicates::find_duplicates(&root, &files, &dupes_config);

    let json = serde_json::to_string_pretty(&report).expect("report should serialize to JSON");
    let reparsed: serde_json::Value = serde_json::from_str(&json).expect("JSON should be valid");
    assert!(reparsed["clone_groups"].is_array());
    assert!(reparsed["stats"]["total_files"].is_number());
}

#[test]
fn duplicate_code_skip_local_filters_same_directory() {
    let root = fixture_path("duplicate-code");
    let config = create_config(root.clone());
    let files = fallow_core::discover::discover_files(&config);

    let dupes_config = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 20,
        min_lines: 3,
        skip_local: true,
        ..fallow_core::duplicates::DuplicatesConfig::default()
    };

    let report = fallow_core::duplicates::find_duplicates(&root, &files, &dupes_config);

    assert!(
        report.clone_groups.is_empty(),
        "skip_local should filter same-directory clones"
    );
}

#[test]
fn duplicate_code_min_tokens_threshold_filters() {
    let root = fixture_path("duplicate-code");
    let config = create_config(root.clone());
    let files = fallow_core::discover::discover_files(&config);

    let dupes_config = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 10000,
        min_lines: 1,
        ..fallow_core::duplicates::DuplicatesConfig::default()
    };

    let report = fallow_core::duplicates::find_duplicates(&root, &files, &dupes_config);

    assert!(
        report.clone_groups.is_empty(),
        "Very high min_tokens should find no clones"
    );
}

#[test]
fn duplicate_code_find_duplicates_in_project_convenience() {
    let root = fixture_path("duplicate-code");

    let dupes_config = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 20,
        min_lines: 3,
        ..fallow_core::duplicates::DuplicatesConfig::default()
    };

    let report = fallow_core::duplicates::find_duplicates_in_project(&root, &dupes_config);

    assert!(
        !report.clone_groups.is_empty(),
        "Convenience function should detect clones"
    );
}

#[test]
fn ignore_imports_removes_import_only_clones() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    std::fs::create_dir_all(&src).expect("create src");

    let imports = "import { A } from './a';\n\
                    import { B } from './b';\n\
                    import { C } from './c';\n\
                    import { D } from './d';\n\
                    import { E } from './e';\n\
                    import { F } from './f';\n\
                    import { G } from './g';\n\
                    import { H } from './h';\n";

    let file1 = format!("{imports}\nexport function foo() {{ return A + B + C; }}\n");
    let file2 = format!("{imports}\nexport function bar() {{ return D * E * F; }}\n");

    std::fs::write(src.join("file1.ts"), &file1).expect("write file1");
    std::fs::write(src.join("file2.ts"), &file2).expect("write file2");
    std::fs::write(dir.path().join("package.json"), r#"{"name": "test"}"#)
        .expect("write package.json");

    let files = vec![
        DiscoveredFile {
            id: FileId(0),
            path: src.join("file1.ts"),
            size_bytes: file1.len() as u64,
        },
        DiscoveredFile {
            id: FileId(1),
            path: src.join("file2.ts"),
            size_bytes: file2.len() as u64,
        },
    ];

    // ignore_imports now defaults to true, so counting import blocks requires
    // an explicit opt-out (the `--no-ignore-imports` / `ignoreImports: false`
    // path).
    let config_with_imports = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 10,
        min_lines: 3,
        ignore_imports: false,
        ..Default::default()
    };
    let report_with =
        fallow_core::duplicates::find_duplicates(dir.path(), &files, &config_with_imports);
    assert!(
        !report_with.clone_groups.is_empty(),
        "With ignore_imports=false, identical import blocks should be detected as clones"
    );

    let config_ignore = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 10,
        min_lines: 3,
        ignore_imports: true,
        ..Default::default()
    };
    let report_without =
        fallow_core::duplicates::find_duplicates(dir.path(), &files, &config_ignore);
    assert!(
        report_without.clone_groups.is_empty(),
        "With ignore_imports=true, import-only clones should be eliminated, but found {} groups",
        report_without.clone_groups.len()
    );
}

#[test]
fn ignore_imports_removes_module_wiring_clones() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let src = dir.path().join("src");
    for rel in ["a", "b", "c", "d"] {
        std::fs::create_dir_all(src.join(rel)).expect("create source dir");
    }

    let re_exports = "export { alpha } from './alpha';\n\
                      export { beta } from './beta';\n\
                      export * as gamma from './gamma';\n\
                      export * from './delta';\n";
    let requires = "const alpha = require('./alpha');\n\
                    const { beta } = require('./beta');\n\
                    var gamma = require('./gamma');\n\
                    let delta = require('./delta');\n";

    let files = [
        ("src/a/index.ts", re_exports),
        ("src/b/index.ts", re_exports),
        ("src/c/index.js", requires),
        ("src/d/index.js", requires),
    ];
    for (rel, source) in files {
        std::fs::write(dir.path().join(rel), source).expect("write fixture file");
    }
    std::fs::write(dir.path().join("package.json"), r#"{"name": "test"}"#)
        .expect("write package.json");

    let discovered: Vec<_> = files
        .into_iter()
        .enumerate()
        .map(|(idx, (rel, source))| DiscoveredFile {
            id: FileId(idx as u32),
            path: dir.path().join(rel),
            size_bytes: source.len() as u64,
        })
        .collect();

    let config_with_wiring = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 5,
        min_lines: 3,
        ignore_imports: false,
        ..Default::default()
    };
    let report_with =
        fallow_core::duplicates::find_duplicates(dir.path(), &discovered, &config_with_wiring);
    assert!(
        report_with.clone_groups.len() >= 2,
        "With ignore_imports=false, re-export and require wiring blocks should be detected as clones"
    );

    let config_ignore = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 5,
        min_lines: 3,
        ignore_imports: true,
        ..Default::default()
    };
    let report_without =
        fallow_core::duplicates::find_duplicates(dir.path(), &discovered, &config_ignore);
    assert!(
        report_without.clone_groups.is_empty(),
        "With ignore_imports=true, module-wiring clones should be eliminated, but found {} groups",
        report_without.clone_groups.len()
    );
}

fn default_ignore_fixture_files(root: &std::path::Path) -> Vec<DiscoveredFile> {
    ["src/foo.ts", "lib/foo.js", ".next/static/chunks/foo.js"]
        .into_iter()
        .enumerate()
        .map(|(idx, rel)| {
            let path = root.join(rel);
            let size_bytes = std::fs::metadata(&path)
                .expect("fixture file should exist")
                .len();
            DiscoveredFile {
                id: FileId(idx as u32),
                path,
                size_bytes,
            }
        })
        .collect()
}

fn cloned_relative_files(
    root: &std::path::Path,
    report: &fallow_core::duplicates::DuplicationReport,
) -> FxHashSet<String> {
    report
        .clone_groups
        .iter()
        .flat_map(|group| group.instances.iter())
        .map(|instance| {
            instance
                .file
                .strip_prefix(root)
                .expect("clone path should be under fixture root")
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect()
}

#[test]
fn duplicate_default_ignores_skip_framework_cache_but_not_lib() {
    let root = fixture_path("duplicates_default_ignores");
    let files = default_ignore_fixture_files(&root);
    let dupes_config = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 10,
        min_lines: 3,
        cross_language: true,
        ..Default::default()
    };

    let (report, skips) = fallow_core::duplicates::find_duplicates_with_default_ignore_skips(
        &root,
        &files,
        &dupes_config,
    );
    let cloned_files = cloned_relative_files(&root, &report);

    assert!(cloned_files.contains("src/foo.ts"));
    assert!(
        cloned_files.contains("lib/foo.js"),
        "lib is authored-looking and must not be a default ignore"
    );
    assert!(
        !cloned_files.contains(".next/static/chunks/foo.js"),
        ".next should be skipped by built-in duplicates ignores"
    );
    assert_eq!(skips.total, 1);
    assert_eq!(skips.by_pattern[0].pattern, "**/.next/**");
    assert_eq!(skips.by_pattern[0].count, 1);
}

#[test]
fn duplicate_ignore_defaults_false_replaces_defaults_with_user_ignore() {
    let root = fixture_path("duplicates_default_ignores");
    let files = default_ignore_fixture_files(&root);
    let dupes_config = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 10,
        min_lines: 3,
        cross_language: true,
        ignore_defaults: false,
        ignore: vec!["**/lib/**".to_string()],
        ..Default::default()
    };

    let (report, skips) = fallow_core::duplicates::find_duplicates_with_default_ignore_skips(
        &root,
        &files,
        &dupes_config,
    );
    let cloned_files = cloned_relative_files(&root, &report);

    assert!(cloned_files.contains("src/foo.ts"));
    assert!(
        !cloned_files.contains("lib/foo.js"),
        "user ignore should remove lib when defaults are disabled"
    );
    assert!(
        cloned_files.contains(".next/static/chunks/foo.js"),
        ".next should be analyzed when ignoreDefaults is false"
    );
    assert_eq!(skips.total, 0);
}

/// CSS program Phase 4: a near-miss / value-drifted CSS clone (two rule blocks
/// with the same recipe but `0`/`0px` and `#fff`/`#ffffff` drift) now forms a
/// clone group, where the pre-change character-naive tokenizer produced different
/// token streams and reported nothing. Proves the CSS-aware canonicalization
/// reaches the SA-IS engine end to end.
#[test]
fn fuzzy_css_clones_surface_via_value_canonicalization() {
    let root = fixture_path("css-fuzzy-clones");
    let config = create_config(root.clone());
    let files = fallow_core::discover::discover_files(&config);

    let dupes_config = fallow_core::duplicates::DuplicatesConfig {
        min_tokens: 20,
        min_lines: 3,
        ..fallow_core::duplicates::DuplicatesConfig::default()
    };

    let report = fallow_core::duplicates::find_duplicates(&root, &files, &dupes_config);

    let cloned_files: FxHashSet<String> = report
        .clone_groups
        .iter()
        .flat_map(|g| g.instances.iter())
        .map(|inst| inst.file.file_name().unwrap().to_string_lossy().to_string())
        .collect();

    assert!(
        cloned_files.contains("card.css") && cloned_files.contains("panel.css"),
        "value-drifted CSS recipe should form a cross-file clone group: {cloned_files:?}"
    );
}
