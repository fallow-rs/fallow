//! Issues #3817 and #3818: a workspace package whose `exports` point into
//! `dist/src/` (tsconfig `rootDir: "./"`, `outDir: "./dist"`) keeps the import
//! credit when the build output exists, and a warm graph cache sees the change
//! when the build output appears.

use std::path::{Path, PathBuf};

use fallow_config::{FallowConfig, OutputFormat};

const LIB_TSCONFIG: &str = r#"{ "compilerOptions": { "rootDir": "./", "outDir": "./dist", "declaration": true }, "include": ["src"] }"#;

/// Write the reproduction from the issues. `lib_tsconfig` is the tsconfig of
/// the library package, or `None` for a library without one.
fn write_project(root: &Path, lib_tsconfig: Option<&str>) {
    let lib = root.join("packages/lib");
    let app = root.join("packages/app");
    std::fs::create_dir_all(lib.join("src")).expect("lib source directory");
    std::fs::create_dir_all(app.join("src")).expect("app source directory");
    std::fs::create_dir_all(app.join("node_modules/@r")).expect("app node_modules");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "repro", "private": true }"#,
    )
    .expect("root manifest");
    std::fs::write(
        root.join("pnpm-workspace.yaml"),
        "packages:\n  - packages/*\n",
    )
    .expect("pnpm workspace file");
    std::fs::write(
        lib.join("package.json"),
        r#"{ "name": "@r/lib", "type": "module", "exports": { ".": { "types": "./dist/src/index.d.ts", "import": "./dist/src/index.js" } } }"#,
    )
    .expect("lib manifest");
    if let Some(tsconfig) = lib_tsconfig {
        std::fs::write(lib.join("tsconfig.json"), tsconfig).expect("lib tsconfig");
    }
    std::fs::write(
        lib.join("src/index.ts"),
        "export const used = 1; export const unused = 2;\n",
    )
    .expect("lib source");
    std::fs::write(
        app.join("package.json"),
        r#"{ "name": "@r/app", "type": "module", "main": "src/main.ts", "dependencies": { "@r/lib": "workspace:*" } }"#,
    )
    .expect("app manifest");
    std::fs::write(
        app.join("src/main.ts"),
        "import { used } from \"@r/lib\"; console.log(used);\n",
    )
    .expect("app source");
    link_dir(&lib, &app.join("node_modules/@r/lib"));
}

#[cfg(unix)]
fn link_dir(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("node_modules symlink");
}

#[cfg(windows)]
fn link_dir(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_dir(target, link).expect("node_modules symlink");
}

/// Write what `tsc` emits for `rootDir: "./"`.
fn write_build_output(root: &Path) {
    let dist = root.join("packages/lib/dist/src");
    std::fs::create_dir_all(&dist).expect("dist directory");
    std::fs::write(
        dist.join("index.js"),
        "export const used = 1; export const unused = 2;\n",
    )
    .expect("built js");
    std::fs::write(
        dist.join("index.d.ts"),
        "export declare const used: number; export declare const unused: number;\n",
    )
    .expect("built declarations");
}

fn config(root: &Path, cache_dir: Option<PathBuf>) -> fallow_config::ResolvedConfig {
    let raw = FallowConfig {
        include_entry_exports: true,
        ..Default::default()
    };
    let mut config = raw.resolve(
        root.to_path_buf(),
        OutputFormat::Human,
        4,
        cache_dir.is_none(),
        true,
        None,
    );
    if let Some(cache_dir) = cache_dir {
        config.cache_dir = cache_dir;
    }
    config
}

fn unused_export_names(config: &fallow_config::ResolvedConfig) -> Vec<String> {
    let results = fallow_core::analyze(config).expect("analysis should succeed");
    let mut names: Vec<String> = results
        .unused_exports
        .iter()
        .map(|export| export.export.export_name.clone())
        .collect();
    names.sort();
    names
}

#[test]
fn dist_src_exports_keep_credit_when_build_output_exists() {
    let directory = tempfile::tempdir().expect("temporary project directory");
    let root = directory.path();
    write_project(root, Some(LIB_TSCONFIG));

    assert_eq!(
        unused_export_names(&config(root, None)),
        vec!["unused".to_string()],
        "without build output only `unused` is reported"
    );

    write_build_output(root);
    assert_eq!(
        unused_export_names(&config(root, None)),
        vec!["unused".to_string()],
        "with dist/src output on disk, the tsconfig rootDir maps it back to src/"
    );
}

#[test]
fn warm_cache_sees_build_output_that_appears() {
    let directory = tempfile::tempdir().expect("temporary project directory");
    let root = directory.path().join("project");
    let cache_dir = directory.path().join("cache");
    // Without a tsconfig the build output changes the resolution, so a stale
    // cache entry gives a result that differs from a cold run.
    write_project(&root, None);

    let cached = config(&root, Some(cache_dir.clone()));
    let _ = unused_export_names(&cached);
    assert!(
        cache_dir.join("graph-cache.bin").exists(),
        "the first run must persist the graph cache"
    );

    write_build_output(&root);
    let cold = unused_export_names(&config(&root, None));
    let warm = unused_export_names(&cached);
    assert_eq!(
        warm, cold,
        "a warm cache must give the same result as a cold run after the build output appears"
    );
}
