use std::ffi::OsStr;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::process::{Command, Stdio};
use std::{collections::BTreeMap, fs};

use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use fallow_config::OutputFormat;
use fallow_cov_protocol::{
    BlastRadiusEntry as ProtocolBlastRadiusEntry, CaptureQuality, Confidence, CoverageSource,
    Evidence, Finding as ProtocolFinding, FunctionIdentity, HotPath as ProtocolHotPath,
    IdentityResolution, ImportanceEntry as ProtocolImportanceEntry, PROTOCOL_VERSION,
    ReportVerdict, Request, Response, RiskBand, StaticFile, StaticFindings, StaticFunction,
    Verdict, Watermark, function_identity_id,
};
use fallow_license::{
    DEFAULT_HARD_FAIL_DAYS, Feature, LicenseStatus, load_and_verify, load_raw_jwt,
};
use fallow_v8_coverage::V8CoverageDump;
use globset::GlobSet;
use oxc_coverage_instrument::{FileCoverage, FnEntry, Location, Position};
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Deserialize;
use srcmap_sourcemap::{GeneratedLocation, GeneratedOffsetLookup, SourceMap};
use tempfile::TempDir;
use url::Url;

use crate::error::emit_error;
use crate::health::scoring::IstanbulCoverage;
use crate::license::verifying_key;
use fallow_engine::RuntimeCoverageOptions;
use fallow_output::{
    RUNTIME_STALE_AFTER_DAYS, RuntimeCoverageAction, RuntimeCoverageConfidence,
    RuntimeCoverageDataSource, RuntimeCoverageDiscriminators, RuntimeCoverageEvidence,
    RuntimeCoverageFinding, RuntimeCoverageHotPath, RuntimeCoverageMessage,
    RuntimeCoverageProvenance, RuntimeCoverageReport, RuntimeCoverageReportVerdict,
    RuntimeCoverageRiskBand, RuntimeCoverageSchemaVersion, RuntimeCoverageSummary,
    RuntimeCoverageVerdict, RuntimeCoverageWatermark,
};

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

enum GeneratedPositionLookup<'a> {
    SourceText {
        source: &'a str,
        lookup: GeneratedOffsetLookup<'a>,
    },
    V8LineOffsets(fallow_v8_coverage::LineOffsetTable),
}

impl GeneratedPositionLookup<'_> {
    fn generated_position_for_offset(&self, v8_source_offset: u32) -> Option<GeneratedLocation> {
        match self {
            Self::SourceText { source, lookup } => {
                let byte_offset = utf16_source_offset_to_byte_offset(source, v8_source_offset)?;
                lookup.byte_offset_to_position(byte_offset)
            }
            Self::V8LineOffsets(line_offsets) => {
                let pos = line_offsets.position(v8_source_offset);
                Some(GeneratedLocation {
                    line: pos.line.saturating_sub(1),
                    column: pos.column,
                })
            }
        }
    }

    fn original_position_for_offset(
        &self,
        sourcemap: &SourceMap,
        v8_source_offset: u32,
    ) -> Option<srcmap_sourcemap::OriginalLocation> {
        let position = self.generated_position_for_offset(v8_source_offset)?;
        sourcemap.original_position_for(position.line, position.column)
    }
}

#[derive(Debug, Clone)]
struct RemappedFunction {
    path: PathBuf,
    name: String,
    decl: Location,
    loc: Location,
    hits: u32,
}

struct RemappedScript {
    functions: Vec<RemappedFunction>,
    residual_script: Option<fallow_v8_coverage::ScriptCoverage>,
}

#[derive(Debug, Clone)]
struct AccumulatedFunction {
    entry: FnEntry,
    hits: u32,
}

/// Dedup key for merging V8-remapped functions across overlapping script
/// dumps. NOT the protocol's `fallow_cov_protocol::FunctionIdentity` (the
/// cross-surface join key); this is a purely local position-based key used to
/// coalesce the same physical function seen in multiple coverage scripts.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct RemappedFnKey {
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
    fn key(&self) -> RemappedFnKey {
        RemappedFnKey {
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

pub(super) struct RuntimeCoverageAnalysisInput<'a> {
    pub root: &'a Path,
    pub modules: &'a [fallow_types::extract::ModuleInfo],
    pub analysis_output: &'a fallow_engine::DeadCodeAnalysisArtifacts,
    pub istanbul_coverage: Option<&'a IstanbulCoverage>,
    pub file_paths: &'a FxHashMap<fallow_types::discover::FileId, &'a PathBuf>,
    pub ignore_set: &'a GlobSet,
    pub changed_files: Option<&'a FxHashSet<PathBuf>>,
    pub ws_roots: Option<&'a [PathBuf]>,
    pub top: Option<usize>,
    pub codeowners_path: Option<&'a str>,
    pub quiet: bool,
    pub output: OutputFormat,
}

pub(super) fn analyze(
    options: &RuntimeCoverageOptions,
    input: &RuntimeCoverageAnalysisInput<'_>,
) -> Result<RuntimeCoverageReport, ExitCode> {
    let sidecar = discover_sidecar(Some(input.root))
        .map_err(|message| emit_error(&message, 4, input.output))?;
    let prepared_sources = prepare_coverage_sources(&options.path)
        .map_err(|message| emit_error(&message, 5, input.output))?;
    let static_signals =
        build_static_signal_index(input.modules, input.analysis_output, input.file_paths)
            .map_err(|message| emit_error(&message, 2, input.output))?;
    let (request, locations) =
        build_request(options, input, &static_signals, prepared_sources.sources);
    let response = run_sidecar(&sidecar, &request, input.quiet, input.output)?;
    // Resolve the verdict thresholds with the SAME defaults the sidecar applies
    // when they are unset, so the discriminator block (#321) reports the values
    // that actually produced the verdicts.
    let min_observation_volume = options
        .min_observation_volume
        .unwrap_or(MIN_OBSERVATION_VOLUME_DEFAULT);
    let low_traffic_threshold = options
        .low_traffic_threshold
        .unwrap_or(LOW_TRAFFIC_THRESHOLD_DEFAULT);
    let mut report = convert_response(
        response,
        &locations,
        options.watermark,
        min_observation_volume,
        low_traffic_threshold,
    );
    apply_top_limit(&mut report, input.top);
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
    if let Some(path) = env_non_empty("FALLOW_COV_BIN") {
        let candidate = PathBuf::from(&path);
        if candidate.is_file() {
            return Ok(candidate);
        }
        return Err(format!(
            "FALLOW_COV_BIN is set to {path} but no file exists there. Unset FALLOW_COV_BIN to fall back to sidecar auto-discovery, or point it at the fallow-cov binary."
        ));
    }

    if let Some(path) = env_non_empty("FALLOW_COV_BINARY_PATH") {
        let candidate = PathBuf::from(&path);
        if candidate.is_file() {
            return Ok(candidate);
        }
        return Err(format!(
            "FALLOW_COV_BINARY_PATH is set to {path} but no file exists there. Unset FALLOW_COV_BINARY_PATH to fall back to sidecar auto-discovery, or point it at the fallow-cov binary."
        ));
    }

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
    resolve_sidecar_from_output(root, &stdout, output_kind)
}

/// Resolve a sidecar path from a package-manager command's captured stdout.
/// Split out from [`resolve_sidecar_via_command`] so the path-resolution
/// branches stay testable without spawning a subprocess (the spawn-based tests
/// flaked under the instrumented coverage CI run).
fn resolve_sidecar_from_output(
    root: &Path,
    stdout: &str,
    output_kind: PackageManagerOutput,
) -> Option<PathBuf> {
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

/// Construct a protocol [`StaticFunction`] carrying a v2 [`FunctionIdentity`].
///
/// [`StaticFunction`] is `#[non_exhaustive]` in protocol 0.6+ and the crate
/// ships no constructor, so an external producer cannot use struct-literal
/// syntax. We round-trip a `serde_json` object through the derived
/// `Deserialize` (generated inside the protocol crate, which bypasses the
/// non-exhaustive construction restriction). This function fully controls the
/// shape, so the deserialize is infallible today; if a future protocol
/// revision adds a required-without-default field, the `static_function_round_trips`
/// unit test fails in CI rather than this `.expect()` panicking in the paid
/// runtime-coverage path.
///
/// `stable_id` is computed over the repo-relative posix path so it matches the
/// static-inventory producer and the `coverage analyze` consumer (columns are
/// not part of the hash). Resolution is [`IdentityResolution::Unresolved`], NOT
/// `Fallback`: the health path never had column precision by design
/// (`FunctionComplexity` carries only a byte column and source is not retained
/// here), so the identity is resolved no further than `file` / `name` /
/// `start_line`, which is exactly the `Unresolved` contract. The join is
/// unaffected because columns are excluded from `stable_id`.
struct StaticFunctionInput<'a> {
    relative_posix: &'a str,
    name: &'a str,
    start_line: u32,
    end_line: u32,
    cyclomatic: u32,
    static_used: bool,
    test_covered: bool,
    caller_count: u32,
    owner_count: Option<u32>,
    source_hash: Option<String>,
}

#[expect(
    clippy::expect_used,
    reason = "static function test fixtures are constructed from JSON literals"
)]
fn static_function(input: StaticFunctionInput<'_>) -> StaticFunction {
    let identity = FunctionIdentity {
        file: input.relative_posix.to_owned(),
        name: input.name.to_owned(),
        start_line: input.start_line,
        start_column: None,
        end_line: None,
        end_column: None,
        source_hash: input.source_hash,
        resolution: IdentityResolution::Unresolved,
        stable_id: function_identity_id(input.relative_posix, input.name, input.start_line),
    };
    serde_json::from_value(serde_json::json!({
        "name": input.name,
        "start_line": input.start_line,
        "end_line": input.end_line,
        "cyclomatic": input.cyclomatic,
        "static_used": input.static_used,
        "test_covered": input.test_covered,
        "caller_count": input.caller_count,
        "owner_count": input.owner_count,
        "identity": identity,
    }))
    .expect(
        "StaticFunction is built with the exact protocol field shape and cannot fail to deserialize; a future required-without-default protocol field would fail static_function_round_trips in CI",
    )
}

