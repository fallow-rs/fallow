//! The recorded arguments of the exported functions, kept in the gitignored
//! cache directory (`<cache dir>/mutation/recorded-args.json`).
//!
//! The record pass of a run gives the distinct inputs of each wrapped export. The
//! store keeps them for [`TIME_TO_LIVE_SECS`]. The values never leave the
//! cache directory: a report holds only counts and reasons. Before an input
//! is stored, the redaction removes each input that holds a value under a
//! secret key name (`token`, `secret`, `password`, `authorization`, and
//! similar) or a string that looks like a secret (high entropy), and counts
//! it as not replayable. In `shapes` mode, the store keeps the types of the
//! arguments only.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::mutation::RecordArgsMode;
use crate::mutation::protocol::{RecordRefusal, RecordedFunction};
use crate::mutation::replay::canonical;

/// The time that a recorded input stays usable: 7 days.
pub const TIME_TO_LIVE_SECS: u64 = 7 * 24 * 60 * 60;

/// The version of the store format.
const STORE_FORMAT: u32 = 1;

/// The tag key of the replay encoding.
const TAG: &str = "$replay";

/// Key names whose value is a secret. The match ignores case, `-` and `_`.
const SECRET_KEY_PARTS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "pwd",
    "authorization",
    "auth",
    "bearer",
    "jwt",
    "apikey",
    "accesskey",
    "signature",
    "cookie",
    "credential",
    "privatekey",
    "session",
];

/// The reasons that the companion gives for a call that it cannot record.
/// Another reason becomes [`UNREADABLE_REASON`], so a report never holds
/// text from user data.
const KNOWN_REASONS: &[&str] = &[
    "a function",
    "a symbol",
    "a proxy",
    "a cyclic value",
    "a value that is too large",
    "an accessor property",
    "an object with a symbol key",
    "an object that this runtime cannot check for a proxy",
    UNREADABLE_REASON,
];
const UNREADABLE_REASON: &str = "a value that cannot be read";
/// The prefix of the class instance reason, which names a class of the code.
const CLASS_INSTANCE_REASON: &str = "a class instance";

/// A string at least this long, with no space, can look like a secret.
const SECRET_MIN_LEN: usize = 24;
/// The Shannon entropy in bits for each character from which such a string
/// looks like a secret. A UUID or a hex digest stays below it.
const SECRET_MIN_ENTROPY: f64 = 4.2;

const SECRET_KEY_REASON: &str = "a value under a secret key name";
const SECRET_STRING_REASON: &str = "a string that looks like a secret";

/// One stored input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredInput {
    /// The first 12 hex digits of the SHA-256 digest of the canonical
    /// arguments.
    pub input_id: String,
    /// The arguments in the replay encoding. `None` in `shapes` mode.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<serde_json::Value>>,
    /// The types of the arguments.
    pub shape: serde_json::Value,
}

/// The stored inputs of one exported function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredFunction {
    pub recorded_at: u64,
    pub mode: RecordArgsMode,
    pub inputs: Vec<StoredInput>,
    #[serde(default)]
    pub not_replayable: Vec<StoredRefusal>,
}

/// The count of calls of one reason that the store did not keep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRefusal {
    pub reason: String,
    pub count: u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Store {
    format: u32,
    functions: std::collections::BTreeMap<String, StoredFunction>,
}

/// The path of the store below the cache directory.
pub fn store_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("mutation").join("recorded-args.json")
}

/// The input id of an argument list.
pub fn input_id(args: &[serde_json::Value]) -> String {
    let text = canonical(&serde_json::Value::Array(args.to_vec()));
    let digest = Sha256::digest(text.as_bytes());
    digest.iter().take(6).fold(String::new(), |mut hex, byte| {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
        hex
    })
}

