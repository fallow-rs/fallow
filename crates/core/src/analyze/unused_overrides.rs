//! Detection of unused and misconfigured pnpm, npm, and Bun
//! dependency-override entries.
//!
//! pnpm supports forcing transitive dependency versions through two
//! equivalent locations:
//!
//! - `overrides:` top-level in `pnpm-workspace.yaml` (pnpm 9+, canonical)
//! - `pnpm.overrides` in the root `package.json` (legacy form, still supported)
//!
//! npm supports the same mechanism through a top-level `overrides` object in
//! the root `package.json`, with nesting instead of `parent>child` keys. The
//! npm parser flattens nested objects into the shared entry shape, so all
//! three sources run through one analysis path. bun declares overrides through
//! the same top-level `overrides` key, so bun repos are analyzed via the npm
//! parser and resolved against `bun.lock`.
//!
//! bun also honours Yarn's top-level `resolutions` object as an alias of
//! `overrides` (issue #2367). In a bun repository (the root `packageManager`
//! names bun, or no recognised `packageManager` is declared and a `bun.lock`
//! or `bun.lockb` sits at the root) the `resolutions` entries run through the
//! same two detectors, reported with `source: "package.json"`. bun reads
//! `overrides` first and never consults `resolutions` once an `overrides` key
//! exists, so a manifest carrying both is analyzed for `overrides` alone.
//! yarn repositories stay out of scope: yarn never applies `overrides`, the
//! inert entries keep the "declare the pin under `resolutions`" hint, and
//! yarn's own `resolutions` semantics (glob paths, `yarn.lock`) are not
//! modelled.
//!
//! Two findings are emitted:
//!
//! 1. **`unused-dependency-overrides`**: an override whose target package is
//!    absent from both workspace `package.json` dep sections and the
//!    lockfile (`pnpm-lock.yaml`, `package-lock.json`, `npm-shrinkwrap.json`,
//!    or `bun.lock`). Overrides targeting resolved transitive packages are
//!    treated as used because CVE-fix pins often exist only in the lockfile.
//!    When Bun resolution ground truth is unreadable because only the legacy
//!    binary `bun.lockb` exists or the text `bun.lock` is malformed, no
//!    unused-override findings are emitted rather than degrading to
//!    declaration-only analysis that would flag every transitive-only pin.
//!    A workspace diagnostic names the unreadable lockfile and the recovery
//!    step.
//!
//! 2. **`misconfigured-dependency-overrides`**: an override whose key cannot
//!    be parsed or whose value is empty. The active package manager may reject
//!    or ignore these entries; fallow surfaces the issue statically.
//!
//! Suppression is config-only via `ignoreDependencyOverrides: [{ package,
//! source? }]`. Inline suppression is structurally impossible because
//! `pnpm-workspace.yaml` uses YAML comments and `package.json` has no
//! comment syntax.
//!
//! Parent-chain semantics: `react>react-dom` is reported as unused only when
//! BOTH `react` AND `react-dom` are absent from every workspace `package.json`
//! and `pnpm-lock.yaml`. This matches the common CVE-fix pattern where the
//! parent is declared and the override forces a transitive version inside that
//! parent's subtree.

use fallow_config::{
    CompiledIgnoreDependencyOverrideRule, PackageJson, PnpmOverrideData, ResolvedConfig,
    WorkspaceDiagnostic, WorkspaceDiagnosticKind, WorkspaceInfo, append_workspace_diagnostics,
    override_misconfig_reason as parser_misconfig_reason, parse_bun_package_json_resolutions,
    parse_npm_package_json_overrides, parse_pnpm_package_json_overrides,
    parse_pnpm_workspace_overrides, record_workspace_diagnostics,
};
use fallow_types::results::{
    DependencyOverrideMisconfigReason, DependencyOverrideSource, MisconfiguredDependencyOverride,
    UnusedDependencyOverride,
};
use rustc_hash::FxHashSet;

const PNPM_WORKSPACE_FILE: &str = "pnpm-workspace.yaml";
const PNPM_LOCK_FILE: &str = "pnpm-lock.yaml";
const NPM_LOCK_FILE: &str = "package-lock.json";
const NPM_SHRINKWRAP_FILE: &str = "npm-shrinkwrap.json";
const BUN_LOCK_FILE: &str = "bun.lock";
const BUN_LOCKB_FILE: &str = "bun.lockb";
const YARN_LOCK_FILE: &str = "yarn.lock";
const NODE_MODULES_SEGMENT: &str = "node_modules/";
const ROOT_PACKAGE_JSON: &str = "package.json";
const OVERRIDES_KEY: &str = "overrides";
const SOURCE_LABEL_YAML: &str = "pnpm-workspace.yaml";
const SOURCE_LABEL_JSON: &str = "package.json";
const HINT_MAY_BE_TRANSITIVE_PNPM: &str =
    "may target a transitive dependency; pnpm install --frozen-lockfile is the ground truth";
const HINT_MAY_BE_TRANSITIVE_BUN: &str =
    "may target a transitive dependency; bun install --frozen-lockfile is the ground truth";
const HINT_MAY_BE_TRANSITIVE_BUN_RESOLUTIONS: &str = "declared under `resolutions`, which bun applies as an alias of `overrides`; may target a transitive dependency; bun install --frozen-lockfile is the ground truth";
const HINT_MAY_BE_TRANSITIVE_NPM: &str =
    "may target a transitive dependency; npm ci is the ground truth";
const HINT_OVERRIDES_IGNORED_BY_YARN: &str =
    "yarn does not apply `overrides`; declare the pin under `resolutions` instead";
const LOCKFILE_DEPENDENCY_SECTIONS: &[&str] = &[
    "dependencies",
    "optionalDependencies",
    "devDependencies",
    "peerDependencies",
];

/// Combined override state across every source, plus the set of packages
/// declared in any workspace `package.json` dep section.
pub struct PnpmOverrideState {
    /// Entries from `pnpm-workspace.yaml`'s `overrides:` map. Empty when the
    /// file is missing, has no overrides section, or fails to parse.
    workspace_yaml_data: PnpmOverrideData,
    /// Entries from `<root>/package.json`'s `pnpm.overrides` map. Empty when
    /// the file is missing, has no pnpm.overrides section, or fails to parse.
    package_json_data: PnpmOverrideData,
    /// Flattened entries from `<root>/package.json`'s top-level npm
    /// `overrides` object. Empty when the file is missing, has no overrides
    /// section, or fails to parse.
    npm_package_json_data: PnpmOverrideData,
    /// Flat entries from `<root>/package.json`'s Yarn-style top-level
    /// `resolutions` object. Populated only for bun repositories whose
    /// manifest has no `overrides` key (bun ignores `resolutions` otherwise);
    /// empty for every other package manager (issue #2367).
    bun_resolutions_data: PnpmOverrideData,
    /// Every package name that appears in `dependencies` / `devDependencies` /
    /// `peerDependencies` / `optionalDependencies` of any workspace
    /// `package.json` (root + members).
    declared_packages: FxHashSet<String>,
    /// Every package name found in `pnpm-lock.yaml` package/snapshot keys,
    /// `package-lock.json` / `npm-shrinkwrap.json` package paths, `bun.lock`
    /// package specifiers, or dependency sections of any of those lockfiles.
    /// Includes transitive dependencies resolved by the package manager.
    lockfile_packages: FxHashSet<String>,
    /// Why bun resolution ground truth is unavailable, when unused-override
    /// analysis must fail closed instead of offering unsafe removal advice.
    lockfile_resolution_unavailable: Option<BunLockfileFailure>,
    /// Package-manager-appropriate hint attached to every unused-override
    /// finding, chosen from the root `package.json` `packageManager` field
    /// first and the lockfiles present at the root as fallback.
    transitive_hint: &'static str,
}

