//! Storage and resolution of the Fallow Cloud API key.
//!
//! Every command that reads a cloud key uses [`resolve_api_key`]. The order is:
//!
//! 1. the `--api-key` flag of the command,
//! 2. `$FALLOW_API_KEY`,
//! 3. the OS keychain entry that `fallow cloud login` wrote,
//! 4. the credentials file `~/.config/fallow/credentials` (mode 0600), which
//!    `fallow cloud login` writes when no keychain is available.
//!
//! The keychain entry has the service `fallow-cloud` and the API base URL as
//! its account, so a key for a staging cloud never goes to production. The
//! keychain is reached through the tools of the OS (`security` on macOS,
//! `secret-tool` on Linux), so the build needs no keychain library and a
//! machine without a keychain service uses the file. Windows uses the file.
//!
//! `FALLOW_CREDENTIAL_STORE=file` turns the keychain off for one process, and
//! `FALLOW_CREDENTIAL_STORE=none` turns both stores off, for example on a
//! shared CI runner or in a test.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

/// Keychain service name of the stored key.
pub const KEYCHAIN_SERVICE: &str = "fallow-cloud";
/// Environment variable that selects the credential store.
pub const CREDENTIAL_STORE_ENV: &str = "FALLOW_CREDENTIAL_STORE";
/// Environment variable with an explicit API key.
pub const API_KEY_ENV: &str = "FALLOW_API_KEY";
/// Label of the keychain entry.
const KEYCHAIN_LABEL: &str = "Fallow Cloud API key";
/// Format version of the credentials file.
const CREDENTIALS_VERSION: u32 = 1;

/// Where a resolved key came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    /// The `--api-key` flag.
    Flag,
    /// `$FALLOW_API_KEY`.
    Environment,
    /// The OS keychain.
    Keychain,
    /// The credentials file.
    CredentialsFile,
}

impl KeySource {
    /// Short description for human output.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Flag => "--api-key",
            Self::Environment => "FALLOW_API_KEY",
            Self::Keychain => "the OS keychain",
            Self::CredentialsFile => "the credentials file",
        }
    }
}

/// A key and its source.
#[derive(Clone)]
pub struct ResolvedKey {
    pub key: String,
    pub source: KeySource,
}

impl std::fmt::Debug for ResolvedKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedKey")
            .field("key", &"***")
            .field("source", &self.source)
            .finish()
    }
}

/// The API base URL that a stored key belongs to: the explicit endpoint of
/// the command, else `$FALLOW_API_URL`, else the default cloud.
pub fn api_base_url(explicit_endpoint: Option<&str>) -> String {
    let base = match explicit_endpoint.map(str::trim).filter(|v| !v.is_empty()) {
        Some(endpoint) => endpoint.to_owned(),
        None => crate::api::api_url(""),
    };
    base.trim_end_matches('/').to_owned()
}

/// Resolve the cloud API key: `--api-key`, `$FALLOW_API_KEY`, the keychain,
/// then the credentials file. `None` when no source has a key.
pub fn resolve_api_key(
    explicit: Option<&str>,
    explicit_endpoint: Option<&str>,
) -> Option<ResolvedKey> {
    let from_env = std::env::var(API_KEY_ENV).ok();
    select_api_key(explicit, from_env.as_deref(), || {
        stored_api_key(&api_base_url(explicit_endpoint))
    })
}

/// The key of a command that writes to the cloud (an upload) or reads a
/// route that a read-only key cannot open: `--api-key`, then
/// `$FALLOW_API_KEY`. The stored key is left out, because `fallow cloud
/// login` stores a read-only key, and the cloud refuses it for these routes.
pub fn resolve_write_api_key(explicit: Option<&str>) -> Option<ResolvedKey> {
    let from_env = std::env::var(API_KEY_ENV).ok();
    select_api_key(explicit, from_env.as_deref(), || None)
}

