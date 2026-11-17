use std::path::{Path, PathBuf};

use ed25519_dalek::{Signer, SigningKey};

use super::*;

const CURRENT: &str = env!("CARGO_PKG_VERSION");

pub fn write_install(store: &Path, version: &str, protocol: u32) -> PathBuf {
    let dir = store.join(version);
    std::fs::create_dir_all(&dir).expect("create install directory");
    let manifest = BundleManifest {
        schema_version: BUNDLE_MANIFEST_SCHEMA_VERSION,
        fallow_version: version.to_owned(),
        wire_protocol_version: protocol,
        platform: "linux".to_owned(),
        arch: "x64".to_owned(),
        typescript_version: "7.0.2".to_owned(),
    };
    std::fs::write(
        dir.join(BUNDLE_MANIFEST),
        serde_json::to_vec(&manifest).expect("serialize manifest"),
    )
    .expect("write manifest");
    std::fs::write(dir.join(SIDECAR_PACKAGE), "#!/bin/sh\n").expect("write companion");
    dir
}

#[test]
fn bundle_platform_follows_the_node_platform_and_architecture() {
    let x64 = bundle_platform("darwin", "x64").expect("x64 Node on macOS has a bundle");
    assert_eq!(x64.asset_name(), "fallow-type-aware-darwin-x64.tar.gz");
    let arm = bundle_platform("darwin", "arm64").expect("arm64 Node on macOS has a bundle");
    assert_eq!(arm.asset_name(), "fallow-type-aware-darwin-arm64.tar.gz");
    assert_eq!(
        bundle_platform("linux", "arm64")
            .expect("linux arm64")
            .asset_name(),
        "fallow-type-aware-linux-arm64.tar.gz"
    );

    let windows = bundle_platform("win32", "x64").expect_err("no Windows bundle");
    assert!(windows.contains("npm install --global"), "{windows}");
    let other = bundle_platform("linux", "riscv64").expect_err("no riscv64 bundle");
    assert!(other.contains("linux-riscv64"), "{other}");
    assert!(other.contains("darwin-arm64"), "{other}");
}

#[test]
fn store_root_uses_an_absolute_user_directory_only() {
    let env = |values: &'static [(&'static str, &'static str)]| {
        move |key: &str| {
            values
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| PathBuf::from(value))
        }
    };
    assert_eq!(store_root_from(&env(&[])), None);
    assert_eq!(store_root_from(&env(&[("HOME", "relative/home")])), None);
    let root = store_root_from(&env(&[
        ("HOME", "/home/user"),
        ("LOCALAPPDATA", "C:\\Users\\user\\AppData\\Local"),
    ]))
    .expect("a home directory gives a store");
    assert!(root.ends_with(Path::new("fallow").join("fallow-type-aware")));
    if cfg!(target_os = "macos") {
        assert_eq!(
            root,
            PathBuf::from("/home/user/Library/Application Support/fallow/fallow-type-aware")
        );
    } else if cfg!(unix) {
        assert_eq!(
            root,
            PathBuf::from("/home/user/.local/share/fallow/fallow-type-aware")
        );
        let xdg = store_root_from(&env(&[("HOME", "/home/user"), ("XDG_DATA_HOME", "/data")]))
            .expect("XDG data directory");
        assert_eq!(xdg, PathBuf::from("/data/fallow/fallow-type-aware"));
    }
}

#[test]
fn selection_prefers_the_running_version_then_the_newest_same_protocol_install() {
    let store = tempfile::tempdir().expect("store");
    write_install(store.path(), "1.2.0", WIRE_PROTOCOL_VERSION);
    write_install(store.path(), "1.10.0", WIRE_PROTOCOL_VERSION);
    write_install(store.path(), "9.0.0", WIRE_PROTOCOL_VERSION + 1);
    let bundles = installed_bundles(store.path());

    let newest = select_bundle(&bundles, "2.0.0", WIRE_PROTOCOL_VERSION).expect("compatible");
    assert_eq!(newest.manifest.fallow_version, "1.10.0");

    write_install(store.path(), "2.0.0", WIRE_PROTOCOL_VERSION);
    let bundles = installed_bundles(store.path());
    let exact = select_bundle(&bundles, "2.0.0", WIRE_PROTOCOL_VERSION).expect("exact");
    assert_eq!(exact.manifest.fallow_version, "2.0.0");

    assert!(select_bundle(&bundles, "2.0.0", WIRE_PROTOCOL_VERSION + 7).is_none());
}

#[test]
fn an_install_whose_directory_does_not_match_its_manifest_is_ignored() {
    let store = tempfile::tempdir().expect("store");
    let dir = write_install(store.path(), CURRENT, WIRE_PROTOCOL_VERSION);
    std::fs::rename(&dir, store.path().join(".partial-install")).expect("rename install");
    std::fs::write(store.path().join("README"), "not an install").expect("write file");
    assert!(installed_bundles(store.path()).is_empty());
    assert!(find_setup_companion(store.path()).is_none());
}

