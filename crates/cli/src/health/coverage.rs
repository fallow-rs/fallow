use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::process::{Command, Stdio};
use std::{collections::BTreeMap, fs};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use fallow_config::OutputFormat;
use fallow_cov_protocol::{
    CaptureQuality, Confidence, CoverageSource, Evidence, PROTOCOL_VERSION, ReportVerdict, Request,
    Response, RiskBand, StaticFile, StaticFindings, StaticFunction, Verdict, Watermark,
};
use fallow_license::{
    DEFAULT_HARD_FAIL_DAYS, Feature, LicenseStatus, load_and_verify, load_raw_jwt,
};
use fallow_v8_coverage::V8CoverageDump;
use globset::GlobSet;
use oxc_coverage_instrument::{FileCoverage, FnEntry, Location, Position};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use srcmap_sourcemap::SourceMap;
use tempfile::TempDir;
use url::Url;

use crate::error::emit_error;
use crate::health::RuntimeCoverageOptions;
use crate::health::scoring::IstanbulCoverage;
use crate::health_types::{
    RuntimeCoverageAction, RuntimeCoverageConfidence, RuntimeCoverageDataSource,
    RuntimeCoverageEvidence, RuntimeCoverageFinding, RuntimeCoverageHotPath,
    RuntimeCoverageMessage, RuntimeCoverageReport, RuntimeCoverageReportVerdict,
    RuntimeCoverageRiskBand, RuntimeCoverageSummary, RuntimeCoverageVerdict,
    RuntimeCoverageWatermark,
};
use crate::license::verifying_key;

/// Ed25519 public key used to verify the fallow-cov sidecar binary at every
/// spawn. Intentionally SEPARATE from the license-signing pubkey at
/// `crate::license::PUBLIC_KEY_BYTES` so binary and license keys can rotate
/// independently; see `fallow-cloud/decisions/008-sidecar-key-rotation.md`.
///
/// The constant name deliberately avoids the substring `PUBLIC_KEY_BYTES` so
/// the `fallow-cloud/.github/workflows/public-key-parity.yml` Python regex
/// (which matches the first `PUBLIC_KEY_BYTES: [u8; 32]` in the file) never
/// misidentifies it as the license pubkey.
///
/// Must match the `ED25519_BINARY_SIGNING_PUBLIC_KEY` repository variable on
/// `fallow-rs/fallow-cloud` byte-for-byte; the `binary-signing-parity.yml`
/// workflow on fallow-cloud asserts this daily. If you rotate the key, update
/// both sides in the same release cycle per the procedure in ADR 008.
#[cfg(not(feature = "test-sidecar-key"))]
const BINARY_SIGNING_VERIFY_KEY: [u8; 32] = [
    19, 101, 100, 202, 175, 194, 21, 42, 215, 158, 125, 99, 218, 176, 85, 44, 62, 175, 122, 137,
    33, 144, 210, 11, 56, 216, 191, 101, 249, 27, 112, 27,
];

/// Test-only sidecar binary-signing pubkey, derived from the deterministic
/// seed `[0xAA; 32]` at `crates/cli/tests/common/test_signing_keys.rs`. Enabled
/// only by the `test-sidecar-key` cargo feature. A `compile_error!` below
/// refuses to let this feature coexist with a release build so it cannot ship
/// to users by accident.
#[cfg(feature = "test-sidecar-key")]
const BINARY_SIGNING_VERIFY_KEY: [u8; 32] = [
    0xe7, 0x34, 0xea, 0x6c, 0x2b, 0x62, 0x57, 0xde, 0x72, 0x35, 0x5e, 0x47, 0x2a, 0xa0, 0x5a, 0x4c,
    0x48, 0x7e, 0x6b, 0x46, 0x3c, 0x02, 0x9e, 0xd3, 0x06, 0xdf, 0x2f, 0x01, 0xb5, 0x63, 0x6b, 0x58,
];

// Hard stop: `test-sidecar-key` ships the test pubkey instead of the real
// binary-signing pubkey. A release build with this feature active would accept
// stub sidecars signed by any party in possession of the seed. Debug builds
// only.
#[cfg(all(feature = "test-sidecar-key", not(debug_assertions)))]
compile_error!(
    "feature `test-sidecar-key` must never be enabled in release builds; it swaps the sidecar binary-signing pubkey for a test keypair whose seed is public"
);

type FunctionLocations = FxHashMap<(String, String), Option<u32>>;

struct PreparedCoverageSources {
    sources: Vec<CoverageSource>,
    _temp_dir: Option<TempDir>,
}

#[derive(Default)]
struct StaticSignalIndex {
    unused_files: FxHashSet<PathBuf>,
    exported_names: FxHashMap<PathBuf, FxHashSet<String>>,
    exported_lines: FxHashMap<PathBuf, FxHashSet<u32>>,
    unused_export_names: FxHashMap<PathBuf, FxHashSet<String>>,
    unused_export_lines: FxHashMap<PathBuf, FxHashSet<u32>>,
    test_referenced_export_names: FxHashMap<PathBuf, FxHashSet<String>>,
    test_referenced_export_lines: FxHashMap<PathBuf, FxHashSet<u32>>,
}

#[derive(Debug, Clone, Deserialize)]
struct SourceMapCacheEntry {
    #[serde(default)]
    url: Option<String>,
    data: serde_json::Value,
    #[serde(default, rename = "lineLengths")]
    line_lengths: Vec<u32>,
}

#[derive(Debug, Clone)]
struct RemappedFunction {
    path: PathBuf,
    name: String,
    decl: Location,
    loc: Location,
    hits: u32,
}

#[derive(Debug, Clone)]
struct AccumulatedFunction {
    entry: FnEntry,
    hits: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct FunctionIdentity {
    name: String,
    decl_start: (u32, u32),
    loc_start: (u32, u32),
    loc_end: (u32, u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LocalPackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageManagerOutput {
    BinaryPath,
    BinDir,
    NodeModulesDir,
}

impl RemappedFunction {
    fn identity(&self) -> FunctionIdentity {
        FunctionIdentity {
            name: self.name.clone(),
            decl_start: (self.decl.start.line, self.decl.start.column),
            loc_start: (self.loc.start.line, self.loc.start.column),
            loc_end: (self.loc.end.line, self.loc.end.column),
        }
    }
}

pub fn prepare_options(
    path: &Path,
    min_invocations_hot: u64,
    min_observation_volume: Option<u32>,
    low_traffic_threshold: Option<f64>,
    output: OutputFormat,
) -> Result<RuntimeCoverageOptions, ExitCode> {
    let jwt = match load_raw_jwt() {
        Ok(Some(jwt)) => jwt,
        Ok(None) => {
            return Ok(RuntimeCoverageOptions {
                path: path.to_path_buf(),
                min_invocations_hot,
                min_observation_volume,
                low_traffic_threshold,
                license_jwt: String::new(),
                watermark: None,
            });
        }
        Err(err) => return Err(emit_error(&format!("license: {err}"), 3, output)),
    };

    let key = match verifying_key() {
        Ok(key) => key,
        Err(message) => return Err(emit_error(&message, 3, output)),
    };
    let status = match load_and_verify(&key, DEFAULT_HARD_FAIL_DAYS) {
        Ok(status) => status,
        Err(err) => return Err(emit_error(&format!("license: {err}"), 3, output)),
    };

    validate_license_status(&status, &key, output)?;

    Ok(RuntimeCoverageOptions {
        path: path.to_path_buf(),
        min_invocations_hot,
        min_observation_volume,
        low_traffic_threshold,
        license_jwt: jwt,
        watermark: if status.show_watermark() {
            Some(RuntimeCoverageWatermark::LicenseExpiredGrace)
        } else {
            None
        },
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "sidecar invocation needs the same filter context as health analysis"
)]
pub(super) fn analyze(
    options: &RuntimeCoverageOptions,
    root: &Path,
    modules: &[fallow_types::extract::ModuleInfo],
    analysis_output: &fallow_core::AnalysisOutput,
    istanbul_coverage: Option<&IstanbulCoverage>,
    file_paths: &FxHashMap<fallow_types::discover::FileId, &PathBuf>,
    ignore_set: &GlobSet,
    changed_files: Option<&FxHashSet<PathBuf>>,
    ws_roots: Option<&[PathBuf]>,
    top: Option<usize>,
    codeowners_path: Option<&str>,
    quiet: bool,
    output: OutputFormat,
) -> Result<RuntimeCoverageReport, ExitCode> {
    let sidecar =
        discover_sidecar(Some(root)).map_err(|message| emit_error(&message, 4, output))?;
    let prepared_sources = prepare_coverage_sources(&options.path)
        .map_err(|message| emit_error(&message, 5, output))?;
    let static_signals = build_static_signal_index(modules, analysis_output, file_paths)
        .map_err(|message| emit_error(&message, 2, output))?;
    let (request, locations) = build_request(
        options,
        root,
        modules,
        analysis_output,
        &static_signals,
        istanbul_coverage,
        file_paths,
        ignore_set,
        changed_files,
        ws_roots,
        prepared_sources.sources,
        codeowners_path,
    );
    let response = run_sidecar(&sidecar, &request, quiet, output)?;
    let mut report = convert_response(response, &locations, options.watermark);
    apply_top_limit(&mut report, top);
    Ok(report)
}

fn validate_license_status(
    status: &LicenseStatus,
    _key: &VerifyingKey,
    output: OutputFormat,
) -> Result<(), ExitCode> {
    match status {
        LicenseStatus::Missing => Err(emit_error(
            "Continuous runtime monitoring requires a valid license or trial. Run: fallow license activate --trial --email you@company.com",
            3,
            output,
        )),
        LicenseStatus::HardFail {
            days_since_expiry, ..
        } => Err(emit_error(
            &format!(
                "license expired {days_since_expiry} days ago. Refresh with: fallow license refresh"
            ),
            3,
            output,
        )),
        _ if !status.permits(&Feature::RuntimeCoverage) => Err(emit_error(
            "License is valid but does not include continuous runtime monitoring. Upgrade at fallow.tools/upgrade.",
            3,
            output,
        )),
        _ => Ok(()),
    }
}

pub fn discover_sidecar(root: Option<&Path>) -> Result<PathBuf, String> {
    // `FALLOW_COV_BIN` is an explicit override: if the user sets it, they
    // expect fallow to either use that path or error. Silently falling
    // through to auto-discovery when the path is missing / not a file
    // contradicts the "explicit beats implicit" contract documented in
    // `.claude/rules/cli-crate.md`.
    if let Some(path) = env_non_empty("FALLOW_COV_BIN") {
        let candidate = PathBuf::from(&path);
        if candidate.is_file() {
            return Ok(candidate);
        }
        return Err(format!(
            "FALLOW_COV_BIN is set to {path} but no file exists there. Unset FALLOW_COV_BIN to fall back to sidecar auto-discovery, or point it at the fallow-cov binary."
        ));
    }

    // `FALLOW_COV_BINARY_PATH` is the air-gap / pre-placed-binary override.
    // Precedes project-local, canonical, and PATH lookup so users in
    // enterprise / Docker / distro-packaged setups can point fallow straight
    // at a specific binary without having it on PATH. Same explicit-beats-
    // implicit semantics as FALLOW_COV_BIN: if it's set and invalid, error.
    if let Some(path) = env_non_empty("FALLOW_COV_BINARY_PATH") {
        let candidate = PathBuf::from(&path);
        if candidate.is_file() {
            return Ok(candidate);
        }
        return Err(format!(
            "FALLOW_COV_BINARY_PATH is set to {path} but no file exists there. Unset FALLOW_COV_BINARY_PATH to fall back to sidecar auto-discovery, or point it at the fallow-cov binary."
        ));
    }

    // Prefer the platform-specific package's real binary over the wrapper at
    // `node_modules/.bin/fallow-cov`. The wrapper is a Node.js script that
    // re-execs the platform binary; its path has no adjacent `.sig` file, so
    // sig verification fails if we point at the wrapper. The real binary
    // lives at `node_modules/@fallow-cli/fallow-cov-<platform>/fallow-cov`
    // with its signature alongside.
    if let Some(root) = root
        && let Some(path) = find_platform_package_sidecar(root)
    {
        return Ok(path);
    }
    if let Some(root) = root
        && let Some(path) = find_project_local_sidecar(root)
    {
        return Ok(path);
    }
    if let Some(root) = root
        && let Some(path) = find_package_manager_sidecar(root)
    {
        return Ok(path);
    }

    let canonical = canonical_sidecar_path();
    if canonical.is_file() {
        return Ok(canonical);
    }

    if let Some(path) = find_on_path("fallow-cov") {
        return Ok(path);
    }

    Err(sidecar_missing_message(root))
}

fn env_non_empty(key: &str) -> Option<String> {
    std::env::var(key).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_owned())
        }
    })
}

