//! A cloned repository can commit the config file that `migrate` writes as a
//! symlink to a path outside the project. `migrate` must not write through it.

use std::os::unix::fs::symlink;
use std::path::Path;
use std::process::ExitCode;

use super::run_migrate;

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
