use std::path::Path;

use super::common::create_config;

fn write_package(root: &Path, with_outputs: bool) {
    std::fs::create_dir_all(root.join("source")).expect("source directory");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "configured-output-package",
            "main": "./distribution/index.js",
            "types": "./distribution/index.d.ts",
            "exports": {
                ".": {
                    "types": "./distribution/index.d.ts",
                    "import": "./distribution/index.js"
                }
            }
        }"#,
    )
    .expect("package manifest");
    std::fs::write(
        root.join("tsconfig.build.json"),
        r#"{
            "compilerOptions": {
                "rootDir": "./source",
                "outDir": "./distribution"
            },
            "include": ["source"]
        }"#,
    )
    .expect("TypeScript config");
    std::fs::write(
        root.join("source/index.ts"),
        "export const publicValue = 1;\n",
    )
    .expect("source entry");
    std::fs::write(
        root.join("source/internal.ts"),
        "export const internalValue = 1;\n",
    )
    .expect("unrelated source file");

    if with_outputs {
        std::fs::create_dir_all(root.join("distribution")).expect("output directory");
        std::fs::write(
            root.join("distribution/index.js"),
            "export const publicValue = 1;\n",
        )
        .expect("generated JavaScript entry");
        std::fs::write(
            root.join("distribution/index.d.ts"),
            "export declare const publicValue: 1;\n",
        )
        .expect("generated declaration entry");
    }
}

fn unused_paths(root: &Path) -> Vec<String> {
    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");

    results
        .unused_files
        .iter()
        .filter_map(|unused| {
            unused
                .file
                .path
                .strip_prefix(root)
                .ok()
                .map(|path| path.to_string_lossy().into_owned())
        })
        .collect()
}

#[test]
fn generated_package_entries_resolve_to_configured_source_and_keep_internal_files_private() {
    for with_outputs in [true, false] {
        let directory = tempfile::tempdir().expect("temporary project directory");
        let root = directory.path();
        write_package(root, with_outputs);

        let unused = unused_paths(root);
        assert!(
            !unused.iter().any(|path| path == "source/index.ts"),
            "package exports should keep the configured source entry reachable with outputs present={with_outputs}, unused files: {unused:?}"
        );
        assert!(
            unused.iter().any(|path| path == "source/internal.ts"),
            "mapping one public output must not expose unrelated source files, unused files: {unused:?}"
        );
    }
}

fn write_declaration_only_package(root: &Path, with_output: bool) {
    std::fs::create_dir_all(root.join("source")).expect("source directory");
    std::fs::write(
        root.join("package.json"),
        r#"{
            "name": "declaration-only-package",
            "types": "./types/index.d.ts",
            "exports": {".": {"types": "./types/index.d.ts"}}
        }"#,
    )
    .expect("package manifest");
    std::fs::write(
        root.join("tsconfig.build.json"),
        r#"{
            "compilerOptions": {
                "rootDir": "./source",
                "declarationDir": "./types"
            }
        }"#,
    )
    .expect("TypeScript config");
    std::fs::write(
        root.join("source/index.ts"),
        "export const publicValue = 1;\n",
    )
    .expect("source entry");
    std::fs::write(
        root.join("source/internal.ts"),
        "export const internalValue = 1;\n",
    )
    .expect("unrelated source file");

    if with_output {
        std::fs::create_dir_all(root.join("types")).expect("types directory");
        std::fs::write(
            root.join("types/index.d.ts"),
            "export declare const publicValue: 1;\n",
        )
        .expect("generated declaration entry");
    }
}

#[test]
fn declaration_dir_without_out_dir_resolves_public_types_and_keeps_other_sources_private() {
    for with_output in [true, false] {
        let directory = tempfile::tempdir().expect("temporary project directory");
        let root = directory.path();
        write_declaration_only_package(root, with_output);

        let unused = unused_paths(root);
        assert!(
            !unused.iter().any(|path| path == "source/index.ts"),
            "declarationDir should reach its source entry with output present={with_output}, unused files: {unused:?}"
        );
        assert!(
            unused.iter().any(|path| path == "source/internal.ts"),
            "mapping declarations must not expose unrelated source files, unused files: {unused:?}"
        );
    }
}

fn write_ambiguous_output_package(root: &Path, with_output: bool) {
    std::fs::create_dir_all(root.join("src")).expect("legacy source directory");
    std::fs::create_dir_all(root.join("source")).expect("configured source directory");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"ambiguous-output-package","main":"./dist/index.js","module":"./runtime.ts"}"#,
    )
    .expect("package manifest");
    for (name, root_dir) in [
        ("tsconfig.source.json", "source"),
        ("tsconfig.src.json", "src"),
    ] {
        std::fs::write(
            root.join(name),
            format!(r#"{{"compilerOptions":{{"rootDir":"./{root_dir}","outDir":"./dist"}}}}"#),
        )
        .expect("TypeScript config");
    }
    std::fs::write(root.join("source/index.ts"), "export const value = 1;\n")
        .expect("configured source entry");
    std::fs::write(root.join("src/index.ts"), "export const value = 1;\n")
        .expect("legacy source entry");
    std::fs::write(root.join("runtime.ts"), "export const runtime = 1;\n")
        .expect("independent runtime entry");
    if with_output {
        std::fs::create_dir_all(root.join("dist")).expect("output directory");
        std::fs::write(root.join("dist/index.js"), "export const value = 1;\n")
            .expect("generated output entry");
    }
}

#[test]
fn ambiguous_configured_output_does_not_fall_back_to_a_guessed_source_index() {
    for with_output in [true, false] {
        let directory = tempfile::tempdir().expect("temporary project directory");
        let root = directory.path();
        write_ambiguous_output_package(root, with_output);

        let unused = unused_paths(root);
        assert!(
            unused.iter().any(|path| path == "src/index.ts"),
            "the ambiguous mapping must not select the legacy src candidate with output present={with_output}, unused files: {unused:?}"
        );
        assert!(
            unused.iter().any(|path| path == "source/index.ts"),
            "the ambiguous mapping must not expose either configured source, unused files: {unused:?}"
        );
    }

    let legacy = tempfile::tempdir().expect("legacy project directory");
    std::fs::create_dir_all(legacy.path().join("src")).expect("legacy source directory");
    std::fs::write(
        legacy.path().join("package.json"),
        r#"{"name":"legacy-output-package","main":"./dist/index.js"}"#,
    )
    .expect("legacy package manifest");
    std::fs::write(
        legacy.path().join("src/index.ts"),
        "export const value = 1;\n",
    )
    .expect("legacy source entry");
    let legacy_unused = unused_paths(legacy.path());
    assert!(
        !legacy_unused.iter().any(|path| path == "src/index.ts"),
        "the legacy dist/src fallback should remain for packages without a matching config"
    );
}
