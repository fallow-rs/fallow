use std::process::ExitCode;

use fallow_config::OutputFormat;

use crate::report::format_display_path;
use crate::runtime_support::{LoadConfigArgs, load_config};
use fallow_api::{BoundaryData, ListJsonEnvelope, ListJsonOutputInput};
use fallow_output::WorkspaceInfo;

pub struct ListOptions<'a> {
    pub root: &'a std::path::Path,
    pub config_path: &'a Option<std::path::PathBuf>,
    pub output: OutputFormat,
    pub threads: usize,
    pub no_cache: bool,
    pub entry_points: bool,
    pub files: bool,
    pub plugins: bool,
    pub boundaries: bool,
    pub workspaces: bool,
    pub production: bool,
}

/// Owned listing data assembled by [`collect_list_data`] and borrowed by the
/// JSON / human renderers.
struct ListData {
    show_all: bool,
    plugin_result: Option<fallow_engine::AggregatedPluginResult>,
    discovered: Option<Vec<fallow_engine::DiscoveredFile>>,
    entry_points: Option<Vec<fallow_engine::EntryPoint>>,
    boundary_data: Option<BoundaryData>,
    workspace_data: Option<WorkspaceData>,
}

pub fn run_list(opts: &ListOptions<'_>) -> ExitCode {
    let config = match load_config(
        opts.root,
        opts.config_path,
        LoadConfigArgs {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production: opts.production,
            quiet: true, // list command doesn't need progress bars
        },
    ) {
        Ok(c) => c,
        Err(code) => return code,
    };

    let data = match collect_list_data(opts, &config) {
        Ok(data) => data,
        Err(code) => return code,
    };

    match opts.output {
        OutputFormat::Json => print_list_json(&ListJsonInput {
            opts,
            show_all: data.show_all,
            plugin_result: data.plugin_result.as_ref(),
            discovered: data.discovered.as_deref(),
            entry_points: data.entry_points.as_deref(),
            boundary_data: data.boundary_data.as_ref(),
            workspace_data: data.workspace_data.as_ref(),
        }),
        _ => {
            print_list_human(&ListHumanInput {
                opts,
                show_all: data.show_all,
                plugin_result: data.plugin_result.as_ref(),
                discovered: data.discovered.as_deref(),
                entry_points: data.entry_points.as_deref(),
                boundary_data: data.boundary_data.as_ref(),
                workspace_data: data.workspace_data.as_ref(),
            });
            ExitCode::SUCCESS
        }
    }
}

/// Collect plugins, files, entry points, boundary, and workspace data for a
/// `fallow list` run, honoring which listing modes are active.
fn collect_list_data(
    opts: &ListOptions<'_>,
    config: &fallow_config::ResolvedConfig,
) -> Result<ListData, ExitCode> {
    let show_all = should_show_all(opts);

    let need_plugin_result = opts.plugins || opts.entry_points || show_all;
    let need_files = needs_file_discovery(opts.files, show_all, opts.entry_points, opts.boundaries);
    let discovered = if need_files || need_plugin_result {
        Some(fallow_engine::discover_files_with_plugin_scopes(config))
    } else {
        None
    };

    let plugin_result = collect_plugin_result(opts, config, show_all, discovered.as_deref())?;

    let entry_points = collect_list_entry_points(
        opts,
        config,
        show_all,
        discovered.as_deref(),
        plugin_result.as_ref(),
    );

    let boundary_data = if opts.boundaries {
        Some(fallow_api::compute_boundary_data(
            config,
            discovered.as_deref(),
        ))
    } else {
        None
    };

    let workspace_data = collect_list_workspace_data(opts, config, show_all)?;

    Ok(ListData {
        show_all,
        plugin_result,
        discovered,
        entry_points,
        boundary_data,
        workspace_data,
    })
}

