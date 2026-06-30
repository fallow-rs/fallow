//! Programmatic list-command runtime helpers.

use std::path::Path;

use fallow_config::{AuthoredRule, LogicalGroup, LogicalGroupStatus, ResolvedBoundaryConfig};
use fallow_output::{ListEntryPointOutput, RootEnvelopeMode, WorkspaceInfo, WorkspacesOutput};
use fallow_types::discover::{DiscoveredFile, EntryPoint};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::{
    AnalysisOptions, BoundariesListLogicalGroup, BoundariesListRule, BoundariesListZone,
    BoundariesListing, ListJsonEnvelope, ListJsonOutputInput, ProgrammaticError,
    resolve_programmatic_analysis_context, serialize_list_json_output,
};

type ProgrammaticResult<T> = Result<T, ProgrammaticError>;

/// Options for MCP/project metadata listing through the programmatic API.
#[derive(Debug, Clone, Default)]
pub struct ProjectInfoOptions {
    pub analysis: AnalysisOptions,
    pub entry_points: bool,
    pub files: bool,
    pub plugins: bool,
    pub boundaries: bool,
}

/// Options for `fallow list --boundaries` through the programmatic API.
#[derive(Debug, Clone, Default)]
pub struct ListBoundariesOptions {
    pub analysis: AnalysisOptions,
}

/// Typed output for project metadata listing before JSON serialization.
#[derive(Debug, Clone)]
pub struct ProjectInfoProgrammaticOutput {
    pub plugins: Option<Vec<String>>,
    pub files: Option<Vec<String>>,
    pub entry_points: Option<Vec<ListEntryPointOutput>>,
    pub boundaries: Option<BoundariesListing>,
    pub workspaces: Option<WorkspacesOutput<fallow_config::WorkspaceDiagnostic>>,
    pub envelope: ListJsonEnvelope,
    pub envelope_mode: RootEnvelopeMode,
}

/// Serialize typed project-info output to the stable JSON contract.
///
/// # Errors
///
/// Returns a structured programmatic error when JSON serialization fails.
pub fn serialize_project_info_programmatic_json(
    output: ProjectInfoProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    serialize_list_json_output(
        ListJsonOutputInput {
            plugins: output.plugins,
            files: output.files,
            entry_points: output.entry_points,
            boundaries: output.boundaries,
            workspaces: output.workspaces,
        },
        output.envelope_mode,
        output.envelope,
    )
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize project info output: {err}"), 2)
            .with_code("FALLOW_PROJECT_INFO_SERIALIZE_FAILED")
            .with_context("project_info")
    })
}

/// Typed output for `fallow list --boundaries` before JSON serialization.
#[derive(Debug, Clone)]
pub struct ListBoundariesProgrammaticOutput {
    pub boundaries: BoundariesListing,
    pub envelope_mode: RootEnvelopeMode,
}

/// Serialize typed boundary-list output to the stable JSON contract.
///
/// # Errors
///
/// Returns a structured programmatic error when JSON serialization fails.
pub fn serialize_list_boundaries_programmatic_json(
    output: ListBoundariesProgrammaticOutput,
) -> ProgrammaticResult<serde_json::Value> {
    serialize_list_json_output(
        ListJsonOutputInput::<BoundariesListing, serde_json::Value> {
            plugins: None,
            files: None,
            entry_points: None,
            boundaries: Some(output.boundaries),
            workspaces: None,
        },
        output.envelope_mode,
        ListJsonEnvelope::Boundaries,
    )
    .map_err(|err| {
        ProgrammaticError::new(
            format!("failed to serialize list boundaries output: {err}"),
            2,
        )
        .with_code("FALLOW_LIST_BOUNDARIES_SERIALIZE_FAILED")
        .with_context("list_boundaries")
    })
}

/// Owned boundary listing data shared by CLI and programmatic renderers.
#[derive(Debug, Clone)]
pub struct BoundaryData {
    pub zones: Vec<ZoneInfo>,
    pub rules: Vec<RuleInfo>,
    pub logical_groups: Vec<LogicalGroupInfo>,
    pub is_empty: bool,
}

#[derive(Debug, Clone)]
pub struct ZoneInfo {
    pub name: String,
    pub patterns: Vec<String>,
    pub file_count: usize,
}

