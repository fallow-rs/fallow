//! `fallow cloud login` and `fallow cloud logout`.
//!
//! `login` runs the Fallow Cloud device flow. The CLI asks for a device code
//! and a read-only API key, shows the user code and the URL, and polls until
//! a user confirms the code in the dashboard. The cloud then mints one
//! `fallow_read_k1_` key for that user and organization. The CLI stores the
//! key with [`crate::coverage::credentials::store_api_key`] and never prints it.
//!
//! `logout` removes the stored key from the keychain and from the credentials
//! file. The key stays valid in the cloud until a user revokes it in Settings.

use std::process::ExitCode;
use std::time::{Duration, Instant};

use fallow_config::OutputFormat;
use serde::Deserialize;
use serde_json::json;

use crate::api::{parse_error_envelope, sanitize_network_error, try_api_agent_with_timeout};
use crate::coverage::credentials::{self, KeySource};
use crate::error::emit_error;
use crate::json_style::JsonStyle;

/// Connect timeout of one device flow request.
const CONNECT_TIMEOUT_SECS: u64 = 5;
/// Total timeout of one device flow request.
const REQUEST_TIMEOUT_SECS: u64 = 20;
/// Extra wait after the cloud answers `slow_down`.
const SLOW_DOWN_STEP_SECS: u64 = 5;
/// Longest machine name the cloud accepts.
/// The cloud limit of a machine name, counted in UTF-16 code units as the
/// cloud counts it.
const MAX_MACHINE_NAME_UNITS: usize = 64;
/// Machine name when the host name is not known.
const FALLBACK_MACHINE_NAME: &str = "fallow-cli";

/// Arguments of `fallow cloud login`.
#[derive(Debug, Clone, Default)]
pub struct LoginArgs {
    pub machine_name: Option<String>,
    pub api_url: Option<String>,
}

/// A `fallow cloud` subcommand.
#[derive(Debug, Clone)]
pub enum CloudCommand {
    Login(LoginArgs),
    Logout { api_url: Option<String> },
}

/// Run a `fallow cloud` subcommand.
pub fn run(command: &CloudCommand, output: OutputFormat, json_style: JsonStyle) -> ExitCode {
    match command {
        CloudCommand::Login(args) => run_login(args, output, json_style),
        CloudCommand::Logout { api_url } => run_logout(api_url.as_deref(), output, json_style),
    }
}

