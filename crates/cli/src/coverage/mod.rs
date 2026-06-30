//! `fallow coverage` - runtime coverage onboarding and inventory upload.
//!
//! Today the subtree holds four commands:
//!
//! - `setup`: resumable first-run state machine (optional license + sidecar
//!   + recipe + auto-handoff to `fallow health --runtime-coverage`).
//! - `analyze`: focused runtime coverage analysis. Local mode reads a coverage
//!   artifact; cloud mode explicitly fetches runtime facts from fallow cloud.
//! - `upload-inventory`: push a static function inventory to fallow cloud,
//!   unlocking the `untracked` filter on the dashboard by pairing runtime
//!   coverage data with the AST view of "every function that exists".
//! - `upload-source-maps`: push build source maps so bundled runtime coverage
//!   can resolve back to original source files.

use std::ffi::OsStr;
use std::fmt::Write as FmtWrite;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use fallow_config::{OutputFormat, PackageJson, WorkspaceInfo, atomic_write, discover_workspaces};
use fallow_engine::clear_ambient_git_env;
use fallow_license::{DEFAULT_HARD_FAIL_DAYS, LicenseStatus};
use fallow_types::serde_path;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::health::coverage as runtime_coverage;
use crate::license;

pub use analyze::AnalyzeArgs;
pub use upload_inventory::UploadInventoryArgs;
pub use upload_source_maps::UploadSourceMapsArgs;
pub use upload_static_findings::UploadStaticFindingsArgs;

mod analyze;
mod cloud_client;
mod upload_common;
mod upload_inventory;
mod upload_source_maps;
mod upload_static_findings;

const COVERAGE_DOCS_URL: &str = "https://docs.fallow.tools/analysis/runtime-coverage";
const SETUP_STATE_SCHEMA_VERSION: u8 = 1;

/// Subcommands for `fallow coverage`.
#[derive(Debug, Clone)]
pub enum CoverageSubcommand {
    /// Resumable first-run setup flow.
    Setup(SetupArgs),
    /// Analyze runtime coverage from a local artifact or explicit cloud source.
    Analyze(AnalyzeArgs),
    /// Upload a static function inventory to fallow cloud.
    UploadInventory(UploadInventoryArgs),
    /// Upload JavaScript source maps to fallow cloud.
    UploadSourceMaps(UploadSourceMapsArgs),
    /// Upload static dead-code findings to fallow cloud.
    UploadStaticFindings(UploadStaticFindingsArgs),
}

/// Context shared by `fallow coverage` subcommands.
pub struct RunContext<'a> {
    pub root: &'a Path,
    pub config_path: &'a Option<PathBuf>,
    pub output: OutputFormat,
    pub quiet: bool,
    pub no_cache: bool,
    pub threads: usize,
    pub explain: bool,
}

/// Arguments for `fallow coverage setup`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SetupArgs {
    /// Accept all prompts automatically.
    pub yes: bool,
    /// Print instructions instead of prompting.
    pub non_interactive: bool,
    /// Emit deterministic JSON instructions without prompts, writes, installs, or network calls.
    pub json: bool,
    /// Include field definitions and warning semantics in JSON output.
    pub explain: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameworkKind {
    NextJs,
    NestJs,
    Nuxt,
    SvelteKit,
    Astro,
    Remix,
    ViteBrowser,
    PlainNode,
    Other,
}

