//! What the plugin run knows about files, dependencies and exports beyond
//! the module graph, collected once per analysis for the trace output.

use std::path::Path;

use fallow_types::extract::ExportName;
use fallow_types::trace::{PluginExportCredit, TraceProvenance};

use super::{
    AggregatedPluginResult, CONFIG_ONLY_EXPORT_NAMES, CompiledPathRule, PluginUsedExportRule,
    federation_trace_provenance, push_tooling_trace_credits,
};
use crate::discover::DiscoveredFile;
use crate::extract::ModuleInfo;

/// Which plugin export credits the trace provenance records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportCredits {
    /// Record no export credit. A run without a retained module graph gives
    /// no trace, so it skips the match of each rule against each module.
    Skip,
    /// Record the plugin `used_exports` credits and, when
    /// `include_entry_exports` is on, the config-file export credits, the
    /// same as in the unused-export analysis.
    Collect { include_entry_exports: bool },
}

impl ExportCredits {
    /// The credits of a run: a trace needs the retained module graph.
    #[must_use]
    pub const fn for_run(retain_graph: bool, config: &fallow_config::ResolvedConfig) -> Self {
        if retain_graph {
            Self::Collect {
                include_entry_exports: config.include_entry_exports,
            }
        } else {
            Self::Skip
        }
    }
}

/// Collect the trace provenance of one analysis, with the seeds of the
/// peer-dependency closures.
#[must_use]
pub fn build_trace_provenance(
    root: &Path,
    files: &[DiscoveredFile],
    workspaces: &[fallow_config::WorkspaceInfo],
    modules: &[ModuleInfo],
    plugin_result: &AggregatedPluginResult,
    export_credits: ExportCredits,
) -> TraceProvenance {
    let mut provenance =
        federation_trace_provenance(root, files, &plugin_result.federation_sources, modules);
    push_tooling_trace_credits(&mut provenance, root, workspaces, plugin_result);
    push_config_export_provenance(&mut provenance, root, files, modules, plugin_result);
    if let ExportCredits::Collect {
        include_entry_exports,
    } = export_credits
    {
        push_plugin_export_credits(
            &mut provenance,
            root,
            files,
            modules,
            plugin_result,
            include_entry_exports,
        );
    }
    provenance.with_peer_seeds(crate::analyze::peer_closure_seeds(plugin_result))
}

/// Record each export that a plugin rule marks as used.
///
/// The rules are the plugin `used_exports` rules and, under
/// `includeEntryExports`, the config-file export credits. A rule matches the
/// export names that it lists, or every export for `*`.
fn push_plugin_export_credits(
    provenance: &mut TraceProvenance,
    root: &Path,
    files: &[DiscoveredFile],
    modules: &[ModuleInfo],
    plugin_result: &AggregatedPluginResult,
    include_entry_exports: bool,
) {
    let config_credits: &[PluginUsedExportRule] = if include_entry_exports {
        &plugin_result.config_export_credits
    } else {
        &[]
    };
    let rules: Vec<(CompiledPathRule, &PluginUsedExportRule)> = plugin_result
        .used_exports
        .iter()
        .chain(config_credits)
        .filter_map(|rule| {
            let compiled = CompiledPathRule::for_used_export_rule(
                &rule.rule.path,
                "plugin export credit pattern",
            )?;
            Some((compiled, rule))
        })
        .collect();
    if rules.is_empty() {
        return;
    }
    for module in modules {
        let Some(file) = files.get(module.file_id.0 as usize) else {
            continue;
        };
        let relative = file.path.strip_prefix(root).unwrap_or(&file.path);
        let relative_str = relative.to_string_lossy().replace('\\', "/");
        let mut credits = Vec::new();
        for (compiled, rule) in &rules {
            if !compiled.matches(&relative_str) {
                continue;
            }
            for name in export_names(module) {
                if rule.rule.exports.iter().any(|e| e == "*" || e == name) {
                    credits.push((
                        name.to_owned(),
                        PluginExportCredit {
                            plugin: rule.plugin_name.clone(),
                            pattern: rule.rule.path.pattern.clone(),
                        },
                    ));
                }
            }
        }
        if !credits.is_empty() {
            provenance.push_export_credits(relative.to_path_buf(), credits);
        }
    }
}

/// The names that a module exports: its own exports and its named
/// re-exports. The graph gives a named re-export its own export, so a plugin
/// rule credits it in the analysis too.
fn export_names(module: &ModuleInfo) -> impl Iterator<Item = &str> {
    let own = module.exports.iter().map(|export| match &export.name {
        ExportName::Named(name) => name.as_str(),
        ExportName::Default => "default",
    });
    let re_exported = module
        .re_exports
        .iter()
        .map(|re_export| re_export.exported_name.as_str())
        .filter(|name| *name != "*");
    own.chain(re_exported)
}

/// Record each export that a plugin `used_exports` rule names for its file
/// and that is in [`CONFIG_ONLY_EXPORT_NAMES`]. Only modules that declare such
/// a name pay for the glob match.
fn push_config_export_provenance(
    provenance: &mut TraceProvenance,
    root: &Path,
    files: &[DiscoveredFile],
    modules: &[ModuleInfo],
    plugin_result: &AggregatedPluginResult,
) {
    let rules: Vec<(CompiledPathRule, Vec<&str>)> = plugin_result
        .used_exports
        .iter()
        .filter_map(|rule| {
            let names: Vec<&str> = rule
                .rule
                .exports
                .iter()
                .map(String::as_str)
                .filter(|name| CONFIG_ONLY_EXPORT_NAMES.contains(name))
                .collect();
            if names.is_empty() {
                return None;
            }
            let compiled =
                CompiledPathRule::for_used_export_rule(&rule.rule.path, "used_exports pattern")?;
            Some((compiled, names))
        })
        .collect();
    if rules.is_empty() {
        return;
    }
    for module in modules {
        let config_names: Vec<String> = module
            .exports
            .iter()
            .filter_map(|export| match &export.name {
                ExportName::Named(name)
                    if !export.is_type_only
                        && CONFIG_ONLY_EXPORT_NAMES.contains(&name.as_str()) =>
                {
                    Some(name.clone())
                }
                _ => None,
            })
            .collect();
        if config_names.is_empty() {
            continue;
        }
        let Some(file) = files.get(module.file_id.0 as usize) else {
            continue;
        };
        let relative = file.path.strip_prefix(root).unwrap_or(&file.path);
        let relative_str = relative.to_string_lossy().replace('\\', "/");
        for name in config_names {
            if rules
                .iter()
                .any(|(rule, names)| names.contains(&name.as_str()) && rule.matches(&relative_str))
            {
                provenance.push_config_export(relative.to_path_buf(), name);
            }
        }
    }
}