pub fn canonical_sidecar_path() -> PathBuf {
    let home = fallow_license::user_home_dir().unwrap_or_else(|| PathBuf::from("."));
    let binary = if cfg!(windows) {
        "fallow-cov.exe"
    } else {
        "fallow-cov"
    };
    home.join(".fallow").join("bin").join(binary)
}

fn find_on_path(binary: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var).find_map(|dir| {
        for candidate_name in path_binary_candidates(binary) {
            let candidate = dir.join(candidate_name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    })
}

fn path_binary_candidates(binary: &str) -> Vec<String> {
    let mut candidates = vec![binary.to_owned()];
    if cfg!(windows) {
        candidates.push(format!("{binary}.exe"));
        candidates.push(format!("{binary}.cmd"));
    }
    candidates
}

fn find_project_local_sidecar(root: &Path) -> Option<PathBuf> {
    for ancestor in root.ancestors() {
        let bin_dir = ancestor.join("node_modules").join(".bin");
        for binary in project_local_sidecar_names() {
            let candidate = bin_dir.join(binary);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// Walks up from `root` looking for a platform-specific `fallow-cov` binary
/// inside a `node_modules/@fallow-cli/fallow-cov-<platform>/` subdirectory.
///
/// After `npm install @fallow-cli/fallow-cov`, npm's `optionalDependencies`
/// plus os/cpu/libc filtering installs exactly one platform subpackage. Its
/// binary is the one with an adjacent `.sig` file, which is required for
/// signature verification before spawning. This lookup prefers that real
/// binary over the Node wrapper at `node_modules/.bin/fallow-cov`.
fn find_platform_package_sidecar(root: &Path) -> Option<PathBuf> {
    let binary_name = sidecar_binary_name();
    for ancestor in root.ancestors() {
        let fallow_cli_dir = ancestor.join("node_modules").join("@fallow-cli");
        if let Some(path) = find_scoped_platform_sidecar(&fallow_cli_dir, binary_name) {
            return Some(path);
        }

        let node_modules = ancestor.join("node_modules");
        for store_dir in [".bun", ".pnpm"] {
            if let Some(path) = find_package_store_platform_sidecar(&node_modules, store_dir) {
                return Some(path);
            }
        }
    }
    None
}

fn find_scoped_platform_sidecar(fallow_cli_dir: &Path, binary_name: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(fallow_cli_dir).ok()?;
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        // Match only `fallow-cov-<platform>` subpackages, not the
        // pure-wrapper `fallow-cov` package.
        if !name_str.starts_with("fallow-cov-") {
            continue;
        }
        let candidate = entry.path().join(binary_name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn find_package_store_platform_sidecar(node_modules: &Path, store_dir: &str) -> Option<PathBuf> {
    let binary_name = sidecar_binary_name();
    let store = node_modules.join(store_dir);
    let entries = fs::read_dir(&store).ok()?;
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !name_str.starts_with("@fallow-cli+fallow-cov-") {
            continue;
        }

        let scoped_dir = entry.path().join("node_modules").join("@fallow-cli");
        if let Some(path) = find_scoped_platform_sidecar(&scoped_dir, binary_name) {
            candidates.push((sidecar_package_version_key(&path), path));
        }
    }
    candidates.sort_by(|(left_version, left_path), (right_version, right_path)| {
        right_version
            .cmp(left_version)
            .then_with(|| left_path.cmp(right_path))
    });
    candidates.into_iter().next().map(|(_, path)| path)
}

fn sidecar_package_version_key(binary: &Path) -> Vec<u64> {
    let Some(package_dir) = binary.parent() else {
        return Vec::new();
    };
    let Ok(contents) = fs::read_to_string(package_dir.join("package.json")) else {
        return Vec::new();
    };
    let Ok(package_json) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return Vec::new();
    };
    package_json
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(parse_sidecar_version_key)
        .unwrap_or_default()
}

fn parse_sidecar_version_key(version: &str) -> Vec<u64> {
    version
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

const fn sidecar_binary_name() -> &'static str {
    if cfg!(windows) {
        "fallow-cov.exe"
    } else {
        "fallow-cov"
    }
}

fn find_package_manager_sidecar(root: &Path) -> Option<PathBuf> {
    detect_package_manager(root).and_then(|package_manager| package_manager.resolve_sidecar(root))
}

fn detect_package_manager(root: &Path) -> Option<LocalPackageManager> {
    detect_package_manager_from_field(root).or_else(|| {
        if root.join("bun.lockb").exists() || root.join("bun.lock").exists() {
            Some(LocalPackageManager::Bun)
        } else if root.join("pnpm-lock.yaml").exists() {
            Some(LocalPackageManager::Pnpm)
        } else if root.join("yarn.lock").exists() {
            Some(LocalPackageManager::Yarn)
        } else if root.join("package-lock.json").exists()
            || root.join("npm-shrinkwrap.json").exists()
        {
            Some(LocalPackageManager::Npm)
        } else {
            None
        }
    })
}

fn detect_package_manager_from_field(root: &Path) -> Option<LocalPackageManager> {
    let content = fs::read_to_string(root.join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    let field = value.get("packageManager")?.as_str()?;
    let name = field.split('@').next().unwrap_or(field);
    match name {
        "npm" => Some(LocalPackageManager::Npm),
        "pnpm" => Some(LocalPackageManager::Pnpm),
        "yarn" => Some(LocalPackageManager::Yarn),
        "bun" => Some(LocalPackageManager::Bun),
        _ => None,
    }
}

impl LocalPackageManager {
    const fn install_command(self) -> &'static str {
        match self {
            Self::Npm => "npm install --save-dev @fallow-cli/fallow-cov",
            Self::Pnpm => "pnpm add -D @fallow-cli/fallow-cov",
            Self::Yarn => "yarn add -D @fallow-cli/fallow-cov",
            Self::Bun => "bun add -d @fallow-cli/fallow-cov",
        }
    }

    fn resolve_sidecar(self, root: &Path) -> Option<PathBuf> {
        match self {
            Self::Npm => resolve_sidecar_via_command(
                root,
                OsStr::new("npm"),
                &["root"],
                PackageManagerOutput::NodeModulesDir,
            ),
            Self::Pnpm => resolve_sidecar_via_command(
                root,
                OsStr::new("pnpm"),
                &["bin"],
                PackageManagerOutput::BinDir,
            ),
            Self::Yarn => resolve_sidecar_via_command(
                root,
                OsStr::new("yarn"),
                &["bin", "fallow-cov"],
                PackageManagerOutput::BinaryPath,
            ),
            Self::Bun => resolve_sidecar_via_command(
                root,
                OsStr::new("bun"),
                &["pm", "bin"],
                PackageManagerOutput::BinDir,
            ),
        }
    }
}

fn resolve_sidecar_via_command(
    root: &Path,
    program: &OsStr,
    args: &[&str],
    output_kind: PackageManagerOutput,
) -> Option<PathBuf> {
    let output = Command::new(program)
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8(output.stdout).ok()?;
    let candidate = stdout
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())?;

    match output_kind {
        PackageManagerOutput::BinaryPath => {
            let path = normalize_package_manager_path(root, candidate);
            path.is_file().then_some(path)
        }
        PackageManagerOutput::BinDir => {
            let dir = normalize_package_manager_path(root, candidate);
            project_local_sidecar_names()
                .iter()
                .map(|binary| dir.join(binary))
                .find(|candidate| candidate.is_file())
        }
        PackageManagerOutput::NodeModulesDir => {
            let dir = normalize_package_manager_path(root, candidate).join(".bin");
            project_local_sidecar_names()
                .iter()
                .map(|binary| dir.join(binary))
                .find(|candidate| candidate.is_file())
        }
    }
}

fn normalize_package_manager_path(root: &Path, candidate: &str) -> PathBuf {
    let path = PathBuf::from(candidate);
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn project_local_sidecar_names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["fallow-cov.cmd", "fallow-cov.exe", "fallow-cov"]
    } else {
        &["fallow-cov"]
    }
}

fn sidecar_missing_message(root: Option<&Path>) -> String {
    let mut checks = vec![
        canonical_sidecar_path().display().to_string(),
        "PATH".to_owned(),
    ];
    let mut install_example = "npm install --save-dev @fallow-cli/fallow-cov".to_owned();
    if let Some(root) = root {
        checks.insert(
            0,
            root.join("node_modules/.bin/fallow-cov")
                .display()
                .to_string(),
        );
        if let Some(package_manager) = detect_package_manager(root) {
            checks.insert(1, package_manager.lookup_hint().to_owned());
            package_manager
                .install_command()
                .clone_into(&mut install_example);
        }
    }
    format!(
        "Sidecar binary fallow-cov not found. Checked {}. Install with your package manager (for example `{install_example}`) or set FALLOW_COV_BIN.",
        checks.join(", "),
    )
}

impl LocalPackageManager {
    const fn lookup_hint(self) -> &'static str {
        match self {
            Self::Npm => "`npm root` + `.bin/fallow-cov`",
            Self::Pnpm => "`pnpm bin`",
            Self::Yarn => "`yarn bin fallow-cov`",
            Self::Bun => "`bun pm bin`",
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "request assembly mirrors the health analysis filter context plus prepared coverage inputs"
)]
fn build_request(
    options: &RuntimeCoverageOptions,
    root: &Path,
    modules: &[fallow_types::extract::ModuleInfo],
    analysis_output: &fallow_core::AnalysisOutput,
    static_signals: &StaticSignalIndex,
    istanbul_coverage: Option<&IstanbulCoverage>,
    file_paths: &FxHashMap<fallow_types::discover::FileId, &PathBuf>,
    ignore_set: &GlobSet,
    changed_files: Option<&FxHashSet<PathBuf>>,
    ws_roots: Option<&[PathBuf]>,
    coverage_sources: Vec<CoverageSource>,
    codeowners_path: Option<&str>,
) -> (Request, FunctionLocations) {
    // Sidecar expects a single project_root for path relativization. When a
    // single workspace is scoped, use it; otherwise fall back to the repo root
    // so multi-workspace runs stay unambiguous.
    let project_root = match ws_roots {
        Some([only]) => only.as_path(),
        _ => root,
    };
    let mut files = Vec::new();
    let mut locations = FxHashMap::default();
    let graph = analysis_output.graph.as_ref();
    let codeowners = crate::codeowners::CodeOwners::load(root, codeowners_path).ok();
    for module in modules {
        let Some(&path) = file_paths.get(&module.file_id) else {
            continue;
        };
        let canonical_path =
            istanbul_coverage.map(|_| dunce::canonicalize(path).unwrap_or_else(|_| path.clone()));
        let relative = path.strip_prefix(root).unwrap_or(path);
        let caller_count = graph
            .and_then(|g| g.reverse_deps.get(module.file_id.0 as usize))
            .map_or(0_usize, Vec::len);
        let caller_count = u32::try_from(caller_count).unwrap_or(u32::MAX);
        let owner_count = codeowners
            .as_ref()
            .map(|co| co.owner_count_of(relative).unwrap_or(0));
        if ignore_set.is_match(relative) {
            continue;
        }
        if let Some(changed) = changed_files
            && !changed.contains(path.as_path())
        {
            continue;
        }
        if let Some(ws) = ws_roots
            && !ws.iter().any(|r| path.starts_with(r))
        {
            continue;
        }
        if module.complexity.is_empty() {
            continue;
        }
        let functions = module
            .complexity
            .iter()
            .map(|function| {
                mark_ambiguous_function_line(&mut locations, path, &function.name, function.line);
                let static_used = function_static_used(path, function, static_signals);
                let test_covered = function_test_covered(
                    path,
                    canonical_path.as_deref(),
                    function,
                    static_signals,
                    istanbul_coverage,
                );
                StaticFunction {
                    name: function.name.clone(),
                    start_line: function.line,
                    end_line: function.line.saturating_add(function.line_count),
                    cyclomatic: u32::from(function.cyclomatic),
                    // Export-level dead-code signals are reliable enough to mark
                    // unreferenced exports as statically unused. Internal-only
                    // functions still default to `true` until fallow grows an
                    // intra-file call graph; that avoids false `safe_to_delete`
                    // verdicts when a private helper is only called locally.
                    static_used,
                    // Join real test evidence when available: Istanbul per-function
                    // hits first, then direct test-reachable export references as a
                    // conservative fallback. We intentionally do not infer "covered"
                    // for every function in a test-reachable file.
                    test_covered,
                    caller_count,
                    owner_count,
                }
            })
            .collect();
        files.push(StaticFile {
            path: relative.to_string_lossy().into_owned(),
            functions,
        });
    }
    (
        Request {
            protocol_version: PROTOCOL_VERSION.to_owned(),
            license: fallow_cov_protocol::License {
                jwt: options.license_jwt.clone(),
            },
            project_root: project_root.to_string_lossy().into_owned(),
            coverage_sources,
            static_findings: StaticFindings { files },
            options: fallow_cov_protocol::Options {
                include_hot_paths: true,
                min_invocations_for_hot: Some(options.min_invocations_hot),
                min_observation_volume: options.min_observation_volume,
                low_traffic_threshold: options.low_traffic_threshold,
                // Trace count, period, and deployments come from the beacon side in
                // Phase 3. Phase 2 reads a single coverage dump — the sidecar falls
                // back to summing observed invocations when `trace_count` is None.
                trace_count: None,
                period_days: None,
                deployments_seen: None,
                // Window/instance hints feed `CaptureQuality` on the sidecar.
                // In Phase 2 single-dump local mode all four of trace_count,
                // period_days, deployments_seen, window_seconds, and
                // instances_observed are None; the sidecar derives
                // `CaptureQuality.instances_observed` from the count of
                // distinct deployments it sees in the dump itself.
                // Populated by the beacon transport in Phase 3.
                window_seconds: None,
                instances_observed: None,
            },
        },
        locations,
    )
}

fn build_static_signal_index(
    modules: &[fallow_types::extract::ModuleInfo],
    analysis_output: &fallow_core::AnalysisOutput,
    file_paths: &FxHashMap<fallow_types::discover::FileId, &PathBuf>,
) -> Result<StaticSignalIndex, String> {
    let graph = analysis_output
        .graph
        .as_ref()
        .ok_or_else(|| "analysis graph not available for runtime coverage".to_owned())?;
    let mut index = StaticSignalIndex::default();

    for file in &analysis_output.results.unused_files {
        index.unused_files.insert(file.path.clone());
    }
    for export in &analysis_output.results.unused_exports {
        index
            .unused_export_names
            .entry(export.path.clone())
            .or_default()
            .insert(export.export_name.clone());
        index
            .unused_export_lines
            .entry(export.path.clone())
            .or_default()
            .insert(export.line);
    }

    let module_by_id: FxHashMap<_, _> = modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();
    for node in &graph.modules {
        let Some(&path) = file_paths.get(&node.file_id) else {
            continue;
        };
        let module = module_by_id.get(&node.file_id);
        for export in &node.exports {
            if export.is_type_only {
                continue;
            }

            index
                .exported_names
                .entry(path.clone())
                .or_default()
                .insert(export.name.to_string());

            if let Some(module) = module {
                let (line, _) = fallow_types::extract::byte_offset_to_line_col(
                    &module.line_offsets,
                    export.span.start,
                );
                index
                    .exported_lines
                    .entry(path.clone())
                    .or_default()
                    .insert(line);

                let has_test_ref = export.references.iter().any(|reference| {
                    graph
                        .modules
                        .get(reference.from_file.0 as usize)
                        .is_some_and(fallow_core::graph::ModuleNode::is_test_reachable)
                });
                if has_test_ref {
                    index
                        .test_referenced_export_names
                        .entry(path.clone())
                        .or_default()
                        .insert(export.name.to_string());
                    index
                        .test_referenced_export_lines
                        .entry(path.clone())
                        .or_default()
                        .insert(line);
                }
            }
        }
    }

    Ok(index)
}

fn function_static_used(
    path: &Path,
    function: &fallow_types::extract::FunctionComplexity,
    static_signals: &StaticSignalIndex,
) -> bool {
    if static_signals.unused_files.contains(path) {
        return false;
    }
    if !function_matches_export(path, function, static_signals) {
        return true;
    }
    !static_signals
        .unused_export_names
        .get(path)
        .is_some_and(|names| names.contains(function.name.as_str()))
        && !static_signals
            .unused_export_lines
            .get(path)
            .is_some_and(|lines| lines.contains(&function.line))
}

fn function_test_covered(
    path: &Path,
    canonical_path: Option<&Path>,
    function: &fallow_types::extract::FunctionComplexity,
    static_signals: &StaticSignalIndex,
    istanbul_coverage: Option<&IstanbulCoverage>,
) -> bool {
    if let Some(coverage) = istanbul_coverage
        && let Some(canonical_path) = canonical_path
        && let Some(coverage_pct) = coverage
            .get(canonical_path)
            .and_then(|file| file.lookup(function.name.as_str(), function.line, function.col))
    {
        return coverage_pct > 0.0;
    }

    static_signals
        .test_referenced_export_names
        .get(path)
        .is_some_and(|names| names.contains(function.name.as_str()))
        || static_signals
            .test_referenced_export_lines
            .get(path)
            .is_some_and(|lines| lines.contains(&function.line))
}

fn function_matches_export(
    path: &Path,
    function: &fallow_types::extract::FunctionComplexity,
    static_signals: &StaticSignalIndex,
) -> bool {
    static_signals
        .exported_names
        .get(path)
        .is_some_and(|names| names.contains(function.name.as_str()))
        || static_signals
            .exported_lines
            .get(path)
            .is_some_and(|lines| lines.contains(&function.line))
}

fn mark_ambiguous_function_line(
    locations: &mut FunctionLocations,
    path: &Path,
    function_name: &str,
    line: u32,
) {
    let key = (
        path.to_string_lossy().into_owned(),
        function_name.to_owned(),
    );
    match locations.entry(key) {
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            if entry.get().is_some_and(|existing| existing != line) {
                entry.insert(None);
            }
        }
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(Some(line));
        }
    }
}

