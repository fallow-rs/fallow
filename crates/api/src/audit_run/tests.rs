use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use fallow_types::output_dead_code::{UnusedDependencyFinding, UnusedExportFinding};
use fallow_types::results::{DependencyLocation, UnusedDependency, UnusedExport};
use rustc_hash::FxHashSet;

use super::base_files::{is_analysis_input, is_non_behavioral_doc, js_ts_tokens_equivalent};
use super::*;

fn git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .expect("git command failed");
    assert!(
        output.status.success(),
        "git {:?} failed\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A git repository with one commit that holds `README.md` = `seed\n`.
fn seeded_repo(parent: &Path) -> PathBuf {
    let root = parent.join("repo");
    fs::create_dir_all(&root).expect("repo root should be created");
    fs::write(root.join("README.md"), "seed\n").expect("seed file should be written");
    git(&root, &["init", "-b", "main"]);
    git(&root, &["add", "."]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
    root
}

fn snapshot_with_type_aware(
    identity: Option<fallow_types::semantic::SemanticAnalysisIdentity>,
    gap_signature: Vec<String>,
) -> AuditKeySnapshot {
    AuditKeySnapshot {
        type_aware_identity: identity,
        type_aware_gap_signature: gap_signature,
        ..AuditKeySnapshot::default()
    }
}

fn identity_with_hash(
    project_config_hash: &str,
) -> fallow_types::semantic::SemanticAnalysisIdentity {
    fallow_types::semantic::SemanticAnalysisIdentity {
        project_config_hash: project_config_hash.to_string(),
        ..Default::default()
    }
}

fn meta_with_identity(
    identity: fallow_types::semantic::SemanticAnalysisIdentity,
) -> fallow_types::envelope::TypeAwareMeta {
    fallow_types::envelope::TypeAwareMeta {
        identity: Some(identity),
        ..Default::default()
    }
}

#[test]
fn degrade_reason_absent_for_fully_syntactic_comparison() {
    let base = snapshot_with_type_aware(None, Vec::new());
    assert_eq!(
        type_aware_attribution_degrade_reason(Some(&base), None),
        None
    );
    assert_eq!(type_aware_attribution_degrade_reason(None, None), None);
}

/// Regression test for #2102: a side without a semantic identity made no
/// semantic claims, so the identity of the other side cannot conflict with
/// it and the comparison must not degrade.
#[test]
fn degrade_reason_absent_when_only_one_side_has_type_aware_identity() {
    let base = snapshot_with_type_aware(Some(identity_with_hash("hash-a")), Vec::new());
    assert_eq!(
        type_aware_attribution_degrade_reason(Some(&base), None),
        None
    );

    let base = snapshot_with_type_aware(None, Vec::new());
    let head = meta_with_identity(identity_with_hash("hash-a"));
    assert_eq!(
        type_aware_attribution_degrade_reason(Some(&base), Some(&head)),
        None
    );
}

/// Regression test for #2102: the deferred project-config hash is compatible
/// with any concrete hash, so a diff that adds a file must not degrade.
#[test]
fn degrade_reason_absent_for_deferred_vs_concrete_project_config_hash() {
    let deferred = identity_with_hash(fallow_types::semantic::DEFERRED_PROJECT_CONFIG_HASH);
    let concrete = identity_with_hash("sha256:concrete");

    let base = snapshot_with_type_aware(Some(deferred.clone()), Vec::new());
    let head = meta_with_identity(concrete.clone());
    assert_eq!(
        type_aware_attribution_degrade_reason(Some(&base), Some(&head)),
        None
    );

    let base = snapshot_with_type_aware(Some(concrete), Vec::new());
    let head = meta_with_identity(deferred);
    assert_eq!(
        type_aware_attribution_degrade_reason(Some(&base), Some(&head)),
        None
    );
}

#[test]
fn degrade_reason_when_semantic_identities_are_incompatible() {
    let base = snapshot_with_type_aware(Some(identity_with_hash("hash-a")), Vec::new());
    let head = meta_with_identity(identity_with_hash("hash-b"));
    assert_eq!(
        type_aware_attribution_degrade_reason(Some(&base), Some(&head)),
        Some("their semantic analysis identities are incompatible")
    );
}

#[test]
fn degrade_reason_absent_when_identities_and_gap_signatures_match() {
    let base = snapshot_with_type_aware(Some(identity_with_hash("hash-a")), Vec::new());
    let head = meta_with_identity(identity_with_hash("hash-a"));
    assert_eq!(
        type_aware_attribution_degrade_reason(Some(&base), Some(&head)),
        None
    );
}

#[test]
fn degrade_reason_when_gap_signatures_differ() {
    let base = snapshot_with_type_aware(Some(identity_with_hash("hash-a")), Vec::new());
    let mut head = meta_with_identity(identity_with_hash("hash-a"));
    head.queries = vec![fallow_types::semantic::SemanticQuerySummary {
        query_id: 0,
        capability: fallow_types::semantic::SemanticCapability::SymbolUse,
        assertion: "candidate usage refinement".to_string(),
        status: fallow_types::semantic::SemanticCompleteness::Partial,
        reason_code: None,
        total_evidence_count: 0,
        truncated: false,
        omissions: Vec::new(),
        actions: Vec::new(),
    }];
    assert_eq!(
        type_aware_attribution_degrade_reason(Some(&base), Some(&head)),
        Some("their incomplete semantic query reasons or omissions differ")
    );
}

/// Issue #2220: each demotion source has a label that names the deciding diff.
#[test]
fn dupe_demotion_diff_source_labels_cover_every_state() {
    assert_eq!(
        DupeDemotionDiffSource::Shared("--diff-file pr.diff".to_string()).label("main"),
        "--diff-file pr.diff",
    );
    assert_eq!(
        DupeDemotionDiffSource::Worktree.label("origin/main"),
        "merge-base worktree diff vs origin/main",
    );
    assert_eq!(
        DupeDemotionDiffSource::Skipped.label("main"),
        "skipped: no diff available",
    );
}

#[cfg(unix)]
#[test]
fn remap_focus_files_does_not_canonicalize_through_symlinks() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let real = tmp.path().join("real");
    let link = tmp.path().join("link");
    fs::create_dir_all(&real).expect("real dir");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    let canonical = link.canonicalize().expect("canonicalize symlink");
    assert_ne!(link, canonical, "symlink should not equal its target");

    let from_root = PathBuf::from("/repo");
    let mut focus = FxHashSet::default();
    focus.insert(from_root.join("src/foo.ts"));

    let remapped = remap_focus_files(&focus, &from_root, &link)
        .expect("remap should succeed for in-prefix files");

    let expected = link.join("src/foo.ts");
    assert!(
        remapped.contains(&expected),
        "remapped paths must keep the un-canonical to_root prefix; got {remapped:?}, expected entry {expected:?}"
    );
}

#[test]
fn remap_focus_files_skips_paths_outside_from_root() {
    let from_root = PathBuf::from("/repo/apps/web");
    let to_root = PathBuf::from("/wt/apps/web");
    let mut focus = FxHashSet::default();
    focus.insert(PathBuf::from("/repo/apps/web/src/in.ts"));
    focus.insert(PathBuf::from("/repo/services/api/src/out.ts"));

    let remapped =
        remap_focus_files(&focus, &from_root, &to_root).expect("partial map should succeed");

    assert_eq!(remapped.len(), 1);
    assert!(remapped.contains(&PathBuf::from("/wt/apps/web/src/in.ts")));
}

#[test]
fn remap_focus_files_returns_none_when_no_paths_map() {
    let from_root = PathBuf::from("/repo/apps/web");
    let to_root = PathBuf::from("/wt/apps/web");
    let mut focus = FxHashSet::default();
    focus.insert(PathBuf::from("/elsewhere/foo.ts"));

    let remapped = remap_focus_files(&focus, &from_root, &to_root);
    assert!(
        remapped.is_none(),
        "remap should return None when no paths can be mapped, falling caller back to full corpus"
    );
}

/// The changed-file set comes from `git rev-parse --show-toplevel`, whose
/// spelling can differ from the canonical root of the caller. Those paths must
/// still map: an unmappable set leaves the base snapshot unfiltered, and a
/// filter with the resulting empty set removed every base finding, so each
/// inherited head finding looked introduced under the new-only gate.
#[cfg(unix)]
#[test]
fn remap_focus_files_maps_paths_spelled_against_an_uncanonical_root() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let real = tmp.path().join("real");
    let link = tmp.path().join("link");
    fs::create_dir_all(real.join("src")).expect("real src dir");
    fs::write(real.join("src/foo.ts"), "export const foo = 1;\n").expect("source file");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");

    let mut focus = FxHashSet::default();
    focus.insert(real.join("src/foo.ts"));

    let to_root = PathBuf::from("/wt");
    let remapped = remap_focus_files(&focus, &link, &to_root)
        .expect("paths spelled against an uncanonical root must still map");

    assert!(
        remapped.contains(&to_root.join("src/foo.ts")),
        "expected the focus path to map through the canonicalized root; got {remapped:?}"
    );
}

