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
//!    absent from both workspace `package.json` dep sections and
//!    `pnpm-lock.yaml`. Overrides targeting resolved transitive packages are
//!    treated as used because CVE-fix pins often exist only in the lockfile.
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
//! BOTH `react` AND `react-dom` are absent from every workspace `package.json`
//! and `pnpm-lock.yaml`. This matches the common CVE-fix pattern where the
//! parent is declared and the override forces a transitive version inside that
//! parent's subtree.

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
const PNPM_LOCK_FILE: &str = "pnpm-lock.yaml";
const ROOT_PACKAGE_JSON: &str = "package.json";
const SOURCE_LABEL_YAML: &str = "pnpm-workspace.yaml";
const SOURCE_LABEL_JSON: &str = "package.json";
const HINT_MAY_BE_TRANSITIVE: &str =
    "may target a transitive dependency; pnpm install --frozen-lockfile is the ground truth";
const LOCKFILE_DEPENDENCY_SECTIONS: &[&str] = &[
    "dependencies",
    "optionalDependencies",
    "devDependencies",
    "peerDependencies",
];

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
    /// Every package name found in `pnpm-lock.yaml` package/snapshot keys or
    /// dependency sections. Includes transitive dependencies resolved by pnpm.
    lockfile_packages: FxHashSet<String>,
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
    let lockfile_packages = collect_lockfile_packages(config);

    Some(PnpmOverrideState {
        workspace_yaml_data,
        package_json_data,
        declared_packages,
        lockfile_packages,
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

/// Parse `pnpm-lock.yaml` and collect package names from resolved package keys
/// plus dependency maps. Malformed or missing lockfiles degrade to an empty
/// set, preserving the package.json-only fallback for projects without pnpm.
fn collect_lockfile_packages(config: &ResolvedConfig) -> FxHashSet<String> {
    let lock_path = config.root.join(PNPM_LOCK_FILE);
    let Ok(raw_source) = std::fs::read_to_string(lock_path) else {
        return FxHashSet::default();
    };

    collect_pnpm_lock_packages(&raw_source)
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

/// Emit one `UnusedDependencyOverride` for every parseable override whose
/// target package (and parent, when present) is not declared in any workspace
/// `package.json` or resolved in `pnpm-lock.yaml`.
#[must_use]
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for typed output; serialize with fallow_api::serialize_dead_code_programmatic_json for JSON output. See docs/fallow-core-migration.md and ADR-008."
)]
pub fn find_unused_dependency_overrides(
    state: &PnpmOverrideState,
    config: &ResolvedConfig,
) -> Vec<UnusedDependencyOverride> {
    let mut findings = Vec::new();
    let yaml_path = config.root.join(PNPM_WORKSPACE_FILE);
    let json_path = config.root.join(ROOT_PACKAGE_JSON);
    collect_unused_from_source(&mut UnusedOverrideSourceInput {
        data: &state.workspace_yaml_data,
        source: DependencyOverrideSource::PnpmWorkspaceYaml,
        source_path: &yaml_path,
        declared: &state.declared_packages,
        resolved: &state.lockfile_packages,
        ignore_rules: &config.compiled_ignore_dependency_overrides,
        findings: &mut findings,
    });
    collect_unused_from_source(&mut UnusedOverrideSourceInput {
        data: &state.package_json_data,
        source: DependencyOverrideSource::PnpmPackageJson,
        source_path: &json_path,
        declared: &state.declared_packages,
        resolved: &state.lockfile_packages,
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

        let hint = Some(HINT_MAY_BE_TRANSITIVE.to_string());

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
    note = "fallow_core is internal; use fallow_api::run_dead_code for typed output; serialize with fallow_api::serialize_dead_code_programmatic_json for JSON output. See docs/fallow-core-migration.md and ADR-008."
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
}