fn collect_list_entry_points(
    opts: &ListOptions<'_>,
    config: &fallow_config::ResolvedConfig,
    show_all: bool,
    discovered: Option<&[fallow_engine::DiscoveredFile]>,
    plugin_result: Option<&fallow_engine::AggregatedPluginResult>,
) -> Option<Vec<fallow_engine::EntryPoint>> {
    if !(opts.entry_points || show_all) {
        return None;
    }
    let disc = discovered?;
    let mut entries = fallow_engine::discover_entry_points(config, disc);
    let workspaces = fallow_config::discover_workspaces(opts.root);
    for ws in &workspaces {
        let ws_entries = fallow_engine::discover_workspace_entry_points(&ws.root, config, disc);
        entries.extend(ws_entries);
    }
    if let Some(pr) = plugin_result {
        let plugin_entries = fallow_engine::discover_plugin_entry_points(pr, config, disc);
        entries.extend(plugin_entries);
    }
    Some(entries)
}

fn collect_list_workspace_data(
    opts: &ListOptions<'_>,
    config: &fallow_config::ResolvedConfig,
    show_all: bool,
) -> Result<Option<WorkspaceData>, ExitCode> {
    if !(opts.workspaces || show_all) {
        return Ok(None);
    }
    match fallow_config::discover_workspaces_with_diagnostics(opts.root, &config.ignore_patterns) {
        Ok((workspaces, mut diagnostics)) => {
            append_undeclared_workspace_diagnostics(
                opts.root,
                config,
                &workspaces,
                &mut diagnostics,
            );
            Ok(Some(WorkspaceData {
                workspaces,
                diagnostics,
            }))
        }
        Err(err) => Err(crate::error::emit_error(&err.to_string(), 2, opts.output)),
    }
}

fn append_undeclared_workspace_diagnostics(
    root: &std::path::Path,
    config: &fallow_config::ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    diagnostics: &mut Vec<fallow_config::WorkspaceDiagnostic>,
) {
    let undeclared = fallow_config::find_undeclared_workspaces_with_ignores(
        root,
        workspaces,
        &config.ignore_patterns,
    );
    let already_flagged: rustc_hash::FxHashSet<std::path::PathBuf> = diagnostics
        .iter()
        .map(|d| dunce::canonicalize(&d.path).unwrap_or_else(|_| d.path.clone()))
        .collect();
    for diag in undeclared {
        let canonical = dunce::canonicalize(&diag.path).unwrap_or_else(|_| diag.path.clone());
        if !already_flagged.contains(&canonical) {
            diagnostics.push(diag);
        }
    }
}

/// Determine whether all listing modes should be shown.
///
/// When none of the specific flags is set, the command defaults to
/// showing everything.
const fn should_show_all(opts: &ListOptions<'_>) -> bool {
    !opts.entry_points && !opts.files && !opts.plugins && !opts.boundaries && !opts.workspaces
}

/// Determine whether file discovery is needed.
///
/// Files must be discovered when showing files, when showing all,
/// when computing entry points, or when computing boundary file counts.
const fn needs_file_discovery(
    files: bool,
    show_all: bool,
    entry_points: bool,
    boundaries: bool,
) -> bool {
    files || show_all || entry_points || boundaries
}

fn collect_plugin_result(
    opts: &ListOptions<'_>,
    config: &fallow_config::ResolvedConfig,
    show_all: bool,
    discovered: Option<&[fallow_engine::DiscoveredFile]>,
) -> Result<Option<fallow_engine::AggregatedPluginResult>, ExitCode> {
    if !(opts.plugins || opts.entry_points || show_all) {
        return Ok(None);
    }
    let fallback_discovered;
    let disc = match discovered {
        Some(discovered) => discovered,
        None => {
            fallback_discovered = fallow_engine::discover_files_with_plugin_scopes(config);
            &fallback_discovered
        }
    };
    let file_paths: Vec<std::path::PathBuf> = disc.iter().map(|f| f.path.clone()).collect();
    let registry = fallow_engine::PluginRegistry::new(config.external_plugins.clone());
    let mut result = run_package_plugins(
        &registry,
        &opts.root.join("package.json"),
        opts.root,
        &file_paths,
        opts.output,
    )?
    .unwrap_or_default();
    merge_workspace_plugins(opts, &registry, &file_paths, &mut result)?;
    Ok(Some(result))
}