#[test]
fn base_focus_files_add_the_pre_rename_paths() {
    let changed: FxHashSet<PathBuf> = std::iter::once(PathBuf::from("/repo/src/new.ts")).collect();
    let renames = vec![fallow_engine::changed_files::RenamedFile {
        from: PathBuf::from("/repo/src/old.ts"),
        to: PathBuf::from("/repo/src/new.ts"),
    }];

    let focus = base_focus_files(&changed, &renames);

    let expected: FxHashSet<PathBuf> = [
        PathBuf::from("/repo/src/new.ts"),
        PathBuf::from("/repo/src/old.ts"),
    ]
    .into_iter()
    .collect();
    assert_eq!(focus, expected);
}

fn unused_dependency(package: &str, manifest: &Path) -> UnusedDependencyFinding {
    UnusedDependencyFinding::with_actions(UnusedDependency {
        package_name: package.to_string(),
        location: DependencyLocation::Dependencies,
        path: manifest.to_path_buf(),
        line: 5,
        used_in_workspaces: Vec::new(),
    })
}

/// Push one finding anchored to `changed` (`kept`) and one anchored to
/// `unchanged` (`dropped`) into each dependency collection.
fn fill_dependency_collections(results: &mut AnalysisResults, changed: &Path, unchanged: &Path) {
    use fallow_types::output_dead_code::{
        DevDependencyInProductionFinding, TestOnlyDependencyFinding, TypeOnlyDependencyFinding,
        UnusedCatalogEntryFinding, UnusedDevDependencyFinding, UnusedOptionalDependencyFinding,
    };
    use fallow_types::results::{
        DevDependencyInProduction, TestOnlyDependency, TypeOnlyDependency, UnusedCatalogEntry,
    };

    for (name, manifest) in [("kept", changed), ("dropped", unchanged)] {
        let unused = UnusedDependency {
            package_name: name.to_string(),
            location: DependencyLocation::Dependencies,
            path: manifest.to_path_buf(),
            line: 5,
            used_in_workspaces: Vec::new(),
        };
        results
            .unused_dependencies
            .push(unused_dependency(name, manifest));
        results
            .unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(unused.clone()));
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(unused));
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: name.to_string(),
                    path: manifest.to_path_buf(),
                    line: 5,
                },
            ));
        results
            .test_only_dependencies
            .push(TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: name.to_string(),
                    path: manifest.to_path_buf(),
                    line: 5,
                },
            ));
        results.dev_dependencies_in_production.push(
            DevDependencyInProductionFinding::with_actions(DevDependencyInProduction {
                package_name: name.to_string(),
                path: manifest.to_path_buf(),
                line: 5,
            }),
        );
        results
            .unused_catalog_entries
            .push(UnusedCatalogEntryFinding::with_actions(
                UnusedCatalogEntry {
                    entry_name: name.to_string(),
                    catalog_name: "default".to_string(),
                    path: manifest.to_path_buf(),
                    line: 3,
                    hardcoded_consumers: Vec::new(),
                },
            ));
    }
}