impl FrameworkKind {
    const fn label(self) -> &'static str {
        match self {
            Self::NextJs => "Next.js project",
            Self::NestJs => "NestJS project",
            Self::Nuxt => "Nuxt app",
            Self::SvelteKit => "SvelteKit app",
            Self::Astro => "Astro app",
            Self::Remix => "Remix app",
            Self::ViteBrowser => "Vite browser app",
            Self::PlainNode => "Node service",
            Self::Other => "custom project",
        }
    }

    const fn runtime_targets(self) -> &'static [&'static str] {
        match self {
            Self::NextJs | Self::Nuxt | Self::SvelteKit | Self::Astro | Self::Remix => {
                &["node", "browser"]
            }
            Self::ViteBrowser => &["browser"],
            Self::NestJs | Self::PlainNode | Self::Other => &["node"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

impl PackageManager {
    const fn label(self) -> &'static str {
        match self {
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Bun => "bun",
        }
    }

    fn add_runtime_package_command(self, package: &str) -> String {
        match self {
            Self::Npm => format!("npm install {package}"),
            Self::Pnpm => format!("pnpm add {package}"),
            Self::Yarn => format!("yarn add {package}"),
            Self::Bun => format!("bun add {package}"),
        }
    }

    const fn install_args(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Npm => ("npm", &["install", "--save-dev", "@fallow-cli/fallow-cov"]),
            Self::Pnpm => ("pnpm", &["add", "-D", "@fallow-cli/fallow-cov"]),
            Self::Yarn => ("yarn", &["add", "-D", "@fallow-cli/fallow-cov"]),
            Self::Bun => ("bun", &["add", "-d", "@fallow-cli/fallow-cov"]),
        }
    }

    fn install_command(self) -> String {
        let (program, args) = self.install_args();
        format!("{program} {}", args.join(" "))
    }

    fn run_script(self, script: &str) -> String {
        match self {
            Self::Npm => format!("npm run {script}"),
            Self::Pnpm => format!("pnpm {script}"),
            Self::Yarn => format!("yarn {script}"),
            Self::Bun => format!("bun run {script}"),
        }
    }

    fn exec_binary(self, binary: &str, args: &[&str]) -> String {
        let suffix = if args.is_empty() {
            String::new()
        } else {
            format!(" {}", args.join(" "))
        };
        match self {
            Self::Npm => format!("npx {binary}{suffix}"),
            Self::Pnpm => format!("pnpm exec {binary}{suffix}"),
            Self::Yarn => format!("yarn {binary}{suffix}"),
            Self::Bun => format!("bunx {binary}{suffix}"),
        }
    }
}

#[derive(Debug, Clone)]
struct CoverageSetupContext {
    framework: FrameworkKind,
    package_manager: Option<PackageManager>,
    has_build_script: bool,
    has_start_script: bool,
    has_preview_script: bool,
    node_entry_path: String,
}

#[derive(Debug, Clone)]
struct CoverageSetupMember {
    name: String,
    root: PathBuf,
    context: CoverageSetupContext,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct CoverageSetupState {
    schema_version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    license: Option<SetupLicenseState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    sidecar: Option<SetupSidecarState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    recipe: Option<SetupRecipeState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    completed_at: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SetupLicenseState {
    fingerprint: String,
    updated_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SetupSidecarState {
    #[serde(serialize_with = "serde_path::serialize")]
    path: PathBuf,
    checksum: String,
    updated_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct SetupRecipeState {
    #[serde(serialize_with = "serde_path::serialize")]
    path: PathBuf,
    checksum: String,
    context_fingerprint: String,
    updated_at: i64,
}

#[derive(Debug)]
struct CoverageSetupLock {
    _file: File,
}

impl Default for CoverageSetupState {
    fn default() -> Self {
        Self {
            schema_version: SETUP_STATE_SCHEMA_VERSION,
            license: None,
            sidecar: None,
            recipe: None,
            completed_at: None,
        }
    }
}

impl CoverageSetupLock {
    fn try_acquire(root: &Path) -> Result<Self, String> {
        let lock_path = setup_lock_path(root);
        ensure_fallow_dir(root)?;
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(|err| format!("failed to open {}: {err}", lock_path.display()))?;
        match file.try_lock() {
            Ok(()) => Ok(Self { _file: file }),
            Err(std::fs::TryLockError::WouldBlock) => Err(format!(
                "another fallow coverage setup is already running for this project. \
                 The advisory lock is at {}; retry after that process finishes.",
                display_relative(root, &lock_path)
            )),
            Err(std::fs::TryLockError::Error(err)) => {
                Err(format!("failed to lock {}: {err}", lock_path.display()))
            }
        }
    }
}

impl CoverageSetupContext {
    fn script_runner(&self) -> PackageManager {
        self.package_manager.unwrap_or(PackageManager::Npm)
    }

    fn build_command(&self) -> Option<String> {
        if self.has_build_script {
            return Some(self.script_runner().run_script("build"));
        }
        match self.framework {
            FrameworkKind::NextJs => Some(self.script_runner().exec_binary("next", &["build"])),
            FrameworkKind::Nuxt => Some(self.script_runner().exec_binary("nuxi", &["build"])),
            FrameworkKind::Astro => Some(self.script_runner().exec_binary("astro", &["build"])),
            FrameworkKind::Remix => Some(self.script_runner().exec_binary("remix", &["build"])),
            FrameworkKind::SvelteKit => Some(self.script_runner().exec_binary("vite", &["build"])),
            FrameworkKind::ViteBrowser => {
                Some(self.script_runner().exec_binary("vite", &["build"]))
            }
            FrameworkKind::NestJs | FrameworkKind::PlainNode | FrameworkKind::Other => None,
        }
    }

    fn run_command(&self) -> String {
        if self.has_preview_script
            && matches!(
                self.framework,
                FrameworkKind::Nuxt | FrameworkKind::SvelteKit | FrameworkKind::Astro
            )
        {
            return self.script_runner().run_script("preview");
        }
        if self.has_start_script {
            return self.script_runner().run_script("start");
        }
        match self.framework {
            FrameworkKind::NextJs => self.script_runner().exec_binary("next", &["start"]),
            FrameworkKind::Nuxt => self.script_runner().exec_binary("nuxi", &["preview"]),
            FrameworkKind::Astro => self.script_runner().exec_binary("astro", &["preview"]),
            FrameworkKind::SvelteKit | FrameworkKind::ViteBrowser => {
                self.script_runner().exec_binary("vite", &["preview"])
            }
            FrameworkKind::Remix => "node ./build/index.js".to_owned(),
            FrameworkKind::NestJs => "node dist/main.js".to_owned(),
            FrameworkKind::PlainNode | FrameworkKind::Other => "node dist/server.js".to_owned(),
        }
    }
}

/// Dispatch a `fallow coverage <sub>` invocation.
pub fn run(subcommand: CoverageSubcommand, ctx: &RunContext<'_>) -> ExitCode {
    match subcommand {
        CoverageSubcommand::Setup(args) => run_setup(args, ctx.root),
        CoverageSubcommand::Analyze(args) => analyze::run(&args, ctx),
        CoverageSubcommand::UploadInventory(args) => upload_inventory::run(&args, ctx.root),
        CoverageSubcommand::UploadSourceMaps(args) => upload_source_maps::run(&args, ctx.root),
        CoverageSubcommand::UploadStaticFindings(args) => {
            upload_static_findings::run(&args, ctx.root)
        }
    }
}

fn setup_state_path(root: &Path) -> PathBuf {
    root.join(".fallow").join("setup.json")
}

fn setup_lock_path(root: &Path) -> PathBuf {
    root.join(".fallow").join("setup.lock")
}

fn ensure_fallow_dir(root: &Path) -> Result<(), String> {
    let dir = root.join(".fallow");
    std::fs::create_dir_all(&dir)
        .map_err(|err| format!("failed to create {}: {err}", dir.display()))
}

fn load_setup_state(root: &Path) -> CoverageSetupState {
    let path = setup_state_path(root);
    let Ok(content) = std::fs::read_to_string(&path) else {
        return CoverageSetupState::default();
    };
    match serde_json::from_str::<CoverageSetupState>(&content) {
        Ok(state) if state.schema_version == SETUP_STATE_SCHEMA_VERSION => state,
        Ok(_) => {
            eprintln!(
                "fallow coverage setup: ignoring incompatible setup state at {}",
                display_relative(root, &path)
            );
            CoverageSetupState::default()
        }
        Err(err) => {
            eprintln!(
                "fallow coverage setup: ignoring unreadable setup state at {}: {err}",
                display_relative(root, &path)
            );
            CoverageSetupState::default()
        }
    }
}

fn save_setup_state(root: &Path, state: &CoverageSetupState) -> Result<(), String> {
    ensure_fallow_dir(root)?;
    let path = setup_state_path(root);
    let content = serde_json::to_vec_pretty(state)
        .map_err(|err| format!("failed to serialize {}: {err}", path.display()))?;
    atomic_write(&path, &content)
        .map_err(|err| format!("failed to write {}: {err}", path.display()))
}

fn mark_setup_completed(state: &mut CoverageSetupState) {
    state.completed_at = Some(fallow_license::current_unix_seconds());
}

fn license_status_is_setup_acceptable(
    status: &Result<LicenseStatus, fallow_license::LicenseError>,
) -> bool {
    matches!(
        status,
        Ok(LicenseStatus::Valid { .. }
            | LicenseStatus::ExpiredWarning { .. }
            | LicenseStatus::ExpiredWatermark { .. })
    )
}

fn active_license_fingerprint() -> Option<String> {
    let jwt = fallow_license::load_raw_jwt().ok().flatten()?;
    if jwt.is_empty() {
        return None;
    }
    Some(hash_bytes(jwt.as_bytes()))
}

fn license_state_is_current(
    state: &CoverageSetupState,
    status: &Result<LicenseStatus, fallow_license::LicenseError>,
) -> bool {
    if !license_status_is_setup_acceptable(status) {
        return false;
    }
    let Some(recorded) = state.license.as_ref() else {
        return false;
    };
    active_license_fingerprint().as_deref() == Some(recorded.fingerprint.as_str())
}

fn record_current_license_state(state: &mut CoverageSetupState) {
    if let Some(fingerprint) = active_license_fingerprint() {
        state.license = Some(SetupLicenseState {
            fingerprint,
            updated_at: fallow_license::current_unix_seconds(),
        });
    } else {
        state.license = None;
    }
}

fn sidecar_state_is_current(state: &CoverageSetupState, root: &Path) -> bool {
    let Some(recorded) = state.sidecar.as_ref() else {
        return false;
    };
    if sidecar_env_override_is_set() || !recorded.path.is_file() {
        return false;
    }
    if runtime_coverage::discover_sidecar(Some(root))
        .ok()
        .as_deref()
        != Some(recorded.path.as_path())
    {
        return false;
    }
    hash_file(&recorded.path).as_deref() == Some(recorded.checksum.as_str())
}

fn sidecar_env_override_is_set() -> bool {
    ["FALLOW_COV_BIN", "FALLOW_COV_BINARY_PATH"]
        .iter()
        .any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()))
}

fn record_sidecar_state(state: &mut CoverageSetupState, path: PathBuf) -> Result<(), String> {
    let checksum =
        hash_file(&path).ok_or_else(|| format!("failed to hash sidecar {}", path.display()))?;
    state.sidecar = Some(SetupSidecarState {
        path,
        checksum,
        updated_at: fallow_license::current_unix_seconds(),
    });
    Ok(())
}

fn recipe_state_is_current(
    state: &CoverageSetupState,
    root: &Path,
    context: &CoverageSetupContext,
    expected_contents: &str,
) -> bool {
    let Some(recorded) = state.recipe.as_ref() else {
        return false;
    };
    if recorded.context_fingerprint != setup_context_fingerprint(context) {
        return false;
    }
    if recorded.path != recipe_path(root) || !recorded.path.is_file() {
        return false;
    }
    let expected_checksum = hash_bytes(expected_contents.as_bytes());
    recorded.checksum == expected_checksum
        && hash_file(&recorded.path).as_deref() == Some(expected_checksum.as_str())
}

fn record_recipe_state(
    state: &mut CoverageSetupState,
    path: PathBuf,
    context: &CoverageSetupContext,
    contents: &str,
) {
    state.recipe = Some(SetupRecipeState {
        path,
        checksum: hash_bytes(contents.as_bytes()),
        context_fingerprint: setup_context_fingerprint(context),
        updated_at: fallow_license::current_unix_seconds(),
    });
}

fn setup_context_fingerprint(context: &CoverageSetupContext) -> String {
    hash_bytes(setup_context_key(context).as_bytes())
}

fn setup_context_key(context: &CoverageSetupContext) -> String {
    format!(
        "{:?}|{:?}|{}|{}|{}|{}|{}|{}",
        context.framework,
        context.package_manager,
        context.has_build_script,
        context.has_start_script,
        context.has_preview_script,
        context.node_entry_path,
        context.build_command().unwrap_or_default(),
        context.run_command()
    )
}

fn hash_file(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(hash_bytes(&bytes))
}

fn hash_bytes(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut output = String::with_capacity("sha256:".len() + digest.len() * 2);
    output.push_str("sha256:");
    for byte in digest {
        let _ = write!(&mut output, "{byte:02x}");
    }
    output
}

fn run_setup(args: SetupArgs, root: &Path) -> ExitCode {
    if args.json {
        return run_setup_json(root, args.explain);
    }

    let _lock = match CoverageSetupLock::try_acquire(root) {
        Ok(lock) => lock,
        Err(message) => {
            eprintln!("fallow coverage setup: {message}");
            return ExitCode::from(2);
        }
    };
    let mut setup_state = load_setup_state(root);

    print_setup_intro(root);

    if let Err(exit) = run_setup_license_step(root, args, &mut setup_state) {
        return exit;
    }

    let context = detect_setup_context(root);
    if let Err(exit) = run_setup_sidecar_step(root, args, &context, &mut setup_state) {
        return exit;
    }

    let recipe_path = match run_setup_recipe_step(root, &context, &mut setup_state) {
        Ok(path) => path,
        Err(exit) => return exit,
    };

    mark_setup_completed(&mut setup_state);
    if let Err(message) = save_setup_state(root, &setup_state) {
        eprintln!("fallow coverage setup: {message}");
        return ExitCode::from(2);
    }

    run_setup_final_step(root, &context, &recipe_path)
}

fn print_setup_intro(root: &Path) {
    println!("fallow coverage setup");
    println!();
    println!("What \"runtime coverage\" means: fallow looks at which functions actually");
    println!("ran in your deployed app, so it can say \"this code is never called\" with");
    println!("proof, not just \"this code has no static references.\"");
    println!(
        "Setup progress is saved at {} so interrupted runs can resume.",
        display_relative(root, &setup_state_path(root))
    );
    println!();
}

fn run_setup_license_step(
    root: &Path,
    args: SetupArgs,
    setup_state: &mut CoverageSetupState,
) -> Result<(), ExitCode> {
    let key = license::verifying_key().map_err(|message| setup_error_exit(&message, 2))?;
    let license_state = fallow_license::load_and_verify(&key, DEFAULT_HARD_FAIL_DAYS);
    if license_state_is_current(setup_state, &license_state) {
        println!("Step 1/4: License check... ok (resumed).");
        return Ok(());
    }

    if let Some(exit) = handle_license_step(root, args, &license_state) {
        return Err(exit);
    }
    let license_state = fallow_license::load_and_verify(&key, DEFAULT_HARD_FAIL_DAYS);
    if license_status_is_setup_acceptable(&license_state) {
        record_current_license_state(setup_state);
        save_setup_state(root, setup_state).map_err(|message| setup_error_exit(&message, 2))?;
    } else {
        setup_state.license = None;
    }
    Ok(())
}

fn run_setup_sidecar_step(
    root: &Path,
    args: SetupArgs,
    context: &CoverageSetupContext,
    setup_state: &mut CoverageSetupState,
) -> Result<(), ExitCode> {
    if sidecar_state_is_current(setup_state, root) {
        print_resumed_sidecar_step(setup_state);
        return Ok(());
    }

    if let Some(exit) = handle_sidecar_step(root, args, context.package_manager) {
        return Err(exit);
    }
    let path = runtime_coverage::discover_sidecar(Some(root))
        .map_err(|message| setup_error_exit(&message, 4))?;
    record_sidecar_state(setup_state, path)
        .and_then(|()| save_setup_state(root, setup_state))
        .map_err(|message| setup_error_exit(&message, 2))
}

fn print_resumed_sidecar_step(setup_state: &CoverageSetupState) {
    if let Some(sidecar) = setup_state.sidecar.as_ref() {
        println!(
            "Step 2/4: Sidecar check... ok ({}) (resumed).",
            sidecar.path.to_string_lossy()
        );
    }
}

fn run_setup_recipe_step(
    root: &Path,
    context: &CoverageSetupContext,
    setup_state: &mut CoverageSetupState,
) -> Result<PathBuf, ExitCode> {
    let recipe = recipe_contents(context);
    if recipe_state_is_current(setup_state, root, context, &recipe) {
        return Ok(resumed_recipe_path(root, setup_state));
    }

    let path = write_recipe(root, &recipe).map_err(|message| setup_error_exit(&message, 2))?;
    record_recipe_state(setup_state, path.clone(), context, &recipe);
    save_setup_state(root, setup_state).map_err(|message| setup_error_exit(&message, 2))?;
    Ok(path)
}

fn resumed_recipe_path(root: &Path, setup_state: &CoverageSetupState) -> PathBuf {
    let path = setup_state
        .recipe
        .as_ref()
        .map_or_else(|| recipe_path(root), |recipe| recipe.path.clone());
    println!(
        "Step 3/4: Coverage recipe... ok ({}) (resumed).",
        display_relative(root, &path)
    );
    path
}

fn run_setup_final_step(
    root: &Path,
    context: &CoverageSetupContext,
    recipe_path: &Path,
) -> ExitCode {
    if let Some(coverage_path) = detect_coverage_artifact(root) {
        println!(
            "Step 4/4: Coverage found at {}",
            display_relative(root, &coverage_path)
        );
        println!(
            "Running fallow health --runtime-coverage {} ...",
            display_relative(root, &coverage_path)
        );
        let exit = run_health_analysis(root, &coverage_path);
        print_upload_inventory_hint();
        return exit;
    }

    println!("Step 4/4: Collecting coverage for your app.");
    println!("  -> Detected: {}.", context.framework.label());
    println!(
        "  -> Wrote {} with the {} recipe.",
        display_relative(root, recipe_path),
        context.framework.label()
    );
    println!("  -> Run your app with the instrumentation on, then re-run this command.");
    print_upload_inventory_hint();
    ExitCode::SUCCESS
}

fn setup_error_exit(message: &str, code: u8) -> ExitCode {
    eprintln!("fallow coverage setup: {message}");
    ExitCode::from(code)
}

fn run_setup_json(root: &Path, explain: bool) -> ExitCode {
    let payload = build_setup_json(root, explain);
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    if let Err(err) = serde_json::to_writer_pretty(&mut handle, &payload) {
        eprintln!("fallow coverage setup: failed to write JSON output: {err}");
        return ExitCode::from(2);
    }
    println!();
    ExitCode::SUCCESS
}

#[expect(
    clippy::expect_used,
    reason = "coverage setup envelope contains only infallibly serializable fields"
)]
fn build_setup_json(root: &Path, explain: bool) -> serde_json::Value {
    let envelope = build_setup_envelope(root, explain);
    fallow_output::serialize_coverage_setup_json_output(
        envelope,
        crate::output_runtime::current_root_envelope_mode(),
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    )
    .expect("CoverageSetupOutput serializes infallibly")
}

fn build_setup_envelope(root: &Path, explain: bool) -> fallow_output::CoverageSetupOutput {
    use fallow_output::{CoverageSetupFramework, CoverageSetupOutput, CoverageSetupSchemaVersion};

    let members = detect_setup_members(root);
    let primary_member = members.first();
    let fallback = detect_setup_context(root);
    let primary = primary_member.map_or_else(|| fallback.clone(), |member| member.context.clone());
    let package_manager = primary.script_runner();
    let primary_prefix = primary_member
        .map(|member| member_path_prefix(&display_member_path(root, &member.root)))
        .unwrap_or_default();
    let snippets = primary_member.map_or_else(Vec::new, |_| setup_snippets(&primary));
    let files_to_edit = snippets_to_files(&snippets, &primary_prefix);
    let snippet_values = snippets_to_typed(&snippets, &primary_prefix);
    let runtime_targets = union_setup_runtime_targets(&members);
    let member_values: Vec<fallow_output::CoverageSetupMember> = members
        .iter()
        .map(|member| setup_member_typed(root, member))
        .collect();
    let framework_detected = primary_member.map_or(CoverageSetupFramework::Unknown, |_| {
        framework_to_typed(primary.framework)
    });
    let dockerfile = primary_member.and_then(|_| dockerfile_snippet_string(&primary));
    let warnings = build_setup_envelope_warnings(root, primary_member, &fallback, &primary);

    CoverageSetupOutput {
        schema_version: CoverageSetupSchemaVersion::V1,
        framework_detected,
        package_manager: primary.package_manager.map(package_manager_to_typed),
        runtime_targets,
        members: member_values,
        config_written: None,
        commands: vec![
            package_manager.add_runtime_package_command("@fallow-cli/beacon"),
            package_manager.install_command(),
        ],
        files_to_edit,
        snippets: snippet_values,
        dockerfile_snippet: dockerfile,
        next_steps: vec![
            "Add the snippets to your application.".to_owned(),
            "Deploy with the beacon enabled.".to_owned(),
            "Run fallow health --runtime-coverage ./coverage --format json after collecting a local capture.".to_owned(),
            "Set FALLOW_API_KEY in CI before running fallow coverage upload-inventory.".to_owned(),
        ],
        warnings,
        meta: if explain {
            Some(crate::explain::coverage_setup_meta())
        } else {
            None
        },
    }
}

/// Collect the union of runtime targets across all detected setup members.
fn union_setup_runtime_targets(
    members: &[CoverageSetupMember],
) -> Vec<fallow_output::CoverageSetupRuntimeTarget> {
    union_runtime_targets(members.iter().map(|member| &member.context))
        .into_iter()
        .map(runtime_target_from_str)
        .collect()
}

/// Derive the setup-envelope warnings, appending a no-members notice when no
/// runtime workspace member was detected.
fn build_setup_envelope_warnings(
    root: &Path,
    primary_member: Option<&CoverageSetupMember>,
    fallback: &CoverageSetupContext,
    primary: &CoverageSetupContext,
) -> Vec<String> {
    let mut warnings = primary_member.map_or_else(
        || setup_json_warnings(root, fallback),
        |_| setup_json_warnings(root, primary),
    );
    if primary_member.is_none() {
        warnings.push(
            "No runtime workspace members were detected; emitted install commands only.".to_owned(),
        );
    }
    warnings
}

fn framework_to_typed(kind: FrameworkKind) -> fallow_output::CoverageSetupFramework {
    use fallow_output::CoverageSetupFramework as F;
    match kind {
        FrameworkKind::NextJs => F::NextJs,
        FrameworkKind::NestJs => F::NestJs,
        FrameworkKind::Nuxt => F::Nuxt,
        FrameworkKind::SvelteKit => F::SvelteKit,
        FrameworkKind::Astro => F::Astro,
        FrameworkKind::Remix => F::Remix,
        FrameworkKind::ViteBrowser => F::Vite,
        FrameworkKind::PlainNode => F::PlainNode,
        FrameworkKind::Other => F::Unknown,
    }
}

fn package_manager_to_typed(pm: PackageManager) -> fallow_output::CoverageSetupPackageManager {
    use fallow_output::CoverageSetupPackageManager as P;
    match pm {
        PackageManager::Npm => P::Npm,
        PackageManager::Pnpm => P::Pnpm,
        PackageManager::Yarn => P::Yarn,
        PackageManager::Bun => P::Bun,
    }
}

fn runtime_target_from_str(target: &str) -> fallow_output::CoverageSetupRuntimeTarget {
    use fallow_output::CoverageSetupRuntimeTarget as T;
    match target {
        "browser" => T::Browser,
        _ => T::Node,
    }
}

struct SetupSnippet {
    label: &'static str,
    path: String,
    reason: &'static str,
    content: String,
}

fn setup_member_typed(
    root: &Path,
    member: &CoverageSetupMember,
) -> fallow_output::CoverageSetupMember {
    let member_path = display_member_path(root, &member.root);
    let prefix = member_path_prefix(&member_path);
    let snippets = setup_snippets(&member.context);
    fallow_output::CoverageSetupMember {
        name: member.name.clone(),
        path: member_path,
        framework_detected: framework_to_typed(member.context.framework),
        package_manager: member.context.package_manager.map(package_manager_to_typed),
        runtime_targets: member
            .context
            .framework
            .runtime_targets()
            .iter()
            .map(|t| runtime_target_from_str(t))
            .collect(),
        files_to_edit: snippets_to_files(&snippets, &prefix),
        snippets: snippets_to_typed(&snippets, &prefix),
        dockerfile_snippet: dockerfile_snippet_string(&member.context),
        warnings: setup_json_warnings(&member.root, &member.context),
    }
}

fn snippets_to_files(
    snippets: &[SetupSnippet],
    prefix: &str,
) -> Vec<fallow_output::CoverageSetupFileToEdit> {
    snippets
        .iter()
        .map(|snippet| fallow_output::CoverageSetupFileToEdit {
            path: prefixed_member_path(prefix, &snippet.path),
            reason: snippet.reason.to_owned(),
        })
        .collect()
}

fn snippets_to_typed(
    snippets: &[SetupSnippet],
    prefix: &str,
) -> Vec<fallow_output::CoverageSetupSnippet> {
    snippets
        .iter()
        .map(|snippet| fallow_output::CoverageSetupSnippet {
            label: snippet.label.to_owned(),
            path: prefixed_member_path(prefix, &snippet.path),
            content: snippet.content.clone(),
        })
        .collect()
}

const NEXT_INSTRUMENTATION_SNIPPET: &str = r#"export async function register() {
  if (process.env.NEXT_RUNTIME === "nodejs") {
    const { createNodeBeacon } = await import("@fallow-cli/beacon");
    const beacon = createNodeBeacon({
      apiKey: process.env.FALLOW_API_KEY,
      projectId: process.env.FALLOW_PROJECT_ID ?? "my-app",
      endpoint: process.env.FALLOW_API_URL ?? "https://api.fallow.cloud",
      transport: process.env.FALLOW_TRANSPORT === "fs" ? "fs" : "http",
      writeToDir: process.env.FALLOW_WRITE_TO_DIR,
    });
    beacon.start();
  }
}
"#;