fn prepare_coverage_sources(path: &Path) -> Result<PreparedCoverageSources, String> {
    let mut temp_dir = None;
    if !path.is_dir() {
        let mut sources = Vec::new();
        prepare_single_coverage_source(path, &mut sources, &mut temp_dir, 0)?;
        return Ok(PreparedCoverageSources {
            sources,
            _temp_dir: temp_dir,
        });
    }

    let entries = fs::read_dir(path).map_err(|err| {
        format!(
            "failed to read coverage directory {}: {err}",
            path.display()
        )
    })?;
    let mut json_files = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|entry| entry.is_file() && entry.extension() == Some(OsStr::new("json")))
        .collect::<Vec<_>>();
    json_files.sort();

    if json_files.is_empty() {
        return Ok(PreparedCoverageSources {
            sources: vec![CoverageSource::V8Dir {
                path: path.to_string_lossy().into_owned(),
            }],
            _temp_dir: None,
        });
    }

    let mut sources = Vec::with_capacity(json_files.len());
    for (index, file) in json_files.iter().enumerate() {
        prepare_single_coverage_source(file, &mut sources, &mut temp_dir, index)?;
    }

    Ok(PreparedCoverageSources {
        sources,
        _temp_dir: temp_dir,
    })
}

fn prepare_single_coverage_source(
    path: &Path,
    sources: &mut Vec<CoverageSource>,
    temp_dir: &mut Option<TempDir>,
    index: usize,
) -> Result<(), String> {
    if looks_like_istanbul(path) {
        sources.push(CoverageSource::Istanbul {
            path: path.to_string_lossy().into_owned(),
        });
        return Ok(());
    }

    let Some((remapped_path, residual_path)) = preprocess_v8_coverage_file(path, temp_dir, index)?
    else {
        sources.push(CoverageSource::V8 {
            path: path.to_string_lossy().into_owned(),
        });
        return Ok(());
    };

    sources.push(CoverageSource::Istanbul {
        path: remapped_path.to_string_lossy().into_owned(),
    });
    if let Some(residual_path) = residual_path {
        sources.push(CoverageSource::V8 {
            path: residual_path.to_string_lossy().into_owned(),
        });
    }

    Ok(())
}

fn preprocess_v8_coverage_file(
    path: &Path,
    temp_dir: &mut Option<TempDir>,
    index: usize,
) -> Result<Option<(PathBuf, Option<PathBuf>)>, String> {
    let json = fs::read_to_string(path)
        .map_err(|err| format!("failed to read coverage file {}: {err}", path.display()))?;
    let dump: V8CoverageDump = serde_json::from_str(&json)
        .map_err(|err| format!("failed to parse v8 coverage file {}: {err}", path.display()))?;
    let Some(cache) = parse_source_map_cache(&dump) else {
        return Ok(None);
    };

    let mut remapped_files: BTreeMap<PathBuf, BTreeMap<FunctionIdentity, AccumulatedFunction>> =
        BTreeMap::new();
    let mut residual_scripts = Vec::new();

    for script in dump.result {
        let Some(entry) = cache.get(&script.url) else {
            residual_scripts.push(script);
            continue;
        };
        let Some(mapped) = remap_script_with_source_map(&script, entry) else {
            residual_scripts.push(script);
            continue;
        };
        merge_remapped_functions(&mut remapped_files, mapped);
    }

    if remapped_files.is_empty() {
        return Ok(None);
    }

    let temp_root = ensure_temp_dir(temp_dir)?;
    let remapped_path = temp_root.join(format!("coverage-remapped-{index}.json"));
    write_istanbul_coverage_file(&remapped_path, &remapped_files)?;

    let residual_path = if residual_scripts.is_empty() {
        None
    } else {
        let residual_path = temp_root.join(format!("coverage-residual-{index}.json"));
        let residual_dump = V8CoverageDump {
            result: residual_scripts,
            source_map_cache: None,
        };
        fs::write(
            &residual_path,
            serde_json::to_vec(&residual_dump).map_err(|err| {
                format!(
                    "failed to serialize residual v8 coverage {}: {err}",
                    residual_path.display()
                )
            })?,
        )
        .map_err(|err| {
            format!(
                "failed to write residual v8 coverage {}: {err}",
                residual_path.display()
            )
        })?;
        Some(residual_path)
    };

    Ok(Some((remapped_path, residual_path)))
}

fn parse_source_map_cache(dump: &V8CoverageDump) -> Option<BTreeMap<String, SourceMapCacheEntry>> {
    let raw = dump.source_map_cache.clone()?;
    serde_json::from_value(raw).ok()
}

fn ensure_temp_dir(temp_dir: &mut Option<TempDir>) -> Result<&Path, String> {
    if temp_dir.is_none() {
        *temp_dir = Some(
            tempfile::tempdir()
                .map_err(|err| format!("failed to create remapped coverage tempdir: {err}"))?,
        );
    }
    Ok(temp_dir
        .as_ref()
        .expect("temp dir is always initialized above")
        .path())
}

fn remap_script_with_source_map(
    script: &fallow_v8_coverage::ScriptCoverage,
    entry: &SourceMapCacheEntry,
) -> Option<Vec<RemappedFunction>> {
    let sourcemap = SourceMap::from_json(&entry.data.to_string()).ok()?;
    let offsets = line_offsets_for_script(script, entry)?;
    let mut remapped = Vec::new();

    for function in &script.functions {
        let mapped = remap_function(script, function, entry, &sourcemap, &offsets)?;
        remapped.push(mapped);
    }

    (!remapped.is_empty()).then_some(remapped)
}

fn line_offsets_for_script(
    script: &fallow_v8_coverage::ScriptCoverage,
    entry: &SourceMapCacheEntry,
) -> Option<Vec<u32>> {
    if let Some(path) = file_url_to_path(&script.url)
        && let Ok(source) = fs::read_to_string(path)
    {
        return Some(build_line_offsets_from_source(&source));
    }
    build_line_offsets_from_lengths(&entry.line_lengths)
}

fn build_line_offsets_from_source(source: &str) -> Vec<u32> {
    let mut line_starts = Vec::with_capacity(source.lines().count().saturating_add(1));
    line_starts.push(0);
    let bytes = source.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\n' => {
                line_starts.push((i + 1) as u32);
                i += 1;
            }
            b'\r' => {
                let next = if bytes.get(i + 1) == Some(&b'\n') {
                    i + 2
                } else {
                    i + 1
                };
                line_starts.push(next as u32);
                i = next;
            }
            _ => i += 1,
        }
    }
    line_starts
}

fn build_line_offsets_from_lengths(line_lengths: &[u32]) -> Option<Vec<u32>> {
    if line_lengths.is_empty() {
        return None;
    }
    let mut line_starts = Vec::with_capacity(line_lengths.len());
    line_starts.push(0);
    let mut offset = 0u32;
    for length in line_lengths
        .iter()
        .take(line_lengths.len().saturating_sub(1))
    {
        offset = offset.saturating_add(*length).saturating_add(1);
        line_starts.push(offset);
    }
    Some(line_starts)
}