#[derive(Debug, Clone)]
pub struct RuleInfo {
    pub from: String,
    pub allow: Vec<String>,
}

/// View-model mirror of [`LogicalGroup`] with derived file-count totals.
#[derive(Debug, Clone)]
pub struct LogicalGroupInfo {
    pub name: String,
    pub children: Vec<String>,
    pub auto_discover: Vec<String>,
    pub authored_rule: Option<AuthoredRule>,
    pub fallback_zone: Option<String>,
    pub source_zone_index: usize,
    pub status: LogicalGroupStatus,
    pub file_count: usize,
    pub child_file_count: usize,
    pub fallback_file_count: usize,
    pub merged_from: Option<Vec<usize>>,
    pub original_zone_root: Option<String>,
    pub child_source_indices: Vec<usize>,
}

/// Run `list_boundaries` through the API-owned runtime path.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options or config-load
/// failures.
pub fn run_list_boundaries(
    options: &ListBoundariesOptions,
) -> ProgrammaticResult<ListBoundariesProgrammaticOutput> {
    let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
    resolved.install(|| {
        let project_config = load_list_project_config(&resolved)?;

        let files = fallow_engine::discover_files_with_plugin_scopes(&project_config.config);
        let data = compute_boundary_data(&project_config.config, Some(&files));

        Ok(ListBoundariesProgrammaticOutput {
            boundaries: boundary_data_to_output(&data),
            envelope_mode: RootEnvelopeMode::from_legacy(options.analysis.legacy_envelope),
        })
    })
}

/// Run project metadata listing through the API-owned runtime path.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config-load
/// failures, or plugin regex errors.
pub fn run_project_info(
    options: &ProjectInfoOptions,
) -> ProgrammaticResult<ProjectInfoProgrammaticOutput> {
    let resolved = resolve_programmatic_analysis_context(&options.analysis)?;
    resolved.install(|| {
        let project_config = load_list_project_config(&resolved)?;
        let config = &project_config.config;
        let show_all = project_info_should_show_all(options);
        let need_plugin_result = options.plugins || options.entry_points || show_all;
        let need_files = options.files || options.entry_points || options.boundaries || show_all;
        let discovered = if need_files || need_plugin_result {
            Some(fallow_engine::discover_files_with_plugin_scopes(config))
        } else {
            None
        };

        let plugin_result = collect_plugin_result(
            resolved.root(),
            config,
            options,
            show_all,
            discovered.as_deref(),
        )?;
        let entry_points = collect_entry_points(
            resolved.root(),
            config,
            options,
            show_all,
            discovered.as_deref(),
            plugin_result.as_ref(),
        );
        let boundaries = options.boundaries.then(|| {
            boundary_data_to_output(&compute_boundary_data(config, discovered.as_deref()))
        });
        let workspaces = if show_all {
            Some(collect_workspace_output(resolved.root(), config)?)
        } else {
            None
        };
        let envelope = if boundaries.is_some() {
            ListJsonEnvelope::Boundaries
        } else {
            ListJsonEnvelope::Plain
        };

        Ok(ProjectInfoProgrammaticOutput {
            plugins: collect_plugins(options, show_all, plugin_result.as_ref()),
            files: collect_files(options, show_all, discovered.as_deref(), resolved.root()),
            entry_points: entry_points
                .map(|entries| entry_points_to_output(&entries, resolved.root())),
            boundaries,
            workspaces,
            envelope,
            envelope_mode: RootEnvelopeMode::from_legacy(options.analysis.legacy_envelope),
        })
    })
}

fn load_list_project_config(
    resolved: &crate::ProgrammaticAnalysisContext,
) -> ProgrammaticResult<fallow_engine::ProjectConfig> {
    fallow_engine::config_for_project_analysis(
        resolved.root(),
        resolved.config_path().as_deref(),
        fallow_engine::ProjectConfigOptions {
            output: fallow_types::output_format::OutputFormat::Json,
            no_cache: resolved.no_cache(),
            threads: resolved.threads(),
            production_override: resolved.production_override(),
            quiet: true,
            analysis: fallow_config::ProductionAnalysis::DeadCode,
        },
    )
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to load config: {err}"), 2)
            .with_code("FALLOW_CONFIG_LOAD_FAILED")
            .with_context("analysis.configPath")
    })
}