const NODE_BEACON_SNIPPET: &str = r#"import { createNodeBeacon } from "@fallow-cli/beacon";

const fallowBeacon = createNodeBeacon({
  apiKey: process.env.FALLOW_API_KEY,
  projectId: process.env.FALLOW_PROJECT_ID ?? "my-app",
  endpoint: process.env.FALLOW_API_URL ?? "https://api.fallow.cloud",
  transport: process.env.FALLOW_TRANSPORT === "fs" ? "fs" : "http",
  writeToDir: process.env.FALLOW_WRITE_TO_DIR,
});

fallowBeacon.start();
"#;

const NUXT_SERVER_PLUGIN_SNIPPET: &str = r#"export default defineNitroPlugin(async () => {
  const { createNodeBeacon } = await import("@fallow-cli/beacon");
  const beacon = createNodeBeacon({
    apiKey: process.env.FALLOW_API_KEY,
    projectId: process.env.FALLOW_PROJECT_ID ?? "my-app",
    endpoint: process.env.FALLOW_API_URL ?? "https://api.fallow.cloud",
    transport: process.env.FALLOW_TRANSPORT === "fs" ? "fs" : "http",
    writeToDir: process.env.FALLOW_WRITE_TO_DIR,
  });
  beacon.start();
});
"#;

const BROWSER_BEACON_SNIPPET: &str = r#"import { createBrowserBeacon } from "@fallow-cli/beacon/browser";

const fallowBeacon = createBrowserBeacon({
  apiKey: import.meta.env.VITE_FALLOW_API_KEY,
  projectId: import.meta.env.VITE_FALLOW_PROJECT_ID ?? "my-app",
  endpoint: import.meta.env.VITE_FALLOW_API_URL ?? "https://api.fallow.cloud",
  sampleRate: 0.01,
});

fallowBeacon.start();
"#;

fn setup_snippet(
    label: &'static str,
    path: impl Into<String>,
    reason: &'static str,
    content: &'static str,
) -> SetupSnippet {
    SetupSnippet {
        label,
        path: path.into(),
        reason,
        content: content.to_owned(),
    }
}

