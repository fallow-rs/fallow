use super::common::{create_config, fixture_path};

// ── ESLint relative extends chain (issue #198) ──────────────────

#[test]
fn eslint_relative_extends_config_is_not_reported_unused() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::create_dir_all(root.join("config")).expect("config dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "eslint-chain",
            "private": true,
            "devDependencies": {
                "eslint": "8.57.0",
                "@typescript-eslint/parser": "7.0.0",
                "eslint-config-prettier": "9.1.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("tsconfig.json"),
        r#"{
            "compilerOptions": {
                "target": "ES2022",
                "module": "ES2022",
                "moduleResolution": "bundler",
                "strict": true,
                "skipLibCheck": true
            }
        }"#,
    )
    .expect("tsconfig");
    std::fs::write(
        root.join(".eslintrc.json"),
        r#"{ "root": true, "extends": ["./config/eslintrc.base.js"] }"#,
    )
    .expect("eslint root config");
    std::fs::write(
        root.join("config/eslintrc.base.js"),
        r#"module.exports = {
            extends: ["prettier"],
            overrides: [
                { files: ["*.ts"], parser: "@typescript-eslint/parser", rules: {} }
            ]
        };"#,
    )
    .expect("eslint base config");
    std::fs::write(root.join("src/index.ts"), "export const hello = 'world';")
        .expect("source file");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused_files
            .iter()
            .any(|path| path == "config/eslintrc.base.js"),
        "ESLint base config reached through relative extends should be used, got: {unused_files:?}"
    );

    let unused_dev_dependencies: Vec<&str> = results
        .unused_dev_dependencies
        .iter()
        .map(|dep| dep.package_name.as_str())
        .collect();
    assert!(
        !unused_dev_dependencies.contains(&"@typescript-eslint/parser"),
        "override parser should be credited through the ESLint config chain: {unused_dev_dependencies:?}"
    );
    assert!(
        !unused_dev_dependencies.contains(&"eslint-config-prettier"),
        "extends package should be credited through the ESLint config chain: {unused_dev_dependencies:?}"
    );
}

// ── Type-only circular dependency filtering ──────────────────

#[test]
fn type_only_bidirectional_import_not_reported_as_cycle() {
    let root = fixture_path("type-only-cycle");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // user.ts and post.ts have `import type` from each other.
    // This is NOT a runtime cycle and should not be reported.
    assert!(
        results.circular_dependencies.is_empty(),
        "type-only bidirectional imports should not be reported as circular dependencies, got: {:?}",
        results
            .circular_dependencies
            .iter()
            .map(|cd| &cd.files)
            .collect::<Vec<_>>()
    );
}

#[test]
fn type_only_cycle_still_detects_unused_exports() {
    let root = fixture_path("type-only-cycle");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // The value exports (createUser, createPost) are used by index.ts.
    // No files should be reported as unused.
    let unused_file_names: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert!(
        !unused_file_names.contains(&"user.ts".to_string()),
        "user.ts should not be unused, got: {unused_file_names:?}"
    );
    assert!(
        !unused_file_names.contains(&"post.ts".to_string()),
        "post.ts should not be unused, got: {unused_file_names:?}"
    );
}

// ── Duplicate export common-importer filtering ───────────────