const fn project_info_should_show_all(options: &ProjectInfoOptions) -> bool {
    !options.entry_points && !options.files && !options.plugins && !options.boundaries
}

fn collect_plugins(
    options: &ProjectInfoOptions,
    show_all: bool,
    plugin_result: Option<&fallow_engine::AggregatedPluginResult>,
) -> Option<Vec<String>> {
    if options.plugins || show_all {
        plugin_result.map(|plugin_result| plugin_result.active_plugins().to_vec())
    } else {
        None
    }
}

fn collect_files(
    options: &ProjectInfoOptions,
    show_all: bool,
    discovered: Option<&[DiscoveredFile]>,
    root: &Path,
) -> Option<Vec<String>> {
    if options.files || show_all {
        discovered.map(|files| {
            files
                .iter()
                .map(|file| format_display_path(&file.path, root))
                .collect()
        })
    } else {
        None
    }
}

fn collect_plugin_result(
    root: &Path,
    config: &fallow_config::ResolvedConfig,
    options: &ProjectInfoOptions,
    show_all: bool,
    discovered: Option<&[DiscoveredFile]>,
) -> ProgrammaticResult<Option<fallow_engine::AggregatedPluginResult>> {
    if !(options.plugins || options.entry_points || show_all) {
        return Ok(None);
    }
    let fallback_discovered;
    let files = match discovered {
        Some(discovered) => discovered,
        None => {
            fallback_discovered = fallow_engine::discover_files_with_plugin_scopes(config);
            &fallback_discovered
        }
    };
    let file_paths: Vec<std::path::PathBuf> = files.iter().map(|file| file.path.clone()).collect();
    let registry = fallow_engine::PluginRegistry::new(config.external_plugins.clone());
    let mut result = run_package_plugins(&registry, &root.join("package.json"), root, &file_paths)?
        .unwrap_or_default();
    merge_workspace_plugins(root, &registry, &file_paths, &mut result)?;
    Ok(Some(result))
}

fn run_package_plugins(
    registry: &fallow_engine::PluginRegistry,
    package_path: &Path,
    root: &Path,
    file_paths: &[std::path::PathBuf],
) -> ProgrammaticResult<Option<fallow_engine::AggregatedPluginResult>> {
    let Ok(package) = fallow_config::PackageJson::load(package_path) else {
        return Ok(None);
    };
    registry
        .try_run(&package, root, file_paths)
        .map(Some)
        .map_err(|errors| {
            ProgrammaticError::new(fallow_engine::format_plugin_regex_errors(&errors), 2)
                .with_code("FALLOW_PLUGIN_REGEX_FAILED")
                .with_context("project_info.plugins")
        })
}

fn merge_workspace_plugins(
    root: &Path,
    registry: &fallow_engine::PluginRegistry,
    file_paths: &[std::path::PathBuf],
    result: &mut fallow_engine::AggregatedPluginResult,
) -> ProgrammaticResult<()> {
    for workspace in &fallow_config::discover_workspaces(root) {
        let Some(workspace_result) = run_package_plugins(
            registry,
            &workspace.root.join("package.json"),
            &workspace.root,
            file_paths,
        )?
        else {
            continue;
        };
        result.merge_active_plugins_from(&workspace_result);
    }
    Ok(())
}

fn collect_entry_points(
    root: &Path,
    config: &fallow_config::ResolvedConfig,
    options: &ProjectInfoOptions,
    show_all: bool,
    discovered: Option<&[DiscoveredFile]>,
    plugin_result: Option<&fallow_engine::AggregatedPluginResult>,
) -> Option<Vec<EntryPoint>> {
    if !(options.entry_points || show_all) {
        return None;
    }
    let discovered = discovered?;
    let mut entries = fallow_engine::discover_entry_points(config, discovered);
    for workspace in &fallow_config::discover_workspaces(root) {
        entries.extend(fallow_engine::discover_workspace_entry_points(
            &workspace.root,
            config,
            discovered,
        ));
    }
    if let Some(plugin_result) = plugin_result {
        entries.extend(fallow_engine::discover_plugin_entry_points(
            plugin_result,
            config,
            discovered,
        ));
    }
    Some(entries)
}

