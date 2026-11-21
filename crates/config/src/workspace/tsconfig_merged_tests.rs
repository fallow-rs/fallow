use std::path::{Path, PathBuf};

use super::*;
use crate::workspace::TsconfigExtendsShape;
use crate::workspace::{TsconfigOutputMap, TsconfigOutputResolution};

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().expect("parent folder")).expect("create folder");
    std::fs::write(path, content).expect("write file");
}

/// Write `shape` in a new project folder and read its merged `tsconfig.json`.
fn read_shape(shape: TsconfigExtendsShape) -> (tempfile::TempDir, PathBuf, MergedTsconfig) {
    let directory = tempfile::tempdir().expect("temporary folder");
    let project = directory.path().to_path_buf();
    shape.write(&project);
    let merged = MergedTsconfig::read(&project.join("tsconfig.json"))
        .unwrap_or_else(|| panic!("{}: the leaf parses", shape.name()));
    (directory, project, merged)
}

#[test]
fn every_shape_gives_the_jsx_setting_of_the_right_parent() {
    for shape in TsconfigExtendsShape::ALL {
        let (_directory, _project, merged) = read_shape(shape);
        assert_eq!(
            merged.compiler_option_str("jsx"),
            Some("react-jsx"),
            "{}",
            shape.name()
        );
    }
}

#[test]
fn every_shape_gives_paths_and_base_url_against_the_project() {
    for shape in TsconfigExtendsShape::ALL {
        let (_directory, project, merged) = read_shape(shape);
        assert_eq!(
            merged.path_option("baseUrl"),
            Some(project.clone()),
            "{}",
            shape.name()
        );
        let paths = merged
            .paths()
            .unwrap_or_else(|| panic!("{}: paths", shape.name()));
        assert_eq!(paths.base_dir, project, "{}", shape.name());
        assert_eq!(
            paths.patterns.get("@app/*"),
            Some(&serde_json::json!(["src/*"])),
            "{}",
            shape.name()
        );
    }
}

#[test]
fn every_shape_gives_type_roots_and_file_sets_against_their_config() {
    for shape in TsconfigExtendsShape::ALL {
        let (_directory, project, merged) = read_shape(shape);
        assert_eq!(
            merged.path_list_option("typeRoots"),
            Some(vec![project.join("types")]),
            "{}",
            shape.name()
        );
        let resolved: Vec<PathBuf> = merged
            .file_set_patterns(TsconfigFileSet::Include)
            .unwrap_or_else(|| panic!("{}: include", shape.name()))
            .into_iter()
            .filter_map(|pattern| normalize_path(&pattern.base.join(pattern.pattern)))
            .collect();
        assert_eq!(resolved, vec![project.join("src")], "{}", shape.name());
        assert!(merged.file_set(TsconfigFileSet::Files).is_none());
    }
}

#[test]
fn every_shape_gives_root_dir_and_out_dir_against_the_project() {
    for shape in TsconfigExtendsShape::ALL {
        let (_directory, project, merged) = read_shape(shape);
        assert_eq!(
            merged.path_option("rootDir"),
            Some(project.join("src")),
            "{}",
            shape.name()
        );
        assert_eq!(
            merged.path_option("outDir"),
            Some(project.join("dist")),
            "{}",
            shape.name()
        );
        assert_eq!(
            crate::workspace::parse_tsconfig_root_dir(&project).as_deref(),
            Some("src"),
            "{}",
            shape.name()
        );
    }
}

