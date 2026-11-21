//! Credits that the rules of an entry file give to the names behind its
//! `export *` lines.
//!
//! With `includeEntryExports`, an `export *` of an entry file credits only the
//! names that an importer uses. The entry file has no export symbol for a name
//! that comes through the star, so an `ignoreExports` rule, a plugin
//! `usedExports` rule or a config-file credit on the entry file finds no
//! export to match. This index applies those rules to the star sources.

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_config::{CompiledIgnoreExportRule, ResolvedConfig};

use crate::discover::FileId;
use crate::graph::{ExportNamespace, ModuleGraph, ModuleNode};
use crate::plugins::{ConfigExportCredit, is_export_ignored};

use super::{PluginMatchers, config_credited_exports, matchers_for_module};

struct EntryRules<'a> {
    entry: FileId,
    ignore: Vec<&'a [String]>,
    plugin: Vec<&'a [&'a str]>,
    config: Vec<&'a [String]>,
}

impl EntryRules<'_> {
    fn credits(&self, export_name: &str) -> bool {
        ConfigExportCredit::credits(&self.config, export_name)
            || is_export_ignored(export_name, &self.ignore, &self.plugin)
    }
}

/// The rules of each consumer-checked entry file, by the files that its plain
/// `export *` chains reach.
#[derive(Default)]
pub(super) struct EntryStarCredit<'a> {
    graph: Option<&'a ModuleGraph>,
    rules: Vec<EntryRules<'a>>,
    rules_by_source: FxHashMap<FileId, Vec<usize>>,
}

pub(super) struct EntryStarCreditInput<'a> {
    pub(super) graph: &'a ModuleGraph,
    pub(super) config: &'a ResolvedConfig,
    pub(super) ignore_matchers: &'a [CompiledIgnoreExportRule],
    pub(super) plugin_matchers: &'a PluginMatchers<'a>,
    pub(super) config_export_credit: &'a ConfigExportCredit<'a>,
}

impl<'a> EntryStarCredit<'a> {
    pub(super) fn build(input: &EntryStarCreditInput<'a>) -> Self {
        let mut credit = Self {
            graph: Some(input.graph),
            ..Self::default()
        };
        let no_rules = input.ignore_matchers.is_empty()
            && input.plugin_matchers.is_empty()
            && input.config_export_credit.is_empty();
        if no_rules {
            return credit;
        }
        for module in &input.graph.modules {
            if !module.is_entry_point()
                || !has_plain_star(module)
                || !input.config.include_entry_exports_for(&module.path)
            {
                continue;
            }
            let (ignore, plugin) = matchers_for_module(
                &module.path,
                &input.config.root,
                input.ignore_matchers,
                input.plugin_matchers,
            );
            let config = config_credited_exports(module, input.config, input.config_export_credit);
            if ignore.is_empty() && plugin.is_empty() && config.is_empty() {
                continue;
            }
            let index = credit.rules.len();
            credit.rules.push(EntryRules {
                entry: module.file_id,
                ignore,
                plugin,
                config,
            });
            for source in plain_star_sources(input.graph, module) {
                credit
                    .rules_by_source
                    .entry(source)
                    .or_default()
                    .push(index);
            }
        }
        credit
    }

    /// Whether a rule of an entry file covers `export_name` of `file`.
    ///
    /// The entry file must forward that declaration under the same name. A
    /// local export of the entry file with the same name hides the name of the
    /// star source, and then the rule names the local export only.
    pub(super) fn credits(&self, file: FileId, export_name: &str) -> bool {
        let (Some(graph), Some(indexes)) = (self.graph, self.rules_by_source.get(&file)) else {
            return false;
        };
        indexes.iter().any(|&index| {
            let rules = &self.rules[index];
            rules.credits(export_name) && forwards(graph, rules.entry, export_name, file)
        })
    }
}

/// Whether `entry` exports under `export_name` the declaration of `file`.
fn forwards(graph: &ModuleGraph, entry: FileId, export_name: &str, file: FileId) -> bool {
    [ExportNamespace::Value, ExportNamespace::Type]
        .into_iter()
        .any(|namespace| {
            graph
                .resolve_export_origin(entry, export_name, namespace)
                .is_some_and(|origin| origin.file_id() == file)
        })
}

fn is_plain_star(re_export: &crate::graph::ReExportEdge) -> bool {
    re_export.imported_name == "*" && re_export.exported_name == "*"
}

fn has_plain_star(module: &ModuleNode) -> bool {
    module.re_exports.iter().any(is_plain_star)
}

/// Every file that `entry` reaches through plain `export *` chains.
fn plain_star_sources(graph: &ModuleGraph, entry: &ModuleNode) -> FxHashSet<FileId> {
    let mut sources = FxHashSet::default();
    let mut stack = vec![entry];
    while let Some(module) = stack.pop() {
        for re_export in module.re_exports.iter().filter(|re| is_plain_star(re)) {
            if sources.insert(re_export.source_file)
                && let Some(source) = graph.modules.get(re_export.source_file.0 as usize)
            {
                stack.push(source);
            }
        }
    }
    sources
}