#[test]
fn an_install_of_another_protocol_is_not_discovered() {
    let store = tempfile::tempdir().expect("store");
    write_install(store.path(), CURRENT, WIRE_PROTOCOL_VERSION + 1);
    assert!(find_setup_companion(store.path()).is_none());

    let dir = write_install(store.path(), "0.0.1", WIRE_PROTOCOL_VERSION);
    assert_eq!(
        find_setup_companion(store.path()),
        Some(
            dunce::canonicalize(dir.join(SIDECAR_PACKAGE))
                .expect("canonical companion of the older install")
        )
    );
}

#[cfg(unix)]
#[test]
fn a_linked_companion_in_the_store_is_rejected() {
    let store = tempfile::tempdir().expect("store");
    let dir = write_install(store.path(), CURRENT, WIRE_PROTOCOL_VERSION);
    let outside = tempfile::tempdir().expect("outside");
    let target = outside.path().join(SIDECAR_PACKAGE);
    std::fs::write(&target, "#!/bin/sh\n").expect("write outside companion");
    std::fs::remove_file(dir.join(SIDECAR_PACKAGE)).expect("remove companion");
    std::os::unix::fs::symlink(&target, dir.join(SIDECAR_PACKAGE)).expect("link companion");
    assert!(find_setup_companion(store.path()).is_none());
}

#[test]
fn cleanup_keeps_the_install_that_discovery_uses() {
    let store = tempfile::tempdir().expect("store");
    let older = write_install(store.path(), "0.0.1", WIRE_PROTOCOL_VERSION);
    let other_protocol = write_install(store.path(), "0.0.2", WIRE_PROTOCOL_VERSION + 1);
    let current = write_install(store.path(), CURRENT, WIRE_PROTOCOL_VERSION);

    let plan = cleanup_plan(store.path());
    assert_eq!(plan.keep, Some(current));
    assert_eq!(plan.remove, vec![older, other_protocol]);

    let empty = tempfile::tempdir().expect("empty store");
    assert_eq!(
        cleanup_plan(empty.path()),
        CleanupPlan {
            keep: None,
            remove: Vec::new()
        }
    );
}

#[test]
fn bundle_signature_must_match_the_key_and_the_bytes() {
    let signer = SigningKey::from_bytes(&[7; 32]);
    let key = signer.verifying_key().to_bytes();
    let archive = b"bundle bytes".to_vec();
    let signature = signer.sign(&archive).to_bytes();

    assert!(verify_bundle_signature_with_key(&archive, &signature, &key).is_ok());

    let mut changed = archive.clone();
    changed.push(0);
    assert!(verify_bundle_signature_with_key(&changed, &signature, &key).is_err());

    let other = SigningKey::from_bytes(&[8; 32]).verifying_key().to_bytes();
    assert!(verify_bundle_signature_with_key(&archive, &signature, &other).is_err());

    let short = verify_bundle_signature_with_key(&archive, &signature[..63], &key)
        .expect_err("a short signature is rejected");
    assert!(short.contains("63 bytes"), "{short}");

    let release = verify_bundle_signature(&archive, &signature)
        .expect_err("a test signature does not match the release key");
    assert!(release.contains("release key"), "{release}");
}

#[test]
fn installs_with_the_same_version_core_select_by_version_order_not_by_directory_order() {
    let store = tempfile::tempdir().expect("store");
    write_install(store.path(), "1.2.0-alpha.1", WIRE_PROTOCOL_VERSION);
    write_install(store.path(), "1.2.0-beta.2", WIRE_PROTOCOL_VERSION);
    write_install(store.path(), "1.2.0-beta.10", WIRE_PROTOCOL_VERSION);
    let mut bundles = installed_bundles(store.path());
    let newest = |bundles: &[InstalledBundle]| {
        select_bundle(bundles, "2.0.0", WIRE_PROTOCOL_VERSION)
            .expect("a compatible install")
            .manifest
            .fallow_version
            .clone()
    };
    assert_eq!(newest(&bundles), "1.2.0-beta.10");
    bundles.reverse();
    assert_eq!(newest(&bundles), "1.2.0-beta.10");

    write_install(store.path(), "1.2.0", WIRE_PROTOCOL_VERSION);
    let mut bundles = installed_bundles(store.path());
    assert_eq!(
        newest(&bundles),
        "1.2.0",
        "a release wins over its prereleases"
    );
    bundles.reverse();
    assert_eq!(newest(&bundles), "1.2.0");

    assert_eq!(
        select_bundle(&bundles, "1.2.0-beta.2", WIRE_PROTOCOL_VERSION)
            .expect("the exact version")
            .manifest
            .fallow_version,
        "1.2.0-beta.2",
        "the exact version of the binary wins"
    );
}