fn setup_snippets(context: &CoverageSetupContext) -> Vec<SetupSnippet> {
    match context.framework {
        FrameworkKind::NextJs => vec![setup_snippet(
            "Next.js instrumentation",
            "instrumentation.ts",
            "Initialize the Node runtime beacon during Next.js startup.",
            NEXT_INSTRUMENTATION_SNIPPET,
        )],
        FrameworkKind::NestJs => vec![setup_snippet(
            "NestJS bootstrap",
            "src/main.ts",
            "Start the Node runtime beacon before creating the Nest app.",
            NODE_BEACON_SNIPPET,
        )],
        FrameworkKind::Nuxt => vec![setup_snippet(
            "Nuxt server plugin",
            "server/plugins/fallow.ts",
            "Start the Node runtime beacon when the Nuxt server boots.",
            NUXT_SERVER_PLUGIN_SNIPPET,
        )],
        FrameworkKind::SvelteKit => vec![setup_snippet(
            "SvelteKit server hook",
            "src/hooks.server.ts",
            "Start the Node runtime beacon before handling server requests.",
            NODE_BEACON_SNIPPET,
        )],
        FrameworkKind::Astro => vec![setup_snippet(
            "Astro middleware",
            "src/middleware.ts",
            "Start the Node runtime beacon from the server middleware module.",
            NODE_BEACON_SNIPPET,
        )],
        FrameworkKind::Remix => vec![setup_snippet(
            "Remix server entry",
            "app/entry.server.tsx",
            "Start the Node runtime beacon from the server entry module.",
            NODE_BEACON_SNIPPET,
        )],
        FrameworkKind::ViteBrowser => vec![setup_snippet(
            "Vite browser entry",
            "src/main.ts",
            "Start the browser runtime beacon from the client entry module.",
            BROWSER_BEACON_SNIPPET,
        )],
        FrameworkKind::PlainNode | FrameworkKind::Other => vec![setup_snippet(
            "Node entrypoint",
            context.node_entry_path.clone(),
            "Start the Node runtime beacon before application code handles traffic.",
            NODE_BEACON_SNIPPET,
        )],
    }
}

fn dockerfile_snippet_string(context: &CoverageSetupContext) -> Option<String> {
    if context.framework.runtime_targets().contains(&"node") {
        Some("ENV FALLOW_TRANSPORT=fs\nENV FALLOW_WRITE_TO_DIR=/tmp/fallow-coverage".to_owned())
    } else {
        None
    }
}

fn setup_json_warnings(root: &Path, context: &CoverageSetupContext) -> Vec<String> {
    let mut warnings = Vec::new();
    if context.framework == FrameworkKind::Other {
        warnings.push(
            "Framework was not detected; emitted the plain Node fallback snippet.".to_owned(),
        );
    }
    if context.package_manager.is_none() {
        warnings.push("Package manager was not detected; npm commands were emitted.".to_owned());
    }
    if detect_coverage_artifact(root).is_none() {
        warnings.push("No local coverage artifact was detected yet.".to_owned());
    }
    warnings
}

/// Nudge the user toward `fallow coverage upload-inventory`. The runtime
/// beacon gives the dashboard `called` / `never_called`; the static inventory
/// upload gives it `untracked` (functions that exist but runtime never parsed).
/// Without this hint, trial users finish setup with no signal that the
/// dashboard's Untracked filter needs a second CI step to light up.
fn print_upload_inventory_hint() {
    println!();
    println!("Next, in CI, upload the static function inventory so the dashboard's");
    println!("Untracked filter lights up:");
    println!("  fallow coverage upload-inventory");
    println!("Set FALLOW_API_KEY on the runner. See {COVERAGE_DOCS_URL} for the full CI snippet.");
}

fn handle_license_step(
    root: &Path,
    args: SetupArgs,
    license_state: &Result<LicenseStatus, fallow_license::LicenseError>,
) -> Option<ExitCode> {
    match license_state {
        Ok(
            LicenseStatus::Valid { .. }
            | LicenseStatus::ExpiredWarning { .. }
            | LicenseStatus::ExpiredWatermark { .. },
        ) => {
            println!("Step 1/4: License check... ok.");
            None
        }
        Ok(LicenseStatus::Missing) => {
            println!("Step 1/4: License check... none found.");
            offer_trial_if_needed(root, args)
        }
        Ok(LicenseStatus::HardFail {
            days_since_expiry, ..
        }) => {
            println!("Step 1/4: License check... expired {days_since_expiry} days ago.");
            offer_trial_if_needed(root, args)
        }
        Err(err) => {
            println!("Step 1/4: License check... existing token is invalid ({err}).");
            offer_trial_if_needed(root, args)
        }
    }
}

fn offer_trial_if_needed(root: &Path, args: SetupArgs) -> Option<ExitCode> {
    println!("  -> Single local captures work without a license.");
    let prompt = "  -> Start a 30-day trial for continuous/multi-capture monitoring? [y/N] ";
    let accepted = match confirm_default_no(prompt, args) {
        Ok(accepted) => accepted,
        Err(message) => {
            eprintln!("fallow coverage setup: {message}");
            return Some(ExitCode::from(2));
        }
    };
    if !accepted {
        println!(
            "  -> For continuous monitoring, run: fallow license activate --trial --email you@company.com"
        );
        return None;
    }

    let email = match prompt_email(args) {
        Ok(Some(email)) => email,
        Ok(None) => return None,
        Err(message) => {
            eprintln!("fallow coverage setup: {message}");
            return Some(ExitCode::from(2));
        }
    };

    // `coverage setup` is the interactive human first-run flow, so it always
    // wants the human-readable "stored at" line regardless of `--format`.
    match license::activate_trial(&email, false) {
        Ok(status) => {
            println!(
                "  -> This license is machine-scoped (stored at {}).",
                default_license_display(root)
            );
            println!("     Your teammates each start their own trial.");
            print_trial_status(&status);
            None
        }
        Err(message) => {
            eprintln!("fallow coverage setup: {message}");
            Some(ExitCode::from(7))
        }
    }
}

fn handle_sidecar_step(
    root: &Path,
    args: SetupArgs,
    package_manager: Option<PackageManager>,
) -> Option<ExitCode> {
    match runtime_coverage::discover_sidecar(Some(root)) {
        Ok(path) => {
            println!("Step 2/4: Sidecar check... ok ({})", path.to_string_lossy());
            None
        }
        Err(message) => {
            println!("Step 2/4: Sidecar check... not installed.");
            println!("  -> {message}");
            let install_command = package_manager.map_or_else(
                || "npm install -g @fallow-cli/fallow-cov".to_owned(),
                PackageManager::install_command,
            );
            let prompt = if let Some(package_manager) = package_manager {
                format!(
                    "  -> Install @fallow-cli/fallow-cov with {}? [Y/n] ",
                    package_manager.label()
                )
            } else {
                "  -> Install @fallow-cli/fallow-cov globally via npm? [Y/n] ".to_owned()
            };
            let accepted = match confirm(prompt, args) {
                Ok(accepted) => accepted,
                Err(message) => {
                    eprintln!("fallow coverage setup: {message}");
                    return Some(ExitCode::from(2));
                }
            };
            if !accepted {
                println!("  -> Run: {install_command}");
                println!(
                    "  -> Manual fallback: install a signed binary and place it at {}",
                    runtime_coverage::canonical_sidecar_path().display()
                );
                return Some(ExitCode::SUCCESS);
            }

            match install_sidecar(root, package_manager) {
                Ok(path) => {
                    println!("  -> Installed at {}", path.display());
                    None
                }
                Err(message) => {
                    eprintln!("fallow coverage setup: {message}");
                    Some(ExitCode::from(4))
                }
            }
        }
    }
}

fn confirm_default_no(prompt: impl AsRef<str>, args: SetupArgs) -> Result<bool, String> {
    confirm_with_default(prompt, args, false)
}

fn confirm(prompt: impl AsRef<str>, args: SetupArgs) -> Result<bool, String> {
    confirm_with_default(prompt, args, true)
}

fn confirm_with_default(
    prompt: impl AsRef<str>,
    args: SetupArgs,
    default: bool,
) -> Result<bool, String> {
    let prompt = prompt.as_ref();
    if args.non_interactive {
        println!("{prompt}skipped (--non-interactive)");
        return Ok(false);
    }
    if args.yes {
        println!("{prompt}Y");
        return Ok(true);
    }

    print!("{prompt}");
    io::stdout()
        .flush()
        .map_err(|err| format!("failed to flush stdout: {err}"))?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|err| format!("failed to read stdin: {err}"))?;
    let trimmed = answer.trim().to_ascii_lowercase();
    Ok(if trimmed.is_empty() {
        default
    } else {
        trimmed == "y" || trimmed == "yes"
    })
}

fn prompt_email(args: SetupArgs) -> Result<Option<String>, String> {
    if args.non_interactive {
        println!("  -> Run: fallow license activate --trial --email you@company.com");
        return Ok(None);
    }
    if args.yes {
        let Some(email) = default_trial_email() else {
            println!(
                "  -> Unable to infer an email address for --yes. Run: fallow license activate --trial --email <addr>"
            );
            return Ok(None);
        };
        println!("  -> Email: {email}");
        return Ok(Some(email));
    }

    print!("  -> Email: ");
    io::stdout()
        .flush()
        .map_err(|err| format!("failed to flush stdout: {err}"))?;

    let mut answer = String::new();
    io::stdin()
        .read_line(&mut answer)
        .map_err(|err| format!("failed to read stdin: {err}"))?;
    let trimmed = answer.trim();
    if trimmed.is_empty() {
        return Err("email is required to start a trial".to_owned());
    }
    Ok(Some(trimmed.to_owned()))
}

fn default_trial_email() -> Option<String> {
    std::env::var("EMAIL")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(git_config_email)
}

