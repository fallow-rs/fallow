//! Detection of unused and misconfigured pnpm dependency-override entries.
//!
//! pnpm supports forcing transitive dependency versions through two
//! equivalent locations:
//!
//! - `overrides:` top-level in `pnpm-workspace.yaml` (pnpm 9+, canonical)
//! - `pnpm.overrides` in the root `package.json` (legacy form, still supported)
//!
//! Two findings are emitted:
//!
//! 1. **`unused-dependency-overrides`**: an override whose target package is
//!    not declared in any workspace `package.json` dep section. Conservative
//!    static algorithm: the v1 detector does not read `pnpm-lock.yaml`, so
//!    overrides targeting purely-transitive packages (a common CVE-fix /
//!    canary-aliasing pattern) can produce false positives. Every unused
//!    finding (bare-target AND parent-chain) carries a `hint` flagging the
//!    transitive-dependency possibility so consumers can de-prioritize.
//!
//! 2. **`misconfigured-dependency-overrides`**: an override whose key cannot
//!    be parsed or whose value is empty. `pnpm install` refuses to honor
//!    these entries; fallow surfaces the issue statically.
//!
//! Suppression is config-only via `ignoreDependencyOverrides: [{ package,
//! source? }]`. Inline suppression is structurally impossible because
//! `pnpm-workspace.yaml` uses YAML comments and `package.json` has no
//! comment syntax.
//!
//! Parent-chain semantics: `react>react-dom` is reported as unused only when
//! BOTH `react` AND `react-dom` are absent from every workspace `package.json`.
//! This matches the common CVE-fix pattern where the parent is declared and
//! the override forces a transitive version inside that parent's subtree.

use fallow_config::{
    CompiledIgnoreDependencyOverrideRule, PackageJson, PnpmOverrideData, ResolvedConfig,
    WorkspaceInfo, override_misconfig_reason as parser_misconfig_reason,
    parse_pnpm_package_json_overrides, parse_pnpm_workspace_overrides,
};
use fallow_types::results::{
    DependencyOverrideMisconfigReason, DependencyOverrideSource, MisconfiguredDependencyOverride,
    UnusedDependencyOverride,
};
use rustc_hash::FxHashSet;

const PNPM_WORKSPACE_FILE: &str = "pnpm-workspace.yaml";
const ROOT_PACKAGE_JSON: &str = "package.json";
const SOURCE_LABEL_YAML: &str = "pnpm-workspace.yaml";
const SOURCE_LABEL_JSON: &str = "package.json";
const HINT_MAY_BE_TRANSITIVE: &str =
    "may target a transitive dependency; pnpm install --frozen-lockfile is the ground truth";

/// Combined override state across both sources, plus the set of packages
/// declared in any workspace `package.json` dep section.
pub struct PnpmOverrideState {
    /// Entries from `pnpm-workspace.yaml`'s `overrides:` map. Empty when the
    /// file is missing, has no overrides section, or fails to parse.
    workspace_yaml_data: PnpmOverrideData,
    /// Entries from `<root>/package.json`'s `pnpm.overrides` map. Empty when
    /// the file is missing, has no pnpm.overrides section, or fails to parse.
    package_json_data: PnpmOverrideData,
    /// Every package name that appears in `dependencies` / `devDependencies` /
    /// `peerDependencies` / `optionalDependencies` of any workspace
    /// `package.json` (root + members).
    declared_packages: FxHashSet<String>,
}