#[test]
fn unrelated_route_files_not_flagged_as_duplicate_exports() {
    let root = fixture_path("route-duplicate-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // foo/page.ts and bar/page.ts both export `Area` and `handler`.
    // Each page is imported by its own router (foo/router.ts, bar/router.ts),
    // not by a shared file. No common importer exists for the page files.
    // Neither `Area` nor `handler` should be flagged as duplicates.
    let route_dupes: Vec<&str> = results
        .duplicate_exports
        .iter()
        .filter(|d| d.export_name == "Area" || d.export_name == "handler")
        .map(|d| d.export_name.as_str())
        .collect();
    assert!(
        route_dupes.is_empty(),
        "route files with separate importers should not be flagged as duplicates, got: {route_dupes:?}"
    );
}

#[test]
fn shared_util_duplicates_with_common_importer_still_flagged() {
    let root = fixture_path("route-duplicate-exports");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    // shared/utils.ts and shared/helpers.ts both export `formatDate`.
    // Both are imported by index.ts (shared importer) -- should be flagged.
    let format_date_dupe = results
        .duplicate_exports
        .iter()
        .find(|d| d.export_name == "formatDate");
    assert!(
        format_date_dupe.is_some(),
        "formatDate in shared files with common importer should be flagged, got dupes: {:?}",
        results
            .duplicate_exports
            .iter()
            .map(|d| &d.export_name)
            .collect::<Vec<_>>()
    );
}

// ── Broken tsconfig extends chain (issue #97) ────────────────

#[test]
fn broken_tsconfig_extends_does_not_poison_sibling_resolution() {
    // Solution-style `packages/my-app/tsconfig.json` references
    // `tsconfig.app.json` (valid) and `tsconfig.spec.json` (extends a
    // non-existent `../../tsconfig.json`). Before the fix, the broken
    // sibling's extends chain failed `oxc_resolver::resolve_file` for ALL
    // files in the workspace, including `main.ts` which is only covered by
    // the valid `tsconfig.app.json`. Every relative import was reported as
    // unresolved.
    //
    // The fallback in `resolve_file_with_tsconfig_fallback` retries via
    // `resolver.resolve(dir, specifier)`, bypassing tsconfig discovery.
    let root = fixture_path("tsconfig-broken-extends");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    assert!(
        results.unresolved_imports.is_empty(),
        "broken sibling tsconfig should not poison resolution for files covered \
         by a valid sibling; got unresolved imports: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|u| (u.path.display().to_string(), &u.specifier))
            .collect::<Vec<_>>()
    );
}

#[test]
fn broken_tsconfig_path_alias_is_not_misclassified_as_unlisted_dependency() {
    let root = fixture_path("tsconfig-broken-path-alias").join("app");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unlisted_names: Vec<&str> = results
        .unlisted_dependencies
        .iter()
        .map(|dep| dep.package_name.as_str())
        .collect();
    let unresolved_specifiers: Vec<&str> = results
        .unresolved_imports
        .iter()
        .map(|import| import.specifier.as_str())
        .collect();

    assert!(
        !unlisted_names.contains(&"@gen/foo"),
        "@gen/foo is a declared tsconfig path alias and should not be treated as an unlisted dependency: {unlisted_names:?}"
    );
    assert!(
        unresolved_specifiers.contains(&"@gen/foo"),
        "@gen/foo should remain unresolved when the tsconfig chain is broken: {unresolved_specifiers:?}"
    );
}

#[test]
fn glimmer_typescript_imports_use_tsconfig_path_aliases() {
    let root = fixture_path("glimmer-path-aliases");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_file_paths: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| file.path.to_string_lossy().to_string())
        .collect();

    assert!(
        !unused_file_paths
            .iter()
            .any(|path| path.ends_with("app/services/my-service.ts")),
        ".gts imports should resolve tsconfig path aliases and keep my-service.ts reachable: \
         {unused_file_paths:?}"
    );
    assert!(
        unused_file_paths
            .iter()
            .any(|path| path.ends_with("app/services/unused-service.ts")),
        "the fixture should still report genuinely unused services: {unused_file_paths:?}"
    );
    assert!(
        results.unresolved_imports.is_empty(),
        ".gts tsconfig path alias imports should not be unresolved: {:?}",
        results
            .unresolved_imports
            .iter()
            .map(|import| &import.specifier)
            .collect::<Vec<_>>()
    );
}

// ── Interface-mediated class member usage (issue #132) ──────

#[test]
fn interface_member_usage_does_not_flag_implementer_members() {
    let root = fixture_path("interface-member-usage");
    let config = create_config(root);
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused_members: Vec<String> = results
        .unused_class_members
        .iter()
        .map(|member| format!("{}.{}", member.parent_name, member.member_name))
        .collect();

    assert!(
        !unused_members.contains(&"FixedSizeScrollStrategy.attached".to_string()),
        "attached should be credited through interface-typed access: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"FixedSizeScrollStrategy.attach".to_string()),
        "attach should be credited through interface-typed access: {unused_members:?}"
    );
    assert!(
        !unused_members.contains(&"FixedSizeScrollStrategy.detach".to_string()),
        "detach should be credited through interface-typed access: {unused_members:?}"
    );
    assert!(
        unused_members.contains(&"FixedSizeScrollStrategy.unusedHelper".to_string()),
        "unrelated members should still be reported: {unused_members:?}"
    );
}

// ── Prisma config file (issue #281) ─────────────────────────

#[test]
fn prisma_config_ts_is_recognized_as_entry_point() {
    let dir = tempfile::tempdir().expect("temp dir");
    let root = dir.path();

    std::fs::create_dir_all(root.join("prisma")).expect("prisma dir");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "prisma-config-entry",
            "private": true,
            "devDependencies": {
                "prisma": "6.0.0"
            }
        }"#,
    )
    .expect("package json");
    std::fs::write(
        root.join("prisma/schema.prisma"),
        "generator client { provider = \"prisma-client-js\" }\n",
    )
    .expect("schema.prisma");
    std::fs::write(
        root.join("prisma.config.ts"),
        r#"import { defineConfig } from "prisma/config";

export default defineConfig({
    schema: "prisma/schema.prisma",
});
"#,
    )
    .expect("prisma.config.ts");

    let config = create_config(root.to_path_buf());
    let results = fallow_core::analyze(&config).expect("analysis should succeed");

    let unused: Vec<String> = results
        .unused_files
        .iter()
        .map(|f| f.path.to_string_lossy().replace('\\', "/"))
        .collect();
    assert!(
        !unused.iter().any(|p| p.ends_with("prisma.config.ts")),
        "prisma.config.ts is the Prisma 6.x config-file location and should not be reported \
         as unused. Got: {unused:?}"
    );
}