fn git_config_email() -> Option<String> {
    let mut command = Command::new("git");
    command.args(["config", "user.email"]);
    clear_ambient_git_env(&mut command);
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let email = String::from_utf8(output.stdout).ok()?;
    let trimmed = email.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

fn print_trial_status(status: &LicenseStatus) {
    match status {
        LicenseStatus::Valid {
            days_until_expiry, ..
        } => {
            println!("  -> Trial active. {days_until_expiry} days remaining.");
        }
        LicenseStatus::ExpiredWarning {
            days_since_expiry, ..
        }
        | LicenseStatus::ExpiredWatermark {
            days_since_expiry, ..
        }
        | LicenseStatus::HardFail {
            days_since_expiry, ..
        } => {
            println!(
                "  -> Trial activated, but it is already expired by {days_since_expiry} days."
            );
        }
        LicenseStatus::Missing => {
            println!("  -> Trial request completed, but no license was stored.");
        }
    }
}

fn default_license_display(root: &Path) -> String {
    display_relative(root, &fallow_license::default_license_path())
}

fn install_sidecar(
    root: &Path,
    package_manager: Option<PackageManager>,
) -> Result<PathBuf, String> {
    let (program, args, current_dir, display_command) =
        if let Some(package_manager) = package_manager {
            let (program, args) = package_manager.install_args();
            (program, args, root, package_manager.install_command())
        } else {
            (
                "npm",
                &["install", "-g", "@fallow-cli/fallow-cov"][..],
                root,
                "npm install -g @fallow-cli/fallow-cov".to_owned(),
            )
        };

    let mut command = Command::new(program);
    command.args(args).current_dir(current_dir);
    let status = crate::signal::scoped_child::status(&mut command)
        .map_err(|err| format!("failed to run {display_command}: {err}"))?;

    if !status.success() {
        return Err(format!(
            "{display_command} failed. Install it manually or place the binary in {}",
            runtime_coverage::canonical_sidecar_path().display()
        ));
    }

    runtime_coverage::discover_sidecar(Some(root)).map_err(|_| {
        format!(
            "sidecar install finished but fallow still could not find fallow-cov. Checked project-local node_modules/.bin, {}, and PATH",
            runtime_coverage::canonical_sidecar_path().display()
        )
    })
}

fn detect_setup_context(root: &Path) -> CoverageSetupContext {
    let package_json = PackageJson::load(&root.join("package.json")).ok();
    let framework = detect_framework(package_json.as_ref());
    let package_manager = detect_package_manager(root);
    let scripts = package_json.as_ref().and_then(|pkg| pkg.scripts.as_ref());
    CoverageSetupContext {
        framework,
        package_manager,
        has_build_script: scripts.is_some_and(|scripts| scripts.contains_key("build")),
        has_start_script: scripts.is_some_and(|scripts| scripts.contains_key("start")),
        has_preview_script: scripts.is_some_and(|scripts| scripts.contains_key("preview")),
        node_entry_path: detect_node_entry_path(root, package_json.as_ref()),
    }
}

fn detect_setup_members(root: &Path) -> Vec<CoverageSetupMember> {
    let root_package_manager = detect_package_manager(root);
    let root_package_json = PackageJson::load(&root.join("package.json")).ok();
    let mut workspaces = discover_workspaces(root);
    workspaces.sort_by(|a, b| a.root.cmp(&b.root));
    let has_workspaces = !workspaces.is_empty();
    let mut members = Vec::new();

    if should_include_root_setup_member(root_package_json.as_ref(), has_workspaces, root) {
        members.push(CoverageSetupMember {
            name: root_package_json
                .as_ref()
                .and_then(|package_json| package_json.name.clone())
                .unwrap_or_else(|| "(root)".to_owned()),
            root: root.to_path_buf(),
            context: detect_setup_context_with_package_manager(root, root_package_manager),
        });
    }

    members.extend(workspaces.into_iter().filter_map(|workspace| {
        setup_member_from_workspace(root, workspace, root_package_manager)
    }));
    members
}

fn setup_member_from_workspace(
    project_root: &Path,
    workspace: WorkspaceInfo,
    root_package_manager: Option<PackageManager>,
) -> Option<CoverageSetupMember> {
    if same_path(project_root, &workspace.root) {
        return None;
    }
    let package_json = PackageJson::load(&workspace.root.join("package.json")).ok()?;
    if !is_runtime_workspace_package(&workspace.root, &package_json) {
        return None;
    }
    Some(CoverageSetupMember {
        name: workspace.name,
        context: detect_setup_context_with_package_manager(&workspace.root, root_package_manager),
        root: workspace.root,
    })
}

fn detect_setup_context_with_package_manager(
    root: &Path,
    fallback_package_manager: Option<PackageManager>,
) -> CoverageSetupContext {
    let package_json = PackageJson::load(&root.join("package.json")).ok();
    let framework = detect_framework(package_json.as_ref());
    let package_manager = detect_package_manager(root).or(fallback_package_manager);
    let scripts = package_json.as_ref().and_then(|pkg| pkg.scripts.as_ref());
    CoverageSetupContext {
        framework,
        package_manager,
        has_build_script: scripts.is_some_and(|scripts| scripts.contains_key("build")),
        has_start_script: scripts.is_some_and(|scripts| scripts.contains_key("start")),
        has_preview_script: scripts.is_some_and(|scripts| scripts.contains_key("preview")),
        node_entry_path: detect_node_entry_path(root, package_json.as_ref()),
    }
}

fn detect_framework(package_json: Option<&PackageJson>) -> FrameworkKind {
    let Some(package_json) = package_json else {
        return FrameworkKind::Other;
    };
    let dependencies = package_json.all_dependency_names();
    if dependencies.iter().any(|name| name == "next") {
        FrameworkKind::NextJs
    } else if dependencies.iter().any(|name| name.starts_with("@nestjs/")) {
        FrameworkKind::NestJs
    } else if dependencies
        .iter()
        .any(|name| name == "nuxt" || name == "nuxi")
    {
        FrameworkKind::Nuxt
    } else if dependencies.iter().any(|name| name == "@sveltejs/kit") {
        FrameworkKind::SvelteKit
    } else if dependencies.iter().any(|name| name == "astro") {
        FrameworkKind::Astro
    } else if dependencies
        .iter()
        .any(|name| name == "remix" || name.starts_with("@remix-run/"))
    {
        FrameworkKind::Remix
    } else if dependencies
        .iter()
        .any(|name| is_node_server_framework(name))
    {
        FrameworkKind::PlainNode
    } else if dependencies.iter().any(|name| name == "vite") {
        FrameworkKind::ViteBrowser
    } else if package_json.name.is_some() {
        FrameworkKind::PlainNode
    } else {
        FrameworkKind::Other
    }
}

fn is_node_server_framework(name: &str) -> bool {
    matches!(
        name,
        "elysia" | "express" | "fastify" | "hono" | "koa" | "@koa/router" | "@trpc/server"
    )
}

fn should_include_root_setup_member(
    package_json: Option<&PackageJson>,
    has_workspaces: bool,
    root: &Path,
) -> bool {
    if !has_workspaces {
        return true;
    }

    package_json.is_some_and(|package_json| is_runtime_workspace_package(root, package_json))
}

fn is_runtime_workspace_package(root: &Path, package_json: &PackageJson) -> bool {
    match detect_framework(Some(package_json)) {
        FrameworkKind::ViteBrowser => is_vite_browser_app(root, package_json),
        FrameworkKind::PlainNode | FrameworkKind::Other => {
            has_node_server_dependency(package_json) || has_runtime_script(package_json)
        }
        FrameworkKind::NextJs
        | FrameworkKind::NestJs
        | FrameworkKind::Nuxt
        | FrameworkKind::SvelteKit
        | FrameworkKind::Astro
        | FrameworkKind::Remix => true,
    }
}

fn has_node_server_dependency(package_json: &PackageJson) -> bool {
    package_json
        .all_dependency_names()
        .iter()
        .any(|name| is_node_server_framework(name))
}

fn has_runtime_script(package_json: &PackageJson) -> bool {
    package_json.scripts.as_ref().is_some_and(|scripts| {
        scripts.contains_key("start")
            || scripts.contains_key("preview")
            || scripts.contains_key("dev")
    })
}

fn is_vite_browser_app(root: &Path, package_json: &PackageJson) -> bool {
    let has_vite_dependency = package_json
        .all_dependency_names()
        .iter()
        .any(|name| name == "vite");
    if !has_vite_dependency {
        return false;
    }

    package_json.scripts.as_ref().is_some_and(|scripts| {
        ["dev", "preview"]
            .iter()
            .filter_map(|script_name| scripts.get(*script_name))
            .any(|script| script_invokes_vite_app(script))
    }) || [
        "index.html",
        "src/main.ts",
        "src/main.tsx",
        "src/main.js",
        "src/main.jsx",
        "src/main.mts",
        "src/main.mjs",
    ]
    .iter()
    .any(|candidate| root.join(candidate).is_file())
}

fn script_invokes_vite_app(script: &str) -> bool {
    script
        .split(|character: char| {
            character.is_whitespace()
                || matches!(character, '"' | '\'' | ':' | ';' | '&' | '|' | '(' | ')')
        })
        .any(|token| matches!(token, "vite" | "vite-preview" | "vite-plus" | "vp"))
}

fn detect_node_entry_path(root: &Path, package_json: Option<&PackageJson>) -> String {
    for candidate in [
        "src/index.ts",
        "src/server.ts",
        "src/main.ts",
        "src/app.ts",
        "index.ts",
        "server.ts",
    ] {
        if root.join(candidate).is_file() {
            return candidate.to_owned();
        }
    }

    if let Some(package_json) = package_json {
        for entry in package_json.entry_points() {
            let normalized = entry.trim_start_matches("./");
            if root.join(normalized).is_file() {
                return path_to_json_string(Path::new(normalized));
            }
        }
        if package_json
            .all_dependency_names()
            .iter()
            .any(|name| is_node_server_framework(name))
        {
            return "src/index.ts".to_owned();
        }
    }

    "src/server.ts".to_owned()
}

fn union_runtime_targets<'a>(
    contexts: impl IntoIterator<Item = &'a CoverageSetupContext>,
) -> Vec<&'static str> {
    let mut has_node = false;
    let mut has_browser = false;
    for context in contexts {
        for target in context.framework.runtime_targets() {
            match *target {
                "node" => has_node = true,
                "browser" => has_browser = true,
                _ => {}
            }
        }
    }

    let mut targets = Vec::new();
    if has_node {
        targets.push("node");
    }
    if has_browser {
        targets.push("browser");
    }
    targets
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = dunce::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = dunce::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left == right
}

fn detect_package_manager(root: &Path) -> Option<PackageManager> {
    detect_package_manager_from_field(root).or_else(|| {
        if root.join("bun.lockb").exists() || root.join("bun.lock").exists() {
            Some(PackageManager::Bun)
        } else if root.join("pnpm-lock.yaml").exists() {
            Some(PackageManager::Pnpm)
        } else if root.join("yarn.lock").exists() {
            Some(PackageManager::Yarn)
        } else if root.join("package-lock.json").exists()
            || root.join("npm-shrinkwrap.json").exists()
        {
            Some(PackageManager::Npm)
        } else {
            None
        }
    })
}