fn run_package_plugins(
    registry: &fallow_engine::PluginRegistry,
    package_path: &std::path::Path,
    root: &std::path::Path,
    file_paths: &[std::path::PathBuf],
    output: OutputFormat,
) -> Result<Option<fallow_engine::AggregatedPluginResult>, ExitCode> {
    let Ok(pkg) = fallow_config::PackageJson::load(package_path) else {
        return Ok(None);
    };
    registry
        .try_run(&pkg, root, file_paths)
        .map(Some)
        .map_err(|errors| {
            let message = fallow_engine::format_plugin_regex_errors(&errors);
            crate::error::emit_error(&message, 2, output)
        })
}

fn merge_workspace_plugins(
    opts: &ListOptions<'_>,
    registry: &fallow_engine::PluginRegistry,
    file_paths: &[std::path::PathBuf],
    result: &mut fallow_engine::AggregatedPluginResult,
) -> Result<(), ExitCode> {
    for ws in &fallow_config::discover_workspaces(opts.root) {
        let Some(ws_result) = run_package_plugins(
            registry,
            &ws.root.join("package.json"),
            &ws.root,
            file_paths,
            opts.output,
        )?
        else {
            continue;
        };
        result.merge_active_plugins_from(&ws_result);
    }
    Ok(())
}

/// Print list results as JSON and return the appropriate exit code.
struct ListJsonInput<'a> {
    opts: &'a ListOptions<'a>,
    show_all: bool,
    plugin_result: Option<&'a fallow_engine::AggregatedPluginResult>,
    discovered: Option<&'a [fallow_engine::DiscoveredFile]>,
    entry_points: Option<&'a [fallow_engine::EntryPoint]>,
    boundary_data: Option<&'a BoundaryData>,
    workspace_data: Option<&'a WorkspaceData>,
}

fn print_list_json(input: &ListJsonInput<'_>) -> ExitCode {
    let has_boundaries = input.boundary_data.is_some();
    let workspace_only = input.opts.workspaces
        && !input.opts.plugins
        && !input.opts.files
        && !input.opts.entry_points
        && !input.opts.boundaries;
    let envelope = if has_boundaries {
        ListJsonEnvelope::Boundaries
    } else if workspace_only {
        ListJsonEnvelope::Workspaces
    } else {
        ListJsonEnvelope::Plain
    };

    let output = match fallow_api::serialize_list_json_output(
        build_list_json_output_input(input),
        crate::output_runtime::current_root_envelope_mode(),
        envelope,
    ) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("Error: failed to serialize list output: {err}");
            return ExitCode::from(2);
        }
    };

    match serde_json::to_string_pretty(&output) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("Error: failed to serialize list output: {e}");
            ExitCode::from(2)
        }
    }
}

/// Assemble the typed JSON body for a `fallow list` run, one section per
/// active listing mode.
fn build_list_json_output_input(
    input: &ListJsonInput<'_>,
) -> ListJsonOutputInput<fallow_api::BoundariesListing, fallow_config::WorkspaceDiagnostic> {
    let opts = input.opts;
    let show_all = input.show_all;

    let plugins = if opts.plugins || show_all {
        input
            .plugin_result
            .map(|plugin_result| plugin_result.active_plugins().to_vec())
    } else {
        None
    };

    let files = if opts.files || show_all {
        input.discovered.map(|discovered| {
            discovered
                .iter()
                .map(|file| format_display_path(&file.path, opts.root))
                .collect()
        })
    } else {
        None
    };

    let entry_points = input.entry_points.map(|entries| {
        entries
            .iter()
            .map(|entry| fallow_api::ListEntryPointOutput {
                path: format_display_path(&entry.path, opts.root),
                source: entry.source.to_string(),
            })
            .collect()
    });

    ListJsonOutputInput {
        plugins,
        files,
        entry_points,
        boundaries: input.boundary_data.map(fallow_api::boundary_data_to_output),
        workspaces: input
            .workspace_data
            .map(|workspaces| workspace_data_to_output(opts.root, workspaces)),
    }
}

