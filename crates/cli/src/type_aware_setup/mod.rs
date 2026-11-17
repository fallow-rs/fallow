//! `fallow type-aware setup` and `fallow type-aware cleanup`.
//!
//! Setup installs the signed type-aware bundle of this Fallow version into
//! the user setup directory, for a binary that has no npm launcher. The
//! command asks Node for its platform, gets the bundle from the GitHub
//! release (or from `--from <file>`), checks the detached signature with the
//! release key that this binary holds, and unpacks it into a versioned
//! directory. Discovery checks that directory after `FALLOW_TYPE_AWARE_BIN`
//! and the sibling of the executable.

mod archive;
mod node;

use std::fmt::Write as _;
use std::io::{IsTerminal as _, Write as _};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use fallow_api::type_aware_setup::{
    BUNDLE_MANIFEST, BundleManifest, BundlePlatform, CleanupPlan, bundle_platform, cleanup_plan,
    store_root, verify_bundle_signature_with_key, version_dir,
};
use fallow_config::OutputFormat;

use crate::error::emit_error_with_style;
use crate::exit_codes::{ErrorExitCode, INVALID_INPUT, NETWORK_FAILURE, RESOURCE_UNAVAILABLE};
use crate::json_style::JsonStyle;

const RELEASE_DOWNLOAD_BASE: &str = "https://github.com/fallow-rs/fallow/releases/download";
const SIGNATURE_SUFFIX: &str = ".sig";
const MAX_BUNDLE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SIGNATURE_BYTES: u64 = 1024;
const CONNECT_TIMEOUT_SECS: u64 = 30;
const DOWNLOAD_TIMEOUT_SECS: u64 = 600;
/// Prefix of the temporary directory of an install that is not complete.
const PARTIAL_PREFIX: &str = ".partial-";
/// Prefix of a previous install while a new one moves into its place.
const PREVIOUS_PREFIX: &str = ".previous-";
/// Lock file that a running setup holds, so cleanup keeps its temporary
/// directories.
const SETUP_LOCK: &str = ".setup.lock";

/// Public key that checks a bundle signature: the release key, or under the
/// `test-sidecar-key` feature the key of the public test seed `[0xAA; 32]`.
/// `crates/cli/src/health/coverage.rs` refuses that feature in release builds.
#[cfg(not(feature = "test-sidecar-key"))]
fn bundle_key() -> [u8; 32] {
    fallow_api::type_aware_setup::release_signing_key()
}

#[cfg(feature = "test-sidecar-key")]
fn bundle_key() -> [u8; 32] {
    [
        0xe7, 0x34, 0xea, 0x6c, 0x2b, 0x62, 0x57, 0xde, 0x72, 0x35, 0x5e, 0x47, 0x2a, 0xa0, 0x5a,
        0x4c, 0x48, 0x7e, 0x6b, 0x46, 0x3c, 0x02, 0x9e, 0xd3, 0x06, 0xdf, 0x2f, 0x01, 0xb5, 0x63,
        0x6b, 0x58,
    ]
}

/// Inputs of `fallow type-aware setup`.
pub struct SetupInput<'a> {
    pub root: &'a Path,
    pub yes: bool,
    pub from: Option<&'a Path>,
    pub output: OutputFormat,
    pub json_style: JsonStyle,
}

/// Where the bundle comes from.
enum Source {
    File(PathBuf),
    Release(String),
}

impl Source {
    fn describe(&self) -> String {
        match self {
            Self::File(path) => path.display().to_string(),
            Self::Release(url) => url.clone(),
        }
    }
}

fn release_url(platform: BundlePlatform) -> String {
    format!(
        "{RELEASE_DOWNLOAD_BASE}/v{}/{}",
        env!("CARGO_PKG_VERSION"),
        platform.asset_name()
    )
}

fn failure(message: &str, code: ErrorExitCode, output: OutputFormat, style: JsonStyle) -> ExitCode {
    emit_error_with_style(message, code, output, style)
}