#[test]
fn every_shape_reports_its_chain_problem() {
    for shape in TsconfigExtendsShape::ALL {
        let (_directory, _project, merged) = read_shape(shape);
        assert_eq!(
            merged.is_complete(),
            shape.is_complete(),
            "{}",
            shape.name()
        );
        let issue = merged.issues().first();
        let expected = match shape {
            TsconfigExtendsShape::Cycle => {
                matches!(issue, Some(TsconfigExtendsIssue::Cycle(_)))
            }
            TsconfigExtendsShape::MissingParent => matches!(
                issue,
                Some(TsconfigExtendsIssue::Missing { specifier, .. })
                    if specifier == "./configs/missing.json"
            ),
            TsconfigExtendsShape::MalformedParent => merged.has_malformed_parent(),
            _ => issue.is_none(),
        };
        assert!(expected, "{}: {:?}", shape.name(), merged.issues());
    }
}

#[test]
fn every_shape_maps_output_only_through_a_complete_chain() {
    for shape in TsconfigExtendsShape::ALL {
        let directory = tempfile::tempdir().expect("temporary folder");
        let project = dunce::canonicalize(directory.path()).expect("canonical folder");
        shape.write(&project);
        write(&project.join("src/index.ts"), "export const value = 1;\n");
        let resolution = TsconfigOutputMap::from_project(&project)
            .resolve_source_for_entry("./dist/index.js", &["ts"]);
        let expected = if shape.is_complete() {
            TsconfigOutputResolution::Resolved(project.join("src/index.ts"))
        } else {
            TsconfigOutputResolution::Unconfigured
        };
        assert_eq!(resolution, expected, "{}", shape.name());
    }
}

#[test]
fn child_base_url_applies_to_inherited_paths() {
    let directory = tempfile::tempdir().expect("temporary folder");
    let project = directory.path();
    write(
        &project.join("configs/base.json"),
        r#"{"compilerOptions":{"paths":{"@/*":["*"]}}}"#,
    );
    write(
        &project.join("tsconfig.json"),
        r#"{"extends":"./configs/base.json","compilerOptions":{"baseUrl":"./src"}}"#,
    );
    let merged = MergedTsconfig::read(&project.join("tsconfig.json")).expect("leaf parses");
    assert_eq!(merged.paths().expect("paths").base_dir, project.join("src"));
}

#[test]
fn paths_without_base_url_resolve_against_the_config_that_sets_them() {
    let directory = tempfile::tempdir().expect("temporary folder");
    let project = directory.path();
    write(
        &project.join("configs/base.json"),
        r#"{"compilerOptions":{"paths":{"@/*":["../src/*"]}}}"#,
    );
    write(
        &project.join("tsconfig.json"),
        r#"{"extends":"./configs/base.json"}"#,
    );
    let merged = MergedTsconfig::read(&project.join("tsconfig.json")).expect("leaf parses");
    assert_eq!(
        merged.paths().expect("paths").base_dir,
        project.join("configs")
    );
}

#[test]
fn a_child_key_replaces_the_whole_inherited_value() {
    let directory = tempfile::tempdir().expect("temporary folder");
    let project = directory.path();
    write(
        &project.join("base.json"),
        r#"{"compilerOptions":{"paths":{"@a/*":["a/*"]},"strict":true},"include":["lib"]}"#,
    );
    write(
        &project.join("tsconfig.json"),
        r#"{"extends":"./base.json","compilerOptions":{"paths":{"@b/*":["b/*"]}}}"#,
    );
    let merged = MergedTsconfig::read(&project.join("tsconfig.json")).expect("leaf parses");
    let paths = merged.paths().expect("paths");
    assert_eq!(paths.patterns.len(), 1);
    assert!(paths.patterns.contains_key("@b/*"));
    assert_eq!(
        merged.compiler_option("strict").map(TsconfigValue::value),
        Some(&serde_json::json!(true))
    );
    assert_eq!(
        merged
            .file_set(TsconfigFileSet::Include)
            .map(TsconfigValue::origin),
        Some(project)
    );
}

