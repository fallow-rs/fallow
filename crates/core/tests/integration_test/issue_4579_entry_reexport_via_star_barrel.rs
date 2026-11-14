//! Issue #4579: a type (or value) re-exported by name from an entry point
//! through an `export *` barrel must credit the underlying declaration.

use std::path::Path;

use fallow_config::{FallowConfig, OutputFormat};

fn config_with_entry(root: &Path, entry: &str) -> fallow_config::ResolvedConfig {
    FallowConfig {
        entry: vec![entry.to_string()],
        ..Default::default()
    }
    .resolve(root.to_path_buf(), OutputFormat::Human, 4, true, true, None)
}

fn unused_type_pairs(results: &fallow_core::results::AnalysisResults) -> Vec<(String, String)> {
    results
        .unused_types
        .iter()
        .map(|finding| {
            (
                finding.export.path.to_string_lossy().replace('\\', "/"),
                finding.export.export_name.clone(),
            )
        })
        .collect()
}

fn unused_export_pairs(results: &fallow_core::results::AnalysisResults) -> Vec<(String, String)> {
    results
        .unused_exports
        .iter()
        .map(|finding| {
            (
                finding.export.path.to_string_lossy().replace('\\', "/"),
                finding.export.export_name.clone(),
            )
        })
        .collect()
}

fn contains_pair(pairs: &[(String, String)], suffix: &str, export_name: &str) -> bool {
    pairs
        .iter()
        .any(|(path, name)| path.ends_with(suffix) && name == export_name)
}

#[test]
fn entry_type_reexport_through_star_barrel_is_credited_across_spellings() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    std::fs::create_dir_all(root.join("src/kit")).expect("create src/kit");

    std::fs::write(
        root.join("src/index.ts"),
        concat!(
            "export type { ViaStar } from './kit';\n",
            "export { type ViaInlineType } from './kit';\n",
            "export { ViaUntypedReExport } from './kit';\n",
            "export type { ViaRenamed as PublicViaRenamed } from './kit';\n",
            "export type { ViaNamed } from './kit/named';\n",
        ),
    )
    .expect("write src/index.ts");

    std::fs::write(root.join("src/kit/index.ts"), "export * from './types';\n")
        .expect("write src/kit/index.ts");

    std::fs::write(
        root.join("src/kit/types.ts"),
        concat!(
            "export type ViaStar = { a: string };\n",
            "export type ViaInlineType = { b: number };\n",
            "export type ViaUntypedReExport = { c: boolean };\n",
            "export type ViaRenamed = { d: string };\n",
            "export type UnusedSibling = { dead: true };\n",
        ),
    )
    .expect("write src/kit/types.ts");

    std::fs::write(
        root.join("src/kit/named.ts"),
        "export type { ViaNamed } from './named-types';\n",
    )
    .expect("write src/kit/named.ts");

    std::fs::write(
        root.join("src/kit/named-types.ts"),
        "export type ViaNamed = { b: string };\n",
    )
    .expect("write src/kit/named-types.ts");

    let results =
        fallow_core::analyze(&config_with_entry(root, "src/index.ts")).expect("analyze succeeds");
    let unused_types = unused_type_pairs(&results);

    for credited in [
        "ViaStar",
        "ViaInlineType",
        "ViaUntypedReExport",
        "ViaRenamed",
    ] {
        assert!(
            !contains_pair(&unused_types, "src/kit/types.ts", credited),
            "src/kit/types.ts:{credited} must be credited via entry re-export; got {unused_types:?}"
        );
    }
    assert!(
        !contains_pair(&unused_types, "src/kit/named-types.ts", "ViaNamed"),
        "src/kit/named-types.ts:ViaNamed must stay credited; got {unused_types:?}"
    );
    assert!(
        contains_pair(&unused_types, "src/kit/types.ts", "UnusedSibling"),
        "src/kit/types.ts:UnusedSibling is not re-exported by the entry and must stay reported; got {unused_types:?}"
    );
}