fn detect_package_manager_from_field(root: &Path) -> Option<PackageManager> {
    let content = std::fs::read_to_string(root.join("package.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&content).ok()?;
    let field = value.get("packageManager")?.as_str()?;
    let name = field.split('@').next().unwrap_or(field);
    match name {
        "npm" => Some(PackageManager::Npm),
        "pnpm" => Some(PackageManager::Pnpm),
        "yarn" => Some(PackageManager::Yarn),
        "bun" => Some(PackageManager::Bun),
        _ => None,
    }
}

fn recipe_path(root: &Path) -> PathBuf {
    root.join("docs").join("collect-coverage.md")
}

fn write_recipe(root: &Path, contents: &str) -> Result<PathBuf, String> {
    let docs_dir = root.join("docs");
    std::fs::create_dir_all(&docs_dir)
        .map_err(|err| format!("failed to create {}: {err}", docs_dir.display()))?;
    let path = recipe_path(root);
    atomic_write(&path, contents.as_bytes())
        .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    Ok(path)
}

fn recipe_contents(context: &CoverageSetupContext) -> String {
    let title = match context.framework {
        FrameworkKind::NextJs => "Next.js",
        FrameworkKind::NestJs => "NestJS",
        FrameworkKind::Nuxt => "Nuxt",
        FrameworkKind::SvelteKit => "SvelteKit",
        FrameworkKind::Astro => "Astro",
        FrameworkKind::Remix => "Remix",
        FrameworkKind::ViteBrowser => "Vite",
        FrameworkKind::PlainNode => "Node service",
        FrameworkKind::Other => {
            return format!(
                "# Collect runtime coverage\n\nThis project was not matched to a built-in recipe.\nSee {COVERAGE_DOCS_URL} for framework-specific instructions.\n"
            );
        }
    };

    let mut lines = vec![
        format!("# Collect runtime coverage for {title}"),
        String::new(),
    ];
    lines.push("1. Remove any old dump directory: `rm -rf ./coverage`".to_owned());
    let final_step = if context.has_build_script || context.build_command().is_some() {
        if let Some(build_command) = context.build_command() {
            lines.push(format!("2. Build the app: `{build_command}`"));
        }
        lines.push(format!(
            "3. Start the app with V8 coverage enabled: `NODE_V8_COVERAGE=./coverage {}`",
            context.run_command()
        ));
        lines.push("4. Exercise the routes or jobs you care about.".to_owned());
        lines.push("5. Stop the app and run: `fallow coverage setup`".to_owned());
        "6"
    } else {
        lines.push(format!(
            "2. Start the app with V8 coverage enabled: `NODE_V8_COVERAGE=./coverage {}`",
            context.run_command()
        ));
        lines.push("3. Exercise the app traffic you want to analyze.".to_owned());
        lines.push("4. Stop the process and run: `fallow coverage setup`".to_owned());
        "5"
    };
    lines.push(format!(
        "{final_step}. In CI, after the build, run \
         `fallow coverage upload-inventory` with `FALLOW_API_KEY` set. The \
         upload is what enables the dashboard's Untracked filter (functions \
         that exist but runtime coverage never parsed). Runtime coverage alone \
         only answers `called` vs `never_called`; the static inventory adds \
         the third state."
    ));
    lines.push(String::new());
    lines.join("\n")
}

fn detect_coverage_artifact(root: &Path) -> Option<PathBuf> {
    for file in [
        root.join("coverage/coverage-final.json"),
        root.join(".nyc_output/coverage-final.json"),
    ] {
        if file.is_file() {
            return Some(file);
        }
    }

    [root.join("coverage"), root.join(".nyc_output")]
        .into_iter()
        .find(|dir| dir.is_dir() && directory_has_json(dir))
}

fn directory_has_json(path: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };

    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .any(|entry| entry.extension() == Some(OsStr::new("json")))
}

fn run_health_analysis(root: &Path, coverage_path: &Path) -> ExitCode {
    let current_exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(err) => {
            eprintln!("fallow coverage setup: failed to resolve current executable: {err}");
            return ExitCode::from(2);
        }
    };

    let mut command = Command::new(current_exe);
    command
        .arg("health")
        .arg("--root")
        .arg(root)
        .arg("--runtime-coverage")
        .arg(coverage_path);
    let status = match crate::signal::scoped_child::status(&mut command) {
        Ok(status) => status,
        Err(err) => {
            eprintln!("fallow coverage setup: failed to run health analysis: {err}");
            return ExitCode::from(2);
        }
    };

    match status.code() {
        Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(2)),
        None => ExitCode::from(2),
    }
}

fn display_relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root).map_or_else(
        |_| path.to_string_lossy().into_owned(),
        |relative| format!("./{}", relative.to_string_lossy()),
    )
}

fn display_member_path(root: &Path, path: &Path) -> String {
    let relative = path.strip_prefix(root).unwrap_or(path);
    if relative.as_os_str().is_empty() {
        ".".to_owned()
    } else {
        path_to_json_string(relative)
    }
}

fn member_path_prefix(member_path: &str) -> String {
    if member_path == "." {
        String::new()
    } else {
        member_path.to_owned()
    }
}

fn prefixed_member_path(prefix: &str, path: &str) -> String {
    if prefix.is_empty() {
        path.to_owned()
    } else {
        format!("{prefix}/{path}")
    }
}