#[test]
fn config_dir_token_names_the_folder_of_the_leaf() {
    let directory = tempfile::tempdir().expect("temporary folder");
    let project = directory.path();
    write(
        &project.join("configs/base.json"),
        r#"{"compilerOptions":{"outDir":"${configDir}/dist"}}"#,
    );
    write(
        &project.join("packages/app/tsconfig.json"),
        r#"{"extends":"../../configs/base.json"}"#,
    );
    let merged =
        MergedTsconfig::read(&project.join("packages/app/tsconfig.json")).expect("leaf parses");
    assert_eq!(
        merged.path_option("outDir"),
        Some(project.join("packages/app/dist"))
    );
}

#[test]
fn read_with_source_uses_the_given_leaf_content() {
    let directory = tempfile::tempdir().expect("temporary folder");
    let project = directory.path();
    write(
        &project.join("base.json"),
        r#"{"compilerOptions":{"jsx":"react-jsx"}}"#,
    );
    let merged = MergedTsconfig::read_with_source(
        &project.join("tsconfig.json"),
        r#"{"extends":"./base.json", /* comment */ "compilerOptions":{"strict":true,},}"#,
    )
    .expect("source parses");
    assert_eq!(merged.compiler_option_str("jsx"), Some("react-jsx"));
    assert!(MergedTsconfig::read_with_source(&project.join("tsconfig.json"), "{ nope").is_none());
}