/// Read every override source and walk workspace `package.json` files to
/// build shared analysis state. Returns `None` when no source carries any
/// entries; callers should skip both override detectors in that case.
#[must_use]
pub fn gather_pnpm_override_state(
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
) -> Option<PnpmOverrideState> {
    let yaml_path = config.root.join(PNPM_WORKSPACE_FILE);
    let workspace_yaml_data = std::fs::read_to_string(&yaml_path)
        .ok()
        .map(|yaml_source| {
            parse_pnpm_workspace_overrides(&yaml_source).unwrap_or_else(|error| {
                super::unused_catalog::report_malformed_pnpm_workspace_yaml(
                    &config.root,
                    &yaml_path,
                    error,
                );
                PnpmOverrideData::default()
            })
        })
        .unwrap_or_default();

    let root_pkg_path = config.root.join(ROOT_PACKAGE_JSON);
    let root_pkg_source = std::fs::read_to_string(&root_pkg_path).ok();
    let root_manifest: Option<serde_json::Value> = root_pkg_source
        .as_deref()
        .and_then(|source| serde_json::from_str(source).ok());
    let package_json_data = root_pkg_source
        .as_deref()
        .map(parse_pnpm_package_json_overrides)
        .unwrap_or_default();
    let npm_package_json_data = root_pkg_source
        .as_deref()
        .map(parse_npm_package_json_overrides)
        .unwrap_or_default();

    let declared_manager = declared_package_manager(root_manifest.as_ref());
    // bun's `OverrideMap::parse_append` (src/install/lockfile/OverrideMap.rs)
    // takes the `overrides` property when it exists, whatever its value, and
    // falls through to `resolutions` only when `overrides` is absent, so a
    // manifest with both keys is analyzed for `overrides` alone.
    let manifest_declares_overrides = root_manifest
        .as_ref()
        .is_some_and(|manifest| manifest.get(OVERRIDES_KEY).is_some());
    let uses_bun = uses_bun(declared_manager, &config.root);
    let parsed_bun_resolutions = root_pkg_source
        .as_deref()
        .filter(|_| uses_bun)
        .map(parse_bun_package_json_resolutions)
        .unwrap_or_default();
    let bun_resolutions_shadowed =
        manifest_declares_overrides && !parsed_bun_resolutions.entries.is_empty();
    if bun_resolutions_shadowed {
        record_override_diagnostics(
            config,
            vec![WorkspaceDiagnostic::new(
                &config.root,
                root_pkg_path,
                WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides,
            )],
        );
    }
    let bun_resolutions_data = if manifest_declares_overrides {
        PnpmOverrideData::default()
    } else {
        parsed_bun_resolutions
    };

    if workspace_yaml_data.entries.is_empty()
        && package_json_data.entries.is_empty()
        && npm_package_json_data.entries.is_empty()
        && bun_resolutions_data.entries.is_empty()
    {
        return None;
    }

    let declared_packages = collect_declared_packages(config, workspaces);
    let lockfile_resolution = collect_lockfile_packages(config, declared_manager);

    Some(PnpmOverrideState {
        workspace_yaml_data,
        package_json_data,
        npm_package_json_data,
        bun_resolutions_data,
        declared_packages,
        lockfile_packages: lockfile_resolution.packages,
        lockfile_resolution_unavailable: lockfile_resolution.resolution_unavailable,
        transitive_hint: lockfile_resolution.transitive_hint,
    })
}

/// Walk every workspace `package.json` (root + members) and collect every
/// package name appearing in any dep section.
fn collect_declared_packages(
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
) -> FxHashSet<String> {
    let mut paths = Vec::with_capacity(workspaces.len() + 1);
    paths.push(config.root.join(ROOT_PACKAGE_JSON));
    for ws in workspaces {
        paths.push(ws.root.join(ROOT_PACKAGE_JSON));
    }

    let mut set: FxHashSet<String> = FxHashSet::default();
    for pkg_path in &paths {
        let Ok(raw_source) = std::fs::read_to_string(pkg_path) else {
            continue;
        };
        let Ok(pkg) = serde_json::from_str::<PackageJson>(&raw_source) else {
            continue;
        };
        for deps in [
            pkg.dependencies.as_ref(),
            pkg.dev_dependencies.as_ref(),
            pkg.peer_dependencies.as_ref(),
            pkg.optional_dependencies.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            for name in deps.keys() {
                set.insert(name.clone());
            }
        }
    }

    set
}

/// Resolved-package set gathered from every recognized lockfile at the root,
/// plus the derived analysis knobs that depend on which lockfiles exist.
struct LockfileResolution {
    packages: FxHashSet<String>,
    resolution_unavailable: Option<BunLockfileFailure>,
    transitive_hint: &'static str,
}

#[derive(Clone, Copy)]
enum BunLockfileFailure {
    Binary,
    Text,
}

/// Parse `pnpm-lock.yaml`, `package-lock.json` / `npm-shrinkwrap.json`, and
/// `bun.lock` and collect package names from resolved package keys plus
/// dependency maps. Missing lockfiles preserve the package.json-only fallback.
/// An unreadable Bun lockfile is different: a binary `bun.lockb`, or a text
/// `bun.lock` that fails to parse, sets `resolution_unavailable` when no
/// readable pnpm or npm lockfile provides independent resolution ground truth.
/// Callers then skip unused analysis instead of flagging every transitive-only
/// override.
fn collect_lockfile_packages(
    config: &ResolvedConfig,
    declared_manager: Option<DeclaredPackageManager>,
) -> LockfileResolution {
    let mut packages = FxHashSet::default();

    let mut has_pnpm_lock = false;
    if let Ok(raw_source) = std::fs::read_to_string(config.root.join(PNPM_LOCK_FILE)) {
        has_pnpm_lock = true;
        packages.extend(collect_pnpm_lock_packages(&raw_source));
    }
    // npm renames package-lock.json to npm-shrinkwrap.json for publishable
    // packages; the format is identical and a shrinkwrap repo usually carries
    // no package-lock.json at all.
    let mut has_npm_lock = false;
    for npm_lock_file in [NPM_LOCK_FILE, NPM_SHRINKWRAP_FILE] {
        if let Ok(raw_source) = std::fs::read_to_string(config.root.join(npm_lock_file)) {
            has_npm_lock = true;
            packages.extend(collect_npm_lock_packages(&raw_source));
        }
    }

    let has_bun_lock = config.root.join(BUN_LOCK_FILE).exists();
    let bun_lock_parsed = if let Ok(raw_source) =
        std::fs::read_to_string(config.root.join(BUN_LOCK_FILE))
        && let Some(bun_packages) = collect_bun_lock_packages(&raw_source)
    {
        packages.extend(bun_packages);
        true
    } else {
        false
    };
    let has_bun_lockb = config.root.join(BUN_LOCKB_FILE).exists();
    let has_yarn_lock = config.root.join(YARN_LOCK_FILE).exists();

    let transitive_hint = match declared_manager {
        Some(DeclaredPackageManager::Bun) => HINT_MAY_BE_TRANSITIVE_BUN,
        Some(DeclaredPackageManager::Npm) => HINT_MAY_BE_TRANSITIVE_NPM,
        Some(DeclaredPackageManager::Yarn) => HINT_OVERRIDES_IGNORED_BY_YARN,
        None if has_pnpm_lock => HINT_MAY_BE_TRANSITIVE_PNPM,
        None if has_bun_lock || has_bun_lockb => HINT_MAY_BE_TRANSITIVE_BUN,
        None if has_npm_lock => HINT_MAY_BE_TRANSITIVE_NPM,
        None if has_yarn_lock => HINT_OVERRIDES_IGNORED_BY_YARN,
        Some(DeclaredPackageManager::Pnpm) | None => HINT_MAY_BE_TRANSITIVE_PNPM,
    };

    LockfileResolution {
        packages,
        // A parseable pnpm or npm lockfile is complete resolution ground
        // truth on its own; a stale leftover bun.lockb must not silently
        // disable the analysis when one is present.
        resolution_unavailable: if !bun_lock_parsed && !has_pnpm_lock && !has_npm_lock {
            if has_bun_lock {
                Some(BunLockfileFailure::Text)
            } else if has_bun_lockb {
                Some(BunLockfileFailure::Binary)
            } else {
                None
            }
        } else {
            None
        },
        transitive_hint,
    }
}