fn entry_points_to_output(entries: &[EntryPoint], root: &Path) -> Vec<ListEntryPointOutput> {
    entries
        .iter()
        .map(|entry| ListEntryPointOutput {
            path: format_display_path(&entry.path, root),
            source: entry.source.to_string(),
        })
        .collect()
}

fn collect_workspace_output(
    root: &Path,
    config: &fallow_config::ResolvedConfig,
) -> ProgrammaticResult<WorkspacesOutput<fallow_config::WorkspaceDiagnostic>> {
    let (workspaces, mut diagnostics) =
        fallow_config::discover_workspaces_with_diagnostics(root, &config.ignore_patterns)
            .map_err(|err| {
                ProgrammaticError::new(err.to_string(), 2)
                    .with_code("FALLOW_WORKSPACE_DISCOVERY_FAILED")
                    .with_context("project_info.workspaces")
            })?;
    append_undeclared_workspace_diagnostics(root, config, &workspaces, &mut diagnostics);
    let workspaces = workspaces
        .iter()
        .map(|workspace| {
            let relative = workspace.root.strip_prefix(root).unwrap_or(&workspace.root);
            WorkspaceInfo {
                name: workspace.name.clone(),
                path: relative.display().to_string().replace('\\', "/"),
                is_internal_dependency: workspace.is_internal_dependency,
            }
        })
        .collect::<Vec<_>>();
    Ok(WorkspacesOutput {
        workspace_count: workspaces.len(),
        workspaces,
        workspace_diagnostics: diagnostics,
    })
}

fn append_undeclared_workspace_diagnostics(
    root: &Path,
    config: &fallow_config::ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    diagnostics: &mut Vec<fallow_config::WorkspaceDiagnostic>,
) {
    let undeclared = fallow_config::find_undeclared_workspaces_with_ignores(
        root,
        workspaces,
        &config.ignore_patterns,
    );
    let already_flagged: FxHashSet<std::path::PathBuf> = diagnostics
        .iter()
        .map(|diagnostic| {
            std::fs::canonicalize(&diagnostic.path).unwrap_or_else(|_| diagnostic.path.clone())
        })
        .collect();
    for diagnostic in undeclared {
        let canonical =
            std::fs::canonicalize(&diagnostic.path).unwrap_or_else(|_| diagnostic.path.clone());
        if !already_flagged.contains(&canonical) {
            diagnostics.push(diagnostic);
        }
    }
}

fn format_display_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
        .replace('\\', "/")
}

/// Compute boundary listing data from resolved config and optional discovery.
#[must_use]
pub fn compute_boundary_data(
    config: &fallow_config::ResolvedConfig,
    discovered: Option<&[DiscoveredFile]>,
) -> BoundaryData {
    let boundaries = &config.boundaries;

    if boundaries.is_empty() {
        return BoundaryData {
            zones: vec![],
            rules: vec![],
            logical_groups: vec![],
            is_empty: true,
        };
    }

    let zones = build_boundary_zones(config, discovered);
    let rules = build_boundary_rules(boundaries);
    let logical_groups = build_logical_groups(boundaries, &zones);

    BoundaryData {
        zones,
        rules,
        logical_groups,
        is_empty: false,
    }
}

fn build_boundary_zones(
    config: &fallow_config::ResolvedConfig,
    discovered: Option<&[DiscoveredFile]>,
) -> Vec<ZoneInfo> {
    config
        .boundaries
        .zones
        .iter()
        .map(|zone| ZoneInfo {
            name: zone.name.clone(),
            patterns: zone.matchers.iter().map(|m| m.glob().to_string()).collect(),
            file_count: count_boundary_zone_files(config, discovered, &zone.name),
        })
        .collect()
}

fn count_boundary_zone_files(
    config: &fallow_config::ResolvedConfig,
    discovered: Option<&[DiscoveredFile]>,
    zone_name: &str,
) -> usize {
    discovered.map_or(0, |files| {
        files
            .iter()
            .filter(|file| {
                let rel = file
                    .path
                    .strip_prefix(&config.root)
                    .ok()
                    .map(|path| path.to_string_lossy().replace('\\', "/"));
                rel.is_some_and(|path| config.boundaries.classify_zone(&path) == Some(zone_name))
            })
            .count()
    })
}

