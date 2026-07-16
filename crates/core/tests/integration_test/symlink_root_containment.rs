use super::common::create_config;
use std::path::Path;

#[cfg(unix)]
fn symlink_file(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).expect("create file symlink");
}

#[cfg(windows)]
fn symlink_file(target: &Path, link: &Path) {
    std::os::windows::fs::symlink_file(target, link).expect("create file symlink");
}

#[test]
fn symlink_root_containment() {
    let project = tempfile::tempdir().expect("create project");
    let outside = tempfile::tempdir().expect("create outside dir");
    let root = project.path();
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"symlink-containment","private":true,"main":"src/index.ts"}"#,
    )
    .expect("write package");
    std::fs::write(
        src.join("index.ts"),
        "import { inside } from './inside-link';\nexport const value = inside;\n",
    )
    .expect("write entry");
    std::fs::write(src.join("inside-target.ts"), "export const inside = 1;\n")
        .expect("write inside target");
    std::fs::write(
        outside.path().join("outside-target.ts"),
        "export const leaked = 1;\n",
    )
    .expect("write outside target");
    symlink_file(&src.join("inside-target.ts"), &src.join("inside-link.ts"));
    symlink_file(
        &outside.path().join("outside-target.ts"),
        &src.join("outside-link.ts"),
    );

    let results =
        fallow_core::analyze(&create_config(root.to_path_buf())).expect("analysis should succeed");
    assert!(
        results.unresolved_imports.is_empty(),
        "the in-root symlink must remain analyzable: {:?}",
        results.unresolved_imports
    );
    assert!(
        results.unused_files.iter().all(|finding| {
            !finding
                .file
                .path
                .to_string_lossy()
                .contains("outside-link.ts")
        }),
        "outside symlink content must not enter analysis: {:?}",
        results.unused_files
    );
}