fn remap_function(
    script: &fallow_v8_coverage::ScriptCoverage,
    function: &fallow_v8_coverage::FunctionCoverage,
    entry: &SourceMapCacheEntry,
    sourcemap: &SourceMap,
    line_offsets: &[u32],
) -> Option<RemappedFunction> {
    let outer = function.ranges.first().copied()?;
    let start = offset_to_position(line_offsets, outer.start_offset);
    let end = offset_to_position(line_offsets, outer.end_offset);
    let start_lookup =
        sourcemap.original_position_for(start.line.saturating_sub(1), start.column)?;
    let resolved_path = resolve_original_source_path(
        sourcemap.source(start_lookup.source),
        &script.url,
        entry.url.as_deref(),
    )?;
    let canonical_path = dunce::canonicalize(&resolved_path).unwrap_or(resolved_path);
    let end_lookup = sourcemap
        .original_position_for(end.line.saturating_sub(1), end.column)
        .filter(|lookup| lookup.source == start_lookup.source);
    let end_line = end_lookup
        .as_ref()
        .map_or(start_lookup.line, |lookup| lookup.line)
        .saturating_add(1);
    let end_column = end_lookup
        .as_ref()
        .map_or(start_lookup.column, |lookup| lookup.column);
    let name = start_lookup
        .name
        .map(|index| sourcemap.name(index).to_owned())
        .filter(|name| !name.is_empty())
        .or_else(|| (!function.function_name.is_empty()).then_some(function.function_name.clone()))
        .unwrap_or_else(|| "(anonymous)".to_owned());

    Some(RemappedFunction {
        path: canonical_path,
        name,
        decl: Location {
            start: Position {
                line: start_lookup.line.saturating_add(1),
                column: start_lookup.column,
            },
            end: Position {
                line: start_lookup.line.saturating_add(1),
                column: start_lookup.column,
            },
        },
        loc: Location {
            start: Position {
                line: start_lookup.line.saturating_add(1),
                column: start_lookup.column,
            },
            end: Position {
                line: end_line,
                column: end_column,
            },
        },
        hits: outer.count.min(u64::from(u32::MAX)) as u32,
    })
}

fn offset_to_position(line_offsets: &[u32], byte_offset: u32) -> Position {
    let line_index = match line_offsets.binary_search(&byte_offset) {
        Ok(exact) => exact,
        Err(insertion_point) => insertion_point.saturating_sub(1),
    };
    let line_start = line_offsets[line_index];
    Position {
        line: line_index as u32 + 1,
        column: byte_offset.saturating_sub(line_start),
    }
}

fn resolve_original_source_path(
    raw_source: &str,
    generated_url: &str,
    source_map_url: Option<&str>,
) -> Option<PathBuf> {
    if raw_source.is_empty() {
        return None;
    }
    if let Some(path) = file_url_to_path(raw_source) {
        return Some(path);
    }
    let source_path = PathBuf::from(raw_source);
    if source_path.is_absolute() || looks_like_windows_absolute_path(raw_source) {
        return Some(source_path);
    }
    if Url::parse(raw_source).is_ok() {
        let base_dir = resolve_source_map_base(generated_url, source_map_url)?;
        return resolve_virtual_source_path(raw_source, &base_dir);
    }
    let base_dir = resolve_source_map_base(generated_url, source_map_url)?;
    Some(base_dir.join(source_path))
}

fn resolve_source_map_base(generated_url: &str, source_map_url: Option<&str>) -> Option<PathBuf> {
    let generated_path = file_url_to_path(generated_url)?;
    let generated_dir = generated_path.parent()?.to_path_buf();
    let Some(source_map_url) = source_map_url.filter(|url| !url.is_empty()) else {
        return Some(generated_dir);
    };
    if let Some(path) = file_url_to_path(source_map_url) {
        return path.parent().map(Path::to_path_buf);
    }
    let candidate = PathBuf::from(source_map_url);
    if candidate.is_absolute() {
        return candidate.parent().map(Path::to_path_buf);
    }
    if Url::parse(source_map_url).is_ok() {
        return None;
    }
    generated_dir
        .join(candidate)
        .parent()
        .map(Path::to_path_buf)
}

fn file_url_to_path(value: &str) -> Option<PathBuf> {
    if let Ok(url) = Url::parse(value) {
        return if url.scheme() == "file" {
            url.to_file_path().ok()
        } else {
            None
        };
    }
    let path = PathBuf::from(value);
    (path.is_absolute() || looks_like_windows_absolute_path(value)).then_some(path)
}

fn looks_like_windows_absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\')
}

fn resolve_virtual_source_path(value: &str, base_dir: &Path) -> Option<PathBuf> {
    let url = Url::parse(value).ok()?;
    match url.scheme() {
        "webpack" | "vite" => {
            let candidates = virtual_source_candidates(&url);
            resolve_virtual_candidate(&candidates, base_dir)
        }
        _ => None,
    }
}

fn virtual_source_candidates(url: &Url) -> Vec<PathBuf> {
    let path = url.path().trim_start_matches('/');
    let mut candidates = Vec::new();

    if let Some(host) = url.host_str() {
        let host = host.trim_matches('/');
        if !host.is_empty() && !matches!(host, "." | "_N_E") {
            let combined = PathBuf::from(host).join(path);
            if !combined.as_os_str().is_empty() {
                candidates.push(combined);
            }
        }
    }

    if !path.is_empty() {
        candidates.push(PathBuf::from(path));
    }

    candidates.retain(|candidate| !candidate.as_os_str().is_empty());
    candidates.dedup();
    candidates
}

fn resolve_virtual_candidate(candidates: &[PathBuf], base_dir: &Path) -> Option<PathBuf> {
    for base in base_dir.ancestors() {
        for candidate in candidates {
            let resolved = base.join(candidate);
            if resolved.is_file() {
                return Some(resolved);
            }
        }
    }
    None
}

fn merge_remapped_functions(
    target: &mut BTreeMap<PathBuf, BTreeMap<FunctionIdentity, AccumulatedFunction>>,
    functions: Vec<RemappedFunction>,
) {
    for function in functions {
        let identity = function.identity();
        let file = target.entry(function.path).or_default();
        let entry = file.entry(identity).or_insert_with(|| AccumulatedFunction {
            entry: FnEntry {
                name: function.name.clone(),
                line: function.decl.start.line,
                decl: function.decl.clone(),
                loc: function.loc.clone(),
            },
            hits: 0,
        });
        entry.hits = entry.hits.saturating_add(function.hits);
        if location_precedes(&function.loc.start, &entry.entry.loc.start) {
            entry.entry.loc.start = function.loc.start.clone();
        }
        if location_precedes(&entry.entry.loc.end, &function.loc.end) {
            entry.entry.loc.end = function.loc.end.clone();
        }
    }
}

fn location_precedes(left: &Position, right: &Position) -> bool {
    left.line < right.line || (left.line == right.line && left.column < right.column)
}

fn write_istanbul_coverage_file(
    output_path: &Path,
    files: &BTreeMap<PathBuf, BTreeMap<FunctionIdentity, AccumulatedFunction>>,
) -> Result<(), String> {
    let mut root = BTreeMap::new();
    for (path, functions) in files {
        let mut fn_map = BTreeMap::new();
        let mut f = BTreeMap::new();
        for (index, function) in functions.values().enumerate() {
            let id = index.to_string();
            fn_map.insert(id.clone(), function.entry.clone());
            f.insert(id, function.hits);
        }
        root.insert(
            path.to_string_lossy().into_owned(),
            FileCoverage {
                path: path.to_string_lossy().into_owned(),
                statement_map: BTreeMap::new(),
                fn_map,
                branch_map: BTreeMap::new(),
                s: BTreeMap::new(),
                f,
                b: BTreeMap::new(),
                b_t: None,
                input_source_map: None,
            },
        );
    }

    let bytes = serde_json::to_vec(&root).map_err(|err| {
        format!(
            "failed to serialize remapped istanbul coverage {}: {err}",
            output_path.display()
        )
    })?;
    fs::write(output_path, bytes).map_err(|err| {
        format!(
            "failed to write remapped istanbul coverage {}: {err}",
            output_path.display()
        )
    })
}

fn looks_like_istanbul(path: &Path) -> bool {
    if let Ok(json) = fs::read_to_string(path)
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&json)
    {
        return is_istanbul_coverage_json(&value);
    }

    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name == "coverage-final.json")
}

fn is_istanbul_coverage_json(value: &serde_json::Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };

    if object
        .get("result")
        .is_some_and(serde_json::Value::is_array)
    {
        return false;
    }

    if object.is_empty() {
        return true;
    }

    object.values().any(|entry| {
        let Some(entry) = entry.as_object() else {
            return false;
        };
        ["path", "statementMap", "fnMap", "branchMap", "s", "f", "b"]
            .into_iter()
            .all(|key| entry.contains_key(key))
    })
}

/// Verify the Ed25519 signature of the resolved sidecar binary against
/// `BINARY_SIGNING_VERIFY_KEY`. Runs on every spawn so file-system tampering
/// between install and spawn cannot substitute a malicious binary.
///
/// Strict by design: missing or invalid `.sig` file, wrong signature length,
/// and verification failure all fail hard (exit 4). No warn-and-run fallback.
/// Phase 2.5 ships to no existing users, so there is no install-base on the
/// old unsigned path to accommodate.
fn verify_sidecar_signature(binary: &Path) -> Result<(), String> {
    let sig_path = {
        let mut path = binary.as_os_str().to_os_string();
        path.push(".sig");
        PathBuf::from(path)
    };

    let sig_bytes = fs::read(&sig_path).map_err(|err| {
        format!(
            "Sidecar binary at {} is missing its signature file {}: {err}. The fallow CLI refuses to spawn an unsigned sidecar. Reinstall @fallow-cli/fallow-cov.",
            binary.display(),
            sig_path.display()
        )
    })?;
    let sig_array: [u8; 64] = sig_bytes.as_slice().try_into().map_err(|_| {
        format!(
            "Sidecar signature file at {} is {} bytes; expected 64. Reinstall @fallow-cli/fallow-cov.",
            sig_path.display(),
            sig_bytes.len()
        )
    })?;
    let signature = Signature::from_bytes(&sig_array);

    let key = VerifyingKey::from_bytes(&BINARY_SIGNING_VERIFY_KEY).map_err(|err| {
        format!("compiled-in binary-signing key is invalid: {err} (build-time bug)")
    })?;

    let binary_bytes = fs::read(binary).map_err(|err| {
        format!(
            "failed to read sidecar binary at {} for signature verification: {err}",
            binary.display()
        )
    })?;

    key.verify(&binary_bytes, &signature).map_err(|err| {
        format!(
            "Sidecar binary at {} failed Ed25519 signature verification: {err}. The .sig file does not match the fallow CLI's compiled-in binary-signing public key. Reinstall @fallow-cli/fallow-cov from npm, or if you are building from a pre-release fallow source, rebuild against the published fallow release.",
            binary.display()
        )
    })?;

    Ok(())
}

fn run_sidecar(
    sidecar: &Path,
    request: &Request,
    quiet: bool,
    output: OutputFormat,
) -> Result<Response, ExitCode> {
    verify_sidecar_signature(sidecar).map_err(|message| emit_error(&message, 4, output))?;

    let mut child = Command::new(sidecar)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            emit_error(
                &format!("failed to spawn {}: {err}", sidecar.display()),
                4,
                output,
            )
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(err) = serde_json::to_writer(&mut stdin, request) {
            return Err(emit_error(
                &format!("failed to serialize sidecar request: {err}"),
                4,
                output,
            ));
        }
        if let Err(err) = stdin.flush() {
            return Err(emit_error(
                &format!("failed to flush sidecar request: {err}"),
                4,
                output,
            ));
        }
    }

    let output_data = child
        .wait_with_output()
        .map_err(|err| emit_error(&format!("failed to wait for sidecar: {err}"), 4, output))?;

    if !output_data.stderr.is_empty() && !quiet {
        let stderr = String::from_utf8_lossy(&output_data.stderr);
        eprint!("{stderr}");
    }

    match output_data.status.code() {
        Some(0) => {}
        Some(4) => {
            return Err(emit_error(
                &stderr_message(&output_data.stderr, "sidecar protocol mismatch"),
                4,
                output,
            ));
        }
        Some(5) => {
            return Err(emit_error(
                &stderr_message(
                    &output_data.stderr,
                    "failed to parse runtime coverage input",
                ),
                5,
                output,
            ));
        }
        Some(6) => {
            return Err(emit_error(
                &stderr_message(&output_data.stderr, "sidecar internal error"),
                6,
                output,
            ));
        }
        Some(code) => {
            return Err(emit_error(
                &stderr_message(&output_data.stderr, "sidecar execution failed"),
                u8::try_from(code).unwrap_or(4),
                output,
            ));
        }
        None => {
            return Err(emit_error("sidecar terminated by signal", 4, output));
        }
    }

    let response: Response = serde_json::from_slice(&output_data.stdout).map_err(|err| {
        emit_error(
            &format!("failed to parse sidecar response: {err}"),
            4,
            output,
        )
    })?;

    let supported_major = PROTOCOL_VERSION.split('.').next().unwrap_or("0");
    let response_major = response.protocol_version.split('.').next().unwrap_or("0");
    if response_major != supported_major {
        let message = if response_major > supported_major {
            format!(
                "sidecar emits protocol v{}; this fallow supports up to v{}. Upgrade fallow.",
                response.protocol_version, PROTOCOL_VERSION
            )
        } else {
            format!(
                "sidecar emits protocol v{}; this fallow requires v{}+. Upgrade @fallow-cli/fallow-cov.",
                response.protocol_version, PROTOCOL_VERSION
            )
        };
        return Err(emit_error(&message, 4, output));
    }

    Ok(response)
}