#[derive(Debug, Deserialize)]
struct DataEnvelope<T> {
    data: T,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceStart {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceToken {
    #[serde(default)]
    api_key: Option<IssuedKey>,
    /// Why the cloud did not create a key, for example `owner_required`
    /// when the user who approved the code is not an organization owner.
    #[serde(default)]
    api_key_error: Option<ApiKeyError>,
}

#[derive(Debug, Deserialize)]
struct ApiKeyError {
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

#[derive(Deserialize)]
struct IssuedKey {
    key: String,
    #[serde(default)]
    prefix: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

/// One answer of the token poll.
enum PollOutcome {
    Issued(DeviceToken),
    Pending,
    SlowDown,
    /// The request or its answer was lost. The next poll tries again.
    Transient(String),
    /// An earlier poll finished the login, but its answer did not arrive.
    AlreadyClaimed,
    Failed(String),
}

/// Lost polls in a row after which the login stops.
const MAX_TRANSIENT_POLLS: u32 = 3;

fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_lowercase().chain(chars).collect()
    })
}

/// Revoke steps for a key that the cloud created but this machine did not store.
fn orphan_key_hint(machine_name: &str) -> String {
    format!(
        "Revoke the key \"fallow cloud login ({machine_name})\" at https://fallow.cloud/settings#api-keys, then run `fallow cloud login` again."
    )
}

fn run_login(args: &LoginArgs, output: OutputFormat, json_style: JsonStyle) -> ExitCode {
    let base_url = credentials::api_base_url(args.api_url.as_deref());
    let machine_name = match machine_name(args.machine_name.as_deref()) {
        Ok(name) => name,
        Err(message) => return emit_error(&message, 2, output),
    };
    let agent = match try_api_agent_with_timeout(CONNECT_TIMEOUT_SECS, REQUEST_TIMEOUT_SECS) {
        Ok(agent) => agent,
        Err(err) => return emit_error(&err.to_string(), 2, output),
    };
    let start = match start_device_flow(&agent, &base_url, &machine_name) {
        Ok(start) => start,
        Err(message) => return emit_error(&message, 2, output),
    };
    eprintln!(
        "Open {} and enter the code {}",
        start.verification_uri, start.user_code
    );
    eprintln!("This creates a read-only API key for \"{machine_name}\". Waiting for approval...");

    let token = match poll_for_key(&agent, &base_url, &start, &machine_name) {
        Ok(token) => token,
        Err(message) => return emit_error(&message, 2, output),
    };
    let Some(issued) = token.api_key else {
        return emit_error(&no_key_message(token.api_key_error.as_ref()), 2, output);
    };
    let source = match credentials::store_api_key(&base_url, &issued.key) {
        Ok(source) => source,
        Err(message) => {
            return emit_error(
                &format!(
                    "failed to store the API key: {message}. {}",
                    orphan_key_hint(&machine_name)
                ),
                2,
                output,
            );
        }
    };
    report_login(&issued, source, &base_url, output, json_style)
}

fn report_login(
    issued: &IssuedKey,
    source: KeySource,
    base_url: &str,
    output: OutputFormat,
    json_style: JsonStyle,
) -> ExitCode {
    let store = store_name(source);
    if matches!(output, OutputFormat::Json) {
        let value = json!({
            "kind": "cloud-login",
            "status": "logged_in",
            "api_url": base_url,
            "key_prefix": issued.prefix,
            "key_name": issued.name,
            "store": store,
        });
        return crate::report::emit_report_json(&value, "cloud-login", json_style);
    }
    let prefix = issued.prefix.as_deref().unwrap_or("fallow_read_k1_");
    eprintln!(
        "Logged in. Stored the read-only key {prefix}... in {}.",
        source.label()
    );
    ExitCode::SUCCESS
}

const fn store_name(source: KeySource) -> &'static str {
    match source {
        KeySource::Keychain => "keychain",
        KeySource::CredentialsFile => "credentials_file",
        KeySource::Flag | KeySource::Environment => "none",
    }
}

fn run_logout(api_url: Option<&str>, output: OutputFormat, json_style: JsonStyle) -> ExitCode {
    let base_url = credentials::api_base_url(api_url);
    let removed = match credentials::remove_api_key(&base_url) {
        Ok(removed) => removed,
        Err(message) => return emit_error(&message, 2, output),
    };
    if matches!(output, OutputFormat::Json) {
        let value = json!({
            "kind": "cloud-logout",
            "api_url": base_url,
            "removed_from_keychain": removed.keychain,
            "removed_from_credentials_file": removed.file,
        });
        return crate::report::emit_report_json(&value, "cloud-logout", json_style);
    }
    if removed.keychain || removed.file {
        eprintln!("Logged out. Removed the stored API key for {base_url}.");
        eprintln!(
            "The key stays valid until you revoke it at https://fallow.cloud/settings#api-keys."
        );
    } else {
        eprintln!("No stored API key for {base_url}.");
    }
    ExitCode::SUCCESS
}

fn start_device_flow(
    agent: &ureq::Agent,
    base_url: &str,
    machine_name: &str,
) -> Result<DeviceStart, String> {
    let url = format!("{base_url}/v1/auth/device");
    let mut response = agent
        .post(&url)
        .send_json(json!({ "keyType": "read", "machineName": machine_name }))
        .map_err(|err| sanitize_network_error(&format!("failed to start the login: {err}")))?;
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(|err| format!("failed to read the login answer: {err}"))?;
    if !(200..300).contains(&status) {
        return Err(failure_message("start the login", status, &body));
    }
    serde_json::from_str::<DataEnvelope<DeviceStart>>(&body)
        .map(|envelope| envelope.data)
        .map_err(|err| format!("malformed login answer from Fallow Cloud: {err}"))
}