/// Package managers the corepack `packageManager` field can name.
#[derive(Clone, Copy)]
enum DeclaredPackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

/// Read the corepack `packageManager` field (`"bun@1.3.2"` names bun) from
/// the parsed root `package.json`. Mirrors the packageManager-first probes in
/// the CLI package-manager detectors so the transitive hint cannot name a
/// package manager the repository does not use, for example a bun repo whose
/// lockfile is not committed yet.
fn declared_package_manager(
    root_manifest: Option<&serde_json::Value>,
) -> Option<DeclaredPackageManager> {
    let field = root_manifest?.get("packageManager")?.as_str()?;
    let name = field.split('@').next().unwrap_or(field);
    match name {
        "npm" => Some(DeclaredPackageManager::Npm),
        "pnpm" => Some(DeclaredPackageManager::Pnpm),
        "yarn" => Some(DeclaredPackageManager::Yarn),
        "bun" => Some(DeclaredPackageManager::Bun),
        _ => None,
    }
}

/// Whether the repository installs with bun, which decides if the root
/// manifest's `resolutions` object is an override source. The corepack
/// `packageManager` field wins when it names a known manager; without one, a
/// `bun.lock` or `bun.lockb` at the root counts. A manifest naming npm, pnpm,
/// or yarn is never a bun repository, even next to a leftover bun lockfile,
/// mirroring the packageManager-first rule the transitive hint uses.
fn uses_bun(declared_manager: Option<DeclaredPackageManager>, root: &std::path::Path) -> bool {
    match declared_manager {
        Some(DeclaredPackageManager::Bun) => true,
        Some(_) => false,
        None => root.join(BUN_LOCK_FILE).exists() || root.join(BUN_LOCKB_FILE).exists(),
    }
}

/// Collect package names from bun's text lockfile (`bun.lock`, bun 1.2+).
/// The file is JSONC (bun writes trailing commas); every entry in the
/// `packages` map is keyed by dependency-tree path and holds a tuple whose
/// first element is the resolved `name@version` specifier. Dependency
/// sections under `workspaces` and inside package metadata are covered by the
/// shared dependency-map walk. Returns `None` when the file does not parse,
/// so callers can distinguish "no resolution data" from an empty project.
///
/// Known limitation: the dependency-map walk also credits `peerDependencies`
/// entries that bun lists under `optionalPeers` without installing them,
/// matching package-lock.json behavior. The collected set means "resolvable",
/// not "actually installed", so an override targeting such a peer is
/// conservatively treated as used (a false negative, never removal advice).
fn collect_bun_lock_packages(source: &str) -> Option<FxHashSet<String>> {
    let Ok(value) = fallow_config::jsonc::parse_to_value::<serde_json::Value>(source) else {
        return None;
    };
    // Empty/whitespace input parses as `null`; a valid bun.lock is always an
    // object, so anything else counts as unparseable.
    if !value.is_object() {
        return None;
    }

    let mut packages = FxHashSet::default();
    if let Some(mapping) = value.get("packages").and_then(serde_json::Value::as_object) {
        for entry in mapping.values() {
            let Some(specifier) = entry
                .as_array()
                .and_then(|tuple| tuple.first())
                .and_then(serde_json::Value::as_str)
            else {
                continue;
            };
            if let Some(package_name) = package_name_from_lock_key(specifier) {
                packages.insert(package_name);
            }
        }
    }

    collect_json_dependency_map_names(&value, &mut packages);
    Some(packages)
}

/// Collect package names from `package-lock.json`. Lockfile v2/v3 keys the
/// `packages` map by installation path (`node_modules/<name>`, possibly
/// nested); the name is the segment after the last `node_modules/`. Entries
/// without a `node_modules/` segment (the `""` root and workspace member
/// paths) carry no resolved package name. Legacy v1 dependency trees and
/// per-entry dependency maps are covered by the recursive dependency-map walk.
fn collect_npm_lock_packages(source: &str) -> FxHashSet<String> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(source) else {
        return FxHashSet::default();
    };

    let mut packages = FxHashSet::default();
    if let Some(mapping) = value.get("packages").and_then(serde_json::Value::as_object) {
        for key in mapping.keys() {
            if let Some(idx) = key.rfind(NODE_MODULES_SEGMENT) {
                let name = &key[idx + NODE_MODULES_SEGMENT.len()..];
                if !name.is_empty() {
                    packages.insert(name.to_string());
                }
            }
        }
    }

    collect_json_dependency_map_names(&value, &mut packages);
    packages
}

fn collect_json_dependency_map_names(value: &serde_json::Value, packages: &mut FxHashSet<String>) {
    match value {
        serde_json::Value::Object(mapping) => {
            for (key, child) in mapping {
                if LOCKFILE_DEPENDENCY_SECTIONS.contains(&key.as_str())
                    && let Some(dependencies) = child.as_object()
                {
                    for package_name in dependencies.keys() {
                        packages.insert(package_name.clone());
                    }
                }
                collect_json_dependency_map_names(child, packages);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_json_dependency_map_names(item, packages);
            }
        }
        _ => {}
    }
}

fn collect_pnpm_lock_packages(source: &str) -> FxHashSet<String> {
    let Ok(value) = serde_yaml_ng::from_str::<serde_yaml_ng::Value>(source) else {
        return FxHashSet::default();
    };

    let mut packages = FxHashSet::default();
    let Some(root) = value.as_mapping() else {
        return packages;
    };

    for section in ["packages", "snapshots"] {
        let Some(mapping) = root.get(section).and_then(serde_yaml_ng::Value::as_mapping) else {
            continue;
        };
        for key in mapping.keys().filter_map(serde_yaml_ng::Value::as_str) {
            if let Some(package_name) = package_name_from_lock_key(key) {
                packages.insert(package_name);
            }
        }
    }

    collect_dependency_map_names(&value, &mut packages);
    packages
}

fn collect_dependency_map_names(value: &serde_yaml_ng::Value, packages: &mut FxHashSet<String>) {
    match value {
        serde_yaml_ng::Value::Mapping(mapping) => {
            for (key, child) in mapping {
                if key
                    .as_str()
                    .is_some_and(|name| LOCKFILE_DEPENDENCY_SECTIONS.contains(&name))
                    && let Some(dependencies) = child.as_mapping()
                {
                    for package_name in dependencies.keys().filter_map(serde_yaml_ng::Value::as_str)
                    {
                        packages.insert(package_name.to_string());
                    }
                }
                collect_dependency_map_names(child, packages);
            }
        }
        serde_yaml_ng::Value::Sequence(items) => {
            for item in items {
                collect_dependency_map_names(item, packages);
            }
        }
        _ => {}
    }
}

