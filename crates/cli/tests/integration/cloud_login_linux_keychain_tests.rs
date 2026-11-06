#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "integration tests keep fixture setup concise"
)]

//! `fallow cloud login`, `inspect --cloud` and `logout` with the automatic
//! credential store on Linux, against a mock device flow.
//!
//! The store selection runs as on a user machine: `FALLOW_CREDENTIAL_STORE`
//! is not set. A host without a D-Bus session uses the credentials file with
//! mode 0600. A host with a D-Bus session and `secret-tool` uses the Secret
//! Service. The Secret Service test needs an unlocked keyring, so it is
//! ignored by default. `scripts/linux-keychain-test.sh` starts a D-Bus
//! session with `gnome-keyring` and runs both tests.

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Command;

use crate::common::{CommandOutput, parse_json};
use crate::inspect_cloud_tests::{
    DEVICE_START, DEVICE_TOKEN, Env, INSPECT_JSON, file_context_body, project, run_with, serve,
};

/// The key that the mock device flow issues in [`DEVICE_TOKEN`].
const ISSUED_KEY: &str = "fallow_read_k1_fromlogin";
const LOGIN: &[&str] = &[
    "cloud",
    "login",
    "--machine-name",
    "ci-box",
    "--format",
    "json",
];
const LOGOUT: &[&str] = &["cloud", "logout", "--format", "json"];

/// Use the automatic store. `with_dbus` keeps the D-Bus session of the test
/// process, else the command gets no D-Bus session.
fn automatic_store(with_dbus: bool) -> impl Fn(&mut Command) {
    move |command: &mut Command| {
        command.env_remove("FALLOW_CREDENTIAL_STORE");
        if !with_dbus {
            command.env_remove("DBUS_SESSION_BUS_ADDRESS");
        }
    }
}

/// The key that the Secret Service holds for `account`, or `None`.
fn secret_service_key(account: &str) -> Option<String> {
    let output = Command::new("secret-tool")
        .args(["lookup", "service", "fallow-cloud", "account", account])
        .output()
        .expect("run secret-tool");
    let key = String::from_utf8(output.stdout).unwrap();
    (output.status.success() && !key.trim().is_empty()).then(|| key.trim().to_owned())
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

/// `inspect --cloud` passed and sent the key of the login in `request`.
fn assert_inspect_sent_the_issued_key(inspect: &CommandOutput, request: &str) {
    assert_eq!(inspect.code, 0, "stderr={}", inspect.stderr);
    assert_eq!(parse_json(inspect)["evidence"]["cloud"]["status"], "ok");
    assert!(
        request
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {ISSUED_KEY}")),
        "inspect sends the stored key: {request}"
    );
}

#[test]
fn cloud_login_without_a_dbus_session_stores_the_key_in_a_0600_file() {
    let (dir, home) = project();
    let (endpoint, captured, handle) = serve(vec![
        (200, DEVICE_START.to_owned()),
        (200, DEVICE_TOKEN.to_owned()),
        (200, file_context_body()),
    ]);
    let env = Env {
        api_url: Some(&endpoint),
        api_key: None,
        home: home.path(),
    };

    let login = run_with(LOGIN, dir.path(), &env, automatic_store(false));
    assert_eq!(login.code, 0, "stderr={}", login.stderr);
    assert_eq!(parse_json(&login)["store"], "credentials_file");

    let config = home.path().join(".config").join("fallow");
    let credentials = config.join("credentials");
    assert_eq!(mode(&credentials), 0o600, "credentials file mode");
    assert_eq!(mode(&config), 0o700, "credentials directory mode");
    let stored: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&credentials).unwrap()).unwrap();
    assert_eq!(stored["keys"][&endpoint], ISSUED_KEY);

    let inspect = run_with(INSPECT_JSON, dir.path(), &env, automatic_store(false));
    handle.join().expect("server joins");
    assert_inspect_sent_the_issued_key(&inspect, &captured.lock().unwrap()[2]);

    let logout = run_with(LOGOUT, dir.path(), &env, automatic_store(false));
    assert_eq!(logout.code, 0, "stderr={}", logout.stderr);
    let removed = parse_json(&logout);
    assert_eq!(removed["removed_from_credentials_file"], true);
    assert_eq!(removed["removed_from_keychain"], false);
    assert!(!credentials.exists(), "logout removes the last key");
}

#[test]
#[ignore = "needs an unlocked Secret Service; run scripts/linux-keychain-test.sh"]
fn cloud_login_with_a_secret_service_stores_the_key_in_the_keyring() {
    assert!(
        std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some(),
        "run this test in a D-Bus session with an unlocked keyring"
    );
    let (dir, home) = project();
    let (endpoint, captured, handle) = serve(vec![
        (200, DEVICE_START.to_owned()),
        (200, DEVICE_TOKEN.to_owned()),
        (200, file_context_body()),
    ]);
    let env = Env {
        api_url: Some(&endpoint),
        api_key: None,
        home: home.path(),
    };

    let login = run_with(LOGIN, dir.path(), &env, automatic_store(true));
    assert_eq!(login.code, 0, "stderr={}", login.stderr);
    assert_eq!(parse_json(&login)["store"], "keychain");
    assert_eq!(
        secret_service_key(&endpoint).as_deref(),
        Some(ISSUED_KEY),
        "the keyring holds the key under the API URL"
    );
    let credentials = home.path().join(".config/fallow/credentials");
    assert!(!credentials.exists(), "the key does not go to the file");

    let inspect = run_with(INSPECT_JSON, dir.path(), &env, automatic_store(true));
    handle.join().expect("server joins");
    assert_inspect_sent_the_issued_key(&inspect, &captured.lock().unwrap()[2]);

    let logout = run_with(LOGOUT, dir.path(), &env, automatic_store(true));
    assert_eq!(logout.code, 0, "stderr={}", logout.stderr);
    let removed = parse_json(&logout);
    assert_eq!(removed["removed_from_keychain"], true);
    assert_eq!(removed["removed_from_credentials_file"], false);
    assert_eq!(
        secret_service_key(&endpoint),
        None,
        "logout clears the keyring"
    );
}
