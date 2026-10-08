use super::common::{create_config, fixture_path};

fn relative(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Package entries point to a build output that is not on disk. Each entry
/// maps to its source file through the package manifest, a named bundler
/// config, or a tsconfig that names no source file. True findings stay.
#[test]
fn build_output_entries_map_to_their_source_files() {
    let root = fixture_path("package-dist-to-src-entries");
    let results = fallow_core::analyze(&create_config(root.clone())).expect("analysis succeeds");

    let unused_files: Vec<String> = results
        .unused_files
        .iter()
        .map(|file| relative(&root, &file.file.path))
        .collect();
    let unused_exports: Vec<String> = results
        .unused_exports
        .iter()
        .map(|finding| &finding.export)
        .chain(results.unused_types.iter().map(|finding| &finding.export))
        .map(|export| format!("{}:{}", relative(&root, &export.path), export.export_name))
        .collect();

    for entry in [
        // `dist/core.mjs` maps to `src/core/index.ts`.
        "packages/client-sdk/src/core/index.ts",
        "packages/client-sdk/src/metadata/index.ts",
        // `dist/types/index.d.ts` maps to `src/types/index.ts`.
        "packages/client-sdk/src/types/index.ts",
        // `vite.config.node.ts` and `vite.config.define.ts` name these.
        "packages/sdk/src/cli/cli.ts",
        "packages/sdk/src/cli/command.ts",
        "packages/sdk/src/sdk/define/index.ts",
        // `rollup.config.sdk-dts.mjs` names this.
        "packages/sdk/src/sdk/billing/index.ts",
        // `tsconfig.build.json` maps the script to a missing `./database/`.
        "packages/server/src/database/scripts/setup-db.ts",
        "packages/server/src/database/scripts/setup-db-utils.ts",
    ] {
        assert!(
            !unused_files.iter().any(|unused| unused == entry),
            "{entry} is reachable from a package entry, unused files: {unused_files:?}"
        );
    }

    for export in [
        "packages/client-sdk/src/core/index.ts:createCoreClient",
        "packages/client-sdk/src/core/index.ts:CoreClientOptions",
        "packages/client-sdk/src/metadata/index.ts:createMetadataClient",
        "packages/client-sdk/src/types/index.ts:ClientRecord",
        "packages/sdk/src/sdk/define/index.ts:defineApplication",
        "packages/sdk/src/sdk/define/index.ts:ApplicationConfig",
        "packages/sdk/src/sdk/billing/index.ts:chargeCredits",
    ] {
        assert!(
            !unused_exports.iter().any(|unused| unused == export),
            "{export} is part of a package entry, unused exports: {unused_exports:?}"
        );
    }

    for orphan in [
        "packages/client-sdk/src/orphan.ts",
        "packages/sdk/src/cli/orphan.ts",
        "packages/server/src/database/scripts/orphan-script.ts",
    ] {
        assert!(
            unused_files.iter().any(|unused| unused == orphan),
            "{orphan} is not an entry and must stay reported, unused files: {unused_files:?}"
        );
    }
    assert!(
        unused_exports
            .iter()
            .any(|unused| unused == "packages/client-sdk/src/core/helper.ts:unusedCoreHelper"),
        "an unused export behind an entry must stay reported, unused exports: {unused_exports:?}"
    );
}