fn package_name_from_lock_key(raw_key: &str) -> Option<String> {
    let key = raw_key.trim().trim_start_matches('/');
    if key.is_empty() {
        return None;
    }

    if key.starts_with('@') {
        let scope_end = key.find('/')?;
        let package_segment = &key[scope_end + 1..];
        let name_end = package_segment
            .find(['@', '/', '('])
            .unwrap_or(package_segment.len());
        if name_end == 0 {
            return None;
        }
        return Some(key[..scope_end + 1 + name_end].to_string());
    }

    let name_end = key.find(['@', '/', '(']).unwrap_or(key.len());
    if name_end == 0 {
        return None;
    }
    Some(key[..name_end].to_string())
}

/// Record the matching skip diagnostic when Bun resolution ground truth is
/// unreadable and no pnpm or npm lockfile can replace it. Binary `bun.lockb`
/// anchors the diagnostic at the manifest because the lockfile cannot be read;
/// malformed text `bun.lock` anchors it at that file. Both reach
/// `workspace_diagnostics[]` and one deduplicated stderr warning, so the
/// absence of unused-override findings is explicit.
fn report_bun_override_resolution_skipped(config: &ResolvedConfig, failure: BunLockfileFailure) {
    let (path, kind) = match failure {
        BunLockfileFailure::Binary => (
            config.root.join(ROOT_PACKAGE_JSON),
            WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped,
        ),
        BunLockfileFailure::Text => (
            config.root.join(BUN_LOCK_FILE),
            WorkspaceDiagnosticKind::BunLockOverrideResolutionSkipped,
        ),
    };
    record_override_diagnostics(
        config,
        vec![WorkspaceDiagnostic::new(&config.root, path, kind)],
    );
}

fn record_override_diagnostics(config: &ResolvedConfig, diagnostics: Vec<WorkspaceDiagnostic>) {
    if should_emit_override_warning(config) {
        record_workspace_diagnostics(&config.root, diagnostics);
    } else {
        append_workspace_diagnostics(&config.root, diagnostics);
    }
}

fn should_emit_override_warning(config: &ResolvedConfig) -> bool {
    !config.analysis_snapshot.is_base()
}

/// Emit one `UnusedDependencyOverride` for every parseable override whose
/// target package (and parent, when present) is not declared in any workspace
/// `package.json` or resolved in any recognized lockfile. When resolution is
/// unavailable because Bun lockfile data is unreadable, emits nothing and
/// records the matching workspace diagnostic instead.
#[must_use]
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for typed output; serialize with fallow_api::serialize_dead_code_programmatic_json for JSON output. See docs/fallow-core-migration.md."
)]
pub fn find_unused_dependency_overrides(
    state: &PnpmOverrideState,
    config: &ResolvedConfig,
) -> Vec<UnusedDependencyOverride> {
    if let Some(failure) = state.lockfile_resolution_unavailable {
        report_bun_override_resolution_skipped(config, failure);
        return Vec::new();
    }

    let mut findings = Vec::new();
    let yaml_path = config.root.join(PNPM_WORKSPACE_FILE);
    let json_path = config.root.join(ROOT_PACKAGE_JSON);
    collect_unused_from_source(&mut UnusedOverrideSourceInput {
        data: &state.workspace_yaml_data,
        source: DependencyOverrideSource::PnpmWorkspaceYaml,
        source_path: &yaml_path,
        declared: &state.declared_packages,
        resolved: &state.lockfile_packages,
        hint: state.transitive_hint,
        ignore_rules: &config.compiled_ignore_dependency_overrides,
        findings: &mut findings,
    });
    collect_unused_from_source(&mut UnusedOverrideSourceInput {
        data: &state.package_json_data,
        source: DependencyOverrideSource::PnpmPackageJson,
        source_path: &json_path,
        declared: &state.declared_packages,
        resolved: &state.lockfile_packages,
        hint: state.transitive_hint,
        ignore_rules: &config.compiled_ignore_dependency_overrides,
        findings: &mut findings,
    });
    collect_unused_from_source(&mut UnusedOverrideSourceInput {
        data: &state.npm_package_json_data,
        source: DependencyOverrideSource::PnpmPackageJson,
        source_path: &json_path,
        declared: &state.declared_packages,
        resolved: &state.lockfile_packages,
        hint: state.transitive_hint,
        ignore_rules: &config.compiled_ignore_dependency_overrides,
        findings: &mut findings,
    });
    collect_unused_from_source(&mut UnusedOverrideSourceInput {
        data: &state.bun_resolutions_data,
        source: DependencyOverrideSource::PnpmPackageJson,
        source_path: &json_path,
        declared: &state.declared_packages,
        resolved: &state.lockfile_packages,
        hint: HINT_MAY_BE_TRANSITIVE_BUN_RESOLUTIONS,
        ignore_rules: &config.compiled_ignore_dependency_overrides,
        findings: &mut findings,
    });
    findings
}

struct UnusedOverrideSourceInput<'a> {
    data: &'a PnpmOverrideData,
    source: DependencyOverrideSource,
    source_path: &'a std::path::Path,
    declared: &'a FxHashSet<String>,
    resolved: &'a FxHashSet<String>,
    hint: &'static str,
    ignore_rules: &'a [CompiledIgnoreDependencyOverrideRule],
    findings: &'a mut Vec<UnusedDependencyOverride>,
}

fn collect_unused_from_source(input: &mut UnusedOverrideSourceInput<'_>) {
    for entry in &input.data.entries {
        let Some(parsed) = entry.parsed_key.as_ref() else {
            continue;
        };
        let Some(value) = entry.raw_value.as_ref() else {
            continue;
        };
        if !fallow_config::is_valid_override_value(value) {
            continue;
        }
        // `$package` values reference the version of a dependency declared at
        // the root; resolution is indirect, so credit rather than report.
        if value.starts_with('$') {
            continue;
        }

        let target_declared = input.declared.contains(&parsed.target_package);
        let target_resolved = input.resolved.contains(&parsed.target_package);
        let parent_declared = parsed
            .parent_package
            .as_ref()
            .is_some_and(|p| input.declared.contains(p));
        let parent_resolved = parsed
            .parent_package
            .as_ref()
            .is_some_and(|p| input.resolved.contains(p));
        if target_declared || target_resolved || parent_declared || parent_resolved {
            continue;
        }

        let source_label = source_label_for(input.source);
        if input
            .ignore_rules
            .iter()
            .any(|rule| rule.matches(&parsed.target_package, source_label))
        {
            continue;
        }

        let hint = Some(input.hint.to_string());

        input.findings.push(UnusedDependencyOverride {
            raw_key: entry.raw_key.clone(),
            target_package: parsed.target_package.clone(),
            parent_package: parsed.parent_package.clone(),
            version_constraint: parsed.target_version_selector.clone(),
            version_range: value.clone(),
            source: input.source,
            path: input.source_path.to_path_buf(),
            line: entry.line,
            hint,
        });
    }
}