/// [`resolve_api_key`] over injected sources. The store is read only when the
/// flag and the environment have no key.
fn select_api_key(
    explicit: Option<&str>,
    from_env: Option<&str>,
    stored: impl FnOnce() -> Option<ResolvedKey>,
) -> Option<ResolvedKey> {
    let non_empty = |value: Option<&str>| {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    if let Some(key) = non_empty(explicit) {
        return Some(ResolvedKey {
            key,
            source: KeySource::Flag,
        });
    }
    if let Some(key) = non_empty(from_env) {
        return Some(ResolvedKey {
            key,
            source: KeySource::Environment,
        });
    }
    stored()
}

/// The stored key for `base_url`: the keychain first, then the file.
pub fn stored_api_key(base_url: &str) -> Option<ResolvedKey> {
    if store_mode() == StoreMode::None {
        return None;
    }
    if let Some(key) = keychain().and_then(|keychain| keychain.read(base_url)) {
        return Some(ResolvedKey {
            key,
            source: KeySource::Keychain,
        });
    }
    let path = credentials_path()?;
    read_credentials(&path)
        .keys
        .get(base_url)
        .map(|key| key.trim().to_owned())
        .filter(|key| !key.is_empty())
        .map(|key| ResolvedKey {
            key,
            source: KeySource::CredentialsFile,
        })
}

/// Store `key` for `base_url`: in the keychain when one is available, else in
/// the credentials file. Returns where the key went.
///
/// # Errors
///
/// Returns a message when neither store accepts the key.
pub fn store_api_key(base_url: &str, key: &str) -> Result<KeySource, String> {
    if store_mode() == StoreMode::None {
        return Err(format!("{CREDENTIAL_STORE_ENV}=none turns key storage off"));
    }
    if let Some(keychain) = keychain()
        && keychain.write(base_url, key)
    {
        // A stale file entry must not outlive a newer keychain entry.
        if let Some(path) = credentials_path() {
            let _ = remove_from_file(&path, base_url);
        }
        return Ok(KeySource::Keychain);
    }
    let path = credentials_path()
        .ok_or_else(|| "cannot find a home directory for the credentials file".to_owned())?;
    write_to_file(&path, base_url, key)?;
    Ok(KeySource::CredentialsFile)
}

/// What [`remove_api_key`] removed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Removed {
    pub keychain: bool,
    pub file: bool,
}

/// Remove the stored key for `base_url` from the keychain and from the file.
///
/// # Errors
///
/// Returns a message when the credentials file cannot be rewritten.
pub fn remove_api_key(base_url: &str) -> Result<Removed, String> {
    let keychain = keychain().is_some_and(|keychain| keychain.delete(base_url));
    let file = match credentials_path() {
        Some(path) => remove_from_file(&path, base_url)?,
        None => false,
    };
    Ok(Removed { keychain, file })
}

/// Path of the credentials file: `$XDG_CONFIG_HOME/fallow/credentials`, else
/// `~/.config/fallow/credentials`.
pub fn credentials_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| fallow_license::user_home_dir().map(|home| home.join(".config")))?;
    Some(base.join("fallow").join("credentials"))
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CredentialsFile {
    #[serde(default)]
    version: u32,
    /// API key per API base URL.
    #[serde(default)]
    keys: std::collections::BTreeMap<String, String>,
}

fn read_credentials(path: &Path) -> CredentialsFile {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_to_file(path: &Path, base_url: &str, key: &str) -> Result<(), String> {
    let mut credentials = read_credentials(path);
    credentials.version = CREDENTIALS_VERSION;
    credentials.keys.insert(base_url.to_owned(), key.to_owned());
    write_credentials(path, &credentials)
}

fn remove_from_file(path: &Path, base_url: &str) -> Result<bool, String> {
    if !path.exists() {
        return Ok(false);
    }
    let mut credentials = read_credentials(path);
    if credentials.keys.remove(base_url).is_none() {
        return Ok(false);
    }
    if credentials.keys.is_empty() {
        std::fs::remove_file(path)
            .map_err(|err| format!("failed to remove {}: {err}", path.display()))?;
    } else {
        write_credentials(path, &credentials)?;
    }
    Ok(true)
}

/// Write the file through a temporary file that is created with mode 0600, so
/// the key is never readable by other users, not even for a moment.
fn write_credentials(path: &Path, credentials: &CredentialsFile) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("invalid credentials path {}", path.display()))?;
    create_private_dir(dir)?;
    let text = serde_json::to_string_pretty(credentials)
        .map_err(|err| format!("failed to encode credentials: {err}"))?;
    let tmp = dir.join(format!(".credentials.{}.tmp", std::process::id()));
    let mut file = private_file_options()
        .open(&tmp)
        .map_err(|err| format!("failed to write {}: {err}", tmp.display()))?;
    file.write_all(text.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|err| format!("failed to write {}: {err}", tmp.display()))?;
    drop(file);
    std::fs::rename(&tmp, path).map_err(|err| {
        let _ = std::fs::remove_file(&tmp);
        format!("failed to write {}: {err}", path.display())
    })
}

#[cfg(unix)]
fn private_file_options() -> std::fs::OpenOptions {
    use std::os::unix::fs::OpenOptionsExt as _;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true).mode(0o600);
    options
}

#[cfg(not(unix))]
fn private_file_options() -> std::fs::OpenOptions {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    options
}