fn normalized_key(key: &str) -> String {
    key.chars()
        .filter(|char| *char != '-' && *char != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_secret_key(key: &str) -> bool {
    let key = normalized_key(key);
    SECRET_KEY_PARTS.iter().any(|part| key.contains(part))
}

/// The Shannon entropy of `text`, in bits for each character.
fn entropy(text: &str) -> f64 {
    let mut counts: rustc_hash::FxHashMap<char, u32> = rustc_hash::FxHashMap::default();
    let mut total = 0_u32;
    for char in text.chars() {
        *counts.entry(char).or_default() += 1;
        total += 1;
    }
    let total = f64::from(total);
    counts
        .values()
        .map(|&count| {
            let share = f64::from(count) / total;
            -share * share.log2()
        })
        .sum()
}

/// The characters that separate the parts of a path, a URL or a query. A
/// key is one part, so a long path with many names does not count as one
/// key.
const PART_SEPARATORS: &[char] = &['/', '\\', ':', '?', '&', '=', '#', ',', ';'];

/// A byte array of this size range can be a key.
const KEY_BYTES: std::ops::RangeInclusive<usize> = 16..=512;
/// The byte entropy in bits from which a key-sized byte array looks random.
const KEY_BYTES_MIN_ENTROPY: f64 = 3.5;

/// True for the `values` of a key-sized byte array with random-looking
/// bytes.
fn looks_like_key_bytes(values: &serde_json::Value) -> bool {
    let Some(values) = values.as_array() else {
        return false;
    };
    if !KEY_BYTES.contains(&values.len()) {
        return false;
    }
    let text: String = values
        .iter()
        .filter_map(serde_json::Value::as_u64)
        .filter_map(|byte| char::from_u32(u32::try_from(byte).ok()?))
        .collect();
    entropy(&text) >= KEY_BYTES_MIN_ENTROPY
}

/// A hex text at least this long looks like a key. Its entropy cannot pass
/// [`SECRET_MIN_ENTROPY`], so it has its own rule.
const SECRET_MIN_HEX_LEN: usize = 32;

/// True for one word with a part that looks like a key: long, and with
/// high entropy, or a long hex text.
fn is_secret_word(word: &str) -> bool {
    // A base64 key can hold `/`, so a word of base64 characters only also
    // counts as a whole.
    let base64 = word.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'=' | b'-' | b'_')
    });
    if base64 && word.chars().count() >= SECRET_MIN_LEN && entropy(word) >= SECRET_MIN_ENTROPY {
        return true;
    }
    word.split(PART_SEPARATORS).any(|part| {
        let len = part.chars().count();
        (len >= SECRET_MIN_LEN && entropy(part) >= SECRET_MIN_ENTROPY)
            || (len >= SECRET_MIN_HEX_LEN && part.bytes().all(|byte| byte.is_ascii_hexdigit()))
    })
}

/// True for a three-part base64url text, the shape of a JSON Web Token.
fn is_jwt(word: &str) -> bool {
    let parts: Vec<&str> = word.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            part.len() >= 8
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        })
}

/// True for a URL with a password in its user part, such as
/// `https://user:pass@host`.
fn has_url_password(text: &str) -> bool {
    text.split("://").skip(1).any(|rest| {
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        authority
            .rsplit_once('@')
            .is_some_and(|(user, _)| user.contains(':'))
    })
}

fn looks_like_secret(text: &str) -> bool {
    let lower = text.trim_start().to_ascii_lowercase();
    lower.starts_with("bearer ")
        || lower.starts_with("basic ")
        || text.contains("-----BEGIN")
        || has_url_password(text)
        || text
            .split_whitespace()
            .any(|word| is_secret_word(word) || is_jwt(word))
}

/// The redaction reason of `value`, or `None` when it holds no secret.
fn secret_reason(value: &serde_json::Value) -> Option<&'static str> {
    match value {
        serde_json::Value::String(text) => looks_like_secret(text).then_some(SECRET_STRING_REASON),
        serde_json::Value::Array(items) => items.iter().find_map(secret_reason),
        serde_json::Value::Object(map) => match map.get(TAG).and_then(serde_json::Value::as_str) {
            Some("bytes") if map.get("values").is_some_and(looks_like_key_bytes) => {
                Some(SECRET_STRING_REASON)
            }
            // A map and an escaped object hold `[key, value]` entries. A
            // string key counts as a key name.
            Some("map" | "object") => map.get("entries").and_then(secret_in_entries),
            Some(_) => map.values().find_map(secret_reason),
            None => map.iter().find_map(|(key, item)| {
                if is_secret_key(key) || looks_like_secret(key) {
                    return Some(SECRET_KEY_REASON);
                }
                secret_reason(item)
            }),
        },
        _ => None,
    }
}

/// The reason of the first secret in the `[key, value]` entries of a map or
/// an escaped object.
fn secret_in_entries(entries: &serde_json::Value) -> Option<&'static str> {
    entries.as_array()?.iter().find_map(|entry| {
        let pair = entry.as_array()?;
        let (key, value) = (pair.first()?, pair.get(1)?);
        if key
            .as_str()
            .is_some_and(|key| is_secret_key(key) || looks_like_secret(key))
        {
            return Some(SECRET_KEY_REASON);
        }
        secret_reason(key).or_else(|| secret_reason(value))
    })
}