/// Emit one `MisconfiguredDependencyOverride` for every entry whose key cannot
/// be parsed or whose value is missing.
#[must_use]
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for typed output; serialize with fallow_api::serialize_dead_code_programmatic_json for JSON output. See docs/fallow-core-migration.md."
)]
pub fn find_misconfigured_dependency_overrides(
    state: &PnpmOverrideState,
    config: &ResolvedConfig,
) -> Vec<MisconfiguredDependencyOverride> {
    let mut findings = Vec::new();
    let yaml_path = config.root.join(PNPM_WORKSPACE_FILE);
    let json_path = config.root.join(ROOT_PACKAGE_JSON);
    collect_misconfigured_from_source(
        &state.workspace_yaml_data,
        DependencyOverrideSource::PnpmWorkspaceYaml,
        &yaml_path,
        &config.compiled_ignore_dependency_overrides,
        &mut findings,
    );
    collect_misconfigured_from_source(
        &state.package_json_data,
        DependencyOverrideSource::PnpmPackageJson,
        &json_path,
        &config.compiled_ignore_dependency_overrides,
        &mut findings,
    );
    collect_misconfigured_from_source(
        &state.npm_package_json_data,
        DependencyOverrideSource::PnpmPackageJson,
        &json_path,
        &config.compiled_ignore_dependency_overrides,
        &mut findings,
    );
    collect_misconfigured_from_source(
        &state.bun_resolutions_data,
        DependencyOverrideSource::PnpmPackageJson,
        &json_path,
        &config.compiled_ignore_dependency_overrides,
        &mut findings,
    );
    findings
}

fn collect_misconfigured_from_source(
    data: &PnpmOverrideData,
    source: DependencyOverrideSource,
    source_path: &std::path::Path,
    ignore_rules: &[CompiledIgnoreDependencyOverrideRule],
    findings: &mut Vec<MisconfiguredDependencyOverride>,
) {
    for entry in &data.entries {
        let Some(reason) = parser_misconfig_reason(entry) else {
            continue;
        };

        let target_for_ignore = entry
            .parsed_key
            .as_ref()
            .map_or(entry.raw_key.as_str(), |p| p.target_package.as_str());

        let source_label = source_label_for(source);
        if ignore_rules
            .iter()
            .any(|rule| rule.matches(target_for_ignore, source_label))
        {
            continue;
        }

        let target_package = entry.parsed_key.as_ref().map(|p| p.target_package.clone());

        findings.push(MisconfiguredDependencyOverride {
            raw_key: entry.raw_key.clone(),
            target_package,
            raw_value: entry.raw_value.clone().unwrap_or_default(),
            reason: map_misconfig_reason(reason),
            source,
            path: source_path.to_path_buf(),
            line: entry.line,
        });
    }
}

const fn map_misconfig_reason(
    reason: fallow_config::MisconfigReason,
) -> DependencyOverrideMisconfigReason {
    match reason {
        fallow_config::MisconfigReason::UnparsableKey => {
            DependencyOverrideMisconfigReason::UnparsableKey
        }
        fallow_config::MisconfigReason::EmptyValue => DependencyOverrideMisconfigReason::EmptyValue,
    }
}