/// Run `fallow type-aware setup`.
pub fn run_setup(input: &SetupInput<'_>) -> ExitCode {
    let fail = |message: &str, code| failure(message, code, input.output, input.json_style);
    let node = match node::probe() {
        Ok(node) => node,
        Err(error) => return fail(&error, RESOURCE_UNAVAILABLE),
    };
    let platform = match bundle_platform(&node.platform, &node.arch) {
        Ok(platform) => platform,
        Err(error) => return fail(&error, RESOURCE_UNAVAILABLE),
    };
    let Some(store) = store_root() else {
        return fail(
            "cannot find a user directory for the companion: set HOME (or LOCALAPPDATA on Windows)",
            RESOURCE_UNAVAILABLE,
        );
    };
    let target = version_dir(&store, env!("CARGO_PKG_VERSION"));
    let source = input.from.map_or_else(
        || Source::Release(release_url(platform)),
        |path| Source::File(path.to_path_buf()),
    );

    if !input.yes {
        if !std::io::stdin().is_terminal() {
            return fail(
                "non-interactive type-aware setup requires `--yes`",
                INVALID_INPUT,
            );
        }
        eprintln!("Fallow will install the type-aware companion:");
        eprintln!(
            "  Bundle: {} (Fallow {})",
            platform.asset_name(),
            env!("CARGO_PKG_VERSION")
        );
        eprintln!("  Source: {}", source.describe());
        eprintln!("  Target: {}", target.display());
        eprintln!(
            "  Node: {} {}-{}, which must stay on PATH",
            node.version, node.platform, node.arch
        );
        eprintln!(
            "  License: the bundle holds TypeScript (Apache-2.0); THIRD_PARTY_NOTICES.md lists the notices."
        );
        eprintln!("  Fallow checks the signature with its release key before it unpacks.");
        eprint!("Type `yes` to continue: ");
        let _ = std::io::stderr().flush();
        let mut confirmation = String::new();
        if std::io::stdin().read_line(&mut confirmation).is_err() || confirmation.trim() != "yes" {
            return fail("type-aware setup cancelled", INVALID_INPUT);
        }
    }

    let (archive, signature) = match fetch(&source) {
        Ok(bytes) => bytes,
        Err((error, code)) => return fail(&error, code),
    };
    if let Err(error) = install(&InstallRequest {
        archive: &archive,
        signature: &signature,
        key: &bundle_key(),
        platform,
        store: &store,
        version: env!("CARGO_PKG_VERSION"),
    }) {
        return fail(&error, INVALID_INPUT);
    }

    let status = fallow_api::type_aware_status(input.root);
    match input.output {
        OutputFormat::Json => {
            super::emit_type_aware_status_json(input.root, status, input.output, input.json_style)
        }
        _ => {
            crate::report::sink::outln!("Type-aware companion installed in {}.", target.display());
            if status.available {
                crate::report::sink::outln!(
                    "Run Fallow with `--type-aware` or set `typeAware.enabled: true`."
                );
                ExitCode::SUCCESS
            } else {
                let remediation = status.remediation.unwrap_or_default();
                failure(
                    &format!("the installed companion is not usable: {remediation}"),
                    RESOURCE_UNAVAILABLE,
                    input.output,
                    input.json_style,
                )
            }
        }
    }
}

fn fetch(source: &Source) -> Result<(Vec<u8>, Vec<u8>), (String, ErrorExitCode)> {
    match source {
        Source::File(path) => {
            let mut signature_path = path.clone().into_os_string();
            signature_path.push(SIGNATURE_SUFFIX);
            let signature_path = PathBuf::from(signature_path);
            let archive = read_bounded(path, MAX_BUNDLE_BYTES).map_err(|e| (e, INVALID_INPUT))?;
            let signature = read_bounded(&signature_path, MAX_SIGNATURE_BYTES)
                .map_err(|e| (e, INVALID_INPUT))?;
            Ok((archive, signature))
        }
        Source::Release(url) => {
            let agent =
                crate::api::try_api_agent_with_timeout(CONNECT_TIMEOUT_SECS, DOWNLOAD_TIMEOUT_SECS)
                    .map_err(|error| (error.to_string(), NETWORK_FAILURE))?;
            let archive = download(&agent, url, MAX_BUNDLE_BYTES)?;
            let signature = download(
                &agent,
                &format!("{url}{SIGNATURE_SUFFIX}"),
                MAX_SIGNATURE_BYTES,
            )?;
            Ok((archive, signature))
        }
    }
}

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(format!(
            "{} is not a type-aware bundle file",
            path.display()
        ));
    }
    std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))
}

/// GET `url` and return at most `limit` bytes. The agent honors
/// `HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`, `NO_PROXY` and
/// `FALLOW_CA_BUNDLE`.
fn download(
    agent: &ureq::Agent,
    url: &str,
    limit: u64,
) -> Result<Vec<u8>, (String, ErrorExitCode)> {
    let mut response = agent
        .get(url)
        .call()
        .map_err(|error| (format!("cannot download {url}: {error}"), NETWORK_FAILURE))?;
    let status = response.status().as_u16();
    if status == 404 {
        return Err((
            format!(
                "{url} does not exist. The type-aware bundle is published with each release; a development build has none. Use `--from <file>` with a bundle of this version."
            ),
            NETWORK_FAILURE,
        ));
    }
    if !(200..300).contains(&status) {
        return Err((format!("GET {url} returned HTTP {status}"), NETWORK_FAILURE));
    }
    response
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|error| (format!("cannot read {url}: {error}"), NETWORK_FAILURE))
}