#[cfg(unix)]
fn create_private_dir(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::DirBuilderExt as _;
    if dir.is_dir() {
        return Ok(());
    }
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .map_err(|err| format!("failed to create {}: {err}", dir.display()))
}

#[cfg(not(unix))]
fn create_private_dir(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|err| format!("failed to create {}: {err}", dir.display()))
}

/// An OS keychain reached through a command-line tool.
#[derive(Debug, Clone, Copy)]
enum Keychain {
    /// macOS `security`.
    MacOs,
    /// Linux `secret-tool` (libsecret).
    SecretTool,
}

/// The keychain of this machine, or `None` when the keychain is off or the
/// OS has none that this module can reach.
fn keychain() -> Option<Keychain> {
    if keychain_disabled_by_env() {
        return None;
    }
    if cfg!(target_os = "macos") {
        return Path::new(MACOS_SECURITY)
            .is_file()
            .then_some(Keychain::MacOs);
    }
    if cfg!(target_os = "linux") && std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some() {
        return secret_tool_on_path().then_some(Keychain::SecretTool);
    }
    None
}

fn secret_tool_on_path() -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| dir.join("secret-tool").is_file())
    })
}

fn keychain_disabled_by_env() -> bool {
    store_mode() != StoreMode::Auto
}

/// The credential stores that `FALLOW_CREDENTIAL_STORE` allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StoreMode {
    /// The keychain, then the file (default).
    Auto,
    /// The file only.
    File,
    /// No stored key.
    None,
}

fn store_mode() -> StoreMode {
    match std::env::var(CREDENTIAL_STORE_ENV) {
        Ok(value) if value.trim().eq_ignore_ascii_case("file") => StoreMode::File,
        Ok(value) if value.trim().eq_ignore_ascii_case("none") => StoreMode::None,
        _ => StoreMode::Auto,
    }
}

const MACOS_SECURITY: &str = "/usr/bin/security";

/// The `security -i` command line that stores `key` for `account`. Returns
/// `None` when a value holds a character that the command parser of
/// `security -i` can split or end on.
fn macos_add_command(account: &str, key: &str) -> Option<String> {
    let safe = |value: &str| {
        !value.is_empty()
            && value
                .chars()
                .all(|ch| ch.is_ascii_graphic() && ch != '"' && ch != '\\' && ch != '\'')
    };
    if !safe(account) || !safe(key) {
        return None;
    }
    Some(format!(
        "add-generic-password -U -s \"{KEYCHAIN_SERVICE}\" -a \"{account}\" -l \"{KEYCHAIN_LABEL}\" -w \"{key}\"\n"
    ))
}

impl Keychain {
    fn read(self, account: &str) -> Option<String> {
        let output = match self {
            Self::MacOs => Command::new(MACOS_SECURITY)
                .args([
                    "find-generic-password",
                    "-s",
                    KEYCHAIN_SERVICE,
                    "-a",
                    account,
                    "-w",
                ])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output(),
            Self::SecretTool => Command::new("secret-tool")
                .args(["lookup", "service", KEYCHAIN_SERVICE, "account", account])
                .stdin(Stdio::null())
                .stderr(Stdio::null())
                .output(),
        }
        .ok()?;
        if !output.status.success() {
            return None;
        }
        let key = String::from_utf8(output.stdout).ok()?;
        let key = key.trim();
        (!key.is_empty()).then(|| key.to_owned())
    }

    /// Write the key through stdin, so it never shows in the process list.
    fn write(self, account: &str, key: &str) -> bool {
        let (mut command, input) = match self {
            Self::MacOs => {
                // `security add-generic-password -w` without a value reads the
                // password from the terminal, not from stdin. So send the whole
                // command to `security -i` on stdin instead.
                let Some(input) = macos_add_command(account, key) else {
                    return false;
                };
                let mut command = Command::new(MACOS_SECURITY);
                command.arg("-i");
                (command, input)
            }
            Self::SecretTool => {
                let mut command = Command::new("secret-tool");
                command.args([
                    "store",
                    "--label",
                    KEYCHAIN_LABEL,
                    "service",
                    KEYCHAIN_SERVICE,
                    "account",
                    account,
                ]);
                (command, key.to_owned())
            }
        };
        let Ok(mut child) = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        let wrote = child
            .stdin
            .take()
            .is_some_and(|mut stdin| stdin.write_all(input.as_bytes()).is_ok());
        let succeeded = child.wait().is_ok_and(|status| status.success());
        if wrote && succeeded && self.read(account).as_deref() == Some(key) {
            return true;
        }
        // Do not keep an entry that does not hold this key: reads use the
        // keychain before the file.
        self.delete(account);
        false
    }