fn workspace_data_to_output(
    root: &std::path::Path,
    ws: &WorkspaceData,
) -> fallow_api::WorkspacesOutput {
    let workspaces = ws
        .workspaces
        .iter()
        .map(|w| {
            let relative = w.root.strip_prefix(root).unwrap_or(&w.root);
            WorkspaceInfo {
                name: w.name.clone(),
                path: relative.display().to_string().replace('\\', "/"),
                is_internal_dependency: w.is_internal_dependency,
            }
        })
        .collect::<Vec<_>>();
    fallow_api::WorkspacesOutput {
        workspace_count: workspaces.len(),
        workspaces,
        workspace_diagnostics: ws.diagnostics.clone(),
    }
}

/// Print list results in human-readable format.
struct ListHumanInput<'a> {
    opts: &'a ListOptions<'a>,
    show_all: bool,
    plugin_result: Option<&'a fallow_engine::AggregatedPluginResult>,
    discovered: Option<&'a [fallow_engine::DiscoveredFile]>,
    entry_points: Option<&'a [fallow_engine::EntryPoint]>,
    boundary_data: Option<&'a BoundaryData>,
    workspace_data: Option<&'a WorkspaceData>,
}

fn print_list_human(input: &ListHumanInput<'_>) {
    let opts = input.opts;
    let show_all = input.show_all;
    let plugin_result = input.plugin_result;
    let discovered = input.discovered;
    let entry_points = input.entry_points;
    let boundary_data = input.boundary_data;
    let workspace_data = input.workspace_data;
    if (opts.plugins || show_all)
        && let Some(pr) = plugin_result
    {
        eprintln!("Active plugins:");
        for name in pr.active_plugins() {
            eprintln!("  - {name}");
        }
    }

    if (opts.files || show_all)
        && let Some(disc) = discovered
    {
        eprintln!("Discovered {} files", disc.len());
        for file in disc {
            println!("{}", format_display_path(&file.path, opts.root));
        }
    }

    if let Some(entries) = entry_points {
        eprintln!("Found {} entry points", entries.len());
        for ep in entries {
            println!(
                "{} ({})",
                format_display_path(&ep.path, opts.root),
                ep.source
            );
        }
    }

    if let Some(bd) = boundary_data {
        print_boundary_data_human(bd);
    }

    if let Some(ws) = workspace_data {
        print_workspace_data_human(opts.root, ws, opts.workspaces);
    }
}

/// Human-mode render for the workspaces section.
///
/// When the user opted into `--workspaces` explicitly (or via the
/// `fallow workspaces` alias), the renderer always emits SOMETHING so the
/// user is not staring at silence on a non-monorepo. When the section is
/// rendered as part of the implicit show-all default, an empty result stays
/// silent to avoid noise on single-package projects.
///
/// The `explicit` flag distinguishes the two cases.
fn print_workspace_data_human(root: &std::path::Path, ws: &WorkspaceData, explicit: bool) {
    if ws.workspaces.is_empty() && ws.diagnostics.is_empty() {
        if explicit {
            eprintln!("No workspaces declared (single-package project).");
        }
        return;
    }
    if ws.workspaces.is_empty() {
        eprintln!("No workspaces discovered.");
    } else {
        eprintln!("Discovered {} workspaces", ws.workspaces.len());
        for w in &ws.workspaces {
            let relative = w.root.strip_prefix(root).unwrap_or(&w.root);
            let path_str = relative.display().to_string().replace('\\', "/");
            let suffix = if w.is_internal_dependency {
                " (internal dep)"
            } else {
                ""
            };
            println!("  {} -> {path_str}{suffix}", w.name);
        }
    }
    if !ws.diagnostics.is_empty() {
        eprintln!(
            "{} workspace discovery diagnostic{}:",
            ws.diagnostics.len(),
            if ws.diagnostics.len() == 1 { "" } else { "s" }
        );
        for d in &ws.diagnostics {
            eprintln!("  - {}", d.message);
        }
    }
}

/// View-model carrying discovered workspaces alongside any diagnostics
/// produced during discovery (malformed package.json, unreachable glob
/// matches, missing tsconfig references, undeclared workspaces).
struct WorkspaceData {
    workspaces: Vec<fallow_config::WorkspaceInfo>,
    diagnostics: Vec<fallow_config::WorkspaceDiagnostic>,
}