/// The message of a sign-in that the cloud approved without a key.
fn no_key_message(error: Option<&ApiKeyError>) -> String {
    let Some(error) = error else {
        return "Fallow Cloud approved the login but returned no API key. The cloud may not support `fallow cloud login` yet. Create a read-only key at https://fallow.cloud/settings#api-keys and set FALLOW_API_KEY.".to_owned();
    };
    let reason = error
        .message
        .as_deref()
        .map(|message| message.trim().trim_end_matches('.'))
        .filter(|message| !message.is_empty())
        .unwrap_or("the cloud gave no reason");
    let hint = match error.code.as_deref() {
        Some("owner_required") => {
            " Only an organization owner can approve a key. Ask an owner to approve the code, or to create a read-only key at https://fallow.cloud/settings#api-keys for FALLOW_API_KEY."
        }
        _ => "",
    };
    format!(
        "You signed in, but Fallow Cloud did not create an API key: {reason}.{hint} No key was stored."
    )
}

fn poll_for_key(
    agent: &ureq::Agent,
    base_url: &str,
    start: &DeviceStart,
    machine_name: &str,
) -> Result<DeviceToken, String> {
    let deadline = Instant::now() + Duration::from_secs(start.expires_in);
    let mut interval = Duration::from_secs(start.interval);
    let mut lost = 0;
    loop {
        match poll_once(agent, base_url, &start.device_code) {
            PollOutcome::Issued(token) => return Ok(token),
            PollOutcome::Pending => lost = 0,
            PollOutcome::SlowDown => interval += Duration::from_secs(SLOW_DOWN_STEP_SECS),
            PollOutcome::Transient(message) => {
                lost += 1;
                if lost >= MAX_TRANSIENT_POLLS {
                    return Err(format!(
                        "{message}. If you approved the code, {}",
                        lowercase_first(&orphan_key_hint(machine_name))
                    ));
                }
            }
            PollOutcome::AlreadyClaimed => {
                return Err(format!(
                    "the login was approved, but the API key did not reach this machine. {}",
                    orphan_key_hint(machine_name)
                ));
            }
            PollOutcome::Failed(message) => return Err(message),
        }
        if Instant::now() + interval >= deadline {
            return Err(
                "the login code expired before it was approved. Run `fallow cloud login` again."
                    .to_owned(),
            );
        }
        std::thread::sleep(interval);
    }
}

fn poll_once(agent: &ureq::Agent, base_url: &str, device_code: &str) -> PollOutcome {
    let url = format!("{base_url}/v1/auth/device/token");
    let mut response = match agent
        .post(&url)
        .send_json(json!({ "deviceCode": device_code }))
    {
        Ok(response) => response,
        Err(err) => {
            return PollOutcome::Transient(sanitize_network_error(&format!(
                "failed to check the login: {err}"
            )));
        }
    };
    let status = response.status().as_u16();
    let body = match response.body_mut().read_to_string() {
        Ok(body) => body,
        Err(err) => {
            return PollOutcome::Transient(format!("failed to read the login answer: {err}"));
        }
    };
    if (200..300).contains(&status) {
        return match serde_json::from_str::<DataEnvelope<DeviceToken>>(&body) {
            Ok(envelope) => PollOutcome::Issued(envelope.data),
            Err(err) => {
                PollOutcome::Failed(format!("malformed login answer from Fallow Cloud: {err}"))
            }
        };
    }
    match parse_error_envelope(&body).code() {
        Some("authorization_pending") => PollOutcome::Pending,
        Some("slow_down") => PollOutcome::SlowDown,
        Some("already_claimed") => PollOutcome::AlreadyClaimed,
        Some("expired_token") => PollOutcome::Failed(
            "the login code expired before it was approved. Run `fallow cloud login` again."
                .to_owned(),
        ),
        _ => PollOutcome::Failed(failure_message("finish the login", status, &body)),
    }
}

fn failure_message(operation: &str, status: u16, body: &str) -> String {
    let envelope = parse_error_envelope(body);
    match envelope.message() {
        Some(message) => format!("failed to {operation} (HTTP {status}): {message}"),
        None => format!("failed to {operation} (HTTP {status})"),
    }
}

