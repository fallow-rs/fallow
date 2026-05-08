use std::process::ExitCode;

use fallow_config::OutputFormat;

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
    pub production: bool,
}

pub fn run_list(opts: &ListOptions<'_>) -> ExitCode {
    let config = match load_config(
        opts.root,
        opts.config_path,
        OutputFormat::Human,
        opts.no_cache,
        opts.threads,
        opts.production,
        true, // list command doesn't need progress bars
    ) {
        Ok(c) => c,
        Err(code) => return code,
    };

    let show_all = should_show_all(opts);

    // Run plugin detection when plugin output is requested or when entry-point
    // discovery needs plugin-provided entry points.
    let plugin_result = if opts.plugins || opts.entry_points || show_all {
        let disc = fallow_core::discover::discover_files_with_plugin_scopes(&config);
        let file_paths: Vec<std::path::PathBuf> = disc.iter().map(|f| f.path.clone()).collect();
        let registry = fallow_core::plugins::PluginRegistry::new(config.external_plugins.clone());

        let pkg_path = opts.root.join("package.json");
        let mut result = fallow_config::PackageJson::load(&pkg_path).map_or_else(
            |_| fallow_core::plugins::AggregatedPluginResult::default(),
            |pkg| registry.run(&pkg, opts.root, &file_paths),
        );

        // Also run plugins for workspace packages
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

    // Discover files once if needed by files, entry_points, or boundaries
    let need_files = needs_file_discovery(opts.files, show_all, opts.entry_points, opts.boundaries);
    let discovered = if need_files {
        Some(fallow_core::discover::discover_files_with_plugin_scopes(
            &config,
        ))
    } else {
        None
    };

    // Compute entry points once (shared by both JSON and human output branches)
    let all_entry_points = if (opts.entry_points || show_all)
        && let Some(ref disc) = discovered
    {
        let mut entries = fallow_core::discover::discover_entry_points(&config, disc);
        // Add workspace entry points
        let workspaces = fallow_config::discover_workspaces(opts.root);
        for ws in &workspaces {
            let ws_entries =
                fallow_core::discover::discover_workspace_entry_points(&ws.root, &config, disc);
            entries.extend(ws_entries);
        }
        // Add plugin-discovered entry points
        if let Some(ref pr) = plugin_result {
            let plugin_entries =
                fallow_core::discover::discover_plugin_entry_points(pr, &config, disc);
            entries.extend(plugin_entries);
        }
        Some(entries)
    } else {
        None
    };

    // Boundaries are opt-in to keep the default list view focused on files,
    // plugins, and entry points.
    let boundary_data = if opts.boundaries {
        Some(compute_boundary_data(&config, discovered.as_deref()))
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
        ),
        _ => {
            print_list_human(
                opts,
                show_all,
                plugin_result.as_ref(),
                discovered.as_deref(),
                all_entry_points.as_deref(),
                boundary_data.as_ref(),
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
    !opts.entry_points && !opts.files && !opts.plugins && !opts.boundaries
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

// ── Output helpers ─────────────────────────────────────────────

/// Print list results as JSON and return the appropriate exit code.
fn print_list_json(
    opts: &ListOptions<'_>,
    show_all: bool,
    plugin_result: Option<&fallow_core::plugins::AggregatedPluginResult>,
    discovered: Option<&[fallow_core::discover::DiscoveredFile]>,
    entry_points: Option<&[fallow_core::discover::EntryPoint]>,
    boundary_data: Option<&BoundaryData>,
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
            .map(|f| {
                let relative = f.path.strip_prefix(opts.root).unwrap_or(&f.path);
                serde_json::json!(relative.display().to_string())
            })
            .collect();
        result.insert("file_count".to_string(), serde_json::json!(paths.len()));
        result.insert("files".to_string(), serde_json::json!(paths));
    }

    if let Some(entries) = entry_points {
        let eps: Vec<serde_json::Value> = entries
            .iter()
            .map(|ep| {
                let relative = ep.path.strip_prefix(opts.root).unwrap_or(&ep.path);
                serde_json::json!({
                    "path": relative.display().to_string(),
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

    if let Some(bd) = boundary_data {
        result.insert("boundaries".to_string(), boundary_data_to_json(bd));
    }

    match serde_json::to_string_pretty(&serde_json::Value::Object(result)) {
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
            let relative = file.path.strip_prefix(opts.root).unwrap_or(&file.path);
            println!("{}", relative.display());
        }
    }

    if let Some(entries) = entry_points {
        eprintln!("Found {} entry points", entries.len());
        for ep in entries {
            let relative = ep.path.strip_prefix(opts.root).unwrap_or(&ep.path);
            println!("{} ({})", relative.display(), ep.source);
        }
    }

    if let Some(bd) = boundary_data {
        print_boundary_data_human(bd);
    }
}

// ── Boundary listing helpers ───────────────────────────────────

struct BoundaryData {
    zones: Vec<ZoneInfo>,
    rules: Vec<RuleInfo>,
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

fn compute_boundary_data(
    config: &fallow_config::ResolvedConfig,
    discovered: Option<&[fallow_core::discover::DiscoveredFile]>,
) -> BoundaryData {
    let boundaries = &config.boundaries;

    if boundaries.is_empty() {
        return BoundaryData {
            zones: vec![],
            rules: vec![],
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
                        rel.is_some_and(|p| zone.matchers.iter().any(|m| m.is_match(&p)))
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

    BoundaryData {
        zones,
        rules,
        is_empty: false,
    }
}

fn boundary_data_to_json(bd: &BoundaryData) -> serde_json::Value {
    if bd.is_empty {
        return serde_json::json!({
            "configured": false,
            "zones": [],
            "rules": []
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

    serde_json::json!({
        "configured": true,
        "zone_count": bd.zones.len(),
        "zones": zones,
        "rule_count": bd.rules.len(),
        "rules": rules,
    })
}

fn print_boundary_data_human(bd: &BoundaryData) {
    if bd.is_empty {
        eprintln!("Boundaries: not configured");
        return;
    }

    eprintln!(
        "Boundaries: {} zones, {} rules",
        bd.zones.len(),
        bd.rules.len()
    );

    eprintln!("\nZones:");
    for zone in &bd.zones {
        eprintln!(
            "  {:<20} {} files  {}",
            zone.name,
            zone.file_count,
            zone.patterns.join(", ")
        );
    }

    eprintln!("\nRules:");
    for rule in &bd.rules {
        if rule.allow.is_empty() {
            eprintln!("  {:<20} (isolated, no imports allowed)", rule.from);
        } else {
            eprintln!("  {:<20} → {}", rule.from, rule.allow.join(", "));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── should_show_all ─────────────────────────────────────────

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

    // ── needs_file_discovery ────────────────────────────────────

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
        // plugins=true but show_all=false, files=false, entry_points=false, boundaries=false
        assert!(!needs_file_discovery(false, false, false, false));
    }

    // ── ListOptions construction ────────────────────────────────

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
}
