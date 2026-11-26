//! Files that a runtime loads from a folder, with no import in the source.

use super::common::{create_config, fixture_path};
use super::framework_convention_coverage_common::{
    collect_unused_exports, collect_unused_files, has_unused_export,
};

/// Kysely's `FileMigrationProvider` imports each file of its migration
/// folder, without recursion (issue #5294).
#[test]
fn kysely_file_migration_provider_loads_its_folder() {
    let root = fixture_path("kysely-migration-folder");
    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    let unused_files = collect_unused_files(&root, &results);
    for used in [
        "src/schema/migrations/1744910873969-InitialMigration.ts",
        "src/schema/migrations/1745244781846-AddColumn.ts",
    ] {
        assert!(
            !unused_files.iter().any(|path| path == used),
            "{used} is a migration, unused files: {unused_files:?}"
        );
    }
    assert!(
        unused_files
            .iter()
            .any(|path| path == "src/schema/migrations/archive/old.ts"),
        "Kysely does not read a sub-folder, unused files: {unused_files:?}"
    );

    let unused_exports = collect_unused_exports(&root, &results);
    assert!(
        !has_unused_export(
            &unused_exports,
            "src/schema/migrations/1744910873969-InitialMigration.ts",
            "down"
        ),
        "Kysely reads the down export, unused exports: {unused_exports:?}"
    );

    let duplicates: Vec<&str> = results
        .duplicate_exports
        .iter()
        .map(|duplicate| duplicate.export.export_name.as_str())
        .collect();
    assert!(
        !duplicates.contains(&"up"),
        "each migration exports up for Kysely, duplicate exports: {duplicates:?}"
    );
}

/// A package script starts React Router with a Vite config whose `root` is a
/// sub-folder, and the package does not declare `@react-router/dev`
/// (issue #5295).
#[test]
fn react_router_app_in_vite_root_of_script_config_is_used() {
    let root = fixture_path("react-router-nested-vite-root");
    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis");

    let unused_files = collect_unused_files(&root, &results);
    for used in [
        "packages/email/preview/react-router.config.ts",
        "packages/email/preview/app/root.tsx",
        "packages/email/preview/app/entry.server.tsx",
        "packages/email/preview/app/routes.ts",
        "packages/email/preview/app/routes/home.tsx",
        "packages/email/preview/app/routes/preview.tsx",
        "packages/email/preview/app/components/playground.tsx",
    ] {
        assert!(
            !unused_files.iter().any(|path| path == used),
            "{used} belongs to the preview app, unused files: {unused_files:?}"
        );
    }
    assert!(
        unused_files
            .iter()
            .any(|path| path == "packages/email/preview/app/components/unused.tsx"),
        "no route imports the component, unused files: {unused_files:?}"
    );

    let unused_exports = collect_unused_exports(&root, &results);
    for (path, export) in [
        ("packages/email/preview/app/root.tsx", "Layout"),
        ("packages/email/preview/app/routes/preview.tsx", "loader"),
    ] {
        assert!(
            !has_unused_export(&unused_exports, path, export),
            "React Router reads {path}:{export}, unused exports: {unused_exports:?}"
        );
    }
}
