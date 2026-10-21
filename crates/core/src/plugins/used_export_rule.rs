//! Used-export rules: the export names that a plugin marks as used in the
//! files that a path rule matches.

use super::{AggregatedPluginResult, CompiledPathRule, PathRule};

/// A used-export rule bound to a file-pattern rule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsedExportRule {
    pub(crate) path: PathRule,
    pub(crate) exports: Vec<String>,
}

impl UsedExportRule {
    #[must_use]
    pub(crate) fn new(
        pattern: impl Into<String>,
        exports: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            path: PathRule::new(pattern),
            exports: exports.into_iter().map(Into::into).collect(),
        }
    }

    #[must_use]
    pub(super) fn from_static(pattern: &'static str, exports: &'static [&'static str]) -> Self {
        Self::new(pattern, exports.iter().copied())
    }

    #[must_use]
    pub(super) fn with_excluded_globs<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.path = self.path.with_excluded_globs(patterns);
        self
    }

    #[must_use]
    pub(super) fn with_excluded_regexes<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.path = self.path.with_excluded_regexes(patterns);
        self
    }

    #[must_use]
    pub(super) fn with_excluded_segment_regexes<I, S>(mut self, patterns: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.path = self.path.with_excluded_segment_regexes(patterns);
        self
    }

    #[must_use]
    pub(super) fn prefixed(&self, ws_prefix: &str) -> Self {
        Self {
            path: self.path.prefixed(ws_prefix),
            exports: self.exports.clone(),
        }
    }
}

/// A used-export rule tagged with the plugin that contributed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginUsedExportRule {
    pub(crate) plugin_name: String,
    pub(crate) rule: UsedExportRule,
}

impl PluginUsedExportRule {
    #[must_use]
    pub(crate) fn new(plugin_name: impl Into<String>, rule: UsedExportRule) -> Self {
        Self {
            plugin_name: plugin_name.into(),
            rule,
        }
    }

    #[must_use]
    pub(super) fn prefixed(&self, ws_prefix: &str) -> Self {
        Self {
            plugin_name: self.plugin_name.clone(),
            rule: self.rule.prefixed(ws_prefix),
        }
    }
}

/// The export credit for the config files of active plugins.
///
/// A tool loads its config file and reads some of its exports. Under
/// `includeEntryExports`, these exports count as used. The rules come from the
/// config patterns of each active plugin (the `default` export) and from the
/// anchored `config_file_exports` of each plugin (the export names that the
/// tool reads, or `*` for every export).
///
/// The credit has its own matcher set. Without `includeEntryExports`, fallow
/// skips an entry point as a whole, and the plugin `used_exports` matchers
/// decide whether to analyse it. This credit never feeds that decision, so
/// output without `includeEntryExports` does not change.
pub struct ConfigExportCredit<'a> {
    matchers: Vec<(CompiledPathRule, &'a [String])>,
}

impl<'a> ConfigExportCredit<'a> {
    /// Compile the credit. It is empty unless `include_entry_exports` is on.
    pub fn new(
        plugin_result: Option<&'a AggregatedPluginResult>,
        include_entry_exports: bool,
    ) -> Self {
        let matchers = plugin_result
            .filter(|_| include_entry_exports)
            .map(|pr| {
                pr.config_export_credits
                    .iter()
                    .filter_map(|rule| {
                        CompiledPathRule::for_used_export_rule(
                            &rule.rule.path,
                            "config export credit pattern",
                        )
                        .map(|matcher| (matcher, rule.rule.exports.as_slice()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Self { matchers }
    }

    /// Whether no config pattern gets the credit.
    pub fn is_empty(&self) -> bool {
        self.matchers.is_empty()
    }

    /// The credited export names of each rule that matches the
    /// project-relative `path`.
    pub fn exports_for(&self, path: &str) -> Vec<&'a [String]> {
        self.matchers
            .iter()
            .filter(|(matcher, _)| matcher.matches(path))
            .map(|(_, exports)| *exports)
            .collect()
    }

    /// Whether the export lists from [`Self::exports_for`] credit
    /// `export_name`. The comparison is by name, so
    /// `export { config as default }` matches `default`.
    pub fn credits(credited: &[&[String]], export_name: &str) -> bool {
        credited
            .iter()
            .any(|exports| exports.iter().any(|e| e == "*" || e == export_name))
    }
}