/// The machine name for the key: the flag, else the host name, else
/// `fallow-cli`. Control characters are removed and the name is cut to 64
/// characters, the cloud limit.
fn machine_name(explicit: Option<&str>) -> Result<String, String> {
    if let Some(name) = explicit {
        let cleaned = clean_machine_name(name);
        if cleaned.is_empty() {
            return Err("--machine-name must contain 1 to 64 printable characters".to_owned());
        }
        return Ok(cleaned);
    }
    let cleaned = clean_machine_name(&host_name().unwrap_or_default());
    Ok(if cleaned.is_empty() {
        FALLBACK_MACHINE_NAME.to_owned()
    } else {
        cleaned
    })
}

fn clean_machine_name(raw: &str) -> String {
    let printable: String = raw.chars().filter(|ch| !is_refused_in_name(*ch)).collect();
    let mut name = String::new();
    let mut units = 0;
    for ch in printable.trim().chars() {
        units += ch.len_utf16();
        if units > MAX_MACHINE_NAME_UNITS {
            break;
        }
        name.push(ch);
    }
    name.trim().to_owned()
}

/// The characters that Fallow Cloud refuses in a machine name: control
/// characters, zero-width and direction marks, line and paragraph separators,
/// bidi embeddings and isolates, and the byte order mark.
fn is_refused_in_name(ch: char) -> bool {
    ch.is_control()
        || matches!(
            ch,
            '\u{200B}'..='\u{200F}' | '\u{2028}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'
        )
}

fn host_name() -> Option<String> {
    for key in ["HOSTNAME", "COMPUTERNAME"] {
        if let Ok(value) = std::env::var(key)
            && !value.trim().is_empty()
        {
            return Some(value);
        }
    }
    let output = std::process::Command::new("hostname").output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn machine_name_strips_control_characters_and_caps_length() {
        assert_eq!(clean_machine_name("  box\u{7}\n "), "box");
        let long = "a".repeat(100);
        assert_eq!(
            clean_machine_name(&long).chars().count(),
            MAX_MACHINE_NAME_UNITS
        );
        assert!(machine_name(Some("\n\t")).is_err());
        assert_eq!(machine_name(Some("ci-box")).unwrap(), "ci-box");
    }

    #[test]
    fn machine_name_drops_what_the_cloud_refuses_and_counts_utf16_units() {
        assert_eq!(
            clean_machine_name("Bart\u{200B}'s\u{202E} Mac\u{FEFF}\u{2066}"),
            "Bart's Mac"
        );
        let emoji = clean_machine_name(&"\u{1F600}".repeat(64));
        assert_eq!(emoji.encode_utf16().count(), MAX_MACHINE_NAME_UNITS);
        assert_eq!(emoji.chars().count(), MAX_MACHINE_NAME_UNITS / 2);
    }

    #[test]
    fn a_key_refusal_names_the_cloud_reason_and_the_owner_rule() {
        let token: DeviceToken = serde_json::from_str(
            r#"{"licenseJwt":"jwt","apiKeyError":{"code":"owner_required","message":"Only an owner can create an API key"}}"#,
        )
        .unwrap();
        assert!(token.api_key.is_none());
        let message = no_key_message(token.api_key_error.as_ref());
        assert!(
            message.contains("Only an owner can create an API key"),
            "{message}"
        );
        assert!(message.contains("organization owner"), "{message}");
        assert!(!message.contains("may not support"), "{message}");

        let other = no_key_message(Some(&ApiKeyError {
            code: Some("acknowledgement_required".to_owned()),
            message: Some("Confirm the key in the dashboard.".to_owned()),
        }));
        assert!(
            other.contains("Confirm the key in the dashboard"),
            "{other}"
        );
        assert!(!other.contains(".."), "{other}");
    }

    #[test]
    fn failure_message_names_the_cloud_message() {
        let message = failure_message(
            "start the login",
            429,
            r#"{"error":true,"message":"Too many requests","code":"rate_limited"}"#,
        );
        assert_eq!(
            message,
            "failed to start the login (HTTP 429): Too many requests"
        );
    }
}