fn build_boundary_rules(boundaries: &ResolvedBoundaryConfig) -> Vec<RuleInfo> {
    boundaries
        .rules
        .iter()
        .map(|rule| RuleInfo {
            from: rule.from_zone.clone(),
            allow: rule.allowed_zones.clone(),
        })
        .collect()
}

fn build_logical_groups(
    boundaries: &ResolvedBoundaryConfig,
    zones: &[ZoneInfo],
) -> Vec<LogicalGroupInfo> {
    let zone_count_by_name: FxHashMap<&str, usize> = zones
        .iter()
        .map(|zone| (zone.name.as_str(), zone.file_count))
        .collect();

    boundaries
        .logical_groups
        .iter()
        .map(|group| logical_group_info(group, &zone_count_by_name))
        .collect()
}

fn logical_group_info(
    group: &LogicalGroup,
    zone_count_by_name: &FxHashMap<&str, usize>,
) -> LogicalGroupInfo {
    let child_file_count: usize = group
        .children
        .iter()
        .filter_map(|child| zone_count_by_name.get(child.as_str()).copied())
        .sum();
    let fallback_file_count = group
        .fallback_zone
        .as_deref()
        .and_then(|fallback| zone_count_by_name.get(fallback).copied())
        .unwrap_or(0);

    LogicalGroupInfo {
        name: group.name.clone(),
        children: group.children.clone(),
        auto_discover: group.auto_discover.clone(),
        authored_rule: group.authored_rule.clone(),
        fallback_zone: group.fallback_zone.clone(),
        source_zone_index: group.source_zone_index,
        status: group.status,
        file_count: child_file_count + fallback_file_count,
        child_file_count,
        fallback_file_count,
        merged_from: group.merged_from.clone(),
        original_zone_root: group.original_zone_root.clone(),
        child_source_indices: group.child_source_indices.clone(),
    }
}

/// Convert boundary listing data to the stable output contract.
#[must_use]
pub fn boundary_data_to_output(data: &BoundaryData) -> BoundariesListing {
    if data.is_empty {
        return BoundariesListing {
            configured: false,
            zone_count: 0,
            zones: Vec::new(),
            rule_count: 0,
            rules: Vec::new(),
            logical_group_count: 0,
            logical_groups: Vec::new(),
        };
    }

    BoundariesListing {
        configured: true,
        zone_count: data.zones.len(),
        zones: data
            .zones
            .iter()
            .map(|zone| BoundariesListZone {
                name: zone.name.clone(),
                patterns: zone.patterns.clone(),
                file_count: zone.file_count,
            })
            .collect(),
        rule_count: data.rules.len(),
        rules: data
            .rules
            .iter()
            .map(|rule| BoundariesListRule {
                from: rule.from.clone(),
                allow: rule.allow.clone(),
            })
            .collect(),
        logical_group_count: data.logical_groups.len(),
        logical_groups: data
            .logical_groups
            .iter()
            .map(logical_group_info_to_output)
            .collect(),
    }
}

