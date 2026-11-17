use super::*;

/// A pnpm root with `packages/*`, a nested independent project in `docs`
/// and a `bench` directory whose manifest only sets the module type.
fn write_layout_fixture(root: &Path) {
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"root","private":true}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n  - vite\n  - vite/packages/*\n",
    )
    .unwrap();
    write_package(&root.join("packages/core"), r#"{"name":"@demo/core"}"#);
    write_package(&root.join("bench"), r#"{"type":"module"}"#);
    write_package(
        &root.join("docs"),
        r#"{"name":"demo-docs","private":true,"dependencies":{"vue":"^3.5.0"}}"#,
    );
    std::fs::write(root.join("docs/pnpm-workspace.yaml"), "packages:\n  - .\n").unwrap();
    write_package(&root.join("docs/theme"), r#"{"name":"docs-theme"}"#);
}

fn diagnostics_of_kind<'a>(
    diagnostics: &'a [WorkspaceDiagnostic],
    id: &str,
) -> Vec<&'a WorkspaceDiagnostic> {
    diagnostics.iter().filter(|d| d.kind.id() == id).collect()
}

#[test]
fn undeclared_skips_manifest_without_name_or_dependencies() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_layout_fixture(dir.path());

    let declared = discover_workspaces(dir.path());
    let undeclared = find_undeclared_workspaces(dir.path(), &declared);

    assert!(
        undeclared.iter().all(|d| !d.path.ends_with("bench")),
        "a manifest with no name and no dependencies is not a package: {undeclared:?}"
    );
}

#[test]
fn undeclared_still_reports_nameless_manifest_with_dependencies() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_layout_fixture(dir.path());
    write_package(
        &dir.path().join("bench"),
        r#"{"type":"module","devDependencies":{"tinybench":"^2.0.0"}}"#,
    );

    let declared = discover_workspaces(dir.path());
    let undeclared = find_undeclared_workspaces(dir.path(), &declared);

    assert!(
        undeclared.iter().any(|d| d.path.ends_with("bench")),
        "{undeclared:?}"
    );
}

#[test]
fn nested_pnpm_project_is_registered_with_diagnostic() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_layout_fixture(dir.path());

    let (workspaces, diagnostics) =
        discover_workspaces_with_diagnostics(dir.path(), &crate::IgnorePatternSet::empty())
            .expect("valid root");

    assert!(
        workspaces.iter().any(|ws| ws.name == "demo-docs"),
        "{workspaces:?}"
    );
    let nested = diagnostics_of_kind(&diagnostics, "nested-project-root");
    let [diagnostic] = nested.as_slice() else {
        panic!("expected one nested-project-root diagnostic: {diagnostics:?}");
    };
    assert_eq!(diagnostic.path, dir.path().join("docs"));
    assert!(!diagnostic.degrades_analysis);
}

#[test]
fn nested_pnpm_project_children_are_not_undeclared() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_layout_fixture(dir.path());

    let declared = discover_workspaces(dir.path());
    let undeclared = find_undeclared_workspaces(dir.path(), &declared);

    assert!(undeclared.is_empty(), "{undeclared:?}");
}

#[test]
fn nested_pnpm_project_declared_by_config_pattern_has_no_diagnostic() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_layout_fixture(dir.path());

    let (workspaces, diagnostics) = discover_workspaces_with_diagnostics_and_patterns(
        dir.path(),
        &crate::IgnorePatternSet::empty(),
        &["docs".to_owned()],
    )
    .expect("valid root");

    assert_eq!(
        workspaces
            .iter()
            .filter(|ws| ws.name == "demo-docs")
            .count(),
        1,
        "{workspaces:?}"
    );
    assert!(
        diagnostics_of_kind(&diagnostics, "nested-project-root").is_empty(),
        "{diagnostics:?}"
    );
}

#[test]
fn nested_pnpm_project_honors_ignore_patterns() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_layout_fixture(dir.path());

    let (workspaces, diagnostics) =
        discover_workspaces_with_diagnostics(dir.path(), &build_globset(&["docs/**"]))
            .expect("valid root");

    assert!(
        workspaces.iter().all(|ws| ws.name != "demo-docs"),
        "{workspaces:?}"
    );
    assert!(
        diagnostics_of_kind(&diagnostics, "nested-project-root").is_empty(),
        "{diagnostics:?}"
    );
}

#[test]
fn nested_pnpm_project_is_not_independent_when_a_manifest_pattern_declares_it() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_layout_fixture(dir.path());
    std::fs::write(
        dir.path().join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n  - docs\n",
    )
    .unwrap();

    let (_, diagnostics) =
        discover_workspaces_with_diagnostics(dir.path(), &crate::IgnorePatternSet::empty())
            .expect("valid root");

    assert!(
        diagnostics_of_kind(&diagnostics, "nested-project-root").is_empty(),
        "{diagnostics:?}"
    );
    assert!(!is_independent_pnpm_project(
        dir.path(),
        &dir.path().join("docs")
    ));
}

#[test]
fn independent_pnpm_project_detection() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_layout_fixture(dir.path());

    assert!(is_independent_pnpm_project(
        dir.path(),
        &dir.path().join("docs")
    ));
    assert!(!is_independent_pnpm_project(
        dir.path(),
        &dir.path().join("packages/core")
    ));
    assert!(!is_independent_pnpm_project(dir.path(), dir.path()));
}

#[test]
fn missing_pattern_dir_emits_diagnostic_with_declaring_file() {
    let dir = tempfile::tempdir().expect("create temp dir");
    write_layout_fixture(dir.path());

    let (_, diagnostics) =
        discover_workspaces_with_diagnostics(dir.path(), &crate::IgnorePatternSet::empty())
            .expect("valid root");

    let missing: Vec<(&Path, &WorkspaceDiagnosticKind)> =
        diagnostics_of_kind(&diagnostics, "workspace-pattern-dir-missing")
            .into_iter()
            .map(|d| (d.path.as_path(), &d.kind))
            .collect();
    let yaml = dir.path().join("pnpm-workspace.yaml");
    assert_eq!(
        missing,
        [
            (
                yaml.as_path(),
                &WorkspaceDiagnosticKind::WorkspacePatternDirMissing {
                    pattern: "vite".to_owned()
                }
            ),
            (
                yaml.as_path(),
                &WorkspaceDiagnosticKind::WorkspacePatternDirMissing {
                    pattern: "vite/packages/*".to_owned()
                }
            ),
        ]
    );
    assert!(has_missing_workspace_pattern_dirs(dir.path(), &[]));
}

#[test]
fn missing_configured_pattern_dir_is_anchored_at_the_root() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(dir.path().join("package.json"), r#"{"name":"root"}"#).unwrap();

    let (_, diagnostics) = discover_workspaces_with_diagnostics_and_patterns(
        dir.path(),
        &crate::IgnorePatternSet::empty(),
        &["vendor/*".to_owned()],
    )
    .expect("valid root");

    let [diagnostic] = diagnostics.as_slice() else {
        panic!("expected one diagnostic: {diagnostics:?}");
    };
    assert_eq!(diagnostic.path, Path::new("."));
    assert_eq!(diagnostic.kind.id(), "workspace-pattern-dir-missing");
    assert!(has_missing_workspace_pattern_dirs(
        dir.path(),
        &["vendor/*".to_owned()]
    ));
    assert!(!has_missing_workspace_pattern_dirs(dir.path(), &[]));
}

#[test]
fn existing_dir_with_no_match_emits_no_missing_dir_diagnostic() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"root","workspaces":["packages/*","!vite"]}"#,
    )
    .unwrap();
    std::fs::create_dir_all(dir.path().join("packages")).unwrap();

    let (_, diagnostics) =
        discover_workspaces_with_diagnostics(dir.path(), &crate::IgnorePatternSet::empty())
            .expect("valid root");

    assert!(
        diagnostics_of_kind(&diagnostics, "workspace-pattern-dir-missing").is_empty(),
        "{diagnostics:?}"
    );
    assert!(!has_missing_workspace_pattern_dirs(dir.path(), &[]));
}

#[test]
fn recursive_pattern_without_literal_prefix_is_never_missing() {
    let dir = tempfile::tempdir().expect("create temp dir");
    std::fs::write(
        dir.path().join("package.json"),
        r#"{"name":"root","workspaces":["**/pkg","{a,b}/*"]}"#,
    )
    .unwrap();

    assert!(!has_missing_workspace_pattern_dirs(dir.path(), &[]));
}

/// The `packages` list of a nested independent pnpm project declares more
/// workspaces. pnpm reads the entries relative to the nested project, and
/// keeps negation. An entry that leaves the nested project gives no member.
#[test]
fn nested_pnpm_project_members_are_workspaces() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"root"}"#).unwrap();
    std::fs::write(
        root.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n",
    )
    .unwrap();
    write_package(&root.join("packages/core"), r#"{"name":"core"}"#);
    write_package(&root.join("shared"), r#"{"name":"shared"}"#);
    write_package(&root.join("site"), r#"{"name":"site"}"#);
    std::fs::write(
        root.join("site/pnpm-workspace.yaml"),
        "packages:\n  - .\n  - ./apps/*\n  - '!apps/skip'\n  - ../shared\ncatalog:\n  vue: ^3.5.0\n",
    )
    .unwrap();
    write_package(&root.join("site/apps/web"), r#"{"name":"site-web"}"#);
    write_package(&root.join("site/apps/skip"), r#"{"name":"site-skip"}"#);

    let (workspaces, diagnostics) =
        discover_workspaces_with_diagnostics(root, &crate::IgnorePatternSet::empty())
            .expect("valid root");

    let mut names: Vec<&str> = workspaces.iter().map(|ws| ws.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["core", "site", "site-web"]);
    assert_eq!(
        diagnostics_of_kind(&diagnostics, "nested-project-root").len(),
        1,
        "{diagnostics:?}"
    );
}

/// A nested project that the fallow config `workspaces.patterns` adds keeps
/// that registration. Its members are workspaces all the same.
#[test]
fn nested_pnpm_project_members_with_config_pattern() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(root.join("package.json"), r#"{"name":"root"}"#).unwrap();
    std::fs::write(
        root.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n",
    )
    .unwrap();
    write_package(&root.join("site"), r#"{"name":"site"}"#);
    std::fs::write(
        root.join("site/pnpm-workspace.yaml"),
        "packages:\n  - apps/*\n",
    )
    .unwrap();
    write_package(&root.join("site/apps/web"), r#"{"name":"site-web"}"#);

    let mut names: Vec<String> = discover_workspaces_with_patterns(root, &["site".to_owned()])
        .into_iter()
        .map(|ws| ws.name)
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["site", "site-web"]);
}

fn write_root(root: &Path, pnpm_workspace: &str) {
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"root","private":true}"#,
    )
    .unwrap();
    std::fs::write(root.join("pnpm-workspace.yaml"), pnpm_workspace).unwrap();
}

fn relative_workspace_roots(root: &Path, workspaces: &[WorkspaceInfo]) -> Vec<String> {
    let canonical_root = dunce::canonicalize(root).unwrap();
    let mut roots: Vec<String> = workspaces
        .iter()
        .map(|ws| {
            let canonical = dunce::canonicalize(&ws.root).unwrap();
            canonical
                .strip_prefix(&canonical_root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    roots.sort();
    roots
}

/// pnpm skips a folder without a `package.json` when it expands `**`, so a
/// source folder below a workspace is not a missing package. See #4719.
#[test]
fn recursive_glob_reports_no_diagnostic_for_source_folders() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    write_root(root, "packages:\n  - packages/**\n");
    write_package(&root.join("packages/a"), r#"{"name":"a"}"#);
    write_package(&root.join("packages/b"), r#"{"name":"b"}"#);
    std::fs::create_dir_all(root.join("packages/a/src/deep")).unwrap();
    std::fs::create_dir_all(root.join("packages/b/src")).unwrap();

    let (workspaces, diagnostics) =
        discover_workspaces_with_diagnostics(root, &crate::IgnorePatternSet::empty())
            .expect("valid root");

    assert_eq!(
        relative_workspace_roots(root, &workspaces),
        ["packages/a", "packages/b"]
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
}

/// pnpm reads `**` as zero or more folders, so `pkgs/**` also declares
/// `pkgs` itself. See #4721.
#[test]
fn recursive_glob_includes_its_base_folder() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    write_root(root, "packages:\n  - \"pkgs/**\"\n");
    write_package(&root.join("pkgs"), r#"{"name":"pkgs-self"}"#);
    write_package(&root.join("pkgs/sub"), r#"{"name":"sub"}"#);

    let (workspaces, diagnostics) =
        discover_workspaces_with_diagnostics(root, &crate::IgnorePatternSet::empty())
            .expect("valid root");
    let undeclared = find_undeclared_workspaces_with_ignores(
        root,
        &workspaces,
        &crate::IgnorePatternSet::empty(),
    );

    assert_eq!(
        relative_workspace_roots(root, &workspaces),
        ["pkgs", "pkgs/sub"]
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(undeclared.is_empty(), "{undeclared:?}");
}

/// A root-level `**` pattern never makes the project root a workspace.
#[test]
fn root_level_recursive_glob_skips_the_project_root() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    write_root(root, "packages:\n  - \"**\"\n");
    write_package(&root.join("packages/a"), r#"{"name":"a"}"#);

    let (workspaces, _) =
        discover_workspaces_with_diagnostics(root, &crate::IgnorePatternSet::empty())
            .expect("valid root");

    assert_eq!(relative_workspace_roots(root, &workspaces), ["packages/a"]);
}

/// A folder that a `!` pattern excludes is excluded on purpose, so it gives
/// no diagnostic. See #4720.
#[test]
fn negated_pattern_silences_diagnostics_for_excluded_folders() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    write_root(
        root,
        "packages:\n  - apps/*\n  - \"!apps/api\"\n  - \"!apps/proxy\"\n",
    );
    write_package(&root.join("apps/web"), r#"{"name":"web"}"#);
    write_package(&root.join("apps/api"), r#"{"name":"api"}"#);
    std::fs::create_dir_all(root.join("apps/proxy")).unwrap();
    std::fs::write(root.join("apps/proxy/Dockerfile"), "").unwrap();

    let (workspaces, diagnostics) =
        discover_workspaces_with_diagnostics(root, &crate::IgnorePatternSet::empty())
            .expect("valid root");
    let undeclared = find_undeclared_workspaces_with_ignores(
        root,
        &workspaces,
        &crate::IgnorePatternSet::empty(),
    );

    assert_eq!(relative_workspace_roots(root, &workspaces), ["apps/web"]);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(undeclared.is_empty(), "{undeclared:?}");
}

/// Only a negated pattern silences the undeclared scan. A folder that no
/// pattern names is still reported.
#[test]
fn negated_pattern_keeps_undeclared_diagnostic_for_other_folders() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    write_root(root, "packages:\n  - apps/*\n  - \"!apps/api\"\n");
    write_package(&root.join("apps/web"), r#"{"name":"web"}"#);
    write_package(&root.join("tools/cli"), r#"{"name":"cli"}"#);

    let workspaces = discover_workspaces(root);
    let undeclared = find_undeclared_workspaces(root, &workspaces);

    assert_eq!(undeclared.len(), 1, "{undeclared:?}");
    assert!(undeclared[0].path.ends_with("tools/cli"), "{undeclared:?}");
}

/// A leading `./` names the same folders as the pattern without it, also for
/// a recursive `**` pattern in `package.json`.
#[test]
fn recursive_glob_with_leading_dot_slash_finds_packages() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"root","private":true,"workspaces":["./packages/**"]}"#,
    )
    .unwrap();
    write_package(&root.join("packages/a"), r#"{"name":"a"}"#);
    write_package(&root.join("packages/b"), r#"{"name":"b"}"#);

    let (workspaces, diagnostics) =
        discover_workspaces_with_diagnostics(root, &crate::IgnorePatternSet::empty())
            .expect("valid root");
    let undeclared = find_undeclared_workspaces_with_ignores(
        root,
        &workspaces,
        &crate::IgnorePatternSet::empty(),
    );

    assert_eq!(
        relative_workspace_roots(root, &workspaces),
        ["packages/a", "packages/b"]
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(undeclared.is_empty(), "{undeclared:?}");
}

/// The zero-folder match of `./pkgs/**` also accepts a leading `./`.
#[test]
fn recursive_glob_with_leading_dot_slash_includes_its_base_folder() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let root = dir.path();
    write_root(root, "packages:\n  - \"./pkgs/**\"\n");
    write_package(&root.join("pkgs"), r#"{"name":"pkgs-self"}"#);
    write_package(&root.join("pkgs/sub"), r#"{"name":"sub"}"#);

    let (workspaces, diagnostics) =
        discover_workspaces_with_diagnostics(root, &crate::IgnorePatternSet::empty())
            .expect("valid root");
    let undeclared = find_undeclared_workspaces_with_ignores(
        root,
        &workspaces,
        &crate::IgnorePatternSet::empty(),
    );

    assert_eq!(
        relative_workspace_roots(root, &workspaces),
        ["pkgs", "pkgs/sub"]
    );
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    assert!(undeclared.is_empty(), "{undeclared:?}");
}
