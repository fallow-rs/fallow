use std::io::{BufRead as _, BufReader};
use std::net::TcpListener;

use ed25519_dalek::{Signer as _, SigningKey};
use fallow_api::type_aware_setup::{
    BUNDLE_MANIFEST_SCHEMA_VERSION, bundle_platform, find_setup_companion, wire_protocol_version,
};

use super::archive::tests::archive;
use super::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn signer() -> SigningKey {
    SigningKey::from_bytes(&[3; 32])
}

fn manifest_json(version: &str, platform: &str, arch: &str, protocol: u32) -> Vec<u8> {
    serde_json::to_vec(&BundleManifest {
        schema_version: BUNDLE_MANIFEST_SCHEMA_VERSION,
        fallow_version: version.to_owned(),
        wire_protocol_version: protocol,
        platform: platform.to_owned(),
        arch: arch.to_owned(),
        typescript_version: "7.0.2".to_owned(),
    })
    .expect("serialize manifest")
}

fn bundle(manifest: &[u8]) -> Vec<u8> {
    archive(&[
        ("bundle.json", manifest, 0o644, b'0'),
        ("fallow-type-aware", b"#!/bin/sh\nexit 0\n", 0o755, b'0'),
        ("lib/fallow-type-aware.mjs", b"export {};\n", 0o644, b'0'),
    ])
}

fn linux_x64() -> BundlePlatform {
    bundle_platform("linux", "x64").expect("linux x64 bundle")
}

fn request<'a>(
    archive: &'a [u8],
    signature: &'a [u8],
    key: &'a [u8; 32],
    store: &'a Path,
) -> InstallRequest<'a> {
    InstallRequest {
        archive,
        signature,
        key,
        platform: linux_x64(),
        store,
        version: VERSION,
    }
}

#[test]
fn installs_a_signed_bundle_into_the_versioned_directory() {
    let store = tempfile::tempdir().expect("store");
    let bytes = bundle(&manifest_json(
        VERSION,
        "linux",
        "x64",
        wire_protocol_version(),
    ));
    let signature = signer().sign(&bytes).to_bytes();
    let key = signer().verifying_key().to_bytes();

    let target = install(&request(&bytes, &signature, &key, store.path())).expect("install");

    assert_eq!(target, store.path().join(VERSION));
    assert!(target.join("lib/fallow-type-aware.mjs").is_file());
    assert_eq!(
        find_setup_companion(store.path()),
        Some(dunce::canonicalize(target.join("fallow-type-aware")).expect("canonical companion"))
    );

    // A second setup replaces the install and leaves no temporary directory.
    install(&request(&bytes, &signature, &key, store.path())).expect("reinstall");
    let directories = std::fs::read_dir(store.path())
        .expect("list store")
        .map(|entry| entry.expect("entry"))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.file_name())
        .collect::<Vec<_>>();
    assert_eq!(directories, vec![std::ffi::OsString::from(VERSION)]);
}

#[test]
fn a_bundle_with_a_bad_signature_is_not_unpacked() {
    let store = tempfile::tempdir().expect("store");
    let bytes = bundle(&manifest_json(
        VERSION,
        "linux",
        "x64",
        wire_protocol_version(),
    ));
    let key = signer().verifying_key().to_bytes();

    let other = SigningKey::from_bytes(&[4; 32]).sign(&bytes).to_bytes();
    let error = install(&request(&bytes, &other, &key, store.path())).expect_err("wrong key");
    assert!(error.contains("release key"), "{error}");

    let signature = signer().sign(&bytes).to_bytes();
    let mut changed = bytes.clone();
    let last = changed.len() - 1;
    changed[last] ^= 1;
    assert!(install(&request(&changed, &signature, &key, store.path())).is_err());

    let release = install(&request(
        &bytes,
        &signature,
        &fallow_api::type_aware_setup::release_signing_key(),
        store.path(),
    ))
    .expect_err("a test signature never matches the release key");
    assert!(release.contains("release key"), "{release}");

    assert!(
        std::fs::read_dir(store.path())
            .expect("list store")
            .next()
            .is_none(),
        "nothing is written before the signature check passes"
    );
}

#[test]
fn a_bundle_for_another_version_platform_or_protocol_is_rejected() {
    let key = signer().verifying_key().to_bytes();
    for (manifest, expected) in [
        (
            manifest_json("0.0.1", "linux", "x64", wire_protocol_version()),
            "is for Fallow 0.0.1",
        ),
        (
            manifest_json(VERSION, "darwin", "arm64", wire_protocol_version()),
            "Node reports linux-x64",
        ),
        (
            manifest_json(VERSION, "linux", "x64", wire_protocol_version() + 1),
            "protocol",
        ),
    ] {
        let store = tempfile::tempdir().expect("store");
        let bytes = bundle(&manifest);
        let signature = signer().sign(&bytes).to_bytes();
        let error = install(&request(&bytes, &signature, &key, store.path()))
            .expect_err("mismatched bundle");
        assert!(error.contains(expected), "{expected}: {error}");
        assert!(!store.path().join(VERSION).exists());
    }
}