fn path_to_json_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{
        CoverageSetupContext, FrameworkKind, PackageManager, SetupArgs, build_setup_json,
        detect_coverage_artifact, detect_framework, detect_package_manager, handle_license_step,
        load_setup_state, recipe_contents, recipe_state_is_current, record_recipe_state,
        record_sidecar_state, run_setup, setup_context_fingerprint, setup_state_path,
        sidecar_state_is_current, write_recipe,
    };
    use fallow_config::PackageJson;
    use fallow_license::LicenseStatus;
    use std::path::{Path, PathBuf};
    use tempfile::tempdir;

    #[test]
    fn setup_continues_without_license_for_single_local_capture() {
        let dir = tempdir().expect("tempdir should be created");
        let args = SetupArgs {
            non_interactive: true,
            ..SetupArgs::default()
        };
        let status = Ok(LicenseStatus::Missing);

        assert!(
            handle_license_step(dir.path(), args, &status).is_none(),
            "missing license must not stop setup; single local captures are free"
        );
    }

    #[test]
    fn detect_framework_recognizes_nuxt_projects() {
        let package_json: PackageJson =
            serde_json::from_str(r#"{"name":"demo","dependencies":{"nuxt":"^3.0.0"}}"#)
                .expect("package.json should parse");

        assert_eq!(detect_framework(Some(&package_json)), FrameworkKind::Nuxt);
    }

    #[test]
    fn detect_framework_recognizes_vite_browser_projects() {
        let package_json: PackageJson =
            serde_json::from_str(r#"{"name":"demo","devDependencies":{"vite":"^6.0.0"}}"#)
                .expect("package.json should parse");

        assert_eq!(
            detect_framework(Some(&package_json)),
            FrameworkKind::ViteBrowser
        );
    }

    #[test]
    fn detect_framework_prefers_node_server_frameworks_over_vite() {
        let package_json: PackageJson = serde_json::from_str(
            r#"{"name":"api","dependencies":{"elysia":"^1.0.0"},"devDependencies":{"vite":"^6.0.0"}}"#,
        )
        .expect("package.json should parse");

        assert_eq!(
            detect_framework(Some(&package_json)),
            FrameworkKind::PlainNode
        );
    }

    #[test]
    fn detect_package_manager_prefers_package_manager_field() {
        let dir = tempdir().expect("tempdir should be created");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"demo","packageManager":"bun@1.2.0"}"#,
        )
        .expect("package.json should be written");
        std::fs::write(dir.path().join("pnpm-lock.yaml"), "lockfileVersion: '9.0'")
            .expect("lockfile should be written");

        assert_eq!(
            detect_package_manager(dir.path()),
            Some(PackageManager::Bun)
        );
    }

    #[test]
    fn package_manager_commands_match_each_tool() {
        assert_eq!(PackageManager::Npm.label(), "npm");
        assert_eq!(
            PackageManager::Npm.add_runtime_package_command("pkg"),
            "npm install pkg"
        );
        assert_eq!(
            PackageManager::Npm.install_command(),
            "npm install --save-dev @fallow-cli/fallow-cov"
        );
        assert_eq!(PackageManager::Npm.run_script("build"), "npm run build");
        assert_eq!(PackageManager::Npm.exec_binary("vite", &[]), "npx vite");

        assert_eq!(PackageManager::Pnpm.label(), "pnpm");
        assert_eq!(
            PackageManager::Pnpm.add_runtime_package_command("pkg"),
            "pnpm add pkg"
        );
        assert_eq!(
            PackageManager::Pnpm.install_command(),
            "pnpm add -D @fallow-cli/fallow-cov"
        );
        assert_eq!(PackageManager::Pnpm.run_script("build"), "pnpm build");
        assert_eq!(
            PackageManager::Pnpm.exec_binary("vite", &["preview"]),
            "pnpm exec vite preview"
        );

        assert_eq!(PackageManager::Yarn.label(), "yarn");
        assert_eq!(
            PackageManager::Yarn.add_runtime_package_command("pkg"),
            "yarn add pkg"
        );
        assert_eq!(
            PackageManager::Yarn.install_command(),
            "yarn add -D @fallow-cli/fallow-cov"
        );
        assert_eq!(PackageManager::Yarn.run_script("build"), "yarn build");
        assert_eq!(
            PackageManager::Yarn.exec_binary("vite", &["build"]),
            "yarn vite build"
        );

        assert_eq!(PackageManager::Bun.label(), "bun");
        assert_eq!(
            PackageManager::Bun.add_runtime_package_command("pkg"),
            "bun add pkg"
        );
        assert_eq!(
            PackageManager::Bun.install_command(),
            "bun add -d @fallow-cli/fallow-cov"
        );
        assert_eq!(PackageManager::Bun.run_script("build"), "bun run build");
        assert_eq!(
            PackageManager::Bun.exec_binary("vite", &["preview"]),
            "bunx vite preview"
        );
    }

    #[test]
    fn setup_context_commands_cover_framework_defaults() {
        let context = |framework, package_manager| CoverageSetupContext {
            framework,
            package_manager,
            has_build_script: false,
            has_start_script: false,
            has_preview_script: false,
            node_entry_path: "server.ts".to_owned(),
        };

        let next = context(FrameworkKind::NextJs, Some(PackageManager::Pnpm));
        assert_eq!(
            next.build_command().as_deref(),
            Some("pnpm exec next build")
        );
        assert_eq!(next.run_command(), "pnpm exec next start");

        let nuxt = context(FrameworkKind::Nuxt, Some(PackageManager::Yarn));
        assert_eq!(nuxt.build_command().as_deref(), Some("yarn nuxi build"));
        assert_eq!(nuxt.run_command(), "yarn nuxi preview");

        let astro = context(FrameworkKind::Astro, Some(PackageManager::Bun));
        assert_eq!(astro.build_command().as_deref(), Some("bunx astro build"));
        assert_eq!(astro.run_command(), "bunx astro preview");

        let remix = context(FrameworkKind::Remix, Some(PackageManager::Npm));
        assert_eq!(remix.build_command().as_deref(), Some("npx remix build"));
        assert_eq!(remix.run_command(), "node ./build/index.js");

        let svelte = context(FrameworkKind::SvelteKit, None);
        assert_eq!(svelte.build_command().as_deref(), Some("npx vite build"));
        assert_eq!(svelte.run_command(), "npx vite preview");

        let vite = context(FrameworkKind::ViteBrowser, Some(PackageManager::Pnpm));
        assert_eq!(
            vite.build_command().as_deref(),
            Some("pnpm exec vite build")
        );
        assert_eq!(vite.run_command(), "pnpm exec vite preview");

        let nest = context(FrameworkKind::NestJs, Some(PackageManager::Pnpm));
        assert_eq!(nest.build_command(), None);
        assert_eq!(nest.run_command(), "node dist/main.js");

        let plain = context(FrameworkKind::PlainNode, None);
        assert_eq!(plain.build_command(), None);
        assert_eq!(plain.run_command(), "node dist/server.js");

        let other = context(FrameworkKind::Other, Some(PackageManager::Pnpm));
        assert_eq!(other.build_command(), None);
        assert_eq!(other.run_command(), "node dist/server.js");
        assert_eq!(other.framework.label(), "custom project");
    }

    #[test]
    fn setup_json_emits_workspace_members_and_union_runtime_targets() {
        let dir = tempdir().expect("tempdir should be created");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
  "name": "@demo/api",
  "private": true,
  "packageManager": "pnpm@9.0.0",
  "workspaces": ["apps/*", "packages/*"],
  "scripts": { "start": "node dist/server.js" },
  "dependencies": { "elysia": "^1.0.0" },
  "devDependencies": { "vite": "^6.0.0" }
}"#,
        )
        .expect("root package.json should be written");
        std::fs::create_dir_all(dir.path().join("src")).expect("root src dir should be created");
        std::fs::write(
            dir.path().join("src/index.ts"),
            "export const api = true;\n",
        )
        .expect("root entry should be written");

        for (path, name) in [
            ("apps/admin", "@demo/admin"),
            ("apps/marketing", "@demo/marketing"),
        ] {
            let workspace_dir = dir.path().join(path);
            std::fs::create_dir_all(&workspace_dir).expect("workspace dir should be created");
            std::fs::write(
                workspace_dir.join("package.json"),
                format!(
                    r#"{{"name":"{name}","scripts":{{"dev":"vite","build":"vite build"}},"devDependencies":{{"vite":"^6.0.0"}}}}"#
                ),
            )
            .expect("workspace package.json should be written");
        }
        let library_dir = dir.path().join("packages/shared");
        std::fs::create_dir_all(&library_dir).expect("library dir should be created");
        std::fs::write(
            library_dir.join("package.json"),
            r#"{"name":"@demo/shared","scripts":{"build":"tsc"},"devDependencies":{"typescript":"^6.0.0"}}"#,
        )
        .expect("library package.json should be written");

        let payload = build_setup_json(dir.path(), false);

        assert_eq!(payload["framework_detected"], "plain_node");
        assert_eq!(
            payload["runtime_targets"],
            serde_json::json!(["node", "browser"])
        );
        assert_eq!(payload["files_to_edit"][0]["path"], "src/index.ts");
        assert_eq!(
            payload["members"].as_array().map(Vec::len),
            Some(3),
            "root plus two workspace members should be emitted: {payload:#}"
        );

        let members = payload["members"].as_array().expect("members array");
        assert!(
            members
                .iter()
                .all(|member| member["path"] != "packages/shared"),
            "build-only library workspaces should not receive runtime setup recipes: {payload:#}"
        );
        let root_member = members
            .iter()
            .find(|member| member["path"] == ".")
            .expect("root member should be present");
        assert_eq!(root_member["name"], "@demo/api");
        assert_eq!(root_member["framework_detected"], "plain_node");
        assert_eq!(root_member["runtime_targets"], serde_json::json!(["node"]));
        assert_eq!(root_member["snippets"][0]["path"], "src/index.ts");

        for path in ["apps/admin", "apps/marketing"] {
            let member = members
                .iter()
                .find(|member| member["path"] == path)
                .unwrap_or_else(|| panic!("missing workspace member {path}: {payload:#}"));
            assert_eq!(member["framework_detected"], "vite");
            assert_eq!(member["package_manager"], "pnpm");
            assert_eq!(member["runtime_targets"], serde_json::json!(["browser"]));
            assert_eq!(member["snippets"][0]["path"], format!("{path}/src/main.ts"));
        }
    }

    /// Snapshot test that locks the full `coverage setup --json` workspace
    /// shape. Adding a new field to the payload (e.g. a future `_meta` block)
    /// is an intentional contract change, so the snapshot must be reviewed
    /// with `cargo insta review`. Reverting the workspace-aware fix or the
    /// Vite-app heuristic regenerates a different snapshot.
    #[test]
    fn setup_json_workspace_payload_matches_snapshot() {
        let dir = tempdir().expect("tempdir should be created");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
  "name": "@demo/api",
  "private": true,
  "packageManager": "pnpm@9.0.0",
  "workspaces": ["apps/*", "packages/*"],
  "scripts": { "start": "node dist/server.js" },
  "dependencies": { "elysia": "^1.0.0" },
  "devDependencies": { "vite": "^6.0.0" }
}"#,
        )
        .expect("root package.json should be written");
        std::fs::create_dir_all(dir.path().join("src")).expect("root src dir should be created");
        std::fs::write(
            dir.path().join("src/index.ts"),
            "export const api = true;\n",
        )
        .expect("root entry should be written");

        for (path, name) in [
            ("apps/admin", "@demo/admin"),
            ("apps/marketing", "@demo/marketing"),
        ] {
            let workspace_dir = dir.path().join(path);
            std::fs::create_dir_all(workspace_dir.join("src"))
                .expect("workspace src dir should be created");
            std::fs::write(
                workspace_dir.join("package.json"),
                format!(
                    r#"{{"name":"{name}","scripts":{{"dev":"vite","build":"vite build"}},"devDependencies":{{"vite":"^6.0.0"}}}}"#
                ),
            )
            .expect("workspace package.json should be written");
            std::fs::write(
                workspace_dir.join("src/main.ts"),
                "export const app = true;\n",
            )
            .expect("workspace entry should be written");
        }
        let library_dir = dir.path().join("packages/shared");
        std::fs::create_dir_all(&library_dir).expect("library dir should be created");
        std::fs::write(
            library_dir.join("package.json"),
            r#"{"name":"@demo/shared","scripts":{"build":"tsc"},"devDependencies":{"typescript":"^6.0.0"}}"#,
        )
        .expect("library package.json should be written");

        let payload = build_setup_json(dir.path(), false);

        insta::assert_yaml_snapshot!("coverage_setup_json_workspace", payload);
    }

    #[test]
    fn setup_json_skips_non_runtime_workspace_root() {
        let dir = tempdir().expect("tempdir should be created");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
  "name": "repo",
  "private": true,
  "packageManager": "pnpm@9.0.0",
  "workspaces": ["apps/*"]
}"#,
        )
        .expect("root package.json should be written");

        let api_dir = dir.path().join("apps/api");
        std::fs::create_dir_all(api_dir.join("src")).expect("api src dir should be created");
        std::fs::write(
            api_dir.join("package.json"),
            r#"{"name":"api","dependencies":{"elysia":"^1.0.0"}}"#,
        )
        .expect("api package.json should be written");
        std::fs::write(api_dir.join("src/index.ts"), "export const api = true;\n")
            .expect("api entry should be written");

        let web_dir = dir.path().join("apps/web");
        std::fs::create_dir_all(web_dir.join("src")).expect("web src dir should be created");
        std::fs::write(
            web_dir.join("package.json"),
            r#"{"name":"web","scripts":{"dev":"vite"},"devDependencies":{"vite":"^6.0.0"}}"#,
        )
        .expect("web package.json should be written");
        std::fs::write(web_dir.join("src/main.ts"), "export const web = true;\n")
            .expect("web entry should be written");

        let payload = build_setup_json(dir.path(), false);

        assert_eq!(payload["framework_detected"], "plain_node");
        assert_eq!(
            payload["runtime_targets"],
            serde_json::json!(["node", "browser"])
        );
        assert_eq!(payload["files_to_edit"][0]["path"], "apps/api/src/index.ts");

        let members = payload["members"].as_array().expect("members array");
        assert_eq!(
            members.len(),
            2,
            "only runtime workspaces should be emitted: {payload:#}"
        );
        assert!(
            members.iter().all(|member| member["path"] != "."),
            "workspace aggregator root must not receive a runtime setup recipe: {payload:#}"
        );
        assert_eq!(members[0]["path"], "apps/api");
        assert_eq!(members[0]["snippets"][0]["path"], "apps/api/src/index.ts");
        assert_eq!(members[1]["path"], "apps/web");
        assert_eq!(members[1]["framework_detected"], "vite");
    }

    #[test]
    fn setup_json_skips_vite_build_only_workspace_packages() {
        let dir = tempdir().expect("tempdir should be created");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{
  "name": "repo",
  "private": true,
  "packageManager": "pnpm@9.0.0",
  "workspaces": ["packages/*"],
  "scripts": { "dev": "turbo dev" },
  "devDependencies": { "vite": "^6.0.0" }
}"#,
        )
        .expect("root package.json should be written");

        let library_dir = dir.path().join("packages/ui");
        std::fs::create_dir_all(library_dir.join("src"))
            .expect("library src dir should be created");
        std::fs::write(
            library_dir.join("package.json"),
            r#"{"name":"@repo/ui","scripts":{"build":"vite build"},"devDependencies":{"vite":"^6.0.0"}}"#,
        )
        .expect("library package.json should be written");

        let payload = build_setup_json(dir.path(), false);

        assert_eq!(payload["framework_detected"], "unknown");
        assert_eq!(payload["runtime_targets"], serde_json::json!([]));
        assert_eq!(payload["files_to_edit"], serde_json::json!([]));
        assert_eq!(payload["snippets"], serde_json::json!([]));
        assert_eq!(payload["members"], serde_json::json!([]));
        assert!(
            payload["warnings"]
                .as_array()
                .is_some_and(|warnings| warnings.iter().any(|warning| warning
                    .as_str()
                    .is_some_and(|warning| warning.contains("No runtime workspace members")))),
            "empty runtime workspace detection should be explicit: {payload:#}"
        );
    }

    #[test]
    fn recipe_contents_uses_detected_package_manager_scripts() {
        let context = CoverageSetupContext {
            framework: FrameworkKind::SvelteKit,
            package_manager: Some(PackageManager::Pnpm),
            has_build_script: true,
            has_start_script: false,
            has_preview_script: true,
            node_entry_path: "src/server.ts".to_owned(),
        };

        let recipe = recipe_contents(&context);

        assert!(recipe.contains("`pnpm build`"));
        assert!(recipe.contains("`NODE_V8_COVERAGE=./coverage pnpm preview`"));
    }

    #[test]
    fn recipe_contents_mentions_upload_inventory_ci_step() {
        let context = CoverageSetupContext {
            framework: FrameworkKind::SvelteKit,
            package_manager: Some(PackageManager::Pnpm),
            has_build_script: true,
            has_start_script: false,
            has_preview_script: true,
            node_entry_path: "src/server.ts".to_owned(),
        };
        let recipe = recipe_contents(&context);
        assert!(
            recipe.contains("fallow coverage upload-inventory"),
            "recipe missing upload-inventory CI instruction:\n{recipe}"
        );
        assert!(recipe.contains("FALLOW_API_KEY"));
    }

    #[test]
    fn recipe_contents_mentions_upload_inventory_without_build_script() {
        let context = CoverageSetupContext {
            framework: FrameworkKind::PlainNode,
            package_manager: Some(PackageManager::Npm),
            has_build_script: false,
            has_start_script: false,
            has_preview_script: false,
            node_entry_path: "src/server.ts".to_owned(),
        };
        let recipe = recipe_contents(&context);
        assert!(recipe.contains("fallow coverage upload-inventory"));
    }

    #[test]
    fn setup_json_is_deterministic_and_does_not_write_files() {
        let dir = tempdir().expect("tempdir should be created");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"demo","packageManager":"pnpm@9.0.0","dependencies":{"next":"^16.0.0"}}"#,
        )
        .expect("package.json should be written");

        let payload = build_setup_json(dir.path(), false);

        assert_eq!(payload["schema_version"], "1");
        assert_eq!(payload["framework_detected"], "nextjs");
        assert_eq!(payload["package_manager"], "pnpm");
        assert_eq!(payload["config_written"], serde_json::Value::Null);
        assert_eq!(
            payload["runtime_targets"],
            serde_json::json!(["node", "browser"])
        );
        assert_eq!(payload["commands"][0], "pnpm add @fallow-cli/beacon");
        assert_eq!(payload["commands"][1], "pnpm add -D @fallow-cli/fallow-cov");
        assert_eq!(payload["files_to_edit"][0]["path"], "instrumentation.ts");
        assert!(
            payload["snippets"][0]["content"]
                .as_str()
                .is_some_and(|content| content.contains("createNodeBeacon"))
        );
        assert!(
            !dir.path().join("docs/collect-coverage.md").exists(),
            "JSON setup must not write the human recipe"
        );
        assert!(
            !dir.path().join(".fallow").exists(),
            "JSON setup must not write setup state or lock files"
        );
    }

    #[test]
    fn run_setup_json_does_not_write_state_lock_or_recipe() {
        let dir = tempdir().expect("tempdir should be created");

        let exit = run_setup(
            SetupArgs {
                json: true,
                ..SetupArgs::default()
            },
            dir.path(),
        );

        assert_eq!(exit, std::process::ExitCode::SUCCESS);
        assert!(!dir.path().join(".fallow").exists());
        assert!(!dir.path().join("docs/collect-coverage.md").exists());
    }

    #[test]
    fn corrupt_setup_state_falls_back_to_default() {
        let dir = tempdir().expect("tempdir should be created");
        std::fs::create_dir_all(dir.path().join(".fallow")).expect(".fallow should be created");
        std::fs::write(setup_state_path(dir.path()), "{not json")
            .expect("corrupt setup state should be written");

        let state = load_setup_state(dir.path());

        assert_eq!(state.schema_version, 1);
        assert!(state.license.is_none());
        assert!(state.sidecar.is_none());
        assert!(state.recipe.is_none());
    }

    #[test]
    fn sidecar_state_requires_matching_file_checksum() {
        let dir = tempdir().expect("tempdir should be created");
        let sidecar = write_fake_sidecar(dir.path(), b"first");
        let mut state = super::CoverageSetupState::default();
        record_sidecar_state(&mut state, sidecar.clone()).expect("sidecar state should record");

        assert!(sidecar_state_is_current(&state, dir.path()));

        std::fs::write(&sidecar, b"second").expect("sidecar should be mutated");

        assert!(!sidecar_state_is_current(&state, dir.path()));
    }

    #[test]
    fn recipe_state_requires_current_context_and_file_checksum() {
        let dir = tempdir().expect("tempdir should be created");
        let vite_context = CoverageSetupContext {
            framework: FrameworkKind::ViteBrowser,
            package_manager: Some(PackageManager::Pnpm),
            has_build_script: true,
            has_start_script: false,
            has_preview_script: true,
            node_entry_path: "src/main.ts".to_owned(),
        };
        let next_context = CoverageSetupContext {
            framework: FrameworkKind::NextJs,
            package_manager: Some(PackageManager::Pnpm),
            has_build_script: true,
            has_start_script: true,
            has_preview_script: false,
            node_entry_path: "src/main.ts".to_owned(),
        };
        let vite_recipe = recipe_contents(&vite_context);
        let recipe_path = write_recipe(dir.path(), &vite_recipe).expect("recipe should write");
        let mut state = super::CoverageSetupState::default();
        record_recipe_state(&mut state, recipe_path.clone(), &vite_context, &vite_recipe);

        assert!(recipe_state_is_current(
            &state,
            dir.path(),
            &vite_context,
            &vite_recipe
        ));
        assert!(!recipe_state_is_current(
            &state,
            dir.path(),
            &next_context,
            &recipe_contents(&next_context)
        ));

        std::fs::write(&recipe_path, "manual edit").expect("recipe should be mutated");

        assert!(!recipe_state_is_current(
            &state,
            dir.path(),
            &vite_context,
            &vite_recipe
        ));
    }

    #[test]
    fn recipe_state_requires_path_under_current_root() {
        let old_dir = tempdir().expect("old tempdir should be created");
        let new_dir = tempdir().expect("new tempdir should be created");
        let context = CoverageSetupContext {
            framework: FrameworkKind::PlainNode,
            package_manager: Some(PackageManager::Npm),
            has_build_script: false,
            has_start_script: false,
            has_preview_script: false,
            node_entry_path: "src/server.ts".to_owned(),
        };
        let recipe = recipe_contents(&context);
        let old_recipe_path =
            write_recipe(old_dir.path(), &recipe).expect("old recipe should write");
        let mut state = super::CoverageSetupState::default();
        record_recipe_state(&mut state, old_recipe_path, &context, &recipe);

        assert!(!recipe_state_is_current(
            &state,
            new_dir.path(),
            &context,
            &recipe
        ));
    }

    #[test]
    fn setup_context_fingerprint_changes_when_recipe_inputs_change() {
        let base = CoverageSetupContext {
            framework: FrameworkKind::PlainNode,
            package_manager: Some(PackageManager::Npm),
            has_build_script: false,
            has_start_script: false,
            has_preview_script: false,
            node_entry_path: "src/server.ts".to_owned(),
        };
        let changed = CoverageSetupContext {
            package_manager: Some(PackageManager::Bun),
            ..base.clone()
        };

        assert_ne!(
            setup_context_fingerprint(&base),
            setup_context_fingerprint(&changed)
        );
    }

    #[test]
    fn setup_lock_reports_contention() {
        let dir = tempdir().expect("tempdir should be created");
        let _first =
            super::CoverageSetupLock::try_acquire(dir.path()).expect("first lock should acquire");

        let err = super::CoverageSetupLock::try_acquire(dir.path())
            .expect_err("second lock should report contention");

        assert!(err.contains("another fallow coverage setup is already running"));
        assert!(err.replace('\\', "/").contains(".fallow/setup.lock"));
    }

    #[cfg(unix)]
    #[test]
    fn run_setup_resumes_valid_recipe_state_without_rewriting() {
        use super::{hash_file, save_setup_state};
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("tempdir should be created");
        let root = dir.path();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"api","packageManager":"npm@10.0.0","dependencies":{"elysia":"^1.0.0"}}"#,
        )
        .expect("package.json should be written");
        let sidecar = write_fake_sidecar(root, b"sidecar");
        let context = super::detect_setup_context(root);
        let recipe = recipe_contents(&context);
        let recipe_path = write_recipe(root, &recipe).expect("recipe should be written");
        let mut state = super::CoverageSetupState::default();
        record_sidecar_state(&mut state, sidecar).expect("sidecar should record");
        record_recipe_state(&mut state, recipe_path.clone(), &context, &recipe);
        save_setup_state(root, &state).expect("state should save");

        let docs_dir = root.join("docs");
        let original_mode = std::fs::metadata(&docs_dir)
            .expect("docs metadata should be readable")
            .permissions()
            .mode();
        let mut readonly = std::fs::metadata(&docs_dir)
            .expect("docs metadata should be readable")
            .permissions();
        readonly.set_mode(0o555);
        std::fs::set_permissions(&docs_dir, readonly).expect("docs should become read-only");

        let exit = run_setup(
            SetupArgs {
                non_interactive: true,
                ..SetupArgs::default()
            },
            root,
        );

        let mut restored = std::fs::metadata(&docs_dir)
            .expect("docs metadata should be readable")
            .permissions();
        restored.set_mode(original_mode & 0o7777);
        std::fs::set_permissions(&docs_dir, restored).expect("docs permissions should restore");

        assert_eq!(exit, std::process::ExitCode::SUCCESS);
        assert_eq!(
            std::fs::read_to_string(&recipe_path).expect("recipe should be readable"),
            recipe
        );
        assert_eq!(
            hash_file(&recipe_path).as_deref(),
            state.recipe.as_ref().map(|r| r.checksum.as_str())
        );
    }

    #[test]
    fn setup_json_explain_includes_meta_without_bumping_schema_version() {
        let dir = tempdir().expect("tempdir should be created");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"name":"demo","packageManager":"bun@1.2.0","dependencies":{"elysia":"^1.0.0"}}"#,
        )
        .expect("package.json should be written");

        let payload = build_setup_json(dir.path(), true);

        assert_eq!(payload["schema_version"], "1");
        assert_eq!(
            payload["_meta"]["docs_url"],
            "https://docs.fallow.tools/cli/coverage#agent-readable-json"
        );
        assert!(
            payload["_meta"]["field_definitions"]
                .as_object()
                .is_some_and(|fields| fields.contains_key("members[]"))
        );
        assert!(
            payload["_meta"]["enums"]
                .as_object()
                .is_some_and(|enums| enums.contains_key("runtime_targets"))
        );
    }

    fn write_fake_sidecar(root: &Path, bytes: &[u8]) -> PathBuf {
        let bin_dir = root.join("node_modules").join(".bin");
        std::fs::create_dir_all(&bin_dir).expect("sidecar bin dir should be created");
        let path = if cfg!(windows) {
            bin_dir.join("fallow-cov.cmd")
        } else {
            bin_dir.join("fallow-cov")
        };
        std::fs::write(&path, bytes).expect("sidecar should be written");
        path
    }

    #[test]
    fn detect_coverage_artifact_finds_nyc_output_istanbul_file() {
        let dir = tempdir().expect("tempdir should be created");
        let nyc_dir = dir.path().join(".nyc_output");
        std::fs::create_dir_all(&nyc_dir).expect("nyc dir should be created");
        let coverage_file = nyc_dir.join("coverage-final.json");
        std::fs::write(&coverage_file, "{}").expect("coverage file should be written");

        assert_eq!(detect_coverage_artifact(dir.path()), Some(coverage_file));
    }
}