    fn delete(self, account: &str) -> bool {
        let status = match self {
            Self::MacOs => Command::new(MACOS_SECURITY)
                .args([
                    "delete-generic-password",
                    "-s",
                    KEYCHAIN_SERVICE,
                    "-a",
                    account,
                ])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
            Self::SecretTool => Command::new("secret-tool")
                .args(["clear", "service", KEYCHAIN_SERVICE, "account", account])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
        };
        status.is_ok_and(|status| status.success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored(key: &str) -> ResolvedKey {
        ResolvedKey {
            key: key.to_owned(),
            source: KeySource::Keychain,
        }
    }

    #[test]
    fn flag_wins_over_environment_and_store() {
        let resolved =
            select_api_key(Some("  flag  "), Some("env"), || Some(stored("kc"))).unwrap();
        assert_eq!(resolved.key, "flag");
        assert_eq!(resolved.source, KeySource::Flag);
    }

    #[test]
    fn environment_wins_over_store_and_store_is_not_read() {
        let resolved =
            select_api_key(None, Some("env"), || panic!("store must not be read")).unwrap();
        assert_eq!(resolved.key, "env");
        assert_eq!(resolved.source, KeySource::Environment);
    }

    #[test]
    fn blank_values_fall_through_to_the_store() {
        let resolved = select_api_key(Some("  "), Some(""), || Some(stored("kc"))).unwrap();
        assert_eq!(resolved.key, "kc");
        assert_eq!(resolved.source, KeySource::Keychain);
        assert!(select_api_key(None, None, || None).is_none());
    }

    #[test]
    fn credentials_file_round_trip_keeps_other_urls_and_mode() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fallow").join("credentials");
        write_to_file(&path, "https://api.fallow.cloud", "fallow_read_k1_a").unwrap();
        write_to_file(&path, "http://localhost:3000", "fallow_read_k1_b").unwrap();
        let credentials = read_credentials(&path);
        assert_eq!(credentials.version, CREDENTIALS_VERSION);
        assert_eq!(
            credentials.keys["https://api.fallow.cloud"],
            "fallow_read_k1_a"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(remove_from_file(&path, "https://api.fallow.cloud").unwrap());
        assert!(!remove_from_file(&path, "https://api.fallow.cloud").unwrap());
        assert_eq!(read_credentials(&path).keys.len(), 1);
        assert!(remove_from_file(&path, "http://localhost:3000").unwrap());
        assert!(!path.exists(), "an empty file is removed");
    }

    /// Writes, reads and removes a real OS keychain entry. Ignored by
    /// default, because CI has no keychain. Run it on a developer machine
    /// with `cargo test -p fallow-cli keychain_round_trip -- --ignored`.
    #[test]
    #[ignore = "needs an OS keychain"]
    fn keychain_round_trip() {
        let keychain = keychain().expect("a keychain on this machine");
        let account = format!("http://keychain-probe.invalid/{}", std::process::id());
        assert!(keychain.write(&account, "fallow_read_k1_probe"));
        assert_eq!(
            keychain.read(&account).as_deref(),
            Some("fallow_read_k1_probe")
        );
        assert!(
            keychain.write(&account, "fallow_read_k1_probe2"),
            "an update replaces the key"
        );
        assert_eq!(
            keychain.read(&account).as_deref(),
            Some("fallow_read_k1_probe2")
        );
        assert!(keychain.delete(&account));
        assert_eq!(keychain.read(&account), None);
    }

    #[test]
    fn macos_write_sends_the_key_in_the_command_line_not_as_a_prompt_answer() {
        let input = macos_add_command("https://api.fallow.cloud", "fallow_read_k1_abc").unwrap();
        assert_eq!(
            input,
            "add-generic-password -U -s \"fallow-cloud\" -a \"https://api.fallow.cloud\" \
             -l \"Fallow Cloud API key\" -w \"fallow_read_k1_abc\"\n"
        );
        assert_eq!(input.lines().count(), 1, "one command, no prompt answers");
    }

    #[test]
    fn macos_write_refuses_values_the_command_parser_can_split() {
        assert!(macos_add_command("https://api.fallow.cloud", "a b").is_none());
        assert!(macos_add_command("https://api.fallow.cloud", "a\"b").is_none());
        assert!(macos_add_command("https://api.fallow.cloud", "a\nb").is_none());
        assert!(macos_add_command("https://api.fallow.cloud", "").is_none());
        assert!(macos_add_command("http://x/\\", "key").is_none());
    }

    #[test]
    fn api_base_url_trims_the_trailing_slash() {
        assert_eq!(
            api_base_url(Some(" http://localhost:3000/ ")),
            "http://localhost:3000"
        );
    }
}