fn logical_group_info_to_output(group: &LogicalGroupInfo) -> BoundariesListLogicalGroup {
    BoundariesListLogicalGroup {
        name: group.name.clone(),
        children: group.children.clone(),
        auto_discover: group.auto_discover.clone(),
        status: group.status,
        source_zone_index: group.source_zone_index,
        file_count: group.file_count,
        authored_rule: group.authored_rule.clone(),
        fallback_zone: group.fallback_zone.clone(),
        merged_from: group.merged_from.clone(),
        original_zone_root: group.original_zone_root.clone(),
        child_source_indices: group.child_source_indices.clone(),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn empty_boundary_data() -> BoundaryData {
        BoundaryData {
            zones: vec![],
            rules: vec![],
            logical_groups: vec![],
            is_empty: true,
        }
    }

    fn boundary_data_to_json(data: &BoundaryData) -> serde_json::Value {
        serde_json::to_value(boundary_data_to_output(data))
            .expect("boundary list output should serialize")
    }

    #[test]
    fn project_info_default_sections_match_plain_list_contract() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"project-info-api","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("src/index.ts"),
            "export const value = 1;\n",
        )
        .expect("write source");

        let output = serialize_project_info_programmatic_json(
            run_project_info(&ProjectInfoOptions {
                analysis: AnalysisOptions {
                    root: Some(project.path().to_path_buf()),
                    no_cache: true,
                    ..AnalysisOptions::default()
                },
                ..ProjectInfoOptions::default()
            })
            .expect("project info should run"),
        )
        .expect("project info should serialize");

        assert_eq!(output["file_count"], 1);
        assert_eq!(output["files"][0], "src/index.ts");
        assert_eq!(output["entry_point_count"], 1);
        assert_eq!(output["workspace_count"], 0);
        assert!(output.get("kind").is_none());
    }

    #[test]
    fn boundary_json_empty_includes_logical_groups_key() {
        let value = boundary_data_to_json(&empty_boundary_data());

        assert_eq!(value["configured"], false);
        assert_eq!(value["zone_count"], 0);
        assert_eq!(value["rule_count"], 0);
        assert_eq!(value["logical_group_count"], 0);
        assert_eq!(value["logical_groups"], json!([]));
    }

    #[test]
    fn boundary_json_logical_group_carries_all_fields() {
        let data = BoundaryData {
            zones: vec![ZoneInfo {
                name: "features/auth".to_string(),
                patterns: vec!["src/features/auth/**".to_string()],
                file_count: 3,
            }],
            rules: vec![],
            logical_groups: vec![LogicalGroupInfo {
                name: "features".to_string(),
                children: vec!["features/auth".to_string()],
                auto_discover: vec!["./src/features/".to_string()],
                authored_rule: Some(AuthoredRule {
                    allow: vec!["shared".to_string()],
                    allow_type_only: vec!["types".to_string()],
                }),
                fallback_zone: None,
                source_zone_index: 1,
                status: LogicalGroupStatus::Ok,
                file_count: 3,
                child_file_count: 3,
                fallback_file_count: 0,
                merged_from: None,
                original_zone_root: None,
                child_source_indices: vec![],
            }],
            is_empty: false,
        };

        let value = boundary_data_to_json(&data);
        let group = &value["logical_groups"][0];

        assert_eq!(value["logical_group_count"], 1);
        assert_eq!(group["name"], "features");
        assert_eq!(group["children"][0], "features/auth");
        assert_eq!(group["auto_discover"][0], "./src/features/");
        assert_eq!(group["status"], "ok");
        assert_eq!(group["source_zone_index"], 1);
        assert_eq!(group["file_count"], 3);
        assert_eq!(group["authored_rule"]["allow"][0], "shared");
        assert_eq!(group["authored_rule"]["allow_type_only"][0], "types");
        assert!(group.get("fallback_zone").is_none());
        assert!(group.get("merged_from").is_none());
        assert!(group.get("original_zone_root").is_none());
        assert!(group.get("child_source_indices").is_none());
    }

    #[test]
    fn boundary_json_logical_group_optional_fields_round_trip() {
        let data = BoundaryData {
            zones: vec![],
            rules: vec![],
            logical_groups: vec![LogicalGroupInfo {
                name: "features".to_string(),
                children: vec!["features/auth".to_string(), "features/billing".to_string()],
                auto_discover: vec!["src/features".to_string(), "src/modules".to_string()],
                authored_rule: None,
                fallback_zone: Some("features".to_string()),
                source_zone_index: 0,
                status: LogicalGroupStatus::Empty,
                file_count: 2,
                child_file_count: 0,
                fallback_file_count: 2,
                merged_from: Some(vec![0, 3]),
                original_zone_root: Some("packages/app/".to_string()),
                child_source_indices: vec![0, 1],
            }],
            is_empty: false,
        };

        let group = &boundary_data_to_json(&data)["logical_groups"][0];

        assert_eq!(group["status"], "empty");
        assert_eq!(group["fallback_zone"], "features");
        assert_eq!(group["merged_from"][1], 3);
        assert_eq!(group["original_zone_root"], "packages/app/");
        assert_eq!(group["child_source_indices"][1], 1);
    }
}