struct InstallRequest<'a> {
    archive: &'a [u8],
    signature: &'a [u8],
    key: &'a [u8; 32],
    platform: BundlePlatform,
    store: &'a Path,
    version: &'a str,
}

/// Check, unpack and move a bundle into `<store>/<version>`.
fn install(request: &InstallRequest<'_>) -> Result<PathBuf, String> {
    verify_bundle_signature_with_key(request.archive, request.signature, request.key)?;
    let files = archive::read_bundle(request.archive)?;
    let manifest = bundle_manifest(&files)?;
    check_manifest(&manifest, request.platform, request.version)?;

    std::fs::create_dir_all(request.store)
        .map_err(|error| format!("cannot create {}: {error}", request.store.display()))?;
    let _lock = lock_store(request.store)?;
    let partial = request.store.join(format!(
        "{PARTIAL_PREFIX}{}-{}",
        request.version,
        std::process::id()
    ));
    remove_dir_if_present(&partial)?;
    if let Err(error) = archive::unpack(&files, &partial) {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(error);
    }
    let target = version_dir(request.store, request.version);
    if let Err(error) = replace_install(&partial, &target) {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(error);
    }
    Ok(target)
}

/// Move the unpacked `partial` directory to `target`. A previous install
/// moves aside first and is deleted only after the new one is in place. When
/// the new directory cannot move in, the previous install moves back, and an
/// error names it when that move also fails. A crash between the two moves
/// leaves the previous install in `.previous-<version>-<pid>`; the next setup
/// installs the version again and cleanup removes that directory.
fn replace_install(partial: &Path, target: &Path) -> Result<(), String> {
    replace_install_with(partial, target, |from, to| std::fs::rename(from, to))
}

fn replace_install_with(
    partial: &Path,
    target: &Path,
    rename: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let name = target
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let previous = target.with_file_name(format!("{PREVIOUS_PREFIX}{name}-{}", std::process::id()));
    remove_dir_if_present(&previous)?;
    let moved_aside = match rename(target, &previous) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => {
            return Err(format!(
                "cannot move the previous companion out of {}: {error}",
                target.display()
            ));
        }
    };
    if let Err(error) = rename(partial, target) {
        let mut message = format!("cannot move the companion to {}: {error}", target.display());
        if moved_aside && let Err(restore) = rename(&previous, target) {
            let _ = write!(
                message,
                "; cannot restore the previous install, which stays at {}: {restore}",
                previous.display()
            );
        }
        return Err(message);
    }
    if moved_aside {
        let _ = std::fs::remove_dir_all(&previous);
    }
    Ok(())
}

fn bundle_manifest(files: &[archive::BundleFile]) -> Result<BundleManifest, String> {
    let file = files
        .iter()
        .find(|file| file.path == BUNDLE_MANIFEST)
        .ok_or_else(|| format!("the bundle has no {BUNDLE_MANIFEST}"))?;
    if !files
        .iter()
        .any(|file| file.path == "fallow-type-aware" && file.executable)
    {
        return Err("the bundle has no executable fallow-type-aware launcher".to_owned());
    }
    serde_json::from_slice(&file.data)
        .map_err(|error| format!("the bundle has an invalid {BUNDLE_MANIFEST}: {error}"))
}

fn check_manifest(
    manifest: &BundleManifest,
    platform: BundlePlatform,
    version: &str,
) -> Result<(), String> {
    if manifest.schema_version != fallow_api::type_aware_setup::BUNDLE_MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "the bundle has manifest schema {}; this Fallow reads schema {}",
            manifest.schema_version,
            fallow_api::type_aware_setup::BUNDLE_MANIFEST_SCHEMA_VERSION
        ));
    }
    if manifest.fallow_version != version {
        return Err(format!(
            "the bundle is for Fallow {}, but this is Fallow {version}",
            manifest.fallow_version
        ));
    }
    if manifest.platform != platform.platform || manifest.arch != platform.arch {
        return Err(format!(
            "the bundle is for {}-{}, but Node reports {}-{}",
            manifest.platform, manifest.arch, platform.platform, platform.arch
        ));
    }
    if manifest.wire_protocol_version != fallow_api::type_aware_setup::wire_protocol_version() {
        return Err(format!(
            "the bundle speaks type-aware protocol {}, but this Fallow speaks {}",
            manifest.wire_protocol_version,
            fallow_api::type_aware_setup::wire_protocol_version()
        ));
    }
    Ok(())
}

fn remove_dir_if_present(path: &Path) -> Result<(), String> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot remove {}: {error}", path.display())),
    }
}