#[test]
fn a_bundle_without_a_launcher_is_rejected() {
    let store = tempfile::tempdir().expect("store");
    let manifest = manifest_json(VERSION, "linux", "x64", wire_protocol_version());
    let bytes = archive(&[("bundle.json", &manifest, 0o644, b'0')]);
    let signature = signer().sign(&bytes).to_bytes();
    let key = signer().verifying_key().to_bytes();
    let error = install(&request(&bytes, &signature, &key, store.path())).expect_err("no launcher");
    assert!(error.contains("launcher"), "{error}");
}

#[test]
fn cleanup_removes_older_installs_and_partial_directories_only() {
    let store = tempfile::tempdir().expect("store");
    let key = signer().verifying_key().to_bytes();
    let bytes = bundle(&manifest_json(
        VERSION,
        "linux",
        "x64",
        wire_protocol_version(),
    ));
    let signature = signer().sign(&bytes).to_bytes();
    let current = install(&request(&bytes, &signature, &key, store.path())).expect("install");

    let older_bytes = bundle(&manifest_json(
        "0.0.1",
        "linux",
        "x64",
        wire_protocol_version(),
    ));
    let older_signature = signer().sign(&older_bytes).to_bytes();
    let older = install(&InstallRequest {
        version: "0.0.1",
        ..request(&older_bytes, &older_signature, &key, store.path())
    })
    .expect("install an older version");
    let partial = store.path().join(format!("{PARTIAL_PREFIX}9.9.9-1"));
    std::fs::create_dir_all(&partial).expect("create partial directory");
    let previous = store.path().join(format!("{PREVIOUS_PREFIX}{VERSION}-1"));
    std::fs::create_dir_all(&previous).expect("create previous directory");
    let unrelated = store.path().join("notes");
    std::fs::create_dir_all(&unrelated).expect("create unrelated directory");

    let plan = cleanup_targets(store.path());
    assert_eq!(plan.keep, Some(current.clone()));
    assert_eq!(
        plan.remove,
        vec![partial.clone(), previous.clone(), older.clone()]
    );

    remove_all(&plan.remove).expect("remove");
    assert!(current.is_dir());
    assert!(unrelated.is_dir());
    assert!(!older.exists());
    assert!(!partial.exists());
    assert!(!previous.exists());
}

#[test]
fn node_probe_output_selects_the_node_platform() {
    let info = node::parse_probe(br#"{"platform":"darwin","arch":"x64","version":"22.11.0"}"#)
        .expect("valid probe");
    assert_eq!(
        bundle_platform(&info.platform, &info.arch)
            .expect("x64 Node on a Mac has a bundle")
            .asset_name(),
        "fallow-type-aware-darwin-x64.tar.gz"
    );

    let old = node::parse_probe(br#"{"platform":"linux","arch":"x64","version":"18.20.4"}"#)
        .expect_err("Node 18 is too old");
    assert!(old.contains("Node.js 20 or later"), "{old}");
    assert!(node::parse_probe(b"not json").is_err());
    assert!(node::missing_node_message().contains("on PATH"));
}

fn serve_once(status: &str, body: &'static [u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local server");
    let address = listener.local_addr().expect("local address");
    let status = status.to_owned();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().expect("accept");
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut line = String::new();
        while reader.read_line(&mut line).is_ok_and(|read| read > 2) {
            line.clear();
        }
        let mut stream = stream;
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(body);
    });
    format!("http://{address}/bundle.tar.gz")
}

#[test]
fn download_reads_a_bounded_body_and_names_a_missing_release() {
    let agent = crate::api::try_api_agent_with_timeout(5, 10).expect("agent");
    let body = download(&agent, &serve_once("200 OK", b"bundle"), 64).expect("download");
    assert_eq!(body, b"bundle");

    let (error, code) =
        download(&agent, &serve_once("200 OK", b"too large body"), 4).expect_err("over the limit");
    assert!(error.contains("cannot read"), "{error}");
    assert_eq!(code, NETWORK_FAILURE);

    let (error, _) =
        download(&agent, &serve_once("404 Not Found", b""), 64).expect_err("missing asset");
    assert!(error.contains("--from <file>"), "{error}");
}