fn build_request(
    options: &RuntimeCoverageOptions,
    input: &RuntimeCoverageAnalysisInput<'_>,
    static_signals: &StaticSignalIndex,
    coverage_sources: Vec<CoverageSource>,
) -> (Request, FunctionLocations) {
    let project_root = match input.ws_roots {
        Some([only]) => only.as_path(),
        _ => input.root,
    };
    let (files, locations) = build_static_files(input, static_signals);
    (
        assemble_request(options, project_root, coverage_sources, files),
        locations,
    )
}

/// Build the per-module `StaticFile` list and the ambiguous-line location map.
fn build_static_files(
    input: &RuntimeCoverageAnalysisInput<'_>,
    static_signals: &StaticSignalIndex,
) -> (Vec<StaticFile>, FunctionLocations) {
    let mut files = Vec::new();
    let mut locations = FxHashMap::default();
    let graph = input.analysis_output.graph.as_ref();
    let codeowners = crate::codeowners::CodeOwners::load(input.root, input.codeowners_path).ok();
    for module in input.modules {
        let Some(&path) = input.file_paths.get(&module.file_id) else {
            continue;
        };
        let relative = path.strip_prefix(input.root).unwrap_or(path);
        if !module_is_eligible(input, module, path, relative) {
            continue;
        }
        let canonical_path = input
            .istanbul_coverage
            .map(|_| dunce::canonicalize(path).unwrap_or_else(|_| path.clone()));
        let caller_count = graph.map_or(0_usize, |g| g.direct_importer_count(module.file_id));
        let caller_count = u32::try_from(caller_count).unwrap_or(u32::MAX);
        let owner_count = codeowners
            .as_ref()
            .map(|co| co.owner_count_of(relative).unwrap_or(0));
        let relative_posix = relative.to_string_lossy().replace('\\', "/");
        let functions = module
            .complexity
            .iter()
            .map(|function| {
                mark_ambiguous_function_line(&mut locations, path, &function.name, function.line);
                build_static_function(&BuildStaticFunctionInput {
                    function,
                    path,
                    canonical_path: canonical_path.as_deref(),
                    static_signals,
                    istanbul_coverage: input.istanbul_coverage,
                    relative_posix: &relative_posix,
                    caller_count,
                    owner_count,
                })
            })
            .collect();
        files.push(StaticFile {
            path: relative_posix,
            functions,
        });
    }
    (files, locations)
}

/// Whether a module should contribute functions to the request.
fn module_is_eligible(
    input: &RuntimeCoverageAnalysisInput<'_>,
    module: &fallow_types::extract::ModuleInfo,
    path: &Path,
    relative: &Path,
) -> bool {
    if input.ignore_set.is_match(relative) {
        return false;
    }
    if let Some(changed) = input.changed_files
        && !changed.contains(path)
    {
        return false;
    }
    if let Some(ws) = input.ws_roots
        && !ws.iter().any(|r| path.starts_with(r))
    {
        return false;
    }
    !module.complexity.is_empty()
}

/// Per-function inputs threaded from `build_static_files` into `static_function`.
struct BuildStaticFunctionInput<'a> {
    function: &'a fallow_types::extract::FunctionComplexity,
    path: &'a Path,
    canonical_path: Option<&'a Path>,
    static_signals: &'a StaticSignalIndex,
    istanbul_coverage: Option<&'a IstanbulCoverage>,
    relative_posix: &'a str,
    caller_count: u32,
    owner_count: Option<u32>,
}

/// Derive a `StaticFunction` from one parsed function plus static signals.
fn build_static_function(input: &BuildStaticFunctionInput<'_>) -> StaticFunction {
    let static_used = function_static_used(input.path, input.function, input.static_signals);
    let test_covered = function_test_covered(
        input.path,
        input.canonical_path,
        input.function,
        input.static_signals,
        input.istanbul_coverage,
    );
    static_function(StaticFunctionInput {
        relative_posix: input.relative_posix,
        name: &input.function.name,
        start_line: input.function.line,
        end_line: input
            .function
            .line
            .saturating_add(input.function.line_count),
        cyclomatic: u32::from(input.function.cyclomatic),
        static_used,
        test_covered,
        caller_count: input.caller_count,
        owner_count: input.owner_count,
        source_hash: input.function.source_hash.clone(),
    })
}

/// Assemble the protocol `Request` envelope around the built static files.
fn assemble_request(
    options: &RuntimeCoverageOptions,
    project_root: &Path,
    coverage_sources: Vec<CoverageSource>,
    files: Vec<StaticFile>,
) -> Request {
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
            trace_count: None,
            period_days: None,
            deployments_seen: None,
            window_seconds: None,
            instances_observed: None,
        },
    }
}

fn build_static_signal_index(
    modules: &[fallow_types::extract::ModuleInfo],
    analysis_output: &fallow_engine::DeadCodeAnalysisArtifacts,
    file_paths: &FxHashMap<fallow_types::discover::FileId, &PathBuf>,
) -> Result<StaticSignalIndex, String> {
    let graph = analysis_output
        .graph
        .as_ref()
        .ok_or_else(|| "analysis graph not available for runtime coverage".to_owned())?;
    let mut index = StaticSignalIndex::default();
    index_dead_code_signals(&mut index, analysis_output);

    let module_by_id: FxHashMap<_, _> = modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();
    for export in fallow_engine::module_value_exports(graph) {
        let Some(&path) = file_paths.get(&export.file_id) else {
            continue;
        };
        index_graph_value_export(
            &mut index,
            &export,
            module_by_id.get(&export.file_id).copied(),
            path,
        );
    }

    Ok(index)
}

/// Seed the signal index with unused-file and unused-export dead-code signals.
fn index_dead_code_signals(
    index: &mut StaticSignalIndex,
    analysis_output: &fallow_engine::DeadCodeAnalysisArtifacts,
) {
    for file in &analysis_output.results.unused_files {
        index.unused_files.insert(file.file.path.clone());
    }
    for export in &analysis_output.results.unused_exports {
        index
            .unused_export_names
            .entry(export.export.path.clone())
            .or_default()
            .insert(export.export.export_name.clone());
        index
            .unused_export_lines
            .entry(export.export.path.clone())
            .or_default()
            .insert(export.export.line);
    }
}

/// Index one graph value export, including test-referenced state.
fn index_graph_value_export(
    index: &mut StaticSignalIndex,
    export: &fallow_engine::ModuleValueExport,
    module: Option<&fallow_types::extract::ModuleInfo>,
    path: &Path,
) {
    index
        .exported_names
        .entry(path.to_path_buf())
        .or_default()
        .insert(export.name.clone());

    if let Some(module) = module {
        let (line, _) =
            fallow_types::extract::byte_offset_to_line_col(&module.line_offsets, export.span_start);
        index
            .exported_lines
            .entry(path.to_path_buf())
            .or_default()
            .insert(line);

        if export.test_referenced {
            index
                .test_referenced_export_names
                .entry(path.to_path_buf())
                .or_default()
                .insert(export.name.clone());
            index
                .test_referenced_export_lines
                .entry(path.to_path_buf())
                .or_default()
                .insert(line);
        }
    }
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

    let (remapped_files, residual_scripts) = remap_dump_scripts(dump.result, &cache);

    if remapped_files.is_empty() {
        return Ok(None);
    }

    let temp_root = ensure_temp_dir(temp_dir)?;
    let remapped_path = temp_root.join(format!("coverage-remapped-{index}.json"));
    write_istanbul_coverage_file(&remapped_path, &remapped_files)?;

    let residual_path = write_residual_coverage(temp_root, index, residual_scripts)?;

    Ok(Some((remapped_path, residual_path)))
}

/// Split V8 scripts into source-map-remapped Istanbul functions and the
/// residual scripts that had no usable source map.
fn remap_dump_scripts(
    scripts: Vec<fallow_v8_coverage::ScriptCoverage>,
    cache: &BTreeMap<String, SourceMapCacheEntry>,
) -> (
    BTreeMap<PathBuf, BTreeMap<RemappedFnKey, AccumulatedFunction>>,
    Vec<fallow_v8_coverage::ScriptCoverage>,
) {
    let mut remapped_files: BTreeMap<PathBuf, BTreeMap<RemappedFnKey, AccumulatedFunction>> =
        BTreeMap::new();
    let mut residual_scripts = Vec::new();

    for script in scripts {
        let Some(entry) = cache.get(&script.url) else {
            residual_scripts.push(script);
            continue;
        };
        let Some(mapped) = remap_script_with_source_map(&script, entry) else {
            residual_scripts.push(script);
            continue;
        };
        merge_remapped_functions(&mut remapped_files, mapped.functions);
        if let Some(residual_script) = mapped.residual_script {
            residual_scripts.push(residual_script);
        }
    }

    (remapped_files, residual_scripts)
}