fn stderr_message(stderr: &[u8], fallback: &str) -> String {
    let message = String::from_utf8_lossy(stderr).trim().to_owned();
    if message.is_empty() {
        fallback.to_owned()
    } else {
        message
    }
}

fn convert_response(
    response: Response,
    _locations: &FunctionLocations,
    watermark: Option<RuntimeCoverageWatermark>,
) -> RuntimeCoverageReport {
    let mut findings = response
        .findings
        .into_iter()
        .filter_map(|finding| {
            let verdict = map_verdict(finding.verdict);
            if matches!(verdict, RuntimeCoverageVerdict::Active) {
                return None;
            }
            Some(RuntimeCoverageFinding {
                id: finding.id,
                path: PathBuf::from(finding.file),
                function: finding.function,
                line: finding.line,
                verdict,
                invocations: finding.invocations,
                confidence: map_confidence(finding.confidence),
                evidence: map_evidence(finding.evidence),
                actions: finding
                    .actions
                    .into_iter()
                    .map(|action| RuntimeCoverageAction {
                        kind: action.kind,
                        description: action.description,
                        auto_fixable: action.auto_fixable,
                    })
                    .collect(),
            })
        })
        .collect::<Vec<_>>();

    findings.sort_by(|left, right| {
        verdict_rank(left.verdict)
            .cmp(&verdict_rank(right.verdict))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.function.cmp(&right.function))
    });

    let mut hot_paths = response
        .hot_paths
        .into_iter()
        .map(|entry| RuntimeCoverageHotPath {
            id: entry.id,
            path: PathBuf::from(entry.file),
            function: entry.function,
            line: entry.line,
            invocations: entry.invocations,
            percentile: entry.percentile,
            // Actions on hot paths are reserved for future protocol versions
            // (e.g., a "review-on-change" suggestion). The sidecar protocol
            // at 0.2 does not emit per-hot-path actions, so leave empty.
            actions: Vec::new(),
        })
        .collect::<Vec<_>>();
    hot_paths.sort_by(|left, right| {
        right
            .invocations
            .cmp(&left.invocations)
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.function.cmp(&right.function))
    });

    let mut blast_radius = response
        .blast_radius
        .into_iter()
        .map(
            |entry| crate::health_types::RuntimeCoverageBlastRadiusEntry {
                id: entry.id,
                file: PathBuf::from(entry.file),
                function: entry.function,
                line: entry.line,
                caller_count: entry.caller_count,
                caller_count_weighted_by_traffic: entry.caller_count_weighted_by_traffic,
                deploys_touched: entry.deploys_touched,
                risk_band: map_risk_band(entry.risk_band),
            },
        )
        .collect::<Vec<_>>();
    blast_radius.sort_by(|left, right| {
        risk_band_rank(right.risk_band)
            .cmp(&risk_band_rank(left.risk_band))
            .then_with(|| {
                right
                    .caller_count_weighted_by_traffic
                    .cmp(&left.caller_count_weighted_by_traffic)
            })
            .then_with(|| right.caller_count.cmp(&left.caller_count))
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.function.cmp(&right.function))
    });

    let mut importance = response
        .importance
        .into_iter()
        .map(
            |entry| crate::health_types::RuntimeCoverageImportanceEntry {
                id: entry.id,
                file: PathBuf::from(entry.file),
                function: entry.function,
                line: entry.line,
                invocations: entry.invocations,
                cyclomatic: entry.cyclomatic,
                owner_count: entry.owner_count,
                importance_score: entry.importance_score,
                reason: entry.reason,
            },
        )
        .collect::<Vec<_>>();
    importance.sort_by(|left, right| {
        right
            .importance_score
            .total_cmp(&left.importance_score)
            .then_with(|| right.invocations.cmp(&left.invocations))
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.function.cmp(&right.function))
    });

    let coverage_percent = response.summary.coverage_percent;
    let clamped_percent = if coverage_percent.is_finite() {
        coverage_percent
    } else {
        0.0
    };

    RuntimeCoverageReport {
        verdict: map_report_verdict(&response.verdict),
        summary: RuntimeCoverageSummary {
            data_source: RuntimeCoverageDataSource::Local,
            last_received_at: None,
            functions_tracked: response.summary.functions_tracked as usize,
            functions_hit: response.summary.functions_hit as usize,
            functions_unhit: response.summary.functions_unhit as usize,
            functions_untracked: response.summary.functions_untracked as usize,
            coverage_percent: clamped_percent,
            trace_count: response.summary.trace_count,
            period_days: response.summary.period_days,
            deployments_seen: response.summary.deployments_seen,
            capture_quality: response
                .summary
                .capture_quality
                .as_ref()
                .map(map_capture_quality),
        },
        findings,
        hot_paths,
        blast_radius,
        importance,
        watermark: watermark.or_else(|| response.watermark.as_ref().map(map_watermark)),
        warnings: response
            .warnings
            .into_iter()
            .map(|warning| RuntimeCoverageMessage {
                code: warning.code,
                message: warning.message,
            })
            .collect(),
    }
}

fn apply_top_limit(report: &mut RuntimeCoverageReport, top: Option<usize>) {
    let Some(top) = top else {
        return;
    };
    report.findings.truncate(top);
    report.hot_paths.truncate(top);
    report.blast_radius.truncate(top);
    report.importance.truncate(top);
}

const fn map_risk_band(risk_band: RiskBand) -> RuntimeCoverageRiskBand {
    match risk_band {
        RiskBand::Low => RuntimeCoverageRiskBand::Low,
        RiskBand::Medium => RuntimeCoverageRiskBand::Medium,
        RiskBand::High => RuntimeCoverageRiskBand::High,
    }
}

const fn risk_band_rank(risk_band: RuntimeCoverageRiskBand) -> u8 {
    match risk_band {
        RuntimeCoverageRiskBand::Low => 0,
        RuntimeCoverageRiskBand::Medium => 1,
        RuntimeCoverageRiskBand::High => 2,
    }
}

const fn map_verdict(verdict: Verdict) -> RuntimeCoverageVerdict {
    match verdict {
        Verdict::SafeToDelete => RuntimeCoverageVerdict::SafeToDelete,
        Verdict::ReviewRequired => RuntimeCoverageVerdict::ReviewRequired,
        Verdict::CoverageUnavailable => RuntimeCoverageVerdict::CoverageUnavailable,
        Verdict::LowTraffic => RuntimeCoverageVerdict::LowTraffic,
        Verdict::Active => RuntimeCoverageVerdict::Active,
        Verdict::Unknown => RuntimeCoverageVerdict::Unknown,
    }
}

const fn map_confidence(confidence: Confidence) -> RuntimeCoverageConfidence {
    match confidence {
        Confidence::VeryHigh => RuntimeCoverageConfidence::VeryHigh,
        Confidence::High => RuntimeCoverageConfidence::High,
        Confidence::Medium => RuntimeCoverageConfidence::Medium,
        Confidence::Low => RuntimeCoverageConfidence::Low,
        Confidence::None => RuntimeCoverageConfidence::None,
        Confidence::Unknown => RuntimeCoverageConfidence::Unknown,
    }
}

fn map_evidence(evidence: Evidence) -> RuntimeCoverageEvidence {
    RuntimeCoverageEvidence {
        static_status: evidence.static_status,
        test_coverage: evidence.test_coverage,
        v8_tracking: evidence.v8_tracking,
        untracked_reason: evidence.untracked_reason,
        observation_days: evidence.observation_days,
        deployments_observed: evidence.deployments_observed,
    }
}

fn map_report_verdict(verdict: &ReportVerdict) -> RuntimeCoverageReportVerdict {
    match verdict {
        ReportVerdict::Clean => RuntimeCoverageReportVerdict::Clean,
        ReportVerdict::HotPathChangesNeeded => RuntimeCoverageReportVerdict::HotPathTouched,
        ReportVerdict::ColdCodeDetected => RuntimeCoverageReportVerdict::ColdCodeDetected,
        ReportVerdict::LicenseExpiredGrace => RuntimeCoverageReportVerdict::LicenseExpiredGrace,
        ReportVerdict::Unknown => RuntimeCoverageReportVerdict::Unknown,
    }
}

fn map_watermark(watermark: &Watermark) -> RuntimeCoverageWatermark {
    match watermark {
        Watermark::TrialExpired => RuntimeCoverageWatermark::TrialExpired,
        Watermark::LicenseExpiredGrace => RuntimeCoverageWatermark::LicenseExpiredGrace,
        Watermark::Unknown => RuntimeCoverageWatermark::Unknown,
    }
}

fn map_capture_quality(
    quality: &CaptureQuality,
) -> crate::health_types::RuntimeCoverageCaptureQuality {
    crate::health_types::RuntimeCoverageCaptureQuality {
        window_seconds: quality.window_seconds,
        instances_observed: quality.instances_observed,
        lazy_parse_warning: quality.lazy_parse_warning,
        untracked_ratio_percent: quality.untracked_ratio_percent,
    }
}