#[cfg(test)]
fn boundary_data_to_json(bd: &BoundaryData) -> serde_json::Value {
    match serde_json::to_value(fallow_api::boundary_data_to_output(bd)) {
        Ok(value) => value,
        Err(error) => panic!("boundary list output should serialize: {error}"),
    }
}

fn print_boundary_data_human(bd: &BoundaryData) {
    if bd.is_empty {
        eprintln!("Boundaries: not configured");
        return;
    }

    print_boundary_header(bd);
    print_boundary_zones(&bd.zones);
    print_boundary_rules(&bd.rules);
    print_boundary_logical_groups(&bd.logical_groups);
}

/// Print the `Boundaries: N zones, M rules[, K logical groups]` summary line.
fn print_boundary_header(bd: &BoundaryData) {
    let mut header_parts = vec![
        format!("{} {}", bd.zones.len(), pluralize("zone", bd.zones.len())),
        format!("{} {}", bd.rules.len(), pluralize("rule", bd.rules.len())),
    ];
    if !bd.logical_groups.is_empty() {
        header_parts.push(format!(
            "{} logical {}",
            bd.logical_groups.len(),
            pluralize("group", bd.logical_groups.len())
        ));
    }
    eprintln!("Boundaries: {}", header_parts.join(", "));
}

/// Print the per-zone name / file-count / patterns section.
fn print_boundary_zones(zones: &[fallow_api::ZoneInfo]) {
    if zones.is_empty() {
        return;
    }
    eprintln!("\nZones:");
    for zone in zones {
        eprintln!(
            "  {:<20} {} {}  {}",
            zone.name,
            zone.file_count,
            pluralize("file", zone.file_count),
            zone.patterns.join(", ")
        );
    }
}

/// Print the per-rule from-zone / allowed-zones section.
fn print_boundary_rules(rules: &[fallow_api::RuleInfo]) {
    if rules.is_empty() {
        return;
    }
    eprintln!("\nRules:");
    for rule in rules {
        if rule.allow.is_empty() {
            eprintln!("  {:<20} (isolated, no imports allowed)", rule.from);
        } else {
            eprintln!("  {:<20} → {}", rule.from, rule.allow.join(", "));
        }
    }
}

/// Print the status-ordered logical-groups section.
fn print_boundary_logical_groups(logical_groups: &[fallow_api::LogicalGroupInfo]) {
    if logical_groups.is_empty() {
        return;
    }
    eprintln!("\nLogical groups:");
    let mut ordered: Vec<&fallow_api::LogicalGroupInfo> = logical_groups.iter().collect();
    ordered.sort_by_key(|g| match g.status {
        fallow_config::LogicalGroupStatus::InvalidPath => 0,
        fallow_config::LogicalGroupStatus::Empty => 1,
        fallow_config::LogicalGroupStatus::Ok => 2,
    });
    for g in ordered {
        print_logical_group_row(g);
    }
}

/// Print one logical-group row plus its optional children line.
fn print_logical_group_row(g: &fallow_api::LogicalGroupInfo) {
    let status_suffix = match g.status {
        fallow_config::LogicalGroupStatus::Ok => String::new(),
        fallow_config::LogicalGroupStatus::Empty => " (empty)".to_owned(),
        fallow_config::LogicalGroupStatus::InvalidPath => " (invalid path)".to_owned(),
    };
    let file_count_render = if g.fallback_zone.is_some() {
        format!(
            "{} {} ({} children + {} fallback)",
            g.file_count,
            pluralize("file", g.file_count),
            g.child_file_count,
            g.fallback_file_count
        )
    } else {
        format!("{} {}", g.file_count, pluralize("file", g.file_count))
    };
    eprintln!(
        "  {:<20} {}  autoDiscover: {}{}",
        g.name,
        file_count_render,
        g.auto_discover.join(", "),
        status_suffix
    );
    if !g.children.is_empty() {
        eprintln!("    children: {}", g.children.join(", "));
    }
}