/// Read both override sources and walk workspace `package.json` files to build
/// shared analysis state. Returns `None` when neither source carries any
/// entries; callers should skip both override detectors in that case.
#[must_use]
pub fn gather_pnpm_override_state(
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
) -> Option<PnpmOverrideState> {
    let yaml_path = config.root.join(PNPM_WORKSPACE_FILE);
    let workspace_yaml_data = std::fs::read_to_string(&yaml_path)
        .ok()
        .as_deref()
        .map(parse_pnpm_workspace_overrides)
        .unwrap_or_default();

    let root_pkg_path = config.root.join(ROOT_PACKAGE_JSON);
    let package_json_data = std::fs::read_to_string(&root_pkg_path)
        .ok()
        .as_deref()
        .map(parse_pnpm_package_json_overrides)
        .unwrap_or_default();

    if workspace_yaml_data.entries.is_empty() && package_json_data.entries.is_empty() {
        return None;
    }

    let declared_packages = collect_declared_packages(config, workspaces);

    Some(PnpmOverrideState {
        workspace_yaml_data,
        package_json_data,
        declared_packages,
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

/// Emit one `UnusedDependencyOverride` for every parseable override whose
/// target package (and parent, when present) is not declared in any workspace
/// `package.json`.
#[must_use]
pub fn find_unused_dependency_overrides(
    state: &PnpmOverrideState,
    config: &ResolvedConfig,
) -> Vec<UnusedDependencyOverride> {
    let mut findings = Vec::new();
    let yaml_path = config.root.join(PNPM_WORKSPACE_FILE);
    let json_path = config.root.join(ROOT_PACKAGE_JSON);
    collect_unused_from_source(
        &state.workspace_yaml_data,
        DependencyOverrideSource::PnpmWorkspaceYaml,
        &yaml_path,
        &state.declared_packages,
        &config.compiled_ignore_dependency_overrides,
        &mut findings,
    );
    collect_unused_from_source(
        &state.package_json_data,
        DependencyOverrideSource::PnpmPackageJson,
        &json_path,
        &state.declared_packages,
        &config.compiled_ignore_dependency_overrides,
        &mut findings,
    );
    findings
}

fn collect_unused_from_source(
    data: &PnpmOverrideData,
    source: DependencyOverrideSource,
    source_path: &std::path::Path,
    declared: &FxHashSet<String>,
    ignore_rules: &[CompiledIgnoreDependencyOverrideRule],
    findings: &mut Vec<UnusedDependencyOverride>,
) {
    for entry in &data.entries {
        // Skip misconfigured entries; they are reported by the sibling detector.
        let Some(parsed) = entry.parsed_key.as_ref() else {
            continue;
        };
        let Some(value) = entry.raw_value.as_ref() else {
            continue;
        };
        if !fallow_config::is_valid_override_value(value) {
            continue;
        }

        // Parent-chain semantics: if EITHER parent OR target is declared,
        // consider the override used. This covers the common CVE-fix pattern
        // (parent declared, target transitive).
        let target_declared = declared.contains(&parsed.target_package);
        let parent_declared = parsed
            .parent_package
            .as_ref()
            .is_some_and(|p| declared.contains(p));
        if target_declared || parent_declared {
            continue;
        }

        let source_label = source_label_for(source);
        if ignore_rules
            .iter()
            .any(|rule| rule.matches(&parsed.target_package, source_label))
        {
            continue;
        }

        // Every unused override (bare-target AND parent-chain) is a potential
        // transitive-dependency override, the CVE-fix / canary-aliasing pattern
        // the conservative static algorithm cannot disambiguate without a
        // lockfile. Emit the hint on every finding so agents can de-prioritize
        // and human readers know to verify against `pnpm install`.
        let hint = Some(HINT_MAY_BE_TRANSITIVE.to_string());

        findings.push(UnusedDependencyOverride {
            raw_key: entry.raw_key.clone(),
            target_package: parsed.target_package.clone(),
            parent_package: parsed.parent_package.clone(),
            version_constraint: parsed.target_version_selector.clone(),
            version_range: value.clone(),
            source,
            path: source_path.to_path_buf(),
            line: entry.line,
            hint,
        });
    }
}

/// Emit one `MisconfiguredDependencyOverride` for every entry whose key cannot
/// be parsed or whose value is missing.
#[must_use]
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

        // `target_package` is the parsed package name when the key parses
        // (always for `EmptyValue` findings, never for `UnparsableKey`).
        // Surfacing it lets JSON `add-to-config` actions emit a paste-ready
        // suppression value that matches the actual suppression matcher (which
        // also keys on `target_package`); without it, a raw_key like
        // `"react@<18"` would suggest `{ package: "react@<18" }` that does not
        // suppress the finding (suppressor uses just `"react"`).
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