#[test]
fn a_failed_replace_keeps_the_previous_install() {
    let store = tempfile::tempdir().expect("store");
    let target = store.path().join(VERSION);
    std::fs::create_dir_all(&target).expect("create the previous install");
    std::fs::write(target.join("fallow-type-aware"), "previous")
        .expect("write the previous install");

    let missing = store
        .path()
        .join(format!("{PARTIAL_PREFIX}{VERSION}-missing"));
    replace_install(&missing, &target).expect_err("the new directory does not exist");

    assert_eq!(
        std::fs::read_to_string(target.join("fallow-type-aware"))
            .expect("the previous install stays"),
        "previous"
    );
    let names = std::fs::read_dir(store.path())
        .expect("list store")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<Vec<_>>();
    assert_eq!(names, vec![std::ffi::OsString::from(VERSION)]);
}

#[test]
fn a_replace_swaps_the_install_and_removes_the_previous_one() {
    let store = tempfile::tempdir().expect("store");
    let target = store.path().join(VERSION);
    std::fs::create_dir_all(&target).expect("create the previous install");
    std::fs::write(target.join("fallow-type-aware"), "previous")
        .expect("write the previous install");
    let partial = store.path().join(format!("{PARTIAL_PREFIX}{VERSION}-1"));
    std::fs::create_dir_all(&partial).expect("create the new install");
    std::fs::write(partial.join("fallow-type-aware"), "new").expect("write the new install");

    replace_install(&partial, &target).expect("replace");

    assert_eq!(
        std::fs::read_to_string(target.join("fallow-type-aware")).expect("read the new install"),
        "new"
    );
    let names = std::fs::read_dir(store.path())
        .expect("list store")
        .map(|entry| entry.expect("entry").file_name())
        .collect::<Vec<_>>();
    assert_eq!(names, vec![std::ffi::OsString::from(VERSION)]);
}

#[test]
fn a_failed_rollback_names_where_the_previous_install_is() {
    let store = tempfile::tempdir().expect("store");
    let target = store.path().join(VERSION);
    std::fs::create_dir_all(&target).expect("create the previous install");
    let partial = store.path().join(format!("{PARTIAL_PREFIX}{VERSION}-1"));
    std::fs::create_dir_all(&partial).expect("create the new install");
    let calls = std::cell::Cell::new(0);
    let rename = |from: &Path, to: &Path| {
        calls.set(calls.get() + 1);
        if calls.get() == 1 {
            std::fs::rename(from, to)
        } else {
            Err(std::io::Error::other("injected rename failure"))
        }
    };

    let error = replace_install_with(&partial, &target, rename).expect_err("both moves fail");

    let previous = store
        .path()
        .join(format!("{PREVIOUS_PREFIX}{VERSION}-{}", std::process::id()));
    assert!(previous.is_dir(), "the previous install stays aside");
    assert!(error.contains("cannot move the companion"), "{error}");
    assert!(error.contains("cannot restore"), "{error}");
    assert!(
        error.contains(&previous.display().to_string()),
        "the error names the previous install: {error}"
    );
}

#[test]
fn cleanup_keeps_the_temporary_directories_of_a_running_setup() {
    let store = tempfile::tempdir().expect("store");
    let partial = store.path().join(format!("{PARTIAL_PREFIX}{VERSION}-1"));
    let previous = store.path().join(format!("{PREVIOUS_PREFIX}{VERSION}-1"));
    std::fs::create_dir_all(&partial).expect("create partial directory");
    std::fs::create_dir_all(&previous).expect("create previous directory");
    let older = store.path().join("0.0.1");
    let lock = std::fs::File::create(store.path().join(".setup.lock")).expect("create lock file");
    lock.lock().expect("hold the setup lock");

    let plan = cleanup_targets(store.path());
    assert!(!plan.remove.contains(&partial), "{:?}", plan.remove);
    assert!(!plan.remove.contains(&previous), "{:?}", plan.remove);
    assert!(!plan.remove.contains(&older));

    lock.unlock().expect("release the setup lock");
    let plan = cleanup_targets(store.path());
    assert_eq!(plan.remove, vec![partial, previous]);
}

#[test]
fn a_second_setup_waits_for_the_lock_of_a_running_one() {
    let store = tempfile::tempdir().expect("store");
    let held = lock_store(store.path()).expect("the first setup takes the lock");
    let error = lock_store(store.path()).expect_err("a second setup must not run at once");
    assert!(
        error.contains("another `fallow type-aware setup` runs"),
        "{error}"
    );
    drop(held);
    lock_store(store.path()).expect("the lock is free after the first setup");
}