/// Naive English pluralizer: `(noun, 1)` -> `noun`, otherwise `noun + "s"`.
/// Covers `zone`, `rule`, `group`, `file`; intentionally NOT general-purpose
/// (would need irregulars `boundary`/`boundaries` if used more broadly).
fn pluralize(noun: &str, count: usize) -> String {
    if count == 1 {
        noun.to_owned()
    } else {
        format!("{noun}s")
    }
}

#[cfg(test)]
mod tests {
    use fallow_api::{LogicalGroupInfo, ZoneInfo};

    use super::*;

    fn make_opts(
        entry_points: bool,
        files: bool,
        plugins: bool,
        boundaries: bool,
    ) -> ListOptions<'static> {
        ListOptions {
            root: std::path::Path::new("/project"),
            config_path: &None,
            output: OutputFormat::Human,
            threads: 4,
            no_cache: false,
            entry_points,
            files,
            plugins,
            boundaries,
            workspaces: false,
            production: false,
        }
    }

    #[test]
    fn show_all_when_no_flags_set() {
        assert!(should_show_all(&make_opts(false, false, false, false)));
    }

    #[test]
    fn not_show_all_when_entry_points_set() {
        assert!(!should_show_all(&make_opts(true, false, false, false)));
    }

    #[test]
    fn not_show_all_when_files_set() {
        assert!(!should_show_all(&make_opts(false, true, false, false)));
    }

    #[test]
    fn not_show_all_when_plugins_set() {
        assert!(!should_show_all(&make_opts(false, false, true, false)));
    }

    #[test]
    fn not_show_all_when_boundaries_set() {
        assert!(!should_show_all(&make_opts(false, false, false, true)));
    }

    #[test]
    fn not_show_all_when_all_flags_set() {
        assert!(!should_show_all(&make_opts(true, true, true, true)));
    }

    #[test]
    fn not_show_all_when_two_flags_set() {
        assert!(!should_show_all(&make_opts(true, true, false, false)));
        assert!(!should_show_all(&make_opts(true, false, true, false)));
        assert!(!should_show_all(&make_opts(false, true, true, false)));
    }

    #[test]
    fn needs_discovery_when_files_requested() {
        assert!(needs_file_discovery(true, false, false, false));
    }

    #[test]
    fn needs_discovery_when_show_all() {
        assert!(needs_file_discovery(false, true, false, false));
    }

    #[test]
    fn needs_discovery_when_entry_points_requested() {
        assert!(needs_file_discovery(false, false, true, false));
    }

    #[test]
    fn needs_discovery_when_boundaries_requested() {
        assert!(needs_file_discovery(false, false, false, true));
    }

    #[test]
    fn no_discovery_when_only_plugins() {
        assert!(!needs_file_discovery(false, false, false, false));
    }

    #[test]
    fn list_options_default_flags() {
        let opts = make_opts(false, false, false, false);
        assert!(should_show_all(&opts));
    }

    #[test]
    fn list_options_single_flag() {
        let opts = make_opts(true, false, false, false);
        assert!(!should_show_all(&opts));
        assert!(needs_file_discovery(
            opts.files,
            should_show_all(&opts),
            opts.entry_points,
            opts.boundaries,
        ));
    }

    fn empty_boundary_data() -> BoundaryData {
        BoundaryData {
            zones: vec![],
            rules: vec![],
            logical_groups: vec![],
            is_empty: true,
        }
    }

    #[test]
    fn boundary_json_empty_includes_logical_groups_key() {
        let json = boundary_data_to_json(&empty_boundary_data());
        assert_eq!(json["configured"], false);
        assert!(json["logical_groups"].is_array());
        assert_eq!(json["logical_groups"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn boundary_json_empty_branch_includes_all_count_fields() {
        let json = boundary_data_to_json(&empty_boundary_data());
        assert_eq!(json["zone_count"], 0);
        assert_eq!(json["rule_count"], 0);
        assert_eq!(json["logical_group_count"], 0);
    }

    #[test]
    fn pluralize_singular_plural() {
        assert_eq!(pluralize("file", 0), "files");
        assert_eq!(pluralize("file", 1), "file");
        assert_eq!(pluralize("file", 2), "files");
        assert_eq!(pluralize("zone", 1), "zone");
        assert_eq!(pluralize("group", 1), "group");
    }

    #[test]
    fn boundary_json_logical_group_carries_all_fields() {
        let bd = BoundaryData {
            zones: vec![
                ZoneInfo {
                    name: "features/auth".to_string(),
                    patterns: vec!["src/features/auth/**".to_string()],
                    file_count: 3,
                },
                ZoneInfo {
                    name: "features/billing".to_string(),
                    patterns: vec!["src/features/billing/**".to_string()],
                    file_count: 5,
                },
            ],
            rules: vec![],
            logical_groups: vec![LogicalGroupInfo {
                name: "features".to_string(),
                children: vec!["features/auth".to_string(), "features/billing".to_string()],
                auto_discover: vec!["./src/features/".to_string()],
                authored_rule: Some(fallow_config::AuthoredRule {
                    allow: vec!["shared".to_string()],
                    allow_type_only: vec!["types".to_string()],
                }),
                fallback_zone: None,
                source_zone_index: 1,
                status: fallow_config::LogicalGroupStatus::Ok,
                file_count: 8,
                child_file_count: 8,
                fallback_file_count: 0,
                merged_from: None,
                original_zone_root: None,
                child_source_indices: vec![],
            }],
            is_empty: false,
        };
        let json = boundary_data_to_json(&bd);

        assert_eq!(json["logical_group_count"], 1);
        let groups = json["logical_groups"].as_array().unwrap();
        assert_eq!(groups.len(), 1);
        let g = &groups[0];
        assert_eq!(g["name"], "features");
        assert_eq!(g["children"][0], "features/auth");
        assert_eq!(g["children"][1], "features/billing");
        assert_eq!(g["auto_discover"][0], "./src/features/");
        assert_eq!(g["status"], "ok");
        assert_eq!(g["source_zone_index"], 1);
        assert_eq!(g["file_count"], 8);
        assert_eq!(g["authored_rule"]["allow"][0], "shared");
        assert_eq!(g["authored_rule"]["allow_type_only"][0], "types");
        assert!(g.get("fallback_zone").is_none());
        assert!(g.get("merged_from").is_none());
        assert!(g.get("original_zone_root").is_none());
        assert!(g.get("child_source_indices").is_none());
    }

    #[test]
    fn boundary_json_logical_group_status_serializations() {
        for (status, expected) in [
            (fallow_config::LogicalGroupStatus::Ok, "ok"),
            (fallow_config::LogicalGroupStatus::Empty, "empty"),
            (
                fallow_config::LogicalGroupStatus::InvalidPath,
                "invalid_path",
            ),
        ] {
            let bd = BoundaryData {
                zones: vec![],
                rules: vec![],
                logical_groups: vec![LogicalGroupInfo {
                    name: "features".to_string(),
                    children: vec![],
                    auto_discover: vec!["src/features".to_string()],
                    authored_rule: None,
                    fallback_zone: None,
                    source_zone_index: 0,
                    status,
                    file_count: 0,
                    child_file_count: 0,
                    fallback_file_count: 0,
                    merged_from: None,
                    original_zone_root: None,
                    child_source_indices: vec![],
                }],
                is_empty: false,
            };
            let json = boundary_data_to_json(&bd);
            assert_eq!(json["logical_groups"][0]["status"], expected);
        }
    }

    #[test]
    fn boundary_json_logical_group_fallback_zone_round_trip() {
        let bd = BoundaryData {
            zones: vec![ZoneInfo {
                name: "features".to_string(),
                patterns: vec!["src/features/**".to_string()],
                file_count: 2,
            }],
            rules: vec![],
            logical_groups: vec![LogicalGroupInfo {
                name: "features".to_string(),
                children: vec![],
                auto_discover: vec!["src/features".to_string()],
                authored_rule: None,
                fallback_zone: Some("features".to_string()),
                source_zone_index: 0,
                status: fallow_config::LogicalGroupStatus::Empty,
                file_count: 2,
                child_file_count: 0,
                fallback_file_count: 2,
                merged_from: None,
                original_zone_root: None,
                child_source_indices: vec![],
            }],
            is_empty: false,
        };
        let json = boundary_data_to_json(&bd);
        assert_eq!(json["logical_groups"][0]["fallback_zone"], "features");
    }

    #[test]
    fn boundary_json_logical_group_authored_rule_omits_empty_allow_type_only() {
        let bd = BoundaryData {
            zones: vec![],
            rules: vec![],
            logical_groups: vec![LogicalGroupInfo {
                name: "features".to_string(),
                children: vec![],
                auto_discover: vec!["src/features".to_string()],
                authored_rule: Some(fallow_config::AuthoredRule {
                    allow: vec!["shared".to_string()],
                    allow_type_only: vec![],
                }),
                fallback_zone: None,
                source_zone_index: 0,
                status: fallow_config::LogicalGroupStatus::Empty,
                file_count: 0,
                child_file_count: 0,
                fallback_file_count: 0,
                merged_from: None,
                original_zone_root: None,
                child_source_indices: vec![],
            }],
            is_empty: false,
        };
        let json = boundary_data_to_json(&bd);
        let rule = &json["logical_groups"][0]["authored_rule"];
        assert_eq!(rule["allow"][0], "shared");
        assert!(rule.get("allow_type_only").is_none());
    }

    #[test]
    fn boundary_json_logical_group_merged_from_when_duplicates() {
        let bd = BoundaryData {
            zones: vec![],
            rules: vec![],
            logical_groups: vec![LogicalGroupInfo {
                name: "features".to_string(),
                children: vec![],
                auto_discover: vec!["src/features".to_string(), "src/modules".to_string()],
                authored_rule: None,
                fallback_zone: None,
                source_zone_index: 0,
                status: fallow_config::LogicalGroupStatus::Ok,
                file_count: 0,
                child_file_count: 0,
                fallback_file_count: 0,
                merged_from: Some(vec![0, 3]),
                original_zone_root: None,
                child_source_indices: vec![],
            }],
            is_empty: false,
        };
        let json = boundary_data_to_json(&bd);
        let g = &json["logical_groups"][0];
        assert_eq!(g["merged_from"][0], 0);
        assert_eq!(g["merged_from"][1], 3);
    }

    #[test]
    fn boundary_json_logical_group_original_zone_root_emitted() {
        let bd = BoundaryData {
            zones: vec![],
            rules: vec![],
            logical_groups: vec![LogicalGroupInfo {
                name: "features".to_string(),
                children: vec![],
                auto_discover: vec!["src/features".to_string()],
                authored_rule: None,
                fallback_zone: None,
                source_zone_index: 0,
                status: fallow_config::LogicalGroupStatus::Ok,
                file_count: 0,
                child_file_count: 0,
                fallback_file_count: 0,
                merged_from: None,
                original_zone_root: Some("packages/app/".to_string()),
                child_source_indices: vec![],
            }],
            is_empty: false,
        };
        let json = boundary_data_to_json(&bd);
        assert_eq!(
            json["logical_groups"][0]["original_zone_root"],
            "packages/app/"
        );
    }

    #[test]
    fn boundary_json_logical_group_child_source_indices_emitted_for_multi_path() {
        let bd = BoundaryData {
            zones: vec![],
            rules: vec![],
            logical_groups: vec![LogicalGroupInfo {
                name: "features".to_string(),
                children: vec!["features/auth".to_string(), "features/billing".to_string()],
                auto_discover: vec!["src/features".to_string(), "src/modules".to_string()],
                authored_rule: None,
                fallback_zone: None,
                source_zone_index: 0,
                status: fallow_config::LogicalGroupStatus::Ok,
                file_count: 0,
                child_file_count: 0,
                fallback_file_count: 0,
                merged_from: None,
                original_zone_root: None,
                child_source_indices: vec![0, 1],
            }],
            is_empty: false,
        };
        let json = boundary_data_to_json(&bd);
        assert_eq!(json["logical_groups"][0]["child_source_indices"][0], 0);
        assert_eq!(json["logical_groups"][0]["child_source_indices"][1], 1);
    }
}