#[test]
fn dependency_findings_stay_only_for_changed_manifests() {
    let root = PathBuf::from("/repo");
    let changed_manifest = root.join("packages/a/package.json");
    let unchanged_manifest = root.join("package.json");
    let mut results = AnalysisResults::default();
    fill_dependency_collections(&mut results, &changed_manifest, &unchanged_manifest);
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/unchanged.ts"),
            export_name: "value".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }));
    let changed: FxHashSet<PathBuf> = std::iter::once(changed_manifest).collect();

    scope_dependency_findings(&mut results, &root, &changed);

    macro_rules! names {
        ($field:ident, $name:expr) => {
            (
                stringify!($field),
                results.$field.iter().map($name).collect::<Vec<&str>>(),
            )
        };
    }
    let collections = [
        names!(unused_dependencies, |f| f.dep.package_name.as_str()),
        names!(unused_dev_dependencies, |f| f.dep.package_name.as_str()),
        names!(unused_optional_dependencies, |f| f
            .dep
            .package_name
            .as_str()),
        names!(type_only_dependencies, |f| f.dep.package_name.as_str()),
        names!(test_only_dependencies, |f| f.dep.package_name.as_str()),
        names!(dev_dependencies_in_production, |f| f
            .dep
            .package_name
            .as_str()),
        names!(unused_catalog_entries, |f| f.entry.entry_name.as_str()),
    ];
    for (collection, names) in collections {
        assert_eq!(names, vec!["kept"], "{collection}");
    }
    assert_eq!(
        results.unused_exports.len(),
        1,
        "the dependency scope leaves source findings to the changed-file filter"
    );
}

