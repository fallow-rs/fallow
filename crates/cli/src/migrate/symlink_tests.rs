//! A cloned repository can commit the config file that `migrate` writes as a
//! symlink to a path outside the project. `migrate` must not write through it.
//! A detected config file that links out of the project is not read.

use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::ExitCode;

use super::{migrate_auto_detect, run_migrate};

fn plant_knip_config(root: &Path) {
    std::fs::write(root.join("knip.json"), r#"{"entry": ["src/index.ts"]}"#).unwrap();
}

#[test]
fn migrate_does_not_create_a_config_through_a_dangling_link() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    plant_knip_config(root.path());
    let planted = outside.path().join("new.json");
    symlink(&planted, root.path().join(".fallowrc.json")).unwrap();

    let code = run_migrate(root.path(), false, false, false, None);

    assert_ne!(code, ExitCode::SUCCESS);
    assert!(!planted.exists());
}

#[test]
fn migrate_does_not_create_a_toml_config_through_a_dangling_link() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    plant_knip_config(root.path());
    let planted = outside.path().join("new.toml");
    symlink(&planted, root.path().join("fallow.toml")).unwrap();

    let code = run_migrate(root.path(), true, false, false, None);

    assert_ne!(code, ExitCode::SUCCESS);
    assert!(!planted.exists());
}

#[test]
fn migrate_dry_run_ignores_a_dangling_link() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    plant_knip_config(root.path());
    let planted = outside.path().join("new.json");
    symlink(&planted, root.path().join(".fallowrc.json")).unwrap();

    let code = run_migrate(root.path(), false, false, true, None);

    assert_eq!(code, ExitCode::SUCCESS);
    assert!(!planted.exists());
}

#[test]
fn migrate_does_not_read_a_detected_config_that_links_out_of_the_project() {
    for name in [
        "knip.json",
        ".jscpd.json",
        ".stylelintrc.json",
        "package.json",
    ] {
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let target = outside.path().join("other.json");
        std::fs::write(
            &target,
            r#"{"entry": ["outside-entry.ts"], "minTokens": 77, "rules": {"outside-rule": true}, "knip": {"entry": ["outside-entry.ts"]}}"#,
        )
        .unwrap();
        symlink(&target, root.path().join(name)).unwrap();

        let result = migrate_auto_detect(root.path());

        let text = format!("{result:?}");
        assert!(
            result.as_ref().is_ok_and(|done| !done.warnings.is_empty()),
            "{name}: a refused file is skipped with a warning: {text}"
        );
        assert!(!text.contains("outside-entry"), "{name}: {text}");
        assert!(!text.contains("77"), "{name}: {text}");
    }
}

/// A detected config that links to a shared file of the Git work tree above
/// the project root is read.
#[test]
fn migrate_reads_a_detected_config_that_links_inside_the_work_tree() {
    let top = tempfile::tempdir().unwrap();
    std::fs::create_dir(top.path().join(".git")).unwrap();
    let root = top.path().join("packages/app");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        top.path().join("knip.base.json"),
        r#"{"entry": ["src/shared-entry.ts"]}"#,
    )
    .unwrap();
    symlink("../../knip.base.json", root.join("knip.json")).unwrap();

    let result = migrate_auto_detect(&root).unwrap();

    assert!(
        result.config.to_string().contains("shared-entry"),
        "{}",
        result.config
    );
}