/// Sort order for finding rendering: strongest deletion signal first, noise last.
const fn verdict_rank(verdict: RuntimeCoverageVerdict) -> u8 {
    match verdict {
        RuntimeCoverageVerdict::SafeToDelete => 0,
        RuntimeCoverageVerdict::ReviewRequired => 1,
        RuntimeCoverageVerdict::LowTraffic => 2,
        RuntimeCoverageVerdict::CoverageUnavailable => 3,
        RuntimeCoverageVerdict::Active => 4,
        RuntimeCoverageVerdict::Unknown => 5,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AccumulatedFunction, BINARY_SIGNING_VERIFY_KEY, FunctionIdentity, PackageManagerOutput,
        RemappedFunction, StaticSignalIndex, build_request, build_static_signal_index,
        convert_response, discover_sidecar, looks_like_istanbul, merge_remapped_functions,
        path_binary_candidates, prepare_coverage_sources, resolve_original_source_path,
        resolve_sidecar_via_command, sidecar_binary_name, verify_sidecar_signature,
        write_istanbul_coverage_file,
    };
    use crate::health::RuntimeCoverageOptions;
    use fallow_config::{FallowConfig, OutputFormat};
    use fallow_cov_protocol::{
        Confidence, CoverageSource, DiagnosticMessage, Evidence, Finding, HotPath, ReportVerdict,
        Response, Summary, Verdict,
    };
    use globset::GlobSetBuilder;
    use oxc_coverage_instrument::{Location, Position};
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use url::Url;

    fn empty_analysis_output() -> fallow_core::AnalysisOutput {
        fallow_core::AnalysisOutput {
            results: fallow_core::results::AnalysisResults::default(),
            timings: None,
            graph: None,
            modules: None,
            files: None,
            script_used_packages: FxHashSet::default(),
        }
    }

    #[test]
    fn detects_istanbul_file_by_name() {
        assert!(looks_like_istanbul(
            PathBuf::from("coverage-final.json").as_path()
        ));
        assert!(!looks_like_istanbul(
            PathBuf::from("coverage.json").as_path()
        ));
    }

    #[test]
    fn binary_signing_verify_key_is_32_bytes() {
        // Ed25519 public keys are always 32 bytes. Guards against accidental
        // byte-array edits that would silently break verification.
        assert_eq!(BINARY_SIGNING_VERIFY_KEY.len(), 32);
    }

    // Hard-fail gate for the release process. Asserts the constant is not the
    // all-zeros placeholder that shipped in the Phase 2.5 A' commit. Now runs
    // by default (no `#[ignore]`) so any accidental revert to the placeholder
    // would break `cargo test` immediately.
    #[test]
    fn binary_signing_verify_key_must_not_be_placeholder() {
        assert_ne!(
            BINARY_SIGNING_VERIFY_KEY, [0u8; 32],
            "BINARY_SIGNING_VERIFY_KEY is the all-zeros placeholder. Generate a real keypair per fallow-cloud/decisions/008-sidecar-key-rotation.md and paste the public bytes here before cutting a release."
        );
    }

    // Structural invariant: the runtime-coverage analysis path must not
    // perform any network I/O. Enterprise / air-gapped buyers depend on this.
    // The gate for Phase 2 step 4 of the roadmap is explicitly "integration
    // test asserting zero network calls during analysis"; this source-level
    // assertion is the fastest regression guard for that contract. The sibling
    // integration tests in `crates/cli/tests/runtime_coverage_tests.rs`
    // exercise the full spawn pipeline with a signed stub sidecar.
    #[test]
    fn runtime_coverage_module_has_no_network_code() {
        // Scan only the non-test portion of the file; the FORBIDDEN list below
        // would otherwise match its own entries.
        let full = include_str!("coverage.rs");
        let analysis_source = full.split("#[cfg(test)]").next().unwrap_or(full);
        const FORBIDDEN: &[&str] = &[
            "ureq::",
            "ureq_",
            "reqwest",
            "hyper::",
            "std::net::Tcp",
            "std::net::Udp",
            "std::net::Socket",
            "tokio::net::",
            "rustls::",
            "openssl::ssl",
            "native_tls::",
            "curl::",
        ];
        for needle in FORBIDDEN {
            assert!(
                !analysis_source.contains(needle),
                "crates/cli/src/health/coverage.rs must not reference `{needle}`; the runtime-coverage analysis path is sealed and cannot make network calls",
            );
        }
    }

    #[test]
    fn verify_sidecar_signature_rejects_missing_sig_file() {
        let root = make_temp_dir("cov-sig-missing");
        std::fs::create_dir_all(&root).expect("create temp dir");
        let binary = root.join("fallow-cov");
        std::fs::write(&binary, b"not a real binary").expect("write binary");

        let err = verify_sidecar_signature(&binary).expect_err("missing .sig must fail");
        assert!(
            err.contains("missing its signature file"),
            "error message missing expected guidance: {err}"
        );
        assert!(
            err.contains("Reinstall @fallow-cli/fallow-cov"),
            "error message missing reinstall hint: {err}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verify_sidecar_signature_rejects_wrong_length_sig() {
        let root = make_temp_dir("cov-sig-wrong-length");
        std::fs::create_dir_all(&root).expect("create temp dir");
        let binary = root.join("fallow-cov");
        std::fs::write(&binary, b"not a real binary").expect("write binary");
        let sig_path = {
            let mut path = binary.as_os_str().to_os_string();
            path.push(".sig");
            PathBuf::from(path)
        };
        std::fs::write(&sig_path, [0u8; 32]).expect("write short sig");

        let err = verify_sidecar_signature(&binary).expect_err("short sig must fail");
        assert!(
            err.contains("expected 64"),
            "error message missing length detail: {err}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn verify_sidecar_signature_rejects_bad_signature() {
        let root = make_temp_dir("cov-sig-bad");
        std::fs::create_dir_all(&root).expect("create temp dir");
        let binary = root.join("fallow-cov");
        std::fs::write(&binary, b"not a real binary").expect("write binary");
        let sig_path = {
            let mut path = binary.as_os_str().to_os_string();
            path.push(".sig");
            PathBuf::from(path)
        };
        std::fs::write(&sig_path, [0u8; 64]).expect("write zero sig");

        let err = verify_sidecar_signature(&binary).expect_err("bogus sig must fail");
        assert!(
            err.contains("failed Ed25519 signature verification"),
            "error message missing verification phrase: {err}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn detects_istanbul_file_by_shape_without_canonical_filename() {
        let root = make_temp_dir("coverage-istanbul-shape");
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to create temp dir: {err}"));
        let coverage = root.join("prod-coverage.json");
        std::fs::write(
            &coverage,
            serde_json::json!({
                "src/app.ts": {
                    "path": "src/app.ts",
                    "statementMap": {},
                    "fnMap": {},
                    "branchMap": {},
                    "s": {},
                    "f": {},
                    "b": {}
                }
            })
            .to_string(),
        )
        .unwrap_or_else(|err| panic!("failed to write {}: {err}", coverage.display()));

        assert!(looks_like_istanbul(&coverage));

        let prepared = prepare_coverage_sources(&coverage)
            .unwrap_or_else(|err| panic!("failed to collect coverage sources: {err}"));
        assert!(matches!(
            &prepared.sources[..],
            [CoverageSource::Istanbul { path }] if path.ends_with("prod-coverage.json")
        ));

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn coverage_final_filename_with_v8_shape_still_uses_v8_classification() {
        let root = make_temp_dir("coverage-v8-shape");
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to create temp dir: {err}"));
        let coverage = root.join("coverage-final.json");
        std::fs::write(&coverage, serde_json::json!({ "result": [] }).to_string())
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", coverage.display()));

        assert!(!looks_like_istanbul(&coverage));

        let prepared = prepare_coverage_sources(&coverage)
            .unwrap_or_else(|err| panic!("failed to collect coverage sources: {err}"));
        assert!(matches!(
            &prepared.sources[..],
            [CoverageSource::V8 { path }] if path.ends_with("coverage-final.json")
        ));

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn directory_with_istanbul_and_v8_files_expands_to_per_file_sources() {
        let root = make_temp_dir("coverage-sources");
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to create temp dir: {err}"));
        std::fs::write(root.join("coverage-final.json"), "{}")
            .unwrap_or_else(|err| panic!("failed to write istanbul file: {err}"));
        std::fs::write(root.join("chunk-1.json"), "{\"result\":[]}")
            .unwrap_or_else(|err| panic!("failed to write v8 file: {err}"));

        let prepared = prepare_coverage_sources(&root)
            .unwrap_or_else(|err| panic!("failed to collect coverage sources: {err}"));
        let sources = prepared.sources;

        assert_eq!(sources.len(), 2);
        assert!(matches!(
            &sources[0],
            CoverageSource::V8 { path } if path.ends_with("chunk-1.json")
        ));
        assert!(matches!(
            &sources[1],
            CoverageSource::Istanbul { path } if path.ends_with("coverage-final.json")
        ));

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn discovers_project_local_sidecar_before_global_locations() {
        let root = make_temp_dir("sidecar-local");
        let bin_dir = root.join("node_modules").join(".bin");
        std::fs::create_dir_all(&bin_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", bin_dir.display()));
        let sidecar = if cfg!(windows) {
            bin_dir.join("fallow-cov.cmd")
        } else {
            bin_dir.join("fallow-cov")
        };
        std::fs::write(&sidecar, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", sidecar.display()));

        let resolved = discover_sidecar(Some(&root))
            .unwrap_or_else(|err| panic!("failed to discover local sidecar: {err}"));

        assert_eq!(resolved, sidecar);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    // Regression test for the Phase 2.5 smoke-test finding: when both the
    // `@fallow-cli/fallow-cov-<platform>/fallow-cov` real binary and the
    // `node_modules/.bin/fallow-cov` Node wrapper exist (the usual layout
    // after `npm install @fallow-cli/fallow-cov`), discovery must prefer
    // the platform package's real binary. The wrapper has no adjacent
    // `.sig` file, so pointing at it breaks signature verification.
    #[test]
    fn discovers_platform_package_sidecar_before_bin_wrapper() {
        let root = make_temp_dir("sidecar-platform-pkg");
        let platform_dir = root
            .join("node_modules")
            .join("@fallow-cli")
            .join("fallow-cov-darwin-arm64");
        let bin_dir = root.join("node_modules").join(".bin");
        std::fs::create_dir_all(&platform_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", platform_dir.display()));
        std::fs::create_dir_all(&bin_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", bin_dir.display()));

        let binary_name = if cfg!(windows) {
            "fallow-cov.exe"
        } else {
            "fallow-cov"
        };
        let real_binary = platform_dir.join(binary_name);
        let wrapper = if cfg!(windows) {
            bin_dir.join("fallow-cov.cmd")
        } else {
            bin_dir.join("fallow-cov")
        };
        std::fs::write(&real_binary, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", real_binary.display()));
        std::fs::write(&wrapper, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", wrapper.display()));

        let resolved = discover_sidecar(Some(&root))
            .unwrap_or_else(|err| panic!("failed to discover platform sidecar: {err}"));

        assert_eq!(
            resolved, real_binary,
            "discover_sidecar must prefer the platform package's real binary over the .bin wrapper so signature verification can find the adjacent .sig file"
        );

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn discovers_bun_store_platform_sidecar_before_bin_wrapper() {
        let root = make_temp_dir("sidecar-bun-store");
        let platform_dir = root
            .join("node_modules")
            .join(".bun")
            .join("@fallow-cli+fallow-cov-darwin-arm64@0.1.8")
            .join("node_modules")
            .join("@fallow-cli")
            .join("fallow-cov-darwin-arm64");
        let bin_dir = root.join("node_modules").join(".bin");
        std::fs::create_dir_all(&platform_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", platform_dir.display()));
        std::fs::create_dir_all(&bin_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", bin_dir.display()));

        let real_binary = platform_dir.join(sidecar_binary_name());
        let wrapper = if cfg!(windows) {
            bin_dir.join("fallow-cov.cmd")
        } else {
            bin_dir.join("fallow-cov")
        };
        std::fs::write(&real_binary, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", real_binary.display()));
        std::fs::write(&wrapper, "#!/usr/bin/env node\n")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", wrapper.display()));

        let resolved = discover_sidecar(Some(&root))
            .unwrap_or_else(|err| panic!("failed to discover bun sidecar: {err}"));

        assert_eq!(resolved, real_binary);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn discovers_newest_bun_store_platform_sidecar() {
        let root = make_temp_dir("sidecar-bun-store-newest");
        let store = root.join("node_modules").join(".bun");
        let stale_binary = write_bun_store_sidecar(&store, "0.1.8");
        let current_binary = write_bun_store_sidecar(&store, "0.1.10");

        let resolved = discover_sidecar(Some(&root))
            .unwrap_or_else(|err| panic!("failed to discover bun sidecar: {err}"));

        assert_eq!(
            resolved,
            current_binary,
            "newer package-store sidecars must win over stale versions; stale={}",
            stale_binary.display()
        );

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn discovers_pnpm_store_platform_sidecar_before_bin_wrapper() {
        let root = make_temp_dir("sidecar-pnpm-store");
        // pnpm with `node-linker=isolated` extracts platform packages into
        // `.pnpm/<scope+name>@<version>_<peer-hash>/...`; the peer-hash
        // suffix must not break the `@fallow-cli+fallow-cov-` prefix match.
        let platform_dir = root
            .join("node_modules")
            .join(".pnpm")
            .join("@fallow-cli+fallow-cov-darwin-arm64@0.1.8_abcd1234efgh5678")
            .join("node_modules")
            .join("@fallow-cli")
            .join("fallow-cov-darwin-arm64");
        let bin_dir = root.join("node_modules").join(".bin");
        std::fs::create_dir_all(&platform_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", platform_dir.display()));
        std::fs::create_dir_all(&bin_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", bin_dir.display()));

        let real_binary = platform_dir.join(sidecar_binary_name());
        let wrapper = if cfg!(windows) {
            bin_dir.join("fallow-cov.cmd")
        } else {
            bin_dir.join("fallow-cov")
        };
        std::fs::write(&real_binary, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", real_binary.display()));
        std::fs::write(&wrapper, "#!/usr/bin/env node\n")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", wrapper.display()));

        let resolved = discover_sidecar(Some(&root))
            .unwrap_or_else(|err| panic!("failed to discover pnpm sidecar: {err}"));

        assert_eq!(resolved, real_binary);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn path_binary_candidates_include_windows_cmd_shims() {
        let candidates = path_binary_candidates("fallow-cov");

        if cfg!(windows) {
            assert_eq!(
                candidates,
                vec!["fallow-cov", "fallow-cov.exe", "fallow-cov.cmd"]
            );
        } else {
            assert_eq!(candidates, vec!["fallow-cov"]);
        }
    }

    #[test]
    fn resolves_yarn_sidecar_without_node_modules_bin() {
        let root = make_temp_dir("sidecar-yarn");
        let command_dir = root.join("commands");
        let unplugged_dir = root
            .join(".yarn")
            .join("unplugged")
            .join("fallow-cov")
            .join("node_modules")
            .join(".bin");
        std::fs::create_dir_all(&command_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", command_dir.display()));
        std::fs::create_dir_all(&unplugged_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", unplugged_dir.display()));
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"demo","packageManager":"yarn@4.1.0"}"#,
        )
        .unwrap_or_else(|err| panic!("failed to write package.json: {err}"));
        std::fs::write(root.join("yarn.lock"), "")
            .unwrap_or_else(|err| panic!("failed to write yarn.lock: {err}"));

        let sidecar = if cfg!(windows) {
            unplugged_dir.join("fallow-cov.cmd")
        } else {
            unplugged_dir.join("fallow-cov")
        };
        std::fs::write(&sidecar, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", sidecar.display()));

        let yarn = if cfg!(windows) {
            command_dir.join("yarn.cmd")
        } else {
            command_dir.join("yarn")
        };
        write_fake_yarn_bin_command(&yarn, &sidecar);

        let resolved = resolve_sidecar_via_command(
            &root,
            yarn.as_os_str(),
            &["bin", "fallow-cov"],
            PackageManagerOutput::BinaryPath,
        )
        .unwrap_or_else(|| panic!("failed to resolve yarn-local sidecar"));

        assert_eq!(resolved, sidecar);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn resolves_npm_sidecar_from_node_modules_root() {
        let root = make_temp_dir("sidecar-npm");
        let command_dir = root.join("commands");
        let bin_dir = root.join("custom-node_modules").join(".bin");
        std::fs::create_dir_all(&command_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", command_dir.display()));
        std::fs::create_dir_all(&bin_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", bin_dir.display()));

        let sidecar = if cfg!(windows) {
            bin_dir.join("fallow-cov.cmd")
        } else {
            bin_dir.join("fallow-cov")
        };
        std::fs::write(&sidecar, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", sidecar.display()));

        let npm = if cfg!(windows) {
            command_dir.join("npm.cmd")
        } else {
            command_dir.join("npm")
        };
        write_fake_npm_root_command(&npm, &root.join("custom-node_modules"));

        let resolved = resolve_sidecar_via_command(
            &root,
            npm.as_os_str(),
            &["root"],
            PackageManagerOutput::NodeModulesDir,
        )
        .unwrap_or_else(|| panic!("failed to resolve npm-local sidecar"));

        assert_eq!(resolved, sidecar);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn convert_response_round_trips_ids_and_evidence() {
        let locations = FxHashMap::default();

        let report = convert_response(
            Response {
                protocol_version: "0.2.0".to_owned(),
                verdict: ReportVerdict::ColdCodeDetected,
                summary: Summary {
                    functions_tracked: 1,
                    functions_hit: 0,
                    functions_unhit: 1,
                    functions_untracked: 0,
                    coverage_percent: 0.0,
                    trace_count: 512,
                    period_days: 7,
                    deployments_seen: 2,
                    capture_quality: None,
                },
                findings: vec![Finding {
                    id: "fallow:prod:abc12345".to_owned(),
                    file: "src/app.ts".to_owned(),
                    function: "alpha".to_owned(),
                    line: 8,
                    verdict: Verdict::ReviewRequired,
                    invocations: Some(0),
                    confidence: Confidence::Medium,
                    evidence: Evidence {
                        static_status: "used".to_owned(),
                        test_coverage: "not_covered".to_owned(),
                        v8_tracking: "tracked".to_owned(),
                        untracked_reason: None,
                        observation_days: 7,
                        deployments_observed: 2,
                    },
                    actions: vec![],
                }],
                hot_paths: vec![HotPath {
                    id: "fallow:hot:def67890".to_owned(),
                    file: "src/app.ts".to_owned(),
                    function: "alpha".to_owned(),
                    line: 8,
                    invocations: 20,
                    percentile: 50,
                }],
                blast_radius: vec![],
                importance: vec![],
                watermark: None,
                errors: vec![],
                warnings: vec![DiagnosticMessage {
                    code: "test".to_owned(),
                    message: "warning".to_owned(),
                }],
            },
            &locations,
            None,
        );

        assert_eq!(report.findings[0].id, "fallow:prod:abc12345");
        assert_eq!(report.findings[0].line, 8);
        assert_eq!(
            report.findings[0].verdict,
            crate::health_types::RuntimeCoverageVerdict::ReviewRequired,
        );
        assert_eq!(report.findings[0].evidence.static_status, "used");
        assert_eq!(report.hot_paths[0].id, "fallow:hot:def67890");
        assert_eq!(report.hot_paths[0].percentile, 50);
    }

    #[test]
    fn build_request_uses_workspace_root_for_sidecar_project_root() {
        let root = PathBuf::from("/repo");
        let ws_root = root.join("packages/app");
        let ws_roots = [ws_root.clone()];
        let options = RuntimeCoverageOptions {
            path: root.join("coverage"),
            min_invocations_hot: 100,
            min_observation_volume: None,
            low_traffic_threshold: None,
            license_jwt: "test-jwt".to_owned(),
            watermark: None,
        };
        let ignore_set = GlobSetBuilder::new()
            .build()
            .unwrap_or_else(|err| panic!("failed to build empty globset: {err}"));

        let (request, _locations) = build_request(
            &options,
            &root,
            &[],
            &empty_analysis_output(),
            &StaticSignalIndex::default(),
            None,
            &FxHashMap::default(),
            &ignore_set,
            None,
            Some(&ws_roots),
            vec![],
            None,
        );

        assert_eq!(request.project_root, ws_root.to_string_lossy());
    }

    #[test]
    fn build_request_joins_dead_code_and_direct_test_signals() {
        let root = make_temp_dir("coverage-static-signals");
        let src_dir = root.join("src");
        std::fs::create_dir_all(&src_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", src_dir.display()));
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"demo","main":"src/index.ts"}"#,
        )
        .unwrap_or_else(|err| panic!("failed to write package.json: {err}"));
        std::fs::write(
            src_dir.join("index.ts"),
            "import { tested } from './app';\ntested();\n",
        )
        .unwrap_or_else(|err| panic!("failed to write index.ts: {err}"));
        std::fs::write(
            src_dir.join("app.ts"),
            "export function tested() { return 1; }\n\
             export function cold() { return 2; }\n\
             function internal() { return 3; }\n",
        )
        .unwrap_or_else(|err| panic!("failed to write app.ts: {err}"));
        std::fs::write(
            src_dir.join("app.test.ts"),
            "import { tested } from './app';\ntested();\n",
        )
        .unwrap_or_else(|err| panic!("failed to write app.test.ts: {err}"));

        let config =
            FallowConfig::default().resolve(root.clone(), OutputFormat::Json, 1, true, true);
        let files = fallow_core::discover::discover_files(&config);
        let parse_result = fallow_core::extract::parse_all_files(&files, None, true);
        let modules = parse_result.modules;
        let file_paths: FxHashMap<_, _> = files.iter().map(|file| (file.id, &file.path)).collect();
        let analysis_output = fallow_core::analyze_with_parse_result(&config, &modules)
            .unwrap_or_else(|err| panic!("failed to analyze temp project: {err}"));
        let static_signals = build_static_signal_index(&modules, &analysis_output, &file_paths)
            .unwrap_or_else(|err| panic!("failed to build static signal index: {err}"));
        let app_path = src_dir.join("app.ts");
        let tested_line = modules
            .iter()
            .find_map(|module| {
                file_paths.get(&module.file_id).and_then(|path| {
                    if **path == app_path {
                        module
                            .complexity
                            .iter()
                            .find(|function| function.name == "tested")
                            .map(|function| function.line)
                    } else {
                        None
                    }
                })
            })
            .unwrap_or_else(|| panic!("expected tested function line in parsed module"));
        let mut static_signals = static_signals;
        static_signals
            .test_referenced_export_names
            .entry(app_path.clone())
            .or_default()
            .insert("tested".to_owned());
        static_signals
            .test_referenced_export_lines
            .entry(app_path)
            .or_default()
            .insert(tested_line);

        let options = RuntimeCoverageOptions {
            path: root.join("coverage"),
            min_invocations_hot: 100,
            min_observation_volume: None,
            low_traffic_threshold: None,
            license_jwt: "test-jwt".to_owned(),
            watermark: None,
        };
        let ignore_set = GlobSetBuilder::new()
            .build()
            .unwrap_or_else(|err| panic!("failed to build empty globset: {err}"));

        let (request, _locations) = build_request(
            &options,
            &root,
            &modules,
            &empty_analysis_output(),
            &static_signals,
            None,
            &file_paths,
            &ignore_set,
            None,
            None,
            vec![],
            None,
        );

        let app_file = request
            .static_findings
            .files
            .iter()
            .find(|file| file.path.ends_with("src/app.ts"))
            .unwrap_or_else(|| panic!("expected src/app.ts in sidecar request"));
        let tested = app_file
            .functions
            .iter()
            .find(|function| function.name == "tested")
            .unwrap_or_else(|| panic!("expected tested function in sidecar request"));
        let cold = app_file
            .functions
            .iter()
            .find(|function| function.name == "cold")
            .unwrap_or_else(|| panic!("expected cold function in sidecar request"));
        let internal = app_file
            .functions
            .iter()
            .find(|function| function.name == "internal")
            .unwrap_or_else(|| panic!("expected internal function in sidecar request"));

        assert!(tested.static_used);
        assert!(tested.test_covered);
        assert!(!cold.static_used);
        assert!(!cold.test_covered);
        assert!(internal.static_used);
        assert!(!internal.test_covered);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn remaps_v8_source_map_cache_into_istanbul_sources() {
        let root = make_temp_dir("coverage-remap");
        let src_dir = root.join("src");
        let dist_dir = root.join("dist");
        std::fs::create_dir_all(&src_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", src_dir.display()));
        std::fs::create_dir_all(&dist_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", dist_dir.display()));

        let original = src_dir.join("app.ts");
        std::fs::write(&original, "export function alpha() {}\n")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", original.display()));

        let v8_file = root.join("coverage-v8.json");
        let v8_json = serde_json::json!({
            "result": [{
                "scriptId": "1",
                "url": file_url(&dist_dir.join("bundle.js")),
                "functions": [{
                    "functionName": "alpha",
                    "ranges": [{"startOffset": 0, "endOffset": 18, "count": 3}],
                    "isBlockCoverage": false
                }]
            }],
            "source-map-cache": {
                file_url(&dist_dir.join("bundle.js")): {
                    "url": "bundle.js.map",
                    "data": {
                        "version": 3,
                        "sources": ["../src/app.ts"],
                        "names": [],
                        "mappings": "AAAA"
                    },
                    "lineLengths": [18]
                }
            }
        });
        std::fs::write(&v8_file, serde_json::to_vec(&v8_json).unwrap())
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", v8_file.display()));

        let prepared = prepare_coverage_sources(&v8_file)
            .unwrap_or_else(|err| panic!("failed to preprocess coverage: {err}"));

        assert_eq!(prepared.sources.len(), 1);
        let CoverageSource::Istanbul { path } = &prepared.sources[0] else {
            panic!("expected remapped istanbul coverage source");
        };
        let output = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("failed to read remapped coverage {path}: {err}"));
        let parsed: serde_json::Value = serde_json::from_str(&output)
            .unwrap_or_else(|err| panic!("failed to parse remapped coverage: {err}"));
        let key = dunce::canonicalize(&original)
            .unwrap_or_else(|err| panic!("failed to canonicalize {}: {err}", original.display()))
            .to_string_lossy()
            .into_owned();

        assert!(
            parsed.get(&key).is_some(),
            "expected remapped file key {key}"
        );
        assert_eq!(parsed[&key]["path"], key);
        assert_eq!(parsed[&key]["fnMap"]["0"]["name"], "alpha");
        assert_eq!(parsed[&key]["fnMap"]["0"]["line"], 1);
        assert_eq!(parsed[&key]["f"]["0"], 3);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn falls_back_to_raw_v8_when_any_function_in_script_cannot_be_remapped() {
        let root = make_temp_dir("coverage-remap-partial");
        let src_dir = root.join("src");
        let dist_dir = root.join("dist");
        std::fs::create_dir_all(&src_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", src_dir.display()));
        std::fs::create_dir_all(&dist_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", dist_dir.display()));

        let original = src_dir.join("app.ts");
        std::fs::write(&original, "export function alpha() {}\n")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", original.display()));

        let v8_file = root.join("coverage-v8.json");
        let v8_json = serde_json::json!({
            "result": [{
                "scriptId": "1",
                "url": file_url(&dist_dir.join("bundle.js")),
                "functions": [
                    {
                        "functionName": "alpha",
                        "ranges": [{"startOffset": 0, "endOffset": 18, "count": 3}],
                        "isBlockCoverage": false
                    },
                    {
                        "functionName": "broken",
                        "ranges": [],
                        "isBlockCoverage": false
                    }
                ]
            }],
            "source-map-cache": {
                file_url(&dist_dir.join("bundle.js")): {
                    "url": "bundle.js.map",
                    "data": {
                        "version": 3,
                        "sources": ["../src/app.ts"],
                        "names": [],
                        "mappings": "AAAA"
                    },
                    "lineLengths": [18]
                }
            }
        });
        std::fs::write(&v8_file, serde_json::to_vec(&v8_json).unwrap())
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", v8_file.display()));

        let prepared = prepare_coverage_sources(&v8_file)
            .unwrap_or_else(|err| panic!("failed to preprocess coverage: {err}"));

        assert_eq!(prepared.sources.len(), 1);
        assert!(matches!(
            &prepared.sources[0],
            CoverageSource::V8 { path } if path.ends_with("coverage-v8.json")
        ));

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn remaps_webpack_virtual_source_map_sources() {
        let root = make_temp_dir("coverage-remap-webpack");
        let src_dir = root.join("src");
        let dist_dir = root.join("dist");
        std::fs::create_dir_all(&src_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", src_dir.display()));
        std::fs::create_dir_all(&dist_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", dist_dir.display()));

        let original = src_dir.join("app.ts");
        std::fs::write(&original, "export function alpha() {}\n")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", original.display()));

        let v8_file = root.join("coverage-v8.json");
        let v8_json = serde_json::json!({
            "result": [{
                "scriptId": "1",
                "url": file_url(&dist_dir.join("bundle.js")),
                "functions": [{
                    "functionName": "alpha",
                    "ranges": [{"startOffset": 0, "endOffset": 18, "count": 3}],
                    "isBlockCoverage": false
                }]
            }],
            "source-map-cache": {
                file_url(&dist_dir.join("bundle.js")): {
                    "url": "bundle.js.map",
                    "data": {
                        "version": 3,
                        "sources": ["webpack://src/app.ts"],
                        "names": [],
                        "mappings": "AAAA"
                    },
                    "lineLengths": [18]
                }
            }
        });
        std::fs::write(&v8_file, serde_json::to_vec(&v8_json).unwrap())
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", v8_file.display()));

        let prepared = prepare_coverage_sources(&v8_file)
            .unwrap_or_else(|err| panic!("failed to preprocess coverage: {err}"));

        assert_eq!(prepared.sources.len(), 1);
        let CoverageSource::Istanbul { path } = &prepared.sources[0] else {
            panic!("expected remapped istanbul coverage source");
        };
        let output = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("failed to read remapped coverage {path}: {err}"));
        let parsed: serde_json::Value = serde_json::from_str(&output)
            .unwrap_or_else(|err| panic!("failed to parse remapped coverage: {err}"));
        let key = dunce::canonicalize(&original)
            .unwrap_or_else(|err| panic!("failed to canonicalize {}: {err}", original.display()))
            .to_string_lossy()
            .into_owned();

        assert!(
            parsed.get(&key).is_some(),
            "expected remapped file key {key}"
        );

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn remaps_vite_virtual_source_map_sources() {
        let root = make_temp_dir("coverage-remap-vite");
        let src_dir = root.join("src");
        let dist_dir = root.join("dist");
        std::fs::create_dir_all(&src_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", src_dir.display()));
        std::fs::create_dir_all(&dist_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", dist_dir.display()));

        let original = src_dir.join("app.ts");
        std::fs::write(&original, "export function alpha() {}\n")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", original.display()));

        let v8_file = root.join("coverage-v8.json");
        let v8_json = serde_json::json!({
            "result": [{
                "scriptId": "1",
                "url": file_url(&dist_dir.join("bundle.js")),
                "functions": [{
                    "functionName": "alpha",
                    "ranges": [{"startOffset": 0, "endOffset": 18, "count": 3}],
                    "isBlockCoverage": false
                }]
            }],
            "source-map-cache": {
                file_url(&dist_dir.join("bundle.js")): {
                    "url": "bundle.js.map",
                    "data": {
                        "version": 3,
                        "sources": ["vite://src/app.ts"],
                        "names": [],
                        "mappings": "AAAA"
                    },
                    "lineLengths": [18]
                }
            }
        });
        std::fs::write(&v8_file, serde_json::to_vec(&v8_json).unwrap())
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", v8_file.display()));

        let prepared = prepare_coverage_sources(&v8_file)
            .unwrap_or_else(|err| panic!("failed to preprocess coverage: {err}"));

        assert_eq!(prepared.sources.len(), 1);
        let CoverageSource::Istanbul { path } = &prepared.sources[0] else {
            panic!("expected remapped istanbul coverage source");
        };
        let output = std::fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("failed to read remapped coverage {path}: {err}"));
        let parsed: serde_json::Value = serde_json::from_str(&output)
            .unwrap_or_else(|err| panic!("failed to parse remapped coverage: {err}"));
        let key = dunce::canonicalize(&original)
            .unwrap_or_else(|err| panic!("failed to canonicalize {}: {err}", original.display()))
            .to_string_lossy()
            .into_owned();

        assert!(
            parsed.get(&key).is_some(),
            "expected remapped file key {key}"
        );

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn preserves_windows_absolute_source_map_sources() {
        let resolved = resolve_original_source_path(
            "C:/repo/src/app.ts",
            "file:///C:/repo/dist/bundle.js",
            Some("bundle.js.map"),
        )
        .unwrap_or_else(|| panic!("failed to resolve windows absolute source path"));

        assert_eq!(resolved, PathBuf::from("C:/repo/src/app.ts"));

        let resolved_backslashes = resolve_original_source_path(
            r"C:\repo\src\app.ts",
            "file:///C:/repo/dist/bundle.js",
            Some("bundle.js.map"),
        )
        .unwrap_or_else(|| panic!("failed to resolve windows backslash source path"));

        assert_eq!(resolved_backslashes, PathBuf::from(r"C:\repo\src\app.ts"));
    }

    #[test]
    fn falls_back_to_raw_v8_for_unsupported_source_map_schemes() {
        let root = make_temp_dir("coverage-remap-unsupported");
        let dist_dir = root.join("dist");
        std::fs::create_dir_all(&dist_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", dist_dir.display()));

        let v8_file = root.join("coverage-v8.json");
        let v8_json = serde_json::json!({
            "result": [{
                "scriptId": "1",
                "url": file_url(&dist_dir.join("bundle.js")),
                "functions": [{
                    "functionName": "alpha",
                    "ranges": [{"startOffset": 0, "endOffset": 18, "count": 3}],
                    "isBlockCoverage": false
                }]
            }],
            "source-map-cache": {
                file_url(&dist_dir.join("bundle.js")): {
                    "url": "bundle.js.map",
                    "data": {
                        "version": 3,
                        "sources": ["parcel://src/app.ts"],
                        "names": [],
                        "mappings": "AAAA"
                    },
                    "lineLengths": [18]
                }
            }
        });
        std::fs::write(&v8_file, serde_json::to_vec(&v8_json).unwrap())
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", v8_file.display()));

        let prepared = prepare_coverage_sources(&v8_file)
            .unwrap_or_else(|err| panic!("failed to preprocess coverage: {err}"));

        assert_eq!(prepared.sources.len(), 1);
        assert!(matches!(
            &prepared.sources[0],
            CoverageSource::V8 { path } if path.ends_with("coverage-v8.json")
        ));

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn keeps_same_line_functions_separate_when_columns_differ() {
        let root = make_temp_dir("coverage-remap-identity");
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", root.display()));
        let output = root.join("coverage-final.json");
        let file = dunce::canonicalize(&root)
            .unwrap_or_else(|_| root.clone())
            .join("app.ts");

        let mut files: BTreeMap<PathBuf, BTreeMap<FunctionIdentity, AccumulatedFunction>> =
            BTreeMap::new();
        merge_remapped_functions(
            &mut files,
            vec![
                RemappedFunction {
                    path: file.clone(),
                    name: "alpha".to_owned(),
                    decl: location(1, 0, 1, 0),
                    loc: location(1, 0, 1, 4),
                    hits: 1,
                },
                RemappedFunction {
                    path: file.clone(),
                    name: "alpha".to_owned(),
                    decl: location(1, 8, 1, 8),
                    loc: location(1, 8, 1, 12),
                    hits: 2,
                },
            ],
        );
        write_istanbul_coverage_file(&output, &files)
            .unwrap_or_else(|err| panic!("failed to write remapped coverage: {err}"));

        let parsed: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&output)
                .unwrap_or_else(|err| panic!("failed to read {}: {err}", output.display())),
        )
        .unwrap_or_else(|err| panic!("failed to parse {}: {err}", output.display()));
        let key = file.to_string_lossy().into_owned();

        assert_eq!(parsed[&key]["fnMap"].as_object().unwrap().len(), 2);
        assert_eq!(parsed[&key]["f"]["0"], 1);
        assert_eq!(parsed[&key]["f"]["1"], 2);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    fn make_temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_else(|err| panic!("clock went backwards: {err}"))
            .as_nanos();
        std::env::temp_dir().join(format!("fallow-cli-{name}-{}-{nanos}", std::process::id()))
    }

    fn file_url(path: &Path) -> String {
        Url::from_file_path(path)
            .unwrap_or_else(|()| panic!("failed to convert {} to file url", path.display()))
            .to_string()
    }

    fn location(start_line: u32, start_column: u32, end_line: u32, end_column: u32) -> Location {
        Location {
            start: Position {
                line: start_line,
                column: start_column,
            },
            end: Position {
                line: end_line,
                column: end_column,
            },
        }
    }

    fn write_fake_yarn_bin_command(path: &Path, sidecar: &Path) {
        if cfg!(windows) {
            std::fs::write(
                path,
                format!(
                    "@echo off\r\nif \"%1\"==\"bin\" if \"%2\"==\"fallow-cov\" (\r\n  echo {}\r\n  exit /b 0\r\n)\r\nexit /b 1\r\n",
                    sidecar.display()
                ),
            )
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));
            return;
        }

        std::fs::write(
            path,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"bin\" ] && [ \"$2\" = \"fallow-cov\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nexit 1\n",
                sidecar.display()
            ),
        )
        .unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(path)
                .unwrap_or_else(|err| panic!("failed to stat {}: {err}", path.display()))
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions)
                .unwrap_or_else(|err| panic!("failed to chmod {}: {err}", path.display()));
        }
    }

    fn write_bun_store_sidecar(store: &Path, version: &str) -> PathBuf {
        let platform_dir = store
            .join(format!("@fallow-cli+fallow-cov-darwin-arm64@{version}"))
            .join("node_modules")
            .join("@fallow-cli")
            .join("fallow-cov-darwin-arm64");
        std::fs::create_dir_all(&platform_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", platform_dir.display()));
        std::fs::write(
            platform_dir.join("package.json"),
            format!(r#"{{"name":"@fallow-cli/fallow-cov-darwin-arm64","version":"{version}"}}"#),
        )
        .unwrap_or_else(|err| {
            panic!(
                "failed to write package.json in {}: {err}",
                platform_dir.display()
            )
        });
        let binary = platform_dir.join(sidecar_binary_name());
        std::fs::write(&binary, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", binary.display()));
        binary
    }

    fn write_fake_npm_root_command(path: &Path, node_modules_dir: &Path) {
        if cfg!(windows) {
            std::fs::write(
                path,
                format!(
                    "@echo off\r\nif \"%1\"==\"root\" (\r\n  echo {}\r\n  exit /b 0\r\n)\r\nexit /b 1\r\n",
                    node_modules_dir.display()
                ),
            )
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));
            return;
        }

        std::fs::write(
            path,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"root\" ]; then\n  printf '%s\\n' '{}'\n  exit 0\nfi\nexit 1\n",
                node_modules_dir.display()
            ),
        )
        .unwrap_or_else(|err| panic!("failed to write {}: {err}", path.display()));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            let mut permissions = std::fs::metadata(path)
                .unwrap_or_else(|err| panic!("failed to stat {}: {err}", path.display()))
                .permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(path, permissions)
                .unwrap_or_else(|err| panic!("failed to chmod {}: {err}", path.display()));
        }
    }
}