/// A catalog finding carries its file relative to the analysis root.
#[test]
fn a_relative_catalog_anchor_resolves_against_the_root() {
    let root = PathBuf::from("/repo");
    let finding = |path: &str| {
        fallow_types::output_dead_code::UnusedCatalogEntryFinding::with_actions(
            fallow_types::results::UnusedCatalogEntry {
                entry_name: "old-react".to_string(),
                catalog_name: "default".to_string(),
                path: PathBuf::from(path),
                line: 3,
                hardcoded_consumers: Vec::new(),
            },
        )
    };
    let mut results = AnalysisResults::default();
    results
        .unused_catalog_entries
        .push(finding("pnpm-workspace.yaml"));
    let changed: FxHashSet<PathBuf> = std::iter::once(root.join("pnpm-workspace.yaml")).collect();

    scope_dependency_findings(&mut results, &root, &changed);
    assert_eq!(results.unused_catalog_entries.len(), 1);

    let changed: FxHashSet<PathBuf> =
        std::iter::once(root.join("packages/a/package.json")).collect();
    scope_dependency_findings(&mut results, &root, &changed);
    assert!(results.unused_catalog_entries.is_empty());
}

#[test]
fn production_flags_decide_the_shared_parse() {
    use fallow_engine::project_config::ProductionFlags;

    let all_production = ProductionFlags::from_cli(true, None, None, None);
    assert!(all_production.modes().all_match());
    assert_eq!(
        all_production.override_for(fallow_config::ProductionAnalysis::Health),
        Some(true)
    );

    let split = ProductionFlags::from_cli(false, Some(true), None, None);
    assert!(!split.modes().dead_code_matches_health());
    assert_eq!(
        split.override_for(fallow_config::ProductionAnalysis::Health),
        None,
        "without a flag, the config decides"
    );
    assert!(
        split.effective(
            fallow_config::ProductionAnalysis::Health,
            fallow_config::ProductionConfig::Global(true)
        ),
        "the config decides a mode that no flag sets"
    );
}

#[test]
fn tokens_equivalent_whitespace_only() {
    let a = "export const x = 1;\nexport const y = 2;\n";
    let b = "export const x = 1;\n\n\nexport const y = 2;\n";
    assert!(
        js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
        "whitespace-only change must be treated as equivalent"
    );
}

#[test]
fn tokens_equivalent_comment_only_change() {
    let a = "export const x = 1;\n";
    let b = "// note\nexport const x = 1;\n";
    assert!(
        js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
        "comment-only change must be treated as equivalent (comments emit no tokens)"
    );
}

#[test]
fn tokens_equivalent_identifier_rename_is_not_equivalent() {
    let a = "export const a = 1;\n";
    let b = "export const b = 1;\n";
    assert!(
        !js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
        "identifier rename must be treated as non-equivalent"
    );
}

#[test]
fn tokens_equivalent_string_literal_change_is_not_equivalent() {
    let a = r#"import x from "./a";"#;
    let b = r#"import x from "./b";"#;
    assert!(
        !js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
        "string-literal change must be treated as non-equivalent"
    );
}

#[test]
fn tokens_equivalent_fallow_ignore_marker_forces_false() {
    // The guard fires before tokenization, so a suppression change is never
    // skipped, even for identical content.
    let code = "// fallow-ignore-next-line unused-exports\nexport const x = 1;\n";
    assert!(
        !js_ts_tokens_equivalent(Path::new("a.ts"), code, code),
        "fallow-ignore marker in either side must force false"
    );
}