/// Write the residual (non-remapped) V8 scripts to a temp file, if any.
fn write_residual_coverage(
    temp_root: &Path,
    index: usize,
    residual_scripts: Vec<fallow_v8_coverage::ScriptCoverage>,
) -> Result<Option<PathBuf>, String> {
    if residual_scripts.is_empty() {
        return Ok(None);
    }
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
    Ok(Some(residual_path))
}

fn parse_source_map_cache(dump: &V8CoverageDump) -> Option<BTreeMap<String, SourceMapCacheEntry>> {
    let raw = dump.source_map_cache.clone()?;
    serde_json::from_value(raw).ok()
}

#[expect(
    clippy::expect_used,
    reason = "temp dir option is initialized immediately before it is read"
)]
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
) -> Option<RemappedScript> {
    let sourcemap = SourceMap::from_json(&entry.data.to_string()).ok()?;
    let generated_source = generated_source_for_script(script);
    let positions = match generated_source.as_deref() {
        Some(source) => GeneratedPositionLookup::SourceText {
            source,
            lookup: GeneratedOffsetLookup::new(source),
        },
        None => GeneratedPositionLookup::V8LineOffsets(
            fallow_v8_coverage::LineOffsetTable::from_v8_line_lengths(&entry.line_lengths)?,
        ),
    };
    let mut remapped = Vec::new();
    let mut residual_functions = Vec::new();

    for function in &script.functions {
        match remap_function(script, function, entry, &sourcemap, &positions) {
            Some(mapped) => remapped.push(mapped),
            None => residual_functions.push(function.clone()),
        }
    }

    if remapped.is_empty() {
        return None;
    }

    let residual_script = (!residual_functions.is_empty()).then(|| {
        let mut script = script.clone();
        script.functions = residual_functions;
        script
    });

    Some(RemappedScript {
        functions: remapped,
        residual_script,
    })
}

fn generated_source_for_script(script: &fallow_v8_coverage::ScriptCoverage) -> Option<String> {
    if let Some(path) = file_url_to_path(&script.url)
        && let Ok(source) = fs::read_to_string(path)
    {
        return Some(source);
    }
    None
}

fn utf16_source_offset_to_byte_offset(source: &str, target_offset: u32) -> Option<u32> {
    let mut utf16_offset = 0u32;
    for (byte_offset, ch) in source.char_indices() {
        if utf16_offset == target_offset {
            return u32::try_from(byte_offset).ok();
        }
        utf16_offset = utf16_offset.checked_add(ch.len_utf16() as u32)?;
        if utf16_offset > target_offset {
            return None;
        }
    }
    if utf16_offset == target_offset {
        return u32::try_from(source.len()).ok();
    }
    None
}