/// Inputs of `fallow type-aware cleanup`.
pub struct CleanupInput {
    pub yes: bool,
    pub output: OutputFormat,
    pub json_style: JsonStyle,
}

/// Run `fallow type-aware cleanup`: remove every install in the setup
/// directory except the one that discovery uses, and the temporary and
/// previous directories of an interrupted setup. While a setup runs, its
/// directories stay.
pub fn run_cleanup(input: &CleanupInput) -> ExitCode {
    let fail = |message: &str, code| failure(message, code, input.output, input.json_style);
    if !matches!(input.output, OutputFormat::Human) {
        return fail(
            "type-aware cleanup supports human output only",
            INVALID_INPUT,
        );
    }
    let Some(store) = store_root() else {
        crate::report::sink::outln!("No type-aware setup directory exists.");
        return ExitCode::SUCCESS;
    };
    let plan = cleanup_targets(&store);
    if plan.remove.is_empty() {
        crate::report::sink::outln!("No older type-aware companions to remove.");
        return ExitCode::SUCCESS;
    }
    if !input.yes {
        if !std::io::stdin().is_terminal() {
            return fail(
                "non-interactive type-aware cleanup requires `--yes`",
                INVALID_INPUT,
            );
        }
        eprintln!("Fallow will remove these directories:");
        for dir in &plan.remove {
            eprintln!("  {}", dir.display());
        }
        if let Some(keep) = &plan.keep {
            eprintln!("Fallow keeps {}, which it uses now.", keep.display());
        }
        eprint!("Type `yes` to continue: ");
        let _ = std::io::stderr().flush();
        let mut confirmation = String::new();
        if std::io::stdin().read_line(&mut confirmation).is_err() || confirmation.trim() != "yes" {
            return fail("type-aware cleanup cancelled", INVALID_INPUT);
        }
    }
    match remove_all(&plan.remove) {
        Ok(()) => {
            for dir in &plan.remove {
                crate::report::sink::outln!("Removed {}", dir.display());
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail(&error, INVALID_INPUT),
    }
}

/// The cleanup plan of the API, plus the temporary directories of an
/// interrupted setup.
fn cleanup_targets(store: &Path) -> CleanupPlan {
    let mut plan = cleanup_plan(store);
    if setup_is_running(store) {
        return plan;
    }
    if let Ok(entries) = std::fs::read_dir(store) {
        plan.remove.extend(
            entries
                .filter_map(Result::ok)
                .filter(|entry| {
                    entry.file_type().is_ok_and(|kind| kind.is_dir())
                        && entry.file_name().to_str().is_some_and(|name| {
                            name.starts_with(PARTIAL_PREFIX) || name.starts_with(PREVIOUS_PREFIX)
                        })
                })
                .map(|entry| entry.path()),
        );
    }
    plan.remove.sort();
    plan
}

/// Hold the setup lock of `store` until the returned file drops.
fn lock_store(store: &Path) -> Result<std::fs::File, String> {
    let path = store.join(SETUP_LOCK);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    match try_lock_briefly(&file) {
        Ok(true) => Ok(file),
        Ok(false) => Err(format!(
            "another `fallow type-aware setup` runs for {}; wait until it ends",
            store.display()
        )),
        Err(error) => Err(format!("cannot lock {}: {error}", path.display())),
    }
}

/// True when a setup holds the lock of `store`. An unknown state counts as
/// running, so cleanup never removes a directory that a setup still uses.
fn setup_is_running(store: &Path) -> bool {
    let Ok(file) = std::fs::OpenOptions::new()
        .read(true)
        .open(store.join(SETUP_LOCK))
    else {
        return false;
    };
    !matches!(try_lock_briefly(&file), Ok(true))
}

/// Try the lock a few times. A child process that another thread forks while
/// a lock file is open keeps that lock until it starts its program, so a busy
/// lock is checked again for a short time. A real setup holds the lock for
/// seconds.
fn try_lock_briefly(file: &std::fs::File) -> std::io::Result<bool> {
    const ATTEMPTS: u32 = 10;
    const PAUSE: Duration = Duration::from_millis(20);
    for attempt in 1..=ATTEMPTS {
        match file.try_lock() {
            Ok(()) => return Ok(true),
            Err(std::fs::TryLockError::WouldBlock) => {
                if attempt < ATTEMPTS {
                    std::thread::sleep(PAUSE);
                }
            }
            Err(std::fs::TryLockError::Error(error)) => return Err(error),
        }
    }
    Ok(false)
}

fn remove_all(dirs: &[PathBuf]) -> Result<(), String> {
    for dir in dirs {
        remove_dir_if_present(dir)?;
    }
    Ok(())
}

/// Bound for the Node probe, so a hung `node` cannot block setup.
const NODE_PROBE_TIMEOUT: Duration = Duration::from_secs(20);

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