#[test]
fn tokens_equivalent_comment_markers_force_false() {
    // Fallow reads these markers from comments, and the tokenizer skips
    // comments. A change to one of them can change the findings.
    for comment in [
        "/** @expected-unused */",
        "/** @public */",
        "/** @api public */",
        "/** @internal */",
        "/** @beta */",
        "/** @alpha */",
        "/** @type {import('./types').Foo} */",
    ] {
        let tagged = format!("{comment}\nexport const x = 1;\n");
        let plain = "/** */\nexport const x = 1;\n";
        assert!(
            !js_ts_tokens_equivalent(Path::new("a.ts"), &tagged, plain),
            "a change to `{comment}` must force false"
        );
        assert!(
            !js_ts_tokens_equivalent(Path::new("a.ts"), plain, &tagged),
            "a change to `{comment}` must force false in either direction"
        );
    }
}

#[test]
fn tokens_equivalent_non_js_extension_is_false() {
    let a = ".foo { color: red; }\n";
    let b = ".foo {\n  color: red;\n}\n";
    assert!(
        !js_ts_tokens_equivalent(Path::new("styles.css"), a, b),
        "non-JS/TS extension must always return false"
    );
}

/// KNOWN SOUNDNESS GAP: `TokenKind::TemplateLiteral` carries no payload, so a
/// change to the content of a template literal is invisible to the tokenizer.
/// The test pins the current behavior for a template literal outside an
/// import.
#[test]
fn tokens_equivalent_template_literal_content_change_is_equivalent_known_gap() {
    let a = "const p = `./pages/${x}`;\n";
    let b = "const p = `./views/${x}`;\n";
    assert!(
        js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
        "template-literal content change is CURRENTLY treated as equivalent (known gap)"
    );
}

/// A dynamic import with a template literal feeds module-resolution pattern
/// edges, so the `import(` marker refuses reuse for it.
#[test]
fn tokens_equivalent_dynamic_import_template_change_is_not_equivalent() {
    let a = "const p = import(`./pages/${x}`);\n";
    let b = "const p = import(`./views/${x}`);\n";
    assert!(
        !js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
        "a dynamic import pattern change must force false"
    );
}

/// Companion to the template-literal gap test: a regex-literal content change
/// is also invisible to the tokenizer.
#[test]
fn tokens_equivalent_regex_literal_content_change_is_equivalent_known_gap() {
    let a = "const re = /^foo/;\n";
    let b = "const re = /^bar/;\n";
    assert!(
        js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
        "regex-literal content change is CURRENTLY treated as equivalent (known gap)"
    );
}

#[test]
fn analysis_input_and_doc_classification() {
    assert!(is_analysis_input(Path::new("src/app.ts")));
    assert!(is_analysis_input(Path::new("src/app.tsx")));
    assert!(is_analysis_input(Path::new("src/app.js")));
    assert!(is_analysis_input(Path::new("src/app.jsx")));
    assert!(is_analysis_input(Path::new("src/app.mts")));
    assert!(is_analysis_input(Path::new("src/app.vue")));
    assert!(is_analysis_input(Path::new("src/styles.css")));

    assert!(!is_analysis_input(Path::new("README.md")));
    assert!(!is_analysis_input(Path::new("package.json")));
    assert!(!is_analysis_input(Path::new("image.png")));

    assert!(is_non_behavioral_doc(Path::new("README.md")));
    assert!(is_non_behavioral_doc(Path::new("CHANGELOG.txt")));
    assert!(is_non_behavioral_doc(Path::new("docs/guide.rst")));
    assert!(is_non_behavioral_doc(Path::new("docs/guide.adoc")));

    // `.json` is neither an analysis input nor a documentation file, so the
    // reuse check treats it as behavioral.
    assert!(!is_non_behavioral_doc(Path::new("package.json")));
}

/// [`BaseFileReader`] keeps a missing object (the file is new since base)
/// apart from a request that cannot be written.
#[test]
fn base_file_reader_distinguishes_missing_from_error() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = seeded_repo(tmp.path());
    let mut reader = BaseFileReader::spawn(&repo).expect("reader should spawn");

    assert!(
        matches!(
            reader.read("HEAD", Path::new("README.md")),
            BaseRead::Content(content) if content == "seed\n"
        ),
        "a committed file reads back as content"
    );
    assert_eq!(
        reader.read("HEAD", Path::new("absent.ts")),
        BaseRead::Missing,
        "an object absent at base is Missing, not an error"
    );
    assert_eq!(
        reader.read("HEAD", Path::new("a\nb.ts")),
        BaseRead::Error,
        "a newline path cannot be requested over the batch protocol"
    );
}