#[test]
fn entry_reexport_handles_nested_type_only_and_cyclic_star_barrels_without_false_negatives() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = temp.path();
    std::fs::create_dir_all(root.join("src/nested/deep")).expect("create nested dirs");
    std::fs::create_dir_all(root.join("src/cycle")).expect("create cycle dir");
    std::fs::create_dir_all(root.join("src/dead")).expect("create dead dir");

    std::fs::write(
        root.join("src/index.ts"),
        concat!(
            "import { keepAlive } from './dead/mid';\n",
            "export const booted = keepAlive;\n",
            "export type { DeepType } from './nested';\n",
            "export { Merged } from './nested/type-star';\n",
            "export type { CycledType } from './cycle/a';\n",
        ),
    )
    .expect("write src/index.ts");

    std::fs::write(
        root.join("src/nested/index.ts"),
        "export * from './deep';\n",
    )
    .expect("write nested/index.ts");
    std::fs::write(
        root.join("src/nested/deep/index.ts"),
        "export * from './leaf';\n",
    )
    .expect("write nested/deep/index.ts");
    std::fs::write(
        root.join("src/nested/deep/leaf.ts"),
        "export type DeepType = { depth: 2 };\n",
    )
    .expect("write nested/deep/leaf.ts");

    std::fs::write(
        root.join("src/nested/type-star.ts"),
        "export type * from './merged-leaf';\n",
    )
    .expect("write nested/type-star.ts");
    std::fs::write(
        root.join("src/nested/merged-leaf.ts"),
        "export type Merged = { kind: string };\nexport const Merged = 42;\n",
    )
    .expect("write nested/merged-leaf.ts");

    std::fs::write(root.join("src/cycle/a.ts"), "export * from './b';\n")
        .expect("write cycle/a.ts");
    std::fs::write(
        root.join("src/cycle/b.ts"),
        "export * from './a';\nexport type CycledType = { ok: true };\n",
    )
    .expect("write cycle/b.ts");

    std::fs::write(
        root.join("src/dead/mid.ts"),
        "export const keepAlive = 1;\nexport type { UnconsumedViaStar } from './star';\n",
    )
    .expect("write dead/mid.ts");
    std::fs::write(root.join("src/dead/star.ts"), "export * from './leaf';\n")
        .expect("write dead/star.ts");
    std::fs::write(
        root.join("src/dead/leaf.ts"),
        "export type UnconsumedViaStar = { unused: true };\n",
    )
    .expect("write dead/leaf.ts");

    let results =
        fallow_core::analyze(&config_with_entry(root, "src/index.ts")).expect("analyze succeeds");
    let unused_types = unused_type_pairs(&results);
    let unused_exports = unused_export_pairs(&results);

    assert!(
        !contains_pair(&unused_types, "src/nested/deep/leaf.ts", "DeepType"),
        "2-hop star barrel must credit DeepType; got {unused_types:?}"
    );
    assert!(
        !contains_pair(&unused_types, "src/nested/merged-leaf.ts", "Merged"),
        "export type * must credit the type side of Merged; got {unused_types:?}"
    );
    assert!(
        contains_pair(&unused_exports, "src/nested/merged-leaf.ts", "Merged"),
        "export type * must NOT credit the runtime value side of Merged; got {unused_exports:?}"
    );
    assert!(
        !contains_pair(&unused_types, "src/cycle/b.ts", "CycledType"),
        "cyclic star barrel must terminate and credit CycledType; got {unused_types:?}"
    );
    assert!(
        contains_pair(&unused_types, "src/dead/leaf.ts", "UnconsumedViaStar"),
        "non-entry named re-export with no consumer must still report leaf declaration as unused; got {unused_types:?}"
    );
    assert!(
        contains_pair(&unused_types, "src/dead/mid.ts", "UnconsumedViaStar"),
        "non-entry named re-export stub with no consumer must still report as unused; got {unused_types:?}"
    );
}