const fn source_label_for(source: DependencyOverrideSource) -> &'static str {
    match source {
        DependencyOverrideSource::PnpmWorkspaceYaml => SOURCE_LABEL_YAML,
        DependencyOverrideSource::PnpmPackageJson => SOURCE_LABEL_JSON,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_key_bare_package_with_version() {
        assert_eq!(
            package_name_from_lock_key("react@18.3.1"),
            Some("react".to_string())
        );
    }

    #[test]
    fn lock_key_scoped_package_with_version() {
        assert_eq!(
            package_name_from_lock_key("@types/react@18.2.0"),
            Some("@types/react".to_string())
        );
    }

    #[test]
    fn lock_key_scoped_package_with_peer_suffix() {
        assert_eq!(
            package_name_from_lock_key("@scope/pkg@1.0.0(peer@2.0.0)"),
            Some("@scope/pkg".to_string())
        );
    }

    #[test]
    fn lock_key_pnpm6_leading_slash() {
        assert_eq!(
            package_name_from_lock_key("/react@18.3.1"),
            Some("react".to_string())
        );
    }

    #[test]
    fn lock_key_pnpm6_leading_slash_scoped() {
        assert_eq!(
            package_name_from_lock_key("/@types/react@18.2.0"),
            Some("@types/react".to_string())
        );
    }

    #[test]
    fn lock_key_no_version() {
        assert_eq!(
            package_name_from_lock_key("react"),
            Some("react".to_string())
        );
        assert_eq!(
            package_name_from_lock_key("@scope/pkg"),
            Some("@scope/pkg".to_string())
        );
    }

    #[test]
    fn lock_key_npm_alias() {
        // Resolve npm aliases to the consumer-facing name.
        assert_eq!(
            package_name_from_lock_key("debug@npm:obug@^1.0.2"),
            Some("debug".to_string())
        );
    }

    #[test]
    fn lock_key_paren_only_suffix() {
        assert_eq!(
            package_name_from_lock_key("react(peer@2)"),
            Some("react".to_string())
        );
    }

    #[test]
    fn lock_key_whitespace_is_trimmed() {
        assert_eq!(
            package_name_from_lock_key("   react@1.0.0   "),
            Some("react".to_string())
        );
    }

    #[test]
    fn lock_key_empty_returns_none() {
        assert_eq!(package_name_from_lock_key(""), None);
        assert_eq!(package_name_from_lock_key("   "), None);
        assert_eq!(package_name_from_lock_key("/"), None);
    }

    #[test]
    fn lock_key_malformed_scope_returns_none() {
        assert_eq!(package_name_from_lock_key("@scope"), None);
        assert_eq!(package_name_from_lock_key("@scope/"), None);
    }

    #[test]
    fn collect_lock_packages_handles_lockfile_v9_shape() {
        let source = "lockfileVersion: '9.0'\n\
                      \n\
                      importers:\n  \
                        .:\n    \
                          dependencies:\n      \
                            react:\n        specifier: ^18.0.0\n        version: 18.3.1\n\
                      \n\
                      packages:\n  \
                        react@18.3.1:\n    resolution: {integrity: sha512-r}\n  \
                        postcss@8.5.10:\n    resolution: {integrity: sha512-p}\n\
                      \n\
                      snapshots:\n  \
                        react@18.3.1:\n    dependencies:\n      loose-envify: 1.4.0\n  \
                        postcss@8.5.10: {}\n  \
                        loose-envify@1.4.0: {}\n";
        let packages = collect_pnpm_lock_packages(source);
        assert!(packages.contains("react"));
        assert!(packages.contains("postcss"));
        assert!(packages.contains("loose-envify"));
    }

    #[test]
    fn collect_lock_packages_malformed_yields_empty() {
        let packages = collect_pnpm_lock_packages("lockfileVersion: '9.0\n  this: [[[");
        assert!(packages.is_empty());
    }

    #[test]
    fn collect_lock_packages_empty_yields_empty() {
        assert!(collect_pnpm_lock_packages("").is_empty());
    }

    // Trimmed from a real `bun install` (bun 1.3.x) run; keeps bun's trailing
    // commas so the JSONC dialect is exercised.
    const BUN_LOCK_REAL_SHAPE: &str = r#"{
  "lockfileVersion": 1,
  "configVersion": 1,
  "workspaces": {
    "": {
      "name": "bun-repro",
      "devDependencies": {
        "happy-dom": "^20.10.6",
      },
    },
  },
  "overrides": {
    "ws": "^8.21.0",
  },
  "packages": {
    "@types/whatwg-mimetype": ["@types/whatwg-mimetype@3.0.2", "", {}, "sha512-c2"],
    "happy-dom": ["happy-dom@20.11.6", "", { "dependencies": { "@types/whatwg-mimetype": "^3.0.2", "whatwg-mimetype": "^3.0.0", "ws": "^8.21.0" } }, "sha512-Hl"],
    "whatwg-mimetype": ["whatwg-mimetype@3.0.0", "", {}, "sha512-nt"],
    "ws": ["ws@8.21.3", "", { "peerDependencies": { "bufferutil": "^4.0.1", "utf-8-validate": ">=5.0.2" }, "optionalPeers": ["bufferutil", "utf-8-validate"] }, "sha512-20"],
  }
}
"#;

    #[test]
    fn collect_bun_lock_packages_real_shape() {
        let packages = collect_bun_lock_packages(BUN_LOCK_REAL_SHAPE).expect("bun.lock parses");
        for name in [
            "happy-dom",
            "ws",
            "@types/whatwg-mimetype",
            "whatwg-mimetype",
        ] {
            assert!(packages.contains(name), "missing {name}: {packages:?}");
        }
        assert!(
            !packages.contains("bun-repro"),
            "workspace name is not a resolved package"
        );
    }

    #[test]
    fn collect_bun_lock_packages_override_keys_are_not_credited_as_resolved() {
        // bun mirrors the `overrides` map into bun.lock; only the resolved
        // package graph may credit a target, otherwise every override would
        // trivially count as used.
        let source = r#"{
  "lockfileVersion": 1,
  "workspaces": { "": { "name": "x" } },
  "overrides": { "left-pad": "^1.3.0" },
  "packages": {}
}"#;
        let packages = collect_bun_lock_packages(source).expect("parses");
        assert!(!packages.contains("left-pad"), "got {packages:?}");
    }

    #[test]
    fn collect_bun_lock_packages_nested_tree_path_uses_specifier_name() {
        // Conflicting versions are keyed by tree path ("parent/child"); the
        // resolved name comes from the tuple's first element.
        let source = r#"{
  "lockfileVersion": 1,
  "workspaces": { "": { "name": "x", "dependencies": { "parent-pkg": "^1.0.0" } } },
  "packages": {
    "parent-pkg": ["parent-pkg@1.0.0", "", { "dependencies": { "shared-dep": "^1.0.0" } }, "sha512-a"],
    "parent-pkg/shared-dep": ["shared-dep@1.2.3", "", {}, "sha512-b"],
  }
}"#;
        let packages = collect_bun_lock_packages(source).expect("parses");
        assert!(packages.contains("shared-dep"), "got {packages:?}");
        assert!(packages.contains("parent-pkg"), "got {packages:?}");
    }

    #[test]
    fn collect_bun_lock_packages_malformed_returns_none() {
        assert!(collect_bun_lock_packages("not json {{{").is_none());
        assert!(collect_bun_lock_packages("").is_none());
    }

    // Issue #2358: the bun.lockb-only skip must announce itself through the
    // workspace-diagnostics channel instead of silently emitting nothing.

    const BUN_LOCKB_PLACEHOLDER: &[u8] = b"\x00binary lockfile\x01\x02";

    fn write_bun_manifest(root: &std::path::Path, overrides: Option<&str>) {
        let overrides_field = overrides
            .map(|value| format!(",\n  \"overrides\": {value}"))
            .unwrap_or_default();
        std::fs::write(
            root.join(ROOT_PACKAGE_JSON),
            format!(
                r#"{{
  "name": "issue-2358-bun-lockb",
  "private": true,
  "packageManager": "bun@1.3.2",
  "devDependencies": {{ "happy-dom": "^20.10.6" }}{overrides_field}
}}"#
            ),
        )
        .expect("write package.json");
    }

    fn resolve_config(root: &std::path::Path) -> ResolvedConfig {
        fallow_config::FallowConfig::default().resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        )
    }

    #[expect(
        deprecated,
        reason = "the detector helper is deprecated for external callers; the unit test exercises the internal skip path"
    )]
    fn run_unused_override_detector(
        config: &ResolvedConfig,
    ) -> Option<Vec<UnusedDependencyOverride>> {
        let state = gather_pnpm_override_state(config, &[])?;
        Some(find_unused_dependency_overrides(&state, config))
    }

    fn bun_lockb_skip_diagnostics(
        root: &std::path::Path,
    ) -> Vec<fallow_config::WorkspaceDiagnostic> {
        fallow_config::workspace_diagnostics_for(root)
            .into_iter()
            .filter(|diagnostic| {
                matches!(
                    diagnostic.kind,
                    fallow_config::WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped
                )
            })
            .collect()
    }

    #[test]
    fn bun_lockb_only_with_overrides_records_skip_diagnostic_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_bun_manifest(root, Some(r#"{ "ws": "^8.21.0" }"#));
        std::fs::write(root.join(BUN_LOCKB_FILE), BUN_LOCKB_PLACEHOLDER).expect("write bun.lockb");
        let config = resolve_config(root);

        let findings = run_unused_override_detector(&config).expect("overrides are declared");
        assert!(
            findings.is_empty(),
            "bun.lockb is unreadable; the check must stay skipped: {findings:?}"
        );

        let diagnostics = bun_lockb_skip_diagnostics(root);
        assert_eq!(
            diagnostics.len(),
            1,
            "expected exactly one skip diagnostic: {diagnostics:?}"
        );
        let diagnostic = &diagnostics[0];
        assert_eq!(diagnostic.path, root.join(ROOT_PACKAGE_JSON));
        assert!(
            diagnostic
                .message
                .contains("bun install --save-text-lockfile"),
            "message carries the text-lockfile hint: {}",
            diagnostic.message
        );

        // A second analysis on the same root (watch mode, combined mode) must
        // not stack a duplicate entry.
        let _ = run_unused_override_detector(&config);
        assert_eq!(
            bun_lockb_skip_diagnostics(root).len(),
            1,
            "the registry dedupes on kind + path"
        );
    }

    #[test]
    fn bun_lockb_only_without_overrides_records_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_bun_manifest(root, None);
        std::fs::write(root.join(BUN_LOCKB_FILE), BUN_LOCKB_PLACEHOLDER).expect("write bun.lockb");
        let config = resolve_config(root);

        assert!(
            run_unused_override_detector(&config).is_none(),
            "no override entries means no override state at all"
        );
        assert!(
            bun_lockb_skip_diagnostics(root).is_empty(),
            "nothing was skipped, so nothing is announced"
        );
    }

    #[test]
    fn malformed_text_bun_lock_fails_closed_with_a_diagnostic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_bun_manifest(root, Some(r#"{ "transitive-only": "^1.0.0" }"#));
        std::fs::write(root.join(BUN_LOCK_FILE), "not valid json").expect("write bun.lock");
        let config = resolve_config(root);

        let findings = run_unused_override_detector(&config).expect("overrides are declared");
        assert!(
            findings.is_empty(),
            "unreadable resolution must not produce removal advice"
        );
        assert!(
            fallow_config::workspace_diagnostics_for(root)
                .iter()
                .any(|diagnostic| {
                    matches!(
                        diagnostic.kind,
                        fallow_config::WorkspaceDiagnosticKind::BunLockOverrideResolutionSkipped
                    )
                })
        );
    }

    #[test]
    fn bun_lockb_next_to_text_bun_lock_resolves_and_records_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_bun_manifest(root, Some(r#"{ "ws": "^8.21.0", "left-pad": "^1.3.0" }"#));
        std::fs::write(root.join(BUN_LOCKB_FILE), BUN_LOCKB_PLACEHOLDER).expect("write bun.lockb");
        std::fs::write(root.join(BUN_LOCK_FILE), BUN_LOCK_REAL_SHAPE).expect("write bun.lock");
        let config = resolve_config(root);

        let findings = run_unused_override_detector(&config).expect("overrides are declared");
        let flagged: Vec<&str> = findings
            .iter()
            .map(|finding| finding.target_package.as_str())
            .collect();
        assert_eq!(
            flagged,
            vec!["left-pad"],
            "bun.lock resolves ws transitively; left-pad stays unresolved"
        );
        assert!(
            bun_lockb_skip_diagnostics(root).is_empty(),
            "a parseable bun.lock means resolution ran, so no skip diagnostic"
        );
    }

    #[test]
    fn bun_lockb_next_to_yarn_lock_or_unparseable_bun_lock_still_records_skip() {
        // Neither sibling restores resolution: yarn.lock is never consulted and
        // an unparseable bun.lock yields no package set, so the skip fires and
        // the message must not claim bun.lockb was the only lockfile.
        for (sibling, content) in [
            (YARN_LOCK_FILE, "# yarn lockfile v1\n"),
            (BUN_LOCK_FILE, ""),
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            let root = dir.path();
            write_bun_manifest(root, Some(r#"{ "ws": "^8.21.0" }"#));
            std::fs::write(root.join(BUN_LOCKB_FILE), BUN_LOCKB_PLACEHOLDER)
                .expect("write bun.lockb");
            std::fs::write(root.join(sibling), content).expect("write sibling lockfile");
            let config = resolve_config(root);

            let findings = run_unused_override_detector(&config).expect("overrides are declared");
            assert!(
                findings.is_empty(),
                "{sibling} next to bun.lockb does not restore resolution: {findings:?}"
            );
            let diagnostics: Vec<_> = fallow_config::workspace_diagnostics_for(root)
                .into_iter()
                .filter(|diagnostic| {
                    matches!(
                        diagnostic.kind,
                        fallow_config::WorkspaceDiagnosticKind::BunLockbOverrideResolutionSkipped
                            | fallow_config::WorkspaceDiagnosticKind::BunLockOverrideResolutionSkipped
                    )
                })
                .collect();
            assert_eq!(
                diagnostics.len(),
                1,
                "{sibling} next to bun.lockb still skips and announces it: {diagnostics:?}"
            );
            let message = &diagnostics[0].message;
            if sibling == YARN_LOCK_FILE {
                assert!(
                    !message.contains("only bun.lockb")
                        && message.contains("no parseable text lockfile")
                        && message.contains("delete the stale bun.lockb"),
                    "message describes the binary-lock condition: {message}"
                );
            } else {
                assert!(
                    message.contains("could not be parsed") && message.contains("regenerate"),
                    "message describes the malformed text lockfile: {message}"
                );
            }
        }
    }

    #[test]
    fn no_lockfile_keeps_declaration_only_analysis_without_diagnostic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_bun_manifest(root, Some(r#"{ "ws": "^8.21.0" }"#));
        let config = resolve_config(root);

        let findings = run_unused_override_detector(&config).expect("overrides are declared");
        let flagged: Vec<&str> = findings
            .iter()
            .map(|finding| finding.target_package.as_str())
            .collect();
        assert_eq!(
            flagged,
            vec!["ws"],
            "without any lockfile the declaration-only fallback still reports"
        );
        assert!(
            bun_lockb_skip_diagnostics(root).is_empty(),
            "the diagnostic is specific to bun.lockb, not to a missing lockfile"
        );
    }

    // Issue #2367: bun honours Yarn-style `resolutions` as an `overrides`
    // alias, so a bun manifest that pins versions there must reach the same
    // detectors and the same bun.lockb skip diagnostic.

    fn write_manifest(
        root: &std::path::Path,
        package_manager: Option<&str>,
        overrides: Option<&str>,
        resolutions: Option<&str>,
    ) {
        let package_manager_field = package_manager
            .map(|value| format!(",\n  \"packageManager\": \"{value}\""))
            .unwrap_or_default();
        let overrides_field = overrides
            .map(|value| format!(",\n  \"overrides\": {value}"))
            .unwrap_or_default();
        let resolutions_field = resolutions
            .map(|value| format!(",\n  \"resolutions\": {value}"))
            .unwrap_or_default();
        std::fs::write(
            root.join(ROOT_PACKAGE_JSON),
            format!(
                r#"{{
  "name": "issue-2367-bun-resolutions",
  "private": true,
  "devDependencies": {{ "happy-dom": "^20.10.6" }}{package_manager_field}{overrides_field}{resolutions_field}
}}"#
            ),
        )
        .expect("write package.json");
    }

    #[expect(
        deprecated,
        reason = "the detector helper is deprecated for external callers; the unit test exercises the internal resolutions path"
    )]
    fn run_misconfigured_override_detector(
        config: &ResolvedConfig,
    ) -> Option<Vec<MisconfiguredDependencyOverride>> {
        let state = gather_pnpm_override_state(config, &[])?;
        Some(find_misconfigured_dependency_overrides(&state, config))
    }

    fn flagged_targets(findings: &[UnusedDependencyOverride]) -> Vec<&str> {
        findings
            .iter()
            .map(|finding| finding.target_package.as_str())
            .collect()
    }

    const RESOLUTIONS_WS_AND_LEFT_PAD: &str = r#"{ "ws": "^8.21.0", "left-pad": "^1.3.0" }"#;
    const RESOLUTIONS_LEFT_PAD: &str = r#"{ "left-pad": "^1.3.0" }"#;

    #[test]
    fn bun_nonempty_resolutions_shadowed_by_overrides_are_diagnostic_only() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_manifest(
            root,
            Some("bun@1.3.2"),
            Some(r#"{ "ws": "^8.21.0" }"#),
            Some(RESOLUTIONS_LEFT_PAD),
        );
        let config = resolve_config(root);

        let findings = run_unused_override_detector(&config).expect("overrides are declared");
        assert_eq!(flagged_targets(&findings), vec!["ws"]);
        assert!(
            fallow_config::workspace_diagnostics_for(root)
                .iter()
                .any(|diagnostic| {
                    matches!(
                        diagnostic.kind,
                        fallow_config::WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides
                    )
                })
        );
    }

    #[test]
    fn base_snapshot_keeps_override_diagnostic_structured_without_warning() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_manifest(
            root,
            Some("bun@1.3.2"),
            Some(r#"{ "ws": "^8.21.0" }"#),
            Some(RESOLUTIONS_LEFT_PAD),
        );
        let mut config = resolve_config(root);
        config.analysis_snapshot = fallow_config::AnalysisSnapshot::Base;

        assert!(!should_emit_override_warning(&config));
        let _ = run_unused_override_detector(&config);
        assert!(
            fallow_config::workspace_diagnostics_for(root)
                .iter()
                .any(|diagnostic| {
                    matches!(
                        diagnostic.kind,
                        fallow_config::WorkspaceDiagnosticKind::BunResolutionsShadowedByOverrides
                    )
                })
        );
    }

    #[test]
    fn bun_resolutions_only_next_to_bun_lockb_records_skip_diagnostic_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_manifest(
            root,
            Some("bun@1.3.2"),
            None,
            Some(RESOLUTIONS_WS_AND_LEFT_PAD),
        );
        std::fs::write(root.join(BUN_LOCKB_FILE), BUN_LOCKB_PLACEHOLDER).expect("write bun.lockb");
        let config = resolve_config(root);

        let findings = run_unused_override_detector(&config)
            .expect("resolutions are override state in a bun repository");
        assert!(
            findings.is_empty(),
            "bun.lockb is unreadable; the check must stay skipped: {findings:?}"
        );
        let diagnostics = bun_lockb_skip_diagnostics(root);
        assert_eq!(
            diagnostics.len(),
            1,
            "a resolutions-only manifest announces the skip like an overrides one: {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].path, root.join(ROOT_PACKAGE_JSON));

        let _ = run_unused_override_detector(&config);
        assert_eq!(
            bun_lockb_skip_diagnostics(root).len(),
            1,
            "the registry dedupes on kind + path"
        );
    }

    #[test]
    fn bun_resolutions_resolve_against_text_bun_lock() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_manifest(
            root,
            Some("bun@1.3.2"),
            None,
            Some(RESOLUTIONS_WS_AND_LEFT_PAD),
        );
        std::fs::write(root.join(BUN_LOCK_FILE), BUN_LOCK_REAL_SHAPE).expect("write bun.lock");
        let config = resolve_config(root);

        let findings = run_unused_override_detector(&config)
            .expect("resolutions are override state in a bun repository");
        assert_eq!(
            flagged_targets(&findings),
            vec!["left-pad"],
            "bun.lock resolves ws transitively; left-pad stays unresolved"
        );
        let finding = &findings[0];
        assert_eq!(finding.raw_key, "left-pad");
        assert_eq!(finding.source, DependencyOverrideSource::PnpmPackageJson);
        assert_eq!(finding.path, root.join(ROOT_PACKAGE_JSON));
        assert_eq!(finding.line, 6, "the resolutions object sits on line 6");
        let hint = finding.hint.as_deref().unwrap_or_default();
        assert!(
            hint.contains("resolutions") && hint.contains("bun install --frozen-lockfile"),
            "the bun hint names the resolutions origin: {hint}"
        );
        assert!(
            bun_lockb_skip_diagnostics(root).is_empty(),
            "a parseable bun.lock means resolution ran"
        );
    }

    #[test]
    fn bun_overrides_key_shadows_resolutions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_manifest(
            root,
            Some("bun@1.3.2"),
            Some(r#"{ "ws": "^8.21.0" }"#),
            Some(RESOLUTIONS_LEFT_PAD),
        );
        std::fs::write(root.join(BUN_LOCK_FILE), BUN_LOCK_REAL_SHAPE).expect("write bun.lock");
        let config = resolve_config(root);

        let findings = run_unused_override_detector(&config).expect("overrides are declared");
        assert!(
            findings.is_empty(),
            "bun never reads resolutions once an overrides key exists: {findings:?}"
        );

        let empty_dir = tempfile::tempdir().expect("tempdir");
        let empty_root = empty_dir.path();
        write_manifest(
            empty_root,
            Some("bun@1.3.2"),
            Some("{}"),
            Some(RESOLUTIONS_LEFT_PAD),
        );
        std::fs::write(empty_root.join(BUN_LOCK_FILE), BUN_LOCK_REAL_SHAPE)
            .expect("write bun.lock");
        let config = resolve_config(empty_root);
        assert!(
            run_unused_override_detector(&config).is_none(),
            "an empty overrides object still shadows resolutions, so no override state exists"
        );
    }

    #[derive(Debug)]
    struct NonBunRepository {
        package_manager: Option<&'static str>,
        lockfile: Option<(&'static str, &'static str)>,
    }

    #[test]
    fn resolutions_are_ignored_outside_bun_repositories() {
        let cases = [
            NonBunRepository {
                package_manager: Some("yarn@4.5.0"),
                lockfile: Some((YARN_LOCK_FILE, "# yarn lockfile v1\n")),
            },
            // A declared package manager wins over a leftover bun lockfile.
            NonBunRepository {
                package_manager: Some("npm@10.9.0"),
                lockfile: Some((BUN_LOCK_FILE, BUN_LOCK_REAL_SHAPE)),
            },
            NonBunRepository {
                package_manager: None,
                lockfile: Some((PNPM_LOCK_FILE, "lockfileVersion: '9.0'\n")),
            },
            NonBunRepository {
                package_manager: None,
                lockfile: None,
            },
        ];
        for case in cases {
            let dir = tempfile::tempdir().expect("tempdir");
            let root = dir.path();
            write_manifest(root, case.package_manager, None, Some(RESOLUTIONS_LEFT_PAD));
            if let Some((name, content)) = case.lockfile {
                std::fs::write(root.join(name), content).expect("write lockfile");
            }
            let config = resolve_config(root);
            assert!(
                run_unused_override_detector(&config).is_none(),
                "{case:?}: resolutions are not an override source outside bun repositories"
            );
        }
    }

    #[test]
    fn bun_lockfile_without_package_manager_field_enables_resolutions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_manifest(root, None, None, Some(RESOLUTIONS_WS_AND_LEFT_PAD));
        std::fs::write(root.join(BUN_LOCK_FILE), BUN_LOCK_REAL_SHAPE).expect("write bun.lock");
        let config = resolve_config(root);
        let findings = run_unused_override_detector(&config)
            .expect("a root bun.lock marks the repository as bun");
        assert_eq!(flagged_targets(&findings), vec!["left-pad"]);

        let lockb_dir = tempfile::tempdir().expect("tempdir");
        let lockb_root = lockb_dir.path();
        write_manifest(lockb_root, None, None, Some(RESOLUTIONS_WS_AND_LEFT_PAD));
        std::fs::write(lockb_root.join(BUN_LOCKB_FILE), BUN_LOCKB_PLACEHOLDER)
            .expect("write bun.lockb");
        let config = resolve_config(lockb_root);
        let findings = run_unused_override_detector(&config)
            .expect("a root bun.lockb marks the repository as bun");
        assert!(findings.is_empty(), "bun.lockb is unreadable: {findings:?}");
        assert_eq!(
            bun_lockb_skip_diagnostics(lockb_root).len(),
            1,
            "the skip is announced for the resolutions-only manifest"
        );
    }

    #[test]
    fn bun_resolutions_yarn_path_keys_credit_parents_and_flag_rejected_shapes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        write_manifest(
            root,
            Some("bun@1.3.2"),
            None,
            Some(
                r#"{
    "**/ws": "^8.21.0",
    "happy-dom/left-pad": "^1.3.0",
    "**/left-pad": "^1.3.0",
    "a/b/c": "^1.0.0",
    "nested": { "left-pad": "^1.3.0" }
  }"#,
            ),
        );
        std::fs::write(root.join(BUN_LOCK_FILE), BUN_LOCK_REAL_SHAPE).expect("write bun.lock");
        let config = resolve_config(root);

        let unused = run_unused_override_detector(&config).expect("resolutions are declared");
        let flagged: Vec<(&str, &str)> = unused
            .iter()
            .map(|finding| (finding.raw_key.as_str(), finding.target_package.as_str()))
            .collect();
        assert_eq!(
            flagged,
            vec![("**/left-pad", "left-pad")],
            "ws resolves through bun.lock, happy-dom/left-pad is credited through its declared parent, and the shapes bun rejects never reach the unused check"
        );

        let misconfigured =
            run_misconfigured_override_detector(&config).expect("resolutions are declared");
        let reasons: Vec<(&str, DependencyOverrideMisconfigReason)> = misconfigured
            .iter()
            .map(|finding| (finding.raw_key.as_str(), finding.reason))
            .collect();
        assert_eq!(
            reasons,
            vec![
                ("a/b/c", DependencyOverrideMisconfigReason::UnparsableKey),
                ("nested", DependencyOverrideMisconfigReason::EmptyValue),
            ],
            "a path deeper than one parent and a non-string value are the shapes bun warns about and skips"
        );
        assert!(
            misconfigured
                .iter()
                .all(|finding| finding.source == DependencyOverrideSource::PnpmPackageJson)
        );
    }
}