#[test]
fn a_docs_only_change_reuses_the_head_run_as_the_base() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = seeded_repo(tmp.path());
    fs::write(repo.join("README.md"), "changed\n").expect("edit docs");
    let changed: FxHashSet<PathBuf> = std::iter::once(repo.join("README.md")).collect();

    assert!(can_reuse_current_as_base(&repo, None, "HEAD", &changed));

    fs::write(repo.join("package.json"), "{}\n").expect("add manifest");
    let changed: FxHashSet<PathBuf> = [repo.join("README.md"), repo.join("package.json")]
        .into_iter()
        .collect();
    assert!(
        !can_reuse_current_as_base(&repo, None, "HEAD", &changed),
        "a manifest change can change findings"
    );
}

fn branching_totals(
    branch_points: u32,
    functions: u32,
    peak: u16,
) -> fallow_types::extract::FileBranching {
    fallow_types::extract::FileBranching {
        branch_points,
        functions,
        peak_cyclomatic: peak,
        cognitive: branch_points,
        cognitive_nesting_weight: 0,
        has_module_unit: false,
        has_synthetic_units: false,
    }
}

fn snapshot_with_branching(
    entries: &[(&str, fallow_types::extract::FileBranching)],
) -> AuditKeySnapshot {
    AuditKeySnapshot {
        branching: entries
            .iter()
            .map(|(path, totals)| ((*path).to_string(), *totals))
            .collect(),
        ..AuditKeySnapshot::default()
    }
}

#[test]
fn renaming_a_file_moves_its_branching_totals_onto_the_head_path() {
    // Without the remap, the base entry keeps the old path, the head entry
    // looks like a new file, and a pure rename reads as new branching.
    let root = Path::new("/repo");
    let mut snapshot = snapshot_with_branching(&[("src/old.ts", branching_totals(11, 4, 6))]);
    let renames = vec![fallow_engine::changed_files::RenamedFile {
        from: root.join("src/old.ts"),
        to: root.join("src/new.ts"),
    }];

    snapshot.remap_for_renames(&renames, root);

    assert!(!snapshot.branching.contains_key("src/old.ts"));
    assert_eq!(
        snapshot.branching.get("src/new.ts").copied(),
        Some(branching_totals(11, 4, 6))
    );
}

#[test]
fn a_file_untouched_by_a_rename_keeps_its_branching_key() {
    let root = Path::new("/repo");
    let mut snapshot = snapshot_with_branching(&[
        ("src/old.ts", branching_totals(11, 4, 6)),
        ("src/other.ts", branching_totals(3, 2, 4)),
    ]);
    let renames = vec![fallow_engine::changed_files::RenamedFile {
        from: root.join("src/old.ts"),
        to: root.join("src/new.ts"),
    }];

    snapshot.remap_for_renames(&renames, root);

    assert_eq!(
        snapshot.branching.get("src/other.ts").copied(),
        Some(branching_totals(3, 2, 4))
    );
    assert_eq!(snapshot.branching.len(), 2);
}

#[test]
fn a_rename_moves_dead_code_keys_onto_the_head_path() {
    let root = Path::new("/repo");
    let mut snapshot = AuditKeySnapshot {
        dead_code: std::iter::once("unused-export:src/util.ts:oldUnused".to_string()).collect(),
        ..AuditKeySnapshot::default()
    };
    let renames = vec![fallow_engine::changed_files::RenamedFile {
        from: root.join("src/util.ts"),
        to: root.join("src/helpers.ts"),
    }];

    snapshot.remap_for_renames(&renames, root);

    assert!(
        snapshot
            .dead_code
            .contains("unused-export:src/helpers.ts:oldUnused"),
        "{:?}",
        snapshot.dead_code
    );
}

/// Analyses with one unused export in `src/util.ts`.
struct StubAnalyses {
    results: AnalysisResults,
    config: ResolvedConfig,
    root: PathBuf,
}

impl StubAnalyses {
    fn new(root: &Path) -> Self {
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/util.ts"),
                export_name: "unusedValue".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
                deprecated: false,
                deprecated_reason: None,
            }));
        let config = fallow_config::FallowConfig::default().resolve(
            root.to_path_buf(),
            fallow_types::output_format::OutputFormat::Json,
            1,
            true,
            true,
            None,
        );
        Self {
            results,
            config,
            root: root.to_path_buf(),
        }
    }
}