fn remap_function(
    script: &fallow_v8_coverage::ScriptCoverage,
    function: &fallow_v8_coverage::FunctionCoverage,
    entry: &SourceMapCacheEntry,
    sourcemap: &SourceMap,
    positions: &GeneratedPositionLookup<'_>,
) -> Option<RemappedFunction> {
    let outer = function.ranges.first().copied()?;
    let start_lookup = positions.original_position_for_offset(sourcemap, outer.start_offset)?;
    let resolved_path = resolve_original_source_path(
        sourcemap.source(start_lookup.source),
        &script.url,
        entry.url.as_deref(),
    )?;
    let canonical_path = dunce::canonicalize(&resolved_path).unwrap_or(resolved_path);
    let end_lookup = positions
        .original_position_for_offset(sourcemap, outer.end_offset)
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
    if crate::path_util::is_absolute_path_any_platform(&source_path)
        || crate::path_util::looks_like_windows_absolute_path(raw_source)
    {
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
    if crate::path_util::is_absolute_path_any_platform(&candidate)
        || crate::path_util::looks_like_windows_absolute_path(source_map_url)
    {
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
    (crate::path_util::is_absolute_path_any_platform(&path)
        || crate::path_util::looks_like_windows_absolute_path(value))
    .then_some(path)
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
    target: &mut BTreeMap<PathBuf, BTreeMap<RemappedFnKey, AccumulatedFunction>>,
    functions: Vec<RemappedFunction>,
) {
    for function in functions {
        let key = function.key();
        let file = target.entry(function.path).or_default();
        let entry = file.entry(key).or_insert_with(|| AccumulatedFunction {
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
    files: &BTreeMap<PathBuf, BTreeMap<RemappedFnKey, AccumulatedFunction>>,
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
                x_fallow_function_map: None,
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

    let mut command = Command::new(sidecar);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = crate::signal::ScopedChild::spawn(&mut command).map_err(|err| {
        emit_error(
            &format!("failed to spawn {}: {err}", sidecar.display()),
            4,
            output,
        )
    })?;

    if let Some(stdin) = child.take_stdin() {
        write_sidecar_request(stdin, request, output)?;
    }

    let output_data = child
        .wait_with_output()
        .map_err(|err| emit_error(&format!("failed to wait for sidecar: {err}"), 4, output))?;

    if !output_data.stderr.is_empty() && !quiet {
        let stderr = String::from_utf8_lossy(&output_data.stderr);
        eprint!("{stderr}");
    }

    check_sidecar_exit_status(&output_data, output)?;

    let response: Response = serde_json::from_slice(&output_data.stdout).map_err(|err| {
        emit_error(
            &format!("failed to parse sidecar response: {err}"),
            4,
            output,
        )
    })?;

    check_response_protocol(&response, output)?;

    Ok(response)
}

/// Serialize and flush the protocol request onto the sidecar's stdin.
fn write_sidecar_request(
    mut stdin: impl Write,
    request: &Request,
    output: OutputFormat,
) -> Result<(), ExitCode> {
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
    Ok(())
}

/// Map the sidecar's exit code onto fallow's exit-code ladder.
fn check_sidecar_exit_status(
    output_data: &std::process::Output,
    output: OutputFormat,
) -> Result<(), ExitCode> {
    match output_data.status.code() {
        Some(0) => Ok(()),
        Some(4) => Err(emit_error(
            &stderr_message(&output_data.stderr, "sidecar protocol mismatch"),
            4,
            output,
        )),
        Some(5) => Err(emit_error(
            &stderr_message(
                &output_data.stderr,
                "failed to parse runtime coverage input",
            ),
            5,
            output,
        )),
        Some(6) => Err(emit_error(
            &stderr_message(&output_data.stderr, "sidecar internal error"),
            6,
            output,
        )),
        Some(code) => Err(emit_error(
            &stderr_message(&output_data.stderr, "sidecar execution failed"),
            u8::try_from(code).unwrap_or(4),
            output,
        )),
        None => Err(emit_error("sidecar terminated by signal", 4, output)),
    }
}

/// Reject responses whose protocol major version does not match this build.
fn check_response_protocol(response: &Response, output: OutputFormat) -> Result<(), ExitCode> {
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
    Ok(())
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
    min_observation_volume: u32,
    low_traffic_threshold: f64,
) -> RuntimeCoverageReport {
    let findings = map_runtime_findings(
        response.findings,
        response.summary.trace_count,
        min_observation_volume,
        low_traffic_threshold,
    );
    let hot_paths = map_runtime_hot_paths(response.hot_paths);
    let blast_radius = map_runtime_blast_radius(response.blast_radius);
    let importance = map_runtime_importance(response.importance);

    let coverage_percent = response.summary.coverage_percent;
    let clamped_percent = if coverage_percent.is_finite() {
        coverage_percent
    } else {
        0.0
    };

    // #316 / #319: mirror the cloud runtime-context trust-output contract on the
    // local report. A capture with no tracked functions carries no usable
    // runtime evidence, so it is non-actionable (insufficient_evidence) rather
    // than read as cold. A local capture is fresh (age 0) and origin-unknown.
    let functions_tracked = response.summary.functions_tracked;
    let functions_untracked = response.summary.functions_untracked;
    let denominator = functions_tracked + functions_untracked;
    let untracked_ratio = if denominator == 0 {
        0.0
    } else {
        functions_untracked as f64 / denominator as f64
    };
    let actionable = functions_tracked > 0;
    let (actionability_reason, actionability_verdict) = if actionable {
        (None, None)
    } else {
        (
            Some(
                "No functions were tracked at runtime in this capture, so there is no usable runtime evidence to act on. Treat all functions as do-not-act; this is NOT cold."
                    .to_owned(),
            ),
            Some("insufficient_evidence".to_owned()),
        )
    };
    let provenance = RuntimeCoverageProvenance {
        data_source: RuntimeCoverageDataSource::Local,
        is_production: "unknown".to_owned(),
        freshness_days: Some(0),
        untracked_ratio,
        unresolved_ratio: 0.0,
        stale: false,
        stale_after_days: RUNTIME_STALE_AFTER_DAYS,
    };

    RuntimeCoverageReport {
        schema_version: RuntimeCoverageSchemaVersion::V1,
        verdict: map_report_verdict(&response.verdict),
        signals: Vec::new(),
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
        actionable,
        actionability_reason,
        actionability_verdict,
        provenance,
    }
}

/// Confidence-table defaults the sidecar (`fallow-cov`) applies when the
/// corresponding option is unset. Kept in sync with
/// `fallow-cov/src/analysis.rs` (`MIN_OBSERVATION_VOLUME_DEFAULT`,
/// `LOW_TRAFFIC_THRESHOLD_DEFAULT`); the sidecar lives in a separate repo so the
/// values are mirrored here, not imported. They are resolved CLI-side only to
/// report them in the discriminator block (#321); the sidecar still owns the
/// verdict computation.
const MIN_OBSERVATION_VOLUME_DEFAULT: u32 = 5_000;
const LOW_TRAFFIC_THRESHOLD_DEFAULT: f64 = 0.001;

/// Three-state runtime tracking label from the protocol evidence + invocation
/// count: `untracked` when V8 never saw the function, else `called` /
/// `never_called` by whether it was invoked.
fn tracking_state_label(v8_tracking: &str, invocations: Option<u64>) -> &'static str {
    if v8_tracking == "untracked" {
        "untracked"
    } else if invocations.is_some_and(|count| count > 0) {
        "called"
    } else {
        "never_called"
    }
}

fn map_runtime_findings(
    findings: Vec<ProtocolFinding>,
    trace_count: u64,
    min_observation_volume: u32,
    low_traffic_threshold: f64,
) -> Vec<RuntimeCoverageFinding> {
    let meets_observation_volume = trace_count >= u64::from(min_observation_volume);
    let mut findings = findings
        .into_iter()
        .filter_map(|finding| {
            let verdict = map_verdict(finding.verdict);
            if matches!(verdict, RuntimeCoverageVerdict::Active) {
                return None;
            }
            let (stable_id, source_hash) = finding.identity.map_or((None, None), |identity| {
                (Some(identity.stable_id), identity.source_hash)
            });
            let discriminators = RuntimeCoverageDiscriminators {
                tracking_state: tracking_state_label(
                    &finding.evidence.v8_tracking,
                    finding.invocations,
                )
                .to_owned(),
                invocation_ratio: finding.invocations.map(|invocations| {
                    if trace_count == 0 {
                        0.0
                    } else {
                        invocations as f64 / trace_count as f64
                    }
                }),
                low_traffic_threshold,
                trace_count,
                min_observation_volume,
                meets_observation_volume,
            };
            Some(RuntimeCoverageFinding {
                id: finding.id,
                stable_id,
                source_hash,
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
                discriminators: Some(discriminators),
            })
        })
        .collect::<Vec<_>>();
    findings.sort_by(|left, right| {
        verdict_rank(left.verdict)
            .cmp(&verdict_rank(right.verdict))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.function.cmp(&right.function))
    });
    findings
}

fn map_runtime_hot_paths(entries: Vec<ProtocolHotPath>) -> Vec<RuntimeCoverageHotPath> {
    let mut hot_paths = entries
        .into_iter()
        .map(|entry| RuntimeCoverageHotPath {
            id: entry.id,
            stable_id: entry.identity.map(|identity| identity.stable_id),
            path: PathBuf::from(entry.file),
            function: entry.function,
            line: entry.line,
            end_line: entry.end_line,
            invocations: entry.invocations,
            percentile: entry.percentile,
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
    hot_paths
}

fn map_runtime_blast_radius(
    entries: Vec<ProtocolBlastRadiusEntry>,
) -> Vec<fallow_output::RuntimeCoverageBlastRadiusEntry> {
    let mut blast_radius = entries
        .into_iter()
        .map(|entry| fallow_output::RuntimeCoverageBlastRadiusEntry {
            id: entry.id,
            stable_id: entry.identity.map(|identity| identity.stable_id),
            file: PathBuf::from(entry.file),
            function: entry.function,
            line: entry.line,
            caller_count: entry.caller_count,
            caller_count_weighted_by_traffic: entry.caller_count_weighted_by_traffic,
            deploys_touched: entry.deploys_touched,
            risk_band: map_risk_band(entry.risk_band),
        })
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
    blast_radius
}

fn map_runtime_importance(
    entries: Vec<ProtocolImportanceEntry>,
) -> Vec<fallow_output::RuntimeCoverageImportanceEntry> {
    let mut importance = entries
        .into_iter()
        .map(|entry| fallow_output::RuntimeCoverageImportanceEntry {
            id: entry.id,
            stable_id: entry.identity.map(|identity| identity.stable_id),
            file: PathBuf::from(entry.file),
            function: entry.function,
            line: entry.line,
            invocations: entry.invocations,
            cyclomatic: entry.cyclomatic,
            owner_count: entry.owner_count,
            importance_score: entry.importance_score,
            reason: entry.reason,
        })
        .collect::<Vec<_>>();
    importance.sort_by(|left, right| {
        right
            .importance_score
            .total_cmp(&left.importance_score)
            .then_with(|| right.invocations.cmp(&left.invocations))
            .then_with(|| left.file.cmp(&right.file))
            .then_with(|| left.function.cmp(&right.function))
    });
    importance
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
        RiskBand::High => RuntimeCoverageRiskBand::High,
        RiskBand::Medium | RiskBand::Unknown => RuntimeCoverageRiskBand::Medium,
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
        ReportVerdict::HotPathTouched => RuntimeCoverageReportVerdict::HotPathTouched,
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

fn map_capture_quality(quality: &CaptureQuality) -> fallow_output::RuntimeCoverageCaptureQuality {
    fallow_output::RuntimeCoverageCaptureQuality {
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
        AccumulatedFunction, BINARY_SIGNING_VERIFY_KEY, PackageManagerOutput, RemappedFnKey,
        RemappedFunction, RuntimeCoverageAnalysisInput, StaticFunctionInput, StaticSignalIndex,
        build_request, build_static_signal_index, convert_response, discover_sidecar,
        looks_like_istanbul, merge_remapped_functions, path_binary_candidates,
        prepare_coverage_sources, resolve_original_source_path, resolve_sidecar_from_output,
        resolve_sidecar_via_command, sidecar_binary_name, static_function, tracking_state_label,
        verify_sidecar_signature, write_istanbul_coverage_file,
    };
    use fallow_config::{FallowConfig, OutputFormat};
    use fallow_cov_protocol::{
        Confidence, CoverageSource, DiagnosticMessage, Evidence, Finding, FunctionIdentity,
        HotPath, IdentityResolution, PROTOCOL_VERSION, ReportVerdict, Response, Summary, Verdict,
        function_identity_id,
    };
    use fallow_engine::RuntimeCoverageOptions;
    use globset::{Glob, GlobSetBuilder};
    use oxc_coverage_instrument::{Location, Position};
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::collections::BTreeMap;
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};
    use url::Url;

    fn empty_analysis_output() -> fallow_engine::DeadCodeAnalysisArtifacts {
        fallow_engine::DeadCodeAnalysisArtifacts {
            results: fallow_types::results::AnalysisResults::default(),
            timings: None,
            graph: None,
            modules: None,
            files: None,
            script_used_packages: FxHashSet::default(),
            file_hashes: rustc_hash::FxHashMap::default(),
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
        assert_eq!(BINARY_SIGNING_VERIFY_KEY.len(), 32);
    }

    #[test]
    fn binary_signing_verify_key_must_not_be_placeholder() {
        assert_ne!(
            BINARY_SIGNING_VERIFY_KEY, [0u8; 32],
            "BINARY_SIGNING_VERIFY_KEY is the all-zeros placeholder. Generate a real keypair per fallow-cloud/decisions/008-sidecar-key-rotation.md and paste the public bytes here before cutting a release."
        );
    }

    #[test]
    fn runtime_coverage_module_has_no_network_code() {
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
    fn coverage_directory_ignores_non_json_files_and_sorts_sources() {
        let root = make_temp_dir("coverage-source-sort");
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to create temp dir: {err}"));
        std::fs::write(root.join("notes.txt"), "not coverage")
            .unwrap_or_else(|err| panic!("failed to write notes.txt: {err}"));
        std::fs::write(root.join("z-v8.json"), "{\"result\":[]}")
            .unwrap_or_else(|err| panic!("failed to write z-v8.json: {err}"));
        std::fs::write(root.join("a-istanbul.json"), "{}")
            .unwrap_or_else(|err| panic!("failed to write a-istanbul.json: {err}"));

        let prepared = prepare_coverage_sources(&root)
            .unwrap_or_else(|err| panic!("failed to collect coverage sources: {err}"));
        let sources = prepared.sources;

        assert_eq!(sources.len(), 2);
        assert!(matches!(
            &sources[0],
            CoverageSource::Istanbul { path } if path.ends_with("a-istanbul.json")
        ));
        assert!(matches!(
            &sources[1],
            CoverageSource::V8 { path } if path.ends_with("z-v8.json")
        ));

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn empty_coverage_directory_is_treated_as_v8_directory() {
        let root = make_temp_dir("coverage-empty-dir");
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to create temp dir: {err}"));

        let prepared = prepare_coverage_sources(&root)
            .unwrap_or_else(|err| panic!("failed to collect coverage sources: {err}"));

        assert!(matches!(
            &prepared.sources[..],
            [CoverageSource::V8Dir { path }] if path == &root.to_string_lossy()
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
    fn finds_project_local_sidecar_from_ancestor_node_modules_bin() {
        let root = make_temp_dir("sidecar-project-local");
        let app = root.join("apps").join("web");
        let bin_dir = root.join("node_modules").join(".bin");
        std::fs::create_dir_all(&app)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", app.display()));
        std::fs::create_dir_all(&bin_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", bin_dir.display()));
        let sidecar = bin_dir.join(sidecar_binary_name());
        std::fs::write(&sidecar, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", sidecar.display()));

        assert_eq!(super::find_project_local_sidecar(&app), Some(sidecar));

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn scoped_platform_sidecar_ignores_unusable_package_dirs() {
        let root = make_temp_dir("sidecar-scoped-ignore");
        let scoped = root.join("node_modules").join("@fallow-cli");
        std::fs::create_dir_all(scoped.join("fallow-cov-darwin-arm64"))
            .unwrap_or_else(|err| panic!("failed to create scoped package: {err}"));
        std::fs::create_dir_all(scoped.join("not-fallow-cov"))
            .unwrap_or_else(|err| panic!("failed to create unrelated package: {err}"));

        assert_eq!(
            super::find_scoped_platform_sidecar(&scoped, sidecar_binary_name()),
            None
        );

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn detects_package_manager_from_field_before_lockfiles() {
        let root = make_temp_dir("sidecar-package-manager-field");
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", root.display()));
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"demo","packageManager":"pnpm@9.15.0"}"#,
        )
        .unwrap_or_else(|err| panic!("failed to write package.json: {err}"));
        std::fs::write(root.join("package-lock.json"), "")
            .unwrap_or_else(|err| panic!("failed to write package-lock.json: {err}"));

        assert_eq!(
            super::detect_package_manager(&root),
            Some(super::LocalPackageManager::Pnpm)
        );

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn detects_package_manager_from_lockfiles() {
        for (lockfile, expected) in [
            ("bun.lock", super::LocalPackageManager::Bun),
            ("pnpm-lock.yaml", super::LocalPackageManager::Pnpm),
            ("yarn.lock", super::LocalPackageManager::Yarn),
            ("npm-shrinkwrap.json", super::LocalPackageManager::Npm),
        ] {
            let root = make_temp_dir(&format!("sidecar-lockfile-{lockfile}"));
            std::fs::create_dir_all(&root)
                .unwrap_or_else(|err| panic!("failed to create {}: {err}", root.display()));
            std::fs::write(root.join(lockfile), "")
                .unwrap_or_else(|err| panic!("failed to write {lockfile}: {err}"));

            assert_eq!(super::detect_package_manager(&root), Some(expected));

            std::fs::remove_dir_all(&root)
                .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
        }
    }

    #[test]
    fn detect_package_manager_ignores_unknown_package_manager_field() {
        let root = make_temp_dir("sidecar-package-manager-unknown");
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", root.display()));
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"demo","packageManager":"pnpmish@1.0.0"}"#,
        )
        .unwrap_or_else(|err| panic!("failed to write package.json: {err}"));

        assert_eq!(super::detect_package_manager(&root), None);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn detect_package_manager_falls_back_after_invalid_package_json() {
        let root = make_temp_dir("sidecar-package-manager-invalid");
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", root.display()));
        std::fs::write(root.join("package.json"), "{")
            .unwrap_or_else(|err| panic!("failed to write package.json: {err}"));
        std::fs::write(root.join("package-lock.json"), "")
            .unwrap_or_else(|err| panic!("failed to write package-lock.json: {err}"));

        assert_eq!(
            super::detect_package_manager(&root),
            Some(super::LocalPackageManager::Npm)
        );

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn package_store_sidecar_prefers_highest_version() {
        let root = make_temp_dir("sidecar-store-version");
        let node_modules = root.join("node_modules");
        let store = node_modules.join(".pnpm");
        for version in ["0.1.4", "0.1.8"] {
            let package_dir = store
                .join(format!("@fallow-cli+fallow-cov-darwin-arm64@{version}"))
                .join("node_modules")
                .join("@fallow-cli")
                .join("fallow-cov-darwin-arm64");
            std::fs::create_dir_all(&package_dir)
                .unwrap_or_else(|err| panic!("failed to create {}: {err}", package_dir.display()));
            std::fs::write(
                package_dir.join("package.json"),
                format!(r#"{{"version":"{version}"}}"#),
            )
            .unwrap_or_else(|err| panic!("failed to write package.json: {err}"));
            std::fs::write(package_dir.join(sidecar_binary_name()), "")
                .unwrap_or_else(|err| panic!("failed to write sidecar: {err}"));
        }

        let resolved = super::find_package_store_platform_sidecar(&node_modules, ".pnpm")
            .expect("store sidecar should resolve");

        assert!(resolved.display().to_string().contains("0.1.8"));

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn resolves_yarn_sidecar_without_node_modules_bin() {
        let root = make_temp_dir("sidecar-yarn");
        let unplugged_dir = root
            .join(".yarn")
            .join("unplugged")
            .join("fallow-cov")
            .join("node_modules")
            .join(".bin");
        std::fs::create_dir_all(&unplugged_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", unplugged_dir.display()));

        let sidecar = if cfg!(windows) {
            unplugged_dir.join("fallow-cov.cmd")
        } else {
            unplugged_dir.join("fallow-cov")
        };
        std::fs::write(&sidecar, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", sidecar.display()));

        // `yarn bin fallow-cov` prints the absolute sidecar path on its last
        // line. Parse that output directly instead of spawning yarn, which
        // flaked under the instrumented coverage run.
        let stdout = format!("{}\n", sidecar.display());
        let resolved =
            resolve_sidecar_from_output(&root, &stdout, PackageManagerOutput::BinaryPath)
                .unwrap_or_else(|| panic!("failed to resolve yarn-local sidecar"));

        assert_eq!(resolved, sidecar);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn resolves_npm_sidecar_from_node_modules_root() {
        let root = make_temp_dir("sidecar-npm");
        let bin_dir = root.join("custom-node_modules").join(".bin");
        std::fs::create_dir_all(&bin_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", bin_dir.display()));

        let sidecar = if cfg!(windows) {
            bin_dir.join("fallow-cov.cmd")
        } else {
            bin_dir.join("fallow-cov")
        };
        std::fs::write(&sidecar, "")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", sidecar.display()));

        // `npm root` prints the node_modules dir; NodeModulesDir resolution
        // appends `.bin` plus the sidecar name. Parse the printed dir directly
        // instead of spawning npm.
        let stdout = format!("{}\n", root.join("custom-node_modules").display());
        let resolved =
            resolve_sidecar_from_output(&root, &stdout, PackageManagerOutput::NodeModulesDir)
                .unwrap_or_else(|| panic!("failed to resolve npm-local sidecar"));

        assert_eq!(resolved, sidecar);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn package_manager_sidecar_resolution_ignores_missing_commands() {
        let root = make_temp_dir("sidecar-missing-command");
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", root.display()));

        let resolved = resolve_sidecar_via_command(
            &root,
            std::ffi::OsStr::new("definitely-not-fallow-cov-manager"),
            &["bin"],
            PackageManagerOutput::BinDir,
        );

        assert_eq!(resolved, None);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn parse_source_map_cache_rejects_missing_or_malformed_cache() {
        let missing = fallow_v8_coverage::V8CoverageDump {
            result: Vec::new(),
            source_map_cache: None,
        };
        assert!(super::parse_source_map_cache(&missing).is_none());

        let malformed = fallow_v8_coverage::V8CoverageDump {
            result: Vec::new(),
            source_map_cache: Some(serde_json::json!("not a cache object")),
        };
        assert!(super::parse_source_map_cache(&malformed).is_none());
    }

    #[test]
    fn ensure_temp_dir_reuses_existing_directory() {
        let mut temp_dir = None;
        let first = super::ensure_temp_dir(&mut temp_dir)
            .unwrap_or_else(|err| panic!("failed to create tempdir: {err}"))
            .to_path_buf();
        let second = super::ensure_temp_dir(&mut temp_dir)
            .unwrap_or_else(|err| panic!("failed to reuse tempdir: {err}"))
            .to_path_buf();

        assert_eq!(first, second);
    }

    #[test]
    fn generated_source_for_script_reads_file_urls_only() {
        let root = make_temp_dir("coverage-generated-source");
        std::fs::create_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", root.display()));
        let source = root.join("bundle.js");
        std::fs::write(&source, "function alpha() {}\n")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", source.display()));
        let source_url = file_url(&source);
        let script = script_coverage_with_url(&source_url);

        assert_eq!(
            super::generated_source_for_script(&script),
            Some("function alpha() {}\n".to_owned())
        );

        let remote = script_coverage_with_url("https://cdn.example.com/bundle.js");
        assert_eq!(super::generated_source_for_script(&remote), None);

        let missing_url = file_url(&root.join("missing.js"));
        let missing = script_coverage_with_url(&missing_url);
        assert_eq!(super::generated_source_for_script(&missing), None);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn static_function_round_trips() {
        let sf = static_function(StaticFunctionInput {
            relative_posix: "src/render.tsx",
            name: "render",
            start_line: 42,
            end_line: 50,
            cyclomatic: 4,
            static_used: true,
            test_covered: false,
            caller_count: 0,
            owner_count: None,
            source_hash: Some("0123456789abcdef".to_owned()),
        });
        let value = serde_json::to_value(&sf).expect("serialize StaticFunction");
        assert_eq!(value["name"], "render");
        assert_eq!(value["start_line"], 42);
        assert_eq!(value["end_line"], 50);
        assert_eq!(value["identity"]["resolution"], "unresolved");
        assert_eq!(value["identity"]["stable_id"], "fallow:fn:cb4482d6aef7c79a");
        assert_eq!(value["identity"]["source_hash"], "0123456789abcdef");
        assert!(value["identity"].get("start_column").is_none());
    }

    /// Build a protocol [`Finding`] in tests. `Finding` is `#[non_exhaustive]`
    /// with no constructor, so we round-trip through serde (the enum / nested
    /// values are serialized from constructible typed values to avoid guessing
    /// wire reprs). `identity` lets a test exercise the v2 join-key threading.
    fn proto_finding(identity: Option<&FunctionIdentity>) -> Finding {
        serde_json::from_value(serde_json::json!({
            "id": "fallow:prod:abc12345",
            "file": "src/app.ts",
            "function": "alpha",
            "line": 8,
            "verdict": serde_json::to_value(Verdict::ReviewRequired).unwrap(),
            "invocations": 0,
            "confidence": serde_json::to_value(Confidence::Medium).unwrap(),
            "evidence": serde_json::to_value(Evidence {
                static_status: "used".to_owned(),
                test_coverage: "not_covered".to_owned(),
                v8_tracking: "tracked".to_owned(),
                untracked_reason: None,
                observation_days: 7,
                deployments_observed: 2,
            })
            .unwrap(),
            "actions": [],
            "identity": identity,
        }))
        .expect("valid Finding json")
    }

    fn proto_hot_path() -> HotPath {
        serde_json::from_value(serde_json::json!({
            "id": "fallow:hot:def67890",
            "file": "src/app.ts",
            "function": "alpha",
            "line": 8,
            "end_line": 12,
            "invocations": 20,
            "percentile": 50,
        }))
        .expect("valid HotPath json")
    }

    #[test]
    fn convert_response_threads_identity_stable_id() {
        let identity = FunctionIdentity {
            file: "src/app.ts".to_owned(),
            name: "alpha".to_owned(),
            start_line: 8,
            start_column: None,
            end_line: None,
            end_column: None,
            source_hash: None,
            resolution: IdentityResolution::Resolved,
            stable_id: function_identity_id("src/app.ts", "alpha", 8),
        };
        let response = Response {
            protocol_version: PROTOCOL_VERSION.to_owned(),
            verdict: ReportVerdict::ColdCodeDetected,
            summary: Summary {
                functions_tracked: 1,
                functions_hit: 0,
                functions_unhit: 1,
                functions_untracked: 0,
                coverage_percent: 0.0,
                trace_count: 1,
                period_days: 1,
                deployments_seen: 1,
                capture_quality: None,
            },
            findings: vec![proto_finding(Some(&identity))],
            hot_paths: vec![],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            errors: vec![],
            warnings: vec![],
        };
        let report = convert_response(response, &FxHashMap::default(), None, 5_000, 0.001);
        assert_eq!(report.findings[0].stable_id, Some(identity.stable_id));
    }

    #[test]
    fn convert_response_finding_without_identity_has_no_stable_id() {
        let response = Response {
            protocol_version: PROTOCOL_VERSION.to_owned(),
            verdict: ReportVerdict::ColdCodeDetected,
            summary: Summary {
                functions_tracked: 1,
                functions_hit: 0,
                functions_unhit: 1,
                functions_untracked: 0,
                coverage_percent: 0.0,
                trace_count: 1,
                period_days: 1,
                deployments_seen: 1,
                capture_quality: None,
            },
            findings: vec![proto_finding(None)],
            hot_paths: vec![],
            blast_radius: vec![],
            importance: vec![],
            watermark: None,
            errors: vec![],
            warnings: vec![],
        };
        let report = convert_response(response, &FxHashMap::default(), None, 5_000, 0.001);
        assert_eq!(report.findings[0].stable_id, None);
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
                findings: vec![proto_finding(None)],
                hot_paths: vec![proto_hot_path()],
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
            5_000,
            0.001,
        );

        assert_eq!(report.findings[0].id, "fallow:prod:abc12345");
        assert_eq!(report.findings[0].line, 8);
        assert_eq!(
            report.findings[0].verdict,
            fallow_output::RuntimeCoverageVerdict::ReviewRequired,
        );
        assert_eq!(report.findings[0].evidence.static_status, "used");
        assert_eq!(report.hot_paths[0].id, "fallow:hot:def67890");
        assert_eq!(report.hot_paths[0].percentile, 50);

        // #321: the merge pipeline emits a discriminator block that reproduces
        // the verdict. proto_finding is tracked with 0 invocations => the
        // three-state signal is never_called; ratio is 0/512; trace_count 512 is
        // below the 5000 floor so the confidence cap is visible.
        let discriminators = report.findings[0]
            .discriminators
            .as_ref()
            .expect("merge-pipeline findings carry discriminators");
        assert_eq!(discriminators.tracking_state, "never_called");
        assert_eq!(discriminators.invocation_ratio, Some(0.0));
        assert_eq!(discriminators.trace_count, 512);
        assert!((discriminators.low_traffic_threshold - 0.001).abs() < f64::EPSILON);
        assert_eq!(discriminators.min_observation_volume, 5_000);
        assert!(!discriminators.meets_observation_volume);

        // #316 / #319: tracked functions present => actionable, with a local,
        // origin-unknown, fresh provenance mirroring the cloud contract.
        assert!(report.actionable);
        assert_eq!(report.actionability_reason, None);
        assert_eq!(report.actionability_verdict, None);
        assert_eq!(
            report.provenance.data_source,
            fallow_output::RuntimeCoverageDataSource::Local,
        );
        assert_eq!(report.provenance.is_production, "unknown");
        assert_eq!(report.provenance.freshness_days, Some(0));
        assert!(!report.provenance.stale);
        assert_eq!(report.provenance.stale_after_days, 14);
    }

    #[test]
    fn convert_response_with_no_tracked_functions_is_non_actionable() {
        let report = convert_response(
            Response {
                protocol_version: "0.2.0".to_owned(),
                verdict: ReportVerdict::Clean,
                summary: Summary {
                    functions_tracked: 0,
                    functions_hit: 0,
                    functions_unhit: 0,
                    functions_untracked: 2,
                    coverage_percent: 0.0,
                    trace_count: 0,
                    period_days: 0,
                    deployments_seen: 0,
                    capture_quality: None,
                },
                findings: vec![],
                hot_paths: vec![],
                blast_radius: vec![],
                importance: vec![],
                watermark: None,
                errors: vec![],
                warnings: vec![],
            },
            &FxHashMap::default(),
            None,
            5_000,
            0.001,
        );

        // #316: no tracked functions => no usable runtime evidence => the report
        // is non-actionable with a first-class insufficient_evidence verdict,
        // never read as cold. #319: untracked_ratio reflects the thin capture.
        assert!(!report.actionable);
        assert_eq!(
            report.actionability_verdict.as_deref(),
            Some("insufficient_evidence"),
        );
        assert!(
            report
                .actionability_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("NOT cold")),
        );
        assert!((report.provenance.untracked_ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn tracking_state_label_maps_three_states() {
        // untracked wins regardless of invocation count.
        assert_eq!(tracking_state_label("untracked", None), "untracked");
        assert_eq!(tracking_state_label("untracked", Some(9)), "untracked");
        // tracked + invoked => called; tracked + zero => never_called.
        assert_eq!(tracking_state_label("tracked", Some(3)), "called");
        assert_eq!(tracking_state_label("tracked", Some(0)), "never_called");
        assert_eq!(tracking_state_label("tracked", None), "never_called");
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
        let analysis_output = empty_analysis_output();
        let file_paths = FxHashMap::default();

        let (request, _locations) = build_request(
            &options,
            &RuntimeCoverageAnalysisInput {
                root: &root,
                modules: &[],
                analysis_output: &analysis_output,
                istanbul_coverage: None,
                file_paths: &file_paths,
                ignore_set: &ignore_set,
                changed_files: None,
                ws_roots: Some(&ws_roots),
                top: None,
                codeowners_path: None,
                quiet: true,
                output: OutputFormat::Json,
            },
            &StaticSignalIndex::default(),
            vec![],
        );

        assert_eq!(request.project_root, ws_root.to_string_lossy());
    }

    #[test]
    #[expect(
        clippy::too_many_lines,
        reason = "test fixture; linear setup/assert, length is not a maintainability concern"
    )]
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

        let parsed = fallow_engine::AnalysisSession::from_resolved_config(
            FallowConfig::default().resolve(root.clone(), OutputFormat::Json, 1, true, true, None),
        )
        .into_parsed_parts(true);
        let config = parsed.config;
        let files = parsed.files;
        let modules = parsed.modules;
        let file_paths: FxHashMap<_, _> = files.iter().map(|file| (file.id, &file.path)).collect();
        let analysis_output = fallow_engine::analyze_with_parse_result(&config, &modules)
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
            &RuntimeCoverageAnalysisInput {
                root: &root,
                modules: &modules,
                analysis_output: &analysis_output,
                istanbul_coverage: None,
                file_paths: &file_paths,
                ignore_set: &ignore_set,
                changed_files: None,
                ws_roots: None,
                top: None,
                codeowners_path: None,
                quiet: true,
                output: OutputFormat::Json,
            },
            &static_signals,
            vec![],
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
    fn build_request_applies_changed_workspace_and_ignore_filters() {
        let root = make_temp_dir("coverage-request-filters");
        let app_dir = root.join("packages").join("app");
        let other_dir = root.join("packages").join("other");
        std::fs::create_dir_all(app_dir.join("src"))
            .unwrap_or_else(|err| panic!("failed to create app src dir: {err}"));
        std::fs::create_dir_all(other_dir.join("src"))
            .unwrap_or_else(|err| panic!("failed to create other src dir: {err}"));
        std::fs::write(root.join("package.json"), r#"{"name":"demo"}"#)
            .unwrap_or_else(|err| panic!("failed to write package.json: {err}"));
        std::fs::write(
            app_dir.join("src").join("app.ts"),
            "export function included() { return 1; }\n",
        )
        .unwrap_or_else(|err| panic!("failed to write app.ts: {err}"));
        std::fs::write(
            app_dir.join("src").join("ignored.ts"),
            "export function ignored() { return 2; }\n",
        )
        .unwrap_or_else(|err| panic!("failed to write ignored.ts: {err}"));
        std::fs::write(
            other_dir.join("src").join("other.ts"),
            "export function outside() { return 3; }\n",
        )
        .unwrap_or_else(|err| panic!("failed to write other.ts: {err}"));

        let parsed = fallow_engine::AnalysisSession::from_resolved_config(
            FallowConfig::default().resolve(root.clone(), OutputFormat::Json, 1, true, true, None),
        )
        .into_parsed_parts(true);
        let files = parsed.files;
        let modules = parsed.modules;
        let file_paths: FxHashMap<_, _> = files.iter().map(|file| (file.id, &file.path)).collect();
        let analysis_output = empty_analysis_output();
        let mut changed_files = FxHashSet::default();
        changed_files.insert(app_dir.join("src").join("app.ts"));
        changed_files.insert(app_dir.join("src").join("ignored.ts"));
        changed_files.insert(other_dir.join("src").join("other.ts"));
        let ws_roots = [app_dir];
        let mut ignore_builder = GlobSetBuilder::new();
        ignore_builder.add(
            Glob::new("packages/app/src/ignored.ts")
                .unwrap_or_else(|err| panic!("failed to build ignore glob: {err}")),
        );
        let ignore_set = ignore_builder
            .build()
            .unwrap_or_else(|err| panic!("failed to build ignore set: {err}"));
        let options = RuntimeCoverageOptions {
            path: root.join("coverage"),
            min_invocations_hot: 100,
            min_observation_volume: None,
            low_traffic_threshold: None,
            license_jwt: "test-jwt".to_owned(),
            watermark: None,
        };

        let (request, _locations) = build_request(
            &options,
            &RuntimeCoverageAnalysisInput {
                root: &root,
                modules: &modules,
                analysis_output: &analysis_output,
                istanbul_coverage: None,
                file_paths: &file_paths,
                ignore_set: &ignore_set,
                changed_files: Some(&changed_files),
                ws_roots: Some(&ws_roots),
                top: None,
                codeowners_path: None,
                quiet: true,
                output: OutputFormat::Json,
            },
            &StaticSignalIndex::default(),
            vec![],
        );

        let paths = request
            .static_findings
            .files
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths, vec!["packages/app/src/app.ts"]);

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
    fn remaps_v8_offsets_as_utf16_source_positions() {
        let root = make_temp_dir("coverage-remap-utf16");
        let src_dir = root.join("src");
        let dist_dir = root.join("dist");
        std::fs::create_dir_all(&src_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", src_dir.display()));
        std::fs::create_dir_all(&dist_dir)
            .unwrap_or_else(|err| panic!("failed to create {}: {err}", dist_dir.display()));

        let original = src_dir.join("app.ts");
        std::fs::write(&original, "export function alpha() {}\n")
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", original.display()));

        let generated = "const smile = \"😀\";\nfunction alpha() {}\n";
        let generated_path = dist_dir.join("bundle.js");
        std::fs::write(&generated_path, generated)
            .unwrap_or_else(|err| panic!("failed to write {}: {err}", generated_path.display()));
        let function_byte_offset = generated
            .find("function")
            .expect("generated source should contain function");
        let function_utf16_offset = generated[..function_byte_offset].encode_utf16().count();
        assert_ne!(function_utf16_offset, function_byte_offset);
        let function_v8_offset = function_utf16_offset as u32;

        let v8_file = root.join("coverage-v8.json");
        let v8_json = serde_json::json!({
            "result": [{
                "scriptId": "1",
                "url": file_url(&generated_path),
                "functions": [{
                    "functionName": "alpha",
                    "ranges": [{"startOffset": function_v8_offset, "endOffset": function_v8_offset + 19, "count": 3}],
                    "isBlockCoverage": false
                }]
            }],
            "source-map-cache": {
                file_url(&generated_path): {
                    "url": "bundle.js.map",
                    "data": {
                        "version": 3,
                        "sources": ["../src/app.ts"],
                        "names": [],
                        "mappings": ";AAAA"
                    },
                    "lineLengths": [20, 19]
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
        assert_eq!(parsed[&key]["fnMap"]["0"]["name"], "alpha");
        assert_eq!(parsed[&key]["fnMap"]["0"]["line"], 1);
        assert_eq!(parsed[&key]["f"]["0"], 3);

        std::fs::remove_dir_all(&root)
            .unwrap_or_else(|err| panic!("failed to clean temp dir {}: {err}", root.display()));
    }

    #[test]
    fn keeps_mapped_functions_when_other_functions_cannot_be_remapped() {
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

        assert_eq!(prepared.sources.len(), 2);
        let CoverageSource::Istanbul {
            path: remapped_path,
        } = &prepared.sources[0]
        else {
            panic!("expected remapped istanbul coverage source");
        };
        let remapped_output = std::fs::read_to_string(remapped_path).unwrap_or_else(|err| {
            panic!("failed to read remapped coverage {remapped_path}: {err}")
        });
        let remapped: serde_json::Value = serde_json::from_str(&remapped_output)
            .unwrap_or_else(|err| panic!("failed to parse remapped coverage: {err}"));
        let key = dunce::canonicalize(&original)
            .unwrap_or_else(|err| panic!("failed to canonicalize {}: {err}", original.display()))
            .to_string_lossy()
            .into_owned();

        assert_eq!(remapped[&key]["fnMap"]["0"]["name"], "alpha");
        assert_eq!(remapped[&key]["f"]["0"], 3);

        let CoverageSource::V8 {
            path: residual_path,
        } = &prepared.sources[1]
        else {
            panic!("expected residual v8 coverage source");
        };
        let residual_output = std::fs::read_to_string(residual_path).unwrap_or_else(|err| {
            panic!("failed to read residual coverage {residual_path}: {err}")
        });
        let residual: serde_json::Value = serde_json::from_str(&residual_output)
            .unwrap_or_else(|err| panic!("failed to parse residual coverage: {err}"));
        let residual_functions = residual["result"][0]["functions"]
            .as_array()
            .expect("residual functions array");
        assert_eq!(residual_functions.len(), 1);
        assert_eq!(residual_functions[0]["functionName"], "broken");

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

        let mut files: BTreeMap<PathBuf, BTreeMap<RemappedFnKey, AccumulatedFunction>> =
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

    fn script_coverage_with_url(url: &str) -> fallow_v8_coverage::ScriptCoverage {
        serde_json::from_value(serde_json::json!({
            "scriptId": "1",
            "url": url,
            "functions": []
        }))
        .expect("valid script coverage")
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

    #[test]
    fn parse_sidecar_version_key_splits_on_non_digits() {
        assert_eq!(super::parse_sidecar_version_key("1.2.3"), vec![1, 2, 3]);
        // Prerelease and build separators are non-digit boundaries.
        assert_eq!(
            super::parse_sidecar_version_key("0.1.5-beta.2"),
            vec![0, 1, 5, 2]
        );
        assert_eq!(super::parse_sidecar_version_key("v2"), vec![2]);
        assert!(super::parse_sidecar_version_key("").is_empty());
    }

    #[test]
    fn sidecar_package_version_key_reads_sibling_package_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(dir.path().join("package.json"), r#"{"version":"3.4.5"}"#).unwrap();
        let binary = dir.path().join("fallow-cov");
        assert_eq!(super::sidecar_package_version_key(&binary), vec![3, 4, 5]);

        // No package.json next to the binary -> empty key, never a panic.
        let bare = tempfile::tempdir().expect("tempdir");
        assert!(super::sidecar_package_version_key(&bare.path().join("fallow-cov")).is_empty());
    }

    #[test]
    fn normalize_package_manager_path_joins_relative_against_root() {
        let root = Path::new("/proj");
        assert_eq!(
            super::normalize_package_manager_path(root, "node_modules/.bin/fallow-cov"),
            PathBuf::from("/proj/node_modules/.bin/fallow-cov")
        );
        // Absolute candidates are returned unchanged.
        let abs = if cfg!(windows) {
            "C:\\tools\\fallow-cov"
        } else {
            "/tools/fallow-cov"
        };
        assert_eq!(
            super::normalize_package_manager_path(root, abs),
            PathBuf::from(abs)
        );
    }

    #[test]
    fn project_local_sidecar_names_include_the_bare_binary() {
        let names = super::project_local_sidecar_names();
        assert!(!names.is_empty());
        assert!(names.contains(&"fallow-cov"));
    }

    #[test]
    fn sidecar_missing_message_lists_checked_locations() {
        // Without a project root: canonical path, PATH, and the default npm hint.
        let generic = super::sidecar_missing_message(None);
        assert!(generic.contains("PATH"), "got: {generic}");
        assert!(generic.contains("npm install --save-dev @fallow-cli/fallow-cov"));
        assert!(generic.contains("FALLOW_COV_BIN"));

        // With a pnpm project root: the node_modules bin path and a pnpm hint.
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"p","packageManager":"pnpm@9.0.0"}"#,
        )
        .unwrap();
        let scoped = super::sidecar_missing_message(Some(dir.path()));
        assert!(
            scoped.contains("node_modules/.bin/fallow-cov"),
            "got: {scoped}"
        );
        assert!(scoped.contains("pnpm"), "pnpm hint missing: {scoped}");
    }

    #[test]
    fn sidecar_missing_message_uses_detected_package_manager_guidance() {
        for (name, package_json, lockfile, hint, install) in [
            (
                "npm",
                r#"{"name":"p","packageManager":"npm@10.0.0"}"#,
                "package-lock.json",
                "`npm root` + `.bin/fallow-cov`",
                "npm install --save-dev @fallow-cli/fallow-cov",
            ),
            (
                "pnpm",
                r#"{"name":"p","packageManager":"pnpm@9.0.0"}"#,
                "pnpm-lock.yaml",
                "`pnpm bin`",
                "pnpm add -D @fallow-cli/fallow-cov",
            ),
            (
                "yarn",
                r#"{"name":"p","packageManager":"yarn@4.0.0"}"#,
                "yarn.lock",
                "`yarn bin fallow-cov`",
                "yarn add -D @fallow-cli/fallow-cov",
            ),
            (
                "bun",
                r#"{"name":"p","packageManager":"bun@1.2.0"}"#,
                "bun.lock",
                "`bun pm bin`",
                "bun add -d @fallow-cli/fallow-cov",
            ),
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            std::fs::write(dir.path().join("package.json"), package_json)
                .unwrap_or_else(|err| panic!("failed to write package.json for {name}: {err}"));
            std::fs::write(dir.path().join(lockfile), "")
                .unwrap_or_else(|err| panic!("failed to write {lockfile}: {err}"));

            let message = super::sidecar_missing_message(Some(dir.path()));

            assert!(
                message.contains("node_modules/.bin/fallow-cov"),
                "{name} message should list project-local bin path: {message}"
            );
            assert!(
                message.contains(hint),
                "{name} message should include lookup hint {hint}: {message}"
            );
            assert!(
                message.contains(install),
                "{name} message should include install command {install}: {message}"
            );
        }
    }

    #[test]
    fn detect_package_manager_prefers_package_json_field() {
        let dir = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"packageManager":"bun@1.2.0"}"#,
        )
        .expect("write package.json");
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "").expect("write lockfile");

        assert!(matches!(
            super::detect_package_manager(dir.path()),
            Some(super::LocalPackageManager::Bun)
        ));
    }

    #[test]
    fn detect_package_manager_falls_back_to_lockfiles() {
        for (lockfile, expected) in [
            ("bun.lock", super::LocalPackageManager::Bun),
            ("pnpm-lock.yaml", super::LocalPackageManager::Pnpm),
            ("yarn.lock", super::LocalPackageManager::Yarn),
            ("package-lock.json", super::LocalPackageManager::Npm),
            ("npm-shrinkwrap.json", super::LocalPackageManager::Npm),
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            std::fs::write(dir.path().join(lockfile), "").expect("write lockfile");

            assert!(
                matches!(super::detect_package_manager(dir.path()), Some(actual) if actual == expected),
                "failed for {lockfile}"
            );
        }
    }

    #[test]
    fn package_manager_install_commands_match_detected_tool() {
        for (manager, install, hint) in [
            (
                super::LocalPackageManager::Npm,
                "npm install --save-dev @fallow-cli/fallow-cov",
                "`npm root` + `.bin/fallow-cov`",
            ),
            (
                super::LocalPackageManager::Pnpm,
                "pnpm add -D @fallow-cli/fallow-cov",
                "`pnpm bin`",
            ),
            (
                super::LocalPackageManager::Yarn,
                "yarn add -D @fallow-cli/fallow-cov",
                "`yarn bin fallow-cov`",
            ),
            (
                super::LocalPackageManager::Bun,
                "bun add -d @fallow-cli/fallow-cov",
                "`bun pm bin`",
            ),
        ] {
            assert_eq!(manager.install_command(), install);
            assert_eq!(manager.lookup_hint(), hint);
        }
    }

    #[test]
    fn utf16_offset_maps_surrogate_pairs_to_byte_offsets() {
        // "a😀b": 'a'=1 byte/1 utf16, '😀'=4 bytes/2 utf16, 'b'=1 byte/1 utf16.
        let s = "a😀b";
        assert_eq!(super::utf16_source_offset_to_byte_offset(s, 0), Some(0));
        assert_eq!(super::utf16_source_offset_to_byte_offset(s, 1), Some(1)); // start of emoji
        assert_eq!(super::utf16_source_offset_to_byte_offset(s, 3), Some(5)); // start of 'b'
        assert_eq!(super::utf16_source_offset_to_byte_offset(s, 4), Some(6)); // end of string
        // An offset landing inside the surrogate pair is unmappable.
        assert_eq!(super::utf16_source_offset_to_byte_offset(s, 2), None);
        // Past the end is unmappable.
        assert_eq!(super::utf16_source_offset_to_byte_offset(s, 5), None);

        let ascii = "alpha\nbeta";
        for (utf16_offset, byte_offset) in [(0, 0), (5, 5), (6, 6), (10, 10)] {
            assert_eq!(
                super::utf16_source_offset_to_byte_offset(ascii, utf16_offset),
                Some(byte_offset)
            );
        }
        assert_eq!(super::utf16_source_offset_to_byte_offset(ascii, 11), None);

        assert_eq!(super::utf16_source_offset_to_byte_offset("", 0), Some(0));
        assert_eq!(super::utf16_source_offset_to_byte_offset("", 1), None);
        assert_eq!(super::utf16_source_offset_to_byte_offset("å", 0), Some(0));
        assert_eq!(super::utf16_source_offset_to_byte_offset("å", 1), Some(2));
        assert_eq!(super::utf16_source_offset_to_byte_offset("å", 2), None);
        assert_eq!(
            super::utf16_source_offset_to_byte_offset("a😀b\nc", 4),
            Some(6)
        );
        assert_eq!(
            super::utf16_source_offset_to_byte_offset("a😀b\nc", 5),
            Some(7)
        );
        assert_eq!(
            super::utf16_source_offset_to_byte_offset("a😀b\nc", 6),
            Some(8)
        );
    }

    #[test]
    fn file_url_to_path_accepts_file_urls_and_absolute_paths_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("app.js");
        let url = file_url(&path);
        assert_eq!(super::file_url_to_path(&url), Some(path));
        // Non-file URLs and relative paths are rejected.
        assert_eq!(super::file_url_to_path("https://example.com/app.js"), None);
        assert_eq!(super::file_url_to_path("relative/app.js"), None);
    }

    #[test]
    fn resolve_source_map_base_handles_inline_relative_and_remote_urls() {
        let dir = tempfile::tempdir().expect("tempdir");
        let generated = file_url(&dir.path().join("app.js"));
        // No source-map URL: base is the generated file's directory.
        assert_eq!(
            super::resolve_source_map_base(&generated, None),
            Some(dir.path().to_path_buf())
        );
        // Relative .map URL resolves against the generated directory.
        assert_eq!(
            super::resolve_source_map_base(&generated, Some("app.js.map")),
            Some(dir.path().to_path_buf())
        );
        // A remote http(s) source-map URL has no local base.
        assert_eq!(
            super::resolve_source_map_base(&generated, Some("https://cdn.example.com/app.js.map")),
            None
        );
    }

    #[test]
    fn resolve_original_source_path_handles_file_absolute_and_relative_sources() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src/app.ts");
        let generated = file_url(&dir.path().join("dist/app.js"));

        assert_eq!(
            super::resolve_original_source_path("", &generated, None),
            None
        );
        assert_eq!(
            super::resolve_original_source_path(&file_url(&source), &generated, None),
            Some(source.clone())
        );
        assert_eq!(
            super::resolve_original_source_path(
                source.to_str().expect("utf8 path"),
                &generated,
                None
            ),
            Some(source)
        );
        assert_eq!(
            super::resolve_original_source_path("src/app.ts", &generated, None),
            Some(dir.path().join("dist/src/app.ts"))
        );
    }

    #[test]
    fn resolve_original_source_path_resolves_virtual_sources_from_ancestors() {
        let dir = tempfile::tempdir().expect("tempdir");
        let source = dir.path().join("src/app.ts");
        std::fs::create_dir_all(source.parent().expect("source parent"))
            .expect("create source dir");
        std::fs::write(&source, "export const app = true;").expect("write source");
        let generated = file_url(&dir.path().join("dist/chunks/app.js"));

        assert_eq!(
            super::resolve_original_source_path("webpack://app/src/app.ts", &generated, None),
            Some(source)
        );
    }

    #[test]
    fn virtual_source_candidates_strip_known_pseudo_hosts() {
        let url = Url::parse("webpack://_N_E/./src/app.ts").expect("url");
        let candidates = super::virtual_source_candidates(&url);
        // The `_N_E` pseudo-host is dropped and the url crate normalizes the
        // `/.` segment, leaving the path-only candidate.
        assert!(
            candidates.contains(&PathBuf::from("src/app.ts")),
            "got: {candidates:?}"
        );
        assert!(!candidates.iter().any(|c| c.starts_with("_N_E")));
    }

    #[test]
    fn resolve_virtual_source_path_ignores_non_virtual_schemes() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            super::resolve_virtual_source_path("https://example.com/app.ts", dir.path()),
            None
        );
    }

    #[test]
    fn location_precedes_orders_by_line_then_column() {
        let p = |line, column| Position { line, column };
        assert!(super::location_precedes(&p(1, 0), &p(2, 0)));
        assert!(super::location_precedes(&p(3, 4), &p(3, 9)));
        assert!(!super::location_precedes(&p(3, 9), &p(3, 4)));
        assert!(!super::location_precedes(&p(2, 0), &p(2, 0)));
    }
}
