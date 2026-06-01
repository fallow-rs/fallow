use std::process::ExitCode;

use fallow_config::OutputFormat;

use crate::report::format_display_path;
use crate::runtime_support::load_config;

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

pub fn run_list(opts: &ListOptions<'_>) -> ExitCode {
    let config = match load_config(
        opts.root,
        opts.config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        opts.production,
        true, // list command doesn't need progress bars
    ) {
        Ok(c) => c,
        Err(code) => return code,
    };

    let show_all = should_show_all(opts);

    let plugin_result = if opts.plugins || opts.entry_points || show_all {
        let disc = fallow_core::discover::discover_files_with_plugin_scopes(&config);
        let file_paths: Vec<std::path::PathBuf> = disc.iter().map(|f| f.path.clone()).collect();
        let registry = fallow_core::plugins::PluginRegistry::new(config.external_plugins.clone());

        let pkg_path = opts.root.join("package.json");
        let mut result = fallow_config::PackageJson::load(&pkg_path).map_or_else(
            |_| fallow_core::plugins::AggregatedPluginResult::default(),
            |pkg| registry.run(&pkg, opts.root, &file_paths),
        );

        let workspaces = fallow_config::discover_workspaces(opts.root);
        for ws in &workspaces {
            let ws_pkg_path = ws.root.join("package.json");
            if let Ok(ws_pkg) = fallow_config::PackageJson::load(&ws_pkg_path) {
                let ws_result = registry.run(&ws_pkg, &ws.root, &file_paths);
                for plugin_name in &ws_result.active_plugins {
                    if !result.active_plugins.contains(plugin_name) {
                        result.active_plugins.push(plugin_name.clone());
                    }
                }
            }
        }
        Some(result)
    } else {
        None
    };

    let need_files = needs_file_discovery(opts.files, show_all, opts.entry_points, opts.boundaries);
    let discovered = if need_files {
        Some(fallow_core::discover::discover_files_with_plugin_scopes(
            &config,
        ))
    } else {
        None
    };

    let all_entry_points = if (opts.entry_points || show_all)
        && let Some(ref disc) = discovered
    {
        let mut entries = fallow_core::discover::discover_entry_points(&config, disc);
        let workspaces = fallow_config::discover_workspaces(opts.root);
        for ws in &workspaces {
            let ws_entries =
                fallow_core::discover::discover_workspace_entry_points(&ws.root, &config, disc);
            entries.extend(ws_entries);
        }
        if let Some(ref pr) = plugin_result {
            let plugin_entries =
                fallow_core::discover::discover_plugin_entry_points(pr, &config, disc);
            entries.extend(plugin_entries);
        }
        Some(entries)
    } else {
        None
    };

    let boundary_data = if opts.boundaries {
        Some(compute_boundary_data(&config, discovered.as_deref()))
    } else {
        None
    };

    let workspace_data = if opts.workspaces || show_all {
        match fallow_config::discover_workspaces_with_diagnostics(
            opts.root,
            &config.ignore_patterns,
        ) {
            Ok((workspaces, mut diagnostics)) => {
                let undeclared = fallow_config::find_undeclared_workspaces_with_ignores(
                    opts.root,
                    &workspaces,
                    &config.ignore_patterns,
                );
                let already_flagged: rustc_hash::FxHashSet<std::path::PathBuf> = diagnostics
                    .iter()
                    .map(|d| dunce::canonicalize(&d.path).unwrap_or_else(|_| d.path.clone()))
                    .collect();
                for diag in undeclared {
                    let canonical =
                        dunce::canonicalize(&diag.path).unwrap_or_else(|_| diag.path.clone());
                    if !already_flagged.contains(&canonical) {
                        diagnostics.push(diag);
                    }
                }
                Some(WorkspaceData {
                    workspaces,
                    diagnostics,
                })
            }
            Err(err) => {
                return crate::error::emit_error(&err.to_string(), 2, opts.output);
            }
        }
    } else {
        None
    };

    match opts.output {
        OutputFormat::Json => print_list_json(
            opts,
            show_all,
            plugin_result.as_ref(),
            discovered.as_deref(),
            all_entry_points.as_deref(),
            boundary_data.as_ref(),
            workspace_data.as_ref(),
        ),
        _ => {
            print_list_human(
                opts,
                show_all,
                plugin_result.as_ref(),
                discovered.as_deref(),
                all_entry_points.as_deref(),
                boundary_data.as_ref(),
                workspace_data.as_ref(),
            );
            ExitCode::SUCCESS
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

/// Print list results as JSON and return the appropriate exit code.
fn print_list_json(
    opts: &ListOptions<'_>,
    show_all: bool,
    plugin_result: Option<&fallow_core::plugins::AggregatedPluginResult>,
    discovered: Option<&[fallow_core::discover::DiscoveredFile]>,
    entry_points: Option<&[fallow_core::discover::EntryPoint]>,
    boundary_data: Option<&BoundaryData>,
    workspace_data: Option<&WorkspaceData>,
) -> ExitCode {
    let mut result = serde_json::Map::new();

    if (opts.plugins || show_all)
        && let Some(pr) = plugin_result
    {
        let pl: Vec<serde_json::Value> = pr
            .active_plugins
            .iter()
            .map(|name| serde_json::json!({ "name": name }))
            .collect();
        result.insert("plugins".to_string(), serde_json::json!(pl));
    }

    if (opts.files || show_all)
        && let Some(disc) = discovered
    {
        let paths: Vec<serde_json::Value> = disc
            .iter()
            .map(|f| serde_json::json!(format_display_path(&f.path, opts.root)))
            .collect();
        result.insert("file_count".to_string(), serde_json::json!(paths.len()));
        result.insert("files".to_string(), serde_json::json!(paths));
    }

    if let Some(entries) = entry_points {
        let eps: Vec<serde_json::Value> = entries
            .iter()
            .map(|ep| {
                serde_json::json!({
                    "path": format_display_path(&ep.path, opts.root),
                    "source": ep.source.to_string(),
                })
            })
            .collect();
        result.insert(
            "entry_point_count".to_string(),
            serde_json::json!(eps.len()),
        );
        result.insert("entry_points".to_string(), serde_json::json!(eps));
    }

    let has_boundaries = boundary_data.is_some();
    if let Some(bd) = boundary_data {
        result.insert("boundaries".to_string(), boundary_data_to_json(bd));
    }

    if let Some(ws) = workspace_data {
        let ws_json: Vec<serde_json::Value> = ws
            .workspaces
            .iter()
            .map(|w| {
                let relative = w.root.strip_prefix(opts.root).unwrap_or(&w.root);
                serde_json::json!({
                    "name": w.name,
                    "path": relative.display().to_string().replace('\\', "/"),
                    "is_internal_dependency": w.is_internal_dependency,
                })
            })
            .collect();
        let root_prefix = format!("{}/", opts.root.display());
        let diag_json: Vec<serde_json::Value> = ws
            .diagnostics
            .iter()
            .map(|d| {
                let mut value = serde_json::to_value(d).unwrap_or(serde_json::Value::Null);
                crate::report::strip_root_prefix(&mut value, &root_prefix);
                value
            })
            .collect();
        result.insert(
            "workspace_count".to_string(),
            serde_json::json!(ws_json.len()),
        );
        result.insert("workspaces".to_string(), serde_json::json!(ws_json));
        result.insert(
            "workspace_diagnostics".to_string(),
            serde_json::json!(diag_json),
        );
    }

    let mut output = serde_json::Value::Object(result);
    if has_boundaries {
        crate::output_envelope::apply_root_kind(&mut output, "list-boundaries");
    }

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

/// Print list results in human-readable format.
fn print_list_human(
    opts: &ListOptions<'_>,
    show_all: bool,
    plugin_result: Option<&fallow_core::plugins::AggregatedPluginResult>,
    discovered: Option<&[fallow_core::discover::DiscoveredFile]>,
    entry_points: Option<&[fallow_core::discover::EntryPoint]>,
    boundary_data: Option<&BoundaryData>,
    workspace_data: Option<&WorkspaceData>,
) {
    if (opts.plugins || show_all)
        && let Some(pr) = plugin_result
    {
        eprintln!("Active plugins:");
        for name in &pr.active_plugins {
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

struct BoundaryData {
    zones: Vec<ZoneInfo>,
    rules: Vec<RuleInfo>,
    logical_groups: Vec<LogicalGroupInfo>,
    is_empty: bool,
}

struct ZoneInfo {
    name: String,
    patterns: Vec<String>,
    file_count: usize,
}

struct RuleInfo {
    from: String,
    allow: Vec<String>,
}

/// View-model mirror of [`fallow_config::LogicalGroup`] with the summed
/// `file_count` derived from `zones[]`. The config-layer type stops at
/// "what did the user write?"; this struct adds the analytical view "how
/// many files does the group reach?" so the JSON consumer (Sankey
/// renderer, dashboard, agent tooling) does not have to re-aggregate.
struct LogicalGroupInfo {
    name: String,
    children: Vec<String>,
    auto_discover: Vec<String>,
    authored_rule: Option<fallow_config::AuthoredRule>,
    fallback_zone: Option<String>,
    source_zone_index: usize,
    status: fallow_config::LogicalGroupStatus,
    /// Sum of `file_count` across `children` PLUS the fallback zone's
    /// `file_count` when present. The two halves are kept separately in
    /// [`Self::child_file_count`] and [`Self::fallback_file_count`] so the
    /// human renderer can show the split when a fallback exists.
    file_count: usize,
    /// Subtotal: sum of `file_count` across `children` only. Equals
    /// [`Self::file_count`] when there is no fallback zone.
    child_file_count: usize,
    /// Subtotal: `file_count` of the `fallback_zone`. `0` when there is
    /// no fallback zone.
    fallback_file_count: usize,
    /// Parent zone indices merged into this group when the user declared
    /// the same parent name twice. Mirrors
    /// [`fallow_config::LogicalGroup::merged_from`].
    merged_from: Option<Vec<usize>>,
    /// Parent zone's `root` (subtree scope) as the user authored it.
    /// Mirrors [`fallow_config::LogicalGroup::original_zone_root`].
    original_zone_root: Option<String>,
    /// Parallel to `children`: source path indices. Empty when only one
    /// `auto_discover` path was authored. Mirrors
    /// [`fallow_config::LogicalGroup::child_source_indices`].
    child_source_indices: Vec<usize>,
}

fn compute_boundary_data(
    config: &fallow_config::ResolvedConfig,
    discovered: Option<&[fallow_core::discover::DiscoveredFile]>,
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

    let zones: Vec<ZoneInfo> = boundaries
        .zones
        .iter()
        .map(|zone| {
            let file_count = discovered.map_or(0, |files| {
                files
                    .iter()
                    .filter(|f| {
                        let rel = f
                            .path
                            .strip_prefix(&config.root)
                            .ok()
                            .map(|p| p.to_string_lossy().replace('\\', "/"));
                        rel.is_some_and(|p| {
                            boundaries.classify_zone(&p) == Some(zone.name.as_str())
                        })
                    })
                    .count()
            });
            ZoneInfo {
                name: zone.name.clone(),
                patterns: zone.matchers.iter().map(|m| m.glob().to_string()).collect(),
                file_count,
            }
        })
        .collect();

    let rules: Vec<RuleInfo> = boundaries
        .rules
        .iter()
        .map(|r| RuleInfo {
            from: r.from_zone.clone(),
            allow: r.allowed_zones.clone(),
        })
        .collect();

    let zone_count_by_name: rustc_hash::FxHashMap<&str, usize> = zones
        .iter()
        .map(|z| (z.name.as_str(), z.file_count))
        .collect();

    let logical_groups: Vec<LogicalGroupInfo> = boundaries
        .logical_groups
        .iter()
        .map(|g| {
            let child_file_count: usize = g
                .children
                .iter()
                .filter_map(|child| zone_count_by_name.get(child.as_str()).copied())
                .sum();
            let fallback_file_count = g
                .fallback_zone
                .as_deref()
                .and_then(|fb| zone_count_by_name.get(fb).copied())
                .unwrap_or(0);
            LogicalGroupInfo {
                name: g.name.clone(),
                children: g.children.clone(),
                auto_discover: g.auto_discover.clone(),
                authored_rule: g.authored_rule.clone(),
                fallback_zone: g.fallback_zone.clone(),
                source_zone_index: g.source_zone_index,
                status: g.status,
                file_count: child_file_count + fallback_file_count,
                child_file_count,
                fallback_file_count,
                merged_from: g.merged_from.clone(),
                original_zone_root: g.original_zone_root.clone(),
                child_source_indices: g.child_source_indices.clone(),
            }
        })
        .collect();

    BoundaryData {
        zones,
        rules,
        logical_groups,
        is_empty: false,
    }
}

fn boundary_data_to_json(bd: &BoundaryData) -> serde_json::Value {
    if bd.is_empty {
        return serde_json::json!({
            "configured": false,
            "zone_count": 0,
            "zones": [],
            "rule_count": 0,
            "rules": [],
            "logical_group_count": 0,
            "logical_groups": [],
        });
    }

    let zones: Vec<serde_json::Value> = bd
        .zones
        .iter()
        .map(|z| {
            serde_json::json!({
                "name": z.name,
                "patterns": z.patterns,
                "file_count": z.file_count,
            })
        })
        .collect();

    let rules: Vec<serde_json::Value> = bd
        .rules
        .iter()
        .map(|r| {
            serde_json::json!({
                "from": r.from,
                "allow": r.allow,
            })
        })
        .collect();

    let logical_groups: Vec<serde_json::Value> = bd
        .logical_groups
        .iter()
        .map(logical_group_info_to_json)
        .collect();

    serde_json::json!({
        "configured": true,
        "zone_count": bd.zones.len(),
        "zones": zones,
        "rule_count": bd.rules.len(),
        "rules": rules,
        "logical_group_count": bd.logical_groups.len(),
        "logical_groups": logical_groups,
    })
}

fn logical_group_info_to_json(g: &LogicalGroupInfo) -> serde_json::Value {
    let status = match g.status {
        fallow_config::LogicalGroupStatus::Ok => "ok",
        fallow_config::LogicalGroupStatus::Empty => "empty",
        fallow_config::LogicalGroupStatus::InvalidPath => "invalid_path",
    };
    let mut entry = serde_json::Map::new();
    entry.insert("name".to_string(), serde_json::json!(g.name));
    entry.insert("children".to_string(), serde_json::json!(g.children));
    entry.insert(
        "auto_discover".to_string(),
        serde_json::json!(g.auto_discover),
    );
    entry.insert("status".to_string(), serde_json::json!(status));
    entry.insert(
        "source_zone_index".to_string(),
        serde_json::json!(g.source_zone_index),
    );
    entry.insert("file_count".to_string(), serde_json::json!(g.file_count));
    if let Some(rule) = &g.authored_rule {
        let mut rule_obj = serde_json::Map::new();
        rule_obj.insert("allow".to_string(), serde_json::json!(rule.allow));
        if !rule.allow_type_only.is_empty() {
            rule_obj.insert(
                "allow_type_only".to_string(),
                serde_json::json!(rule.allow_type_only),
            );
        }
        entry.insert(
            "authored_rule".to_string(),
            serde_json::Value::Object(rule_obj),
        );
    }
    if let Some(fb) = &g.fallback_zone {
        entry.insert("fallback_zone".to_string(), serde_json::json!(fb));
    }
    if let Some(chain) = &g.merged_from {
        entry.insert("merged_from".to_string(), serde_json::json!(chain));
    }
    if let Some(root) = &g.original_zone_root {
        entry.insert("original_zone_root".to_string(), serde_json::json!(root));
    }
    if !g.child_source_indices.is_empty() {
        entry.insert(
            "child_source_indices".to_string(),
            serde_json::json!(g.child_source_indices),
        );
    }
    serde_json::Value::Object(entry)
}

fn print_boundary_data_human(bd: &BoundaryData) {
    if bd.is_empty {
        eprintln!("Boundaries: not configured");
        return;
    }

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

    if !bd.zones.is_empty() {
        eprintln!("\nZones:");
        for zone in &bd.zones {
            eprintln!(
                "  {:<20} {} {}  {}",
                zone.name,
                zone.file_count,
                pluralize("file", zone.file_count),
                zone.patterns.join(", ")
            );
        }
    }

    if !bd.rules.is_empty() {
        eprintln!("\nRules:");
        for rule in &bd.rules {
            if rule.allow.is_empty() {
                eprintln!("  {:<20} (isolated, no imports allowed)", rule.from);
            } else {
                eprintln!("  {:<20} → {}", rule.from, rule.allow.join(", "));
            }
        }
    }

    if !bd.logical_groups.is_empty() {
        eprintln!("\nLogical groups:");
        let mut ordered: Vec<&LogicalGroupInfo> = bd.logical_groups.iter().collect();
        ordered.sort_by_key(|g| match g.status {
            fallow_config::LogicalGroupStatus::InvalidPath => 0,
            fallow_config::LogicalGroupStatus::Empty => 1,
            fallow_config::LogicalGroupStatus::Ok => 2,
        });
        for g in ordered {
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