impl AuditAnalyses for StubAnalyses {
    fn view(&self) -> AuditAnalysesView<'_> {
        AuditAnalysesView {
            dead_code: Some(DeadCodeView {
                results: &self.results,
                config: &self.config,
                root: &self.root,
                type_aware: None,
                syntactic_keys: None,
                public_api: None,
            }),
            ..AuditAnalysesView::default()
        }
    }

    fn dead_code_results_mut(&mut self) -> Option<&mut AnalysisResults> {
        Some(&mut self.results)
    }

    fn health_report_mut(&mut self) -> Option<&mut HealthReport> {
        None
    }

    fn record_type_aware_warning(&mut self, _warning: &str) {}
}

struct StubCheckout(PathBuf);

impl BaseCheckout for StubCheckout {
    fn path(&self) -> &Path {
        &self.0
    }
}

/// A backend that counts every base checkout and base run.
struct CountingBackend {
    root: PathBuf,
    base_calls: std::sync::atomic::AtomicUsize,
}

impl AuditBackend for CountingBackend {
    type Analyses = StubAnalyses;
    type Checkout = StubCheckout;
    type CacheKey = ();
    type Error = ();

    fn run_head(&self, _changed_files: &FxHashSet<PathBuf>) -> Result<StubAnalyses, ()> {
        Ok(StubAnalyses::new(&self.root))
    }

    fn create_base_checkout(
        &self,
        _base_ref: &str,
        _base_sha: Option<&str>,
    ) -> Result<StubCheckout, ()> {
        self.base_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(StubCheckout(self.root.clone()))
    }

    fn run_base(
        &self,
        _base_root: &Path,
        _focus: Option<&FxHashSet<PathBuf>>,
    ) -> Result<StubAnalyses, ()> {
        self.base_calls
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut base = StubAnalyses::new(&self.root);
        base.results.unused_exports.clear();
        Ok(base)
    }
}

/// A whitespace-only edit lets the head run stand in for the base: the run
/// creates no base checkout, runs no base analyses, and keeps the head keys
/// as the base snapshot, so the unused export is inherited.
#[test]
fn a_reused_head_run_keeps_the_head_keys_as_the_base_snapshot() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    // The git toplevel is canonical, so the changed paths must be canonical
    // too (the macOS temporary directory is a symbolic link).
    let repo = dunce::canonicalize(seeded_repo(tmp.path())).expect("canonical repo root");
    let util = "export const unusedValue = 1;\n";
    fs::create_dir_all(repo.join("src")).expect("src dir should be created");
    fs::write(repo.join("src/util.ts"), util).expect("write util");
    git(&repo, &["add", "."]);
    git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "util"],
    );
    fs::write(repo.join("src/util.ts"), format!("{util}\n\n")).expect("edit util");
    let changed: FxHashSet<PathBuf> = std::iter::once(repo.join("src/util.ts")).collect();
    assert!(
        can_reuse_current_as_base(&repo, None, "HEAD", &changed),
        "the whitespace-only edit must reach the reuse path"
    );
    let backend = CountingBackend {
        root: repo.clone(),
        base_calls: std::sync::atomic::AtomicUsize::new(0),
    };

    let run = run(
        &backend,
        AuditRunInput {
            root: &repo,
            gate: AuditGate::NewOnly,
            base_ref: "HEAD",
            cache_dir: None,
            changed_files: changed,
        },
    )
    .expect("the stub backend does not fail")
    .expect("the run has changed files");

    assert_eq!(
        backend.base_calls.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "the reuse path must not check out or analyze the base"
    );
    assert!(run.outcome.base_snapshot_skipped);
    let head_keys = AuditKeySnapshot::from_view(&run.analyses.view()).dead_code;
    let base_keys = run
        .outcome
        .base_snapshot
        .as_ref()
        .map(|snapshot| snapshot.dead_code.clone());
    assert_eq!(base_keys, Some(head_keys.clone()));
    assert!(
        head_keys.contains("unused-export:src/util.ts:unusedValue"),
        "{head_keys:?}"
    );
    assert_eq!(run.outcome.attribution.dead_code_inherited, 1);
    assert_eq!(run.outcome.attribution.dead_code_introduced, 0);
    let typed = programmatic_base_snapshot(&run.outcome)
        .expect("the typed output keeps the reused base snapshot");
    assert!(
        typed
            .dead_code
            .contains("unused-export:src/util.ts:unusedValue"),
        "{:?}",
        typed.dead_code
    );
}