/// A reason of the companion as the report may show it.
fn known_reason(reason: &str) -> &'static str {
    if reason.starts_with(CLASS_INSTANCE_REASON) {
        return CLASS_INSTANCE_REASON;
    }
    KNOWN_REASONS
        .iter()
        .find(|known| **known == reason)
        .copied()
        .unwrap_or(UNREADABLE_REASON)
}

/// The types of a value in the replay encoding.
fn shape(value: &serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Null => Value::from("null"),
        Value::Bool(_) => Value::from("boolean"),
        Value::Number(_) => Value::from("number"),
        Value::String(_) => Value::from("string"),
        Value::Array(items) => Value::Array(items.iter().map(shape).collect()),
        Value::Object(map) => match map.get(TAG).and_then(Value::as_str) {
            Some(tag) => Value::from(tag),
            None => Value::Object(
                map.iter()
                    .map(|(key, item)| (key.clone(), shape(item)))
                    .collect(),
            ),
        },
    }
}

fn read_store(path: &Path) -> Store {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Store>(&bytes).ok())
        .filter(|store| store.format == STORE_FORMAT)
        .unwrap_or_default()
}

/// An entry is fresh for its time to live. An entry from the future (a clock
/// that moved back) is not.
const fn is_fresh(entry: &StoredFunction, now: u64) -> bool {
    entry.recorded_at <= now && now - entry.recorded_at <= TIME_TO_LIVE_SECS
}

fn add_refusal(refusals: &mut Vec<StoredRefusal>, reason: &str, count: u64) {
    match refusals.iter_mut().find(|refusal| refusal.reason == reason) {
        Some(refusal) => refusal.count += count,
        None => refusals.push(StoredRefusal {
            reason: reason.to_owned(),
            count,
        }),
    }
}

/// The stored form of one recorded function: redacted, and in `shapes`
/// mode without values.
fn stored_function(function: &RecordedFunction, mode: RecordArgsMode, now: u64) -> StoredFunction {
    let mut not_replayable = Vec::new();
    for RecordRefusal { reason, count } in &function.not_replayable {
        add_refusal(&mut not_replayable, known_reason(reason), *count);
    }
    let mut inputs = Vec::new();
    for args in &function.inputs {
        let as_array = serde_json::Value::Array(args.clone());
        if let Some(reason) = secret_reason(&as_array) {
            add_refusal(&mut not_replayable, reason, 1);
            continue;
        }
        inputs.push(StoredInput {
            input_id: input_id(args),
            args: (mode == RecordArgsMode::Values).then(|| args.clone()),
            shape: shape(&as_array),
        });
    }
    not_replayable.sort_by(|left, right| left.reason.cmp(&right.reason));
    StoredFunction {
        recorded_at: now,
        mode,
        inputs,
        not_replayable,
    }
}

/// The recorded functions with one entry for each key: the inputs and the
/// reasons of a key that the pass gives twice join.
fn merged(functions: &[RecordedFunction]) -> Vec<RecordedFunction> {
    let mut by_key: std::collections::BTreeMap<&str, RecordedFunction> =
        std::collections::BTreeMap::new();
    for function in functions {
        let entry = by_key
            .entry(function.key.as_str())
            .or_insert_with(|| RecordedFunction {
                key: function.key.clone(),
                inputs: Vec::new(),
                not_replayable: Vec::new(),
            });
        for input in &function.inputs {
            if !entry.inputs.contains(input) {
                entry.inputs.push(input.clone());
            }
        }
        entry
            .not_replayable
            .extend(function.not_replayable.iter().cloned());
    }
    by_key.into_values().collect()
}