#[test]
fn a_deep_chain_stops_at_the_depth_limit() {
    let directory = tempfile::tempdir().expect("temporary folder");
    let project = directory.path();
    for index in 0..MAX_EXTENDS_DEPTH + 4 {
        write(
            &project.join(format!("c{index}.json")),
            &format!(r#"{{"extends":"./c{}.json"}}"#, index + 1),
        );
    }
    write(
        &project.join(format!("c{}.json", MAX_EXTENDS_DEPTH + 4)),
        r#"{"compilerOptions":{"jsx":"react-jsx"}}"#,
    );
    let merged = MergedTsconfig::read(&project.join("c0.json")).expect("leaf parses");
    assert!(merged.compiler_option("jsx").is_none());
    assert!(
        merged
            .issues()
            .iter()
            .any(|issue| matches!(issue, TsconfigExtendsIssue::Limit(_)))
    );
}

#[test]
fn a_missing_leaf_reads_as_none() {
    let directory = tempfile::tempdir().expect("temporary folder");
    assert!(MergedTsconfig::read(&directory.path().join("tsconfig.json")).is_none());
}

#[test]
fn a_lattice_of_shared_parents_reports_each_issue_once() {
    // Each layer extends both configs of the next layer, so the number of
    // paths to the bottom layer doubles with each layer.
    const LAYERS: usize = 14;
    let directory = tempfile::tempdir().expect("temporary folder");
    let project = directory.path();
    for layer in 0..LAYERS {
        for index in 0..2 {
            let content = if layer + 1 == LAYERS {
                r#"{"extends":"./missing.json"}"#.to_owned()
            } else {
                let next = layer + 1;
                format!(r#"{{"extends":["./c{next}-0.json","./c{next}-1.json"]}}"#)
            };
            write(&project.join(format!("c{layer}-{index}.json")), &content);
        }
    }
    let merged = MergedTsconfig::read(&project.join("c0-0.json")).expect("leaf parses");
    assert_eq!(merged.issues().len(), 2, "{:?}", merged.issues());
}

#[cfg(unix)]
#[test]
fn the_output_map_resolves_a_package_parent_through_its_real_path() {
    // pnpm links `node_modules/@acme/tsconfig` to a store folder. TypeScript
    // resolves the package config to its real path, so a relative `outDir`
    // in it is relative to the store folder.
    let directory = tempfile::tempdir().expect("temporary folder");
    let project = dunce::canonicalize(directory.path()).expect("canonical folder");
    write(
        &project.join("store/acme/package.json"),
        r#"{"name":"@acme/tsconfig","version":"1.0.0"}"#,
    );
    write(
        &project.join("store/acme/tsconfig.json"),
        r#"{"compilerOptions":{"rootDir":"../../src","outDir":"../../dist"}}"#,
    );
    std::fs::create_dir_all(project.join("node_modules/@acme")).expect("scope folder");
    std::os::unix::fs::symlink(
        project.join("store/acme"),
        project.join("node_modules/@acme/tsconfig"),
    )
    .expect("package link");
    write(
        &project.join("tsconfig.json"),
        r#"{"extends":"@acme/tsconfig"}"#,
    );
    write(&project.join("src/index.ts"), "export const value = 1;\n");
    assert_eq!(
        TsconfigOutputMap::from_project(&project)
            .resolve_source_for_entry("./dist/index.js", &["ts"]),
        TsconfigOutputResolution::Resolved(project.join("src/index.ts"))
    );
}

#[test]
fn config_dir_token_counts_only_at_the_start_of_a_value() {
    let directory = tempfile::tempdir().expect("temporary folder");
    let project = directory.path();
    write(
        &project.join("tsconfig.json"),
        r#"{"compilerOptions":{"outDir":"lib/${configDir}"},"include":["${configDir}/src","test"]}"#,
    );
    let merged = MergedTsconfig::read(&project.join("tsconfig.json")).expect("leaf parses");
    assert_eq!(
        merged.path_option("outDir"),
        Some(project.join("lib/${configDir}"))
    );
    assert_eq!(
        merged.file_set_patterns(TsconfigFileSet::Include),
        Some(vec![
            TsconfigPattern {
                base: project.to_path_buf(),
                pattern: "src".to_owned(),
            },
            TsconfigPattern {
                base: project.to_path_buf(),
                pattern: "test".to_owned(),
            },
        ])
    );
}

#[test]
fn read_with_loader_parses_through_the_loader() {
    let directory = tempfile::tempdir().expect("temporary folder");
    let project = directory.path();
    write(
        &project.join("base.json"),
        r#"{"compilerOptions":{"jsx":"preserve"}}"#,
    );
    write(
        &project.join("tsconfig.json"),
        r#"{"extends":"./base.json"}"#,
    );
    let loads = std::cell::RefCell::new(Vec::new());
    let load = |path: &Path| {
        loads.borrow_mut().push(path.to_path_buf());
        let content = if path.ends_with("base.json") {
            r#"{"compilerOptions":{"jsx":"react-jsx"}}"#
        } else {
            r#"{"extends":"./base.json"}"#
        };
        serde_json::from_str(content).ok().map(std::sync::Arc::new)
    };
    let merged = MergedTsconfig::read_with_loader(&project.join("tsconfig.json"), &load)
        .expect("leaf loads");
    assert_eq!(merged.compiler_option_str("jsx"), Some("react-jsx"));
    assert_eq!(loads.borrow().len(), 2);
}

#[test]
fn paths_give_the_base_url_and_then_the_folder_of_their_config() {
    let directory = tempfile::tempdir().expect("temporary folder");
    let project = directory.path();
    write(
        &project.join("base.json"),
        r#"{"compilerOptions":{"baseUrl":"."}}"#,
    );
    write(
        &project.join("app/tsconfig.json"),
        r#"{"extends":"../base.json","compilerOptions":{"paths":{"~/*":["./src/*"]}}}"#,
    );
    let merged = MergedTsconfig::read(&project.join("app/tsconfig.json")).expect("leaf parses");
    let paths = merged.paths().expect("paths");
    assert_eq!(
        paths.base_dirs().collect::<Vec<_>>(),
        vec![project, project.join("app").as_path()]
    );

    write(
        &project.join("app/tsconfig.json"),
        r#"{"compilerOptions":{"paths":{"~/*":["./src/*"]}}}"#,
    );
    let merged = MergedTsconfig::read(&project.join("app/tsconfig.json")).expect("leaf parses");
    let paths = merged.paths().expect("paths");
    assert_eq!(
        paths.base_dirs().collect::<Vec<_>>(),
        vec![project.join("app").as_path()]
    );
}