/// Writes `bytes` to `path` through a private staging file, so a reader
/// never sees a partial file and other users cannot read the values.
fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let staging = path.with_extension(format!("{}.{unique}.tmp", std::process::id()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    let written = options
        .open(&staging)
        .and_then(|mut file| file.write_all(bytes).and_then(|()| file.sync_all()))
        .and_then(|()| std::fs::rename(&staging, path));
    if written.is_err() {
        let _ = std::fs::remove_file(&staging);
    }
    written
}

/// Removes the expired entries of the store, so a value does not stay on
/// disk past its time to live. A run calls it also without recording.
pub fn prune(cache_dir: &Path, now: u64) {
    let path = store_path(cache_dir);
    if !path.is_file() {
        return;
    }
    let mut store = read_store(&path);
    let before = store.functions.len();
    store.functions.retain(|_, entry| is_fresh(entry, now));
    if store.functions.len() == before {
        return;
    }
    if store.functions.is_empty() {
        let _ = std::fs::remove_file(&path);
    } else if let Ok(bytes) = serde_json::to_vec_pretty(&store) {
        let _ = write_private(&path, &bytes);
    }
}

/// Writes the recorded functions of a run into the store. Each function of
/// the run replaces its earlier entry, and each expired entry goes.
///
/// # Errors
///
/// Returns the IO error of the write.
pub fn store(
    cache_dir: &Path,
    mode: RecordArgsMode,
    functions: &[RecordedFunction],
    now: u64,
) -> std::io::Result<Vec<(String, StoredFunction)>> {
    crate::mutation::last_report::ignore_cache_dir(cache_dir)?;
    let path = store_path(cache_dir);
    let mut store = read_store(&path);
    store.format = STORE_FORMAT;
    store.functions.retain(|_, entry| is_fresh(entry, now));
    let mut written = Vec::new();
    for function in &merged(functions) {
        let entry = stored_function(function, mode, now);
        store.functions.insert(function.key.clone(), entry.clone());
        written.push((function.key.clone(), entry));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_vec_pretty(&store).map_err(std::io::Error::other)?;
    write_private(&path, &text)?;
    Ok(written)
}

/// The fresh stored inputs of `key` (`<file>#<export>`), or `None`.
pub fn load(cache_dir: &Path, key: &str, now: u64) -> Option<StoredFunction> {
    read_store(&store_path(cache_dir))
        .functions
        .remove(key)
        .filter(|entry| is_fresh(entry, now))
}

/// The result of the record pass of a run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordPass {
    pub functions: Vec<RecordedFunction>,
    /// Why the pass gave no records, for example a runner that records
    /// nothing. `None` when the pass ran.
    pub error: Option<String>,
}

/// Stores the recorded functions of a run and gives the report summary:
/// counts and reasons, never values. `None` without recording. `pass` is
/// `None` when no test ran.
pub fn save_recording(
    cache_dir: &Path,
    mode: RecordArgsMode,
    pass: Option<&RecordPass>,
) -> Option<fallow_output::MutationRecordingOutput> {
    let output_mode = match mode {
        RecordArgsMode::Off => return None,
        RecordArgsMode::Shapes => fallow_output::MutationRecordMode::Shapes,
        RecordArgsMode::Values => fallow_output::MutationRecordMode::Values,
    };
    let summary = |saved, error: Option<String>, entries: Vec<(String, StoredFunction)>| {
        fallow_output::MutationRecordingOutput {
            mode: output_mode,
            saved,
            error,
            functions: summary_functions(entries),
        }
    };
    let Some(pass) = pass else {
        return Some(summary(
            false,
            Some("no test ran, so no argument was recorded".to_owned()),
            Vec::new(),
        ));
    };
    if let Some(error) = &pass.error {
        return Some(summary(false, Some(error.clone()), Vec::new()));
    }
    Some(match store(cache_dir, mode, &pass.functions, now_secs()) {
        Ok(entries) => summary(true, None, entries),
        Err(error) => summary(
            false,
            Some(format!(
                "the cache file cannot be written: {}",
                error.kind()
            )),
            merged(&pass.functions)
                .iter()
                .map(|function| (function.key.clone(), stored_function(function, mode, 0)))
                .collect(),
        ),
    })
}

fn summary_functions(
    entries: Vec<(String, StoredFunction)>,
) -> Vec<fallow_output::MutationRecordedFunction> {
    let mut functions: Vec<fallow_output::MutationRecordedFunction> = entries
        .into_iter()
        .filter_map(|(key, entry)| {
            let (file, export) = key.rsplit_once('#')?;
            Some(fallow_output::MutationRecordedFunction {
                file: file.to_owned(),
                export: export.to_owned(),
                input_count: entry.inputs.len(),
                not_replayable: entry
                    .not_replayable
                    .into_iter()
                    .map(|refusal| fallow_output::MutationNotReplayable {
                        reason: refusal.reason,
                        count: refusal.count,
                    })
                    .collect(),
            })
        })
        .collect();
    functions.sort_by(|left, right| {
        (left.file.as_str(), left.export.as_str())
            .cmp(&(right.file.as_str(), right.export.as_str()))
    });
    functions
}

/// The current time in seconds since the Unix epoch.
pub fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

#[cfg(test)]
mod tests;
