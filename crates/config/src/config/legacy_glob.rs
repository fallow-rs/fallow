//! Migration check for the single-segment `*` rule in config path globs.
//!
//! Before this rule, a single `*` or `?` in a config path glob also matched
//! `/`. Now only `**` crosses directories (see [`super::compile_path_glob`]).
//! A config that used a single `*` to match nested files silently matches
//! fewer files. This module finds those patterns, so source discovery can
//! print one warning for each pattern that lost matches on the discovered
//! files.

use std::path::Path;

use globset::{Glob, GlobMatcher, GlobSet, GlobSetBuilder};

use super::FallowConfig;
use super::glob_validation::compile_path_glob;
use crate::external_plugin::{ExternalPluginDef, PluginDetection};
use crate::rule_pack::RulePackDef;

/// The user path globs of one config list, for example one `overrides[]`
/// entry, where at least one pattern has a single-segment wildcard.
///
/// The other patterns of the same list are kept: a file that a sibling
/// pattern still matches did not lose its match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyGlobGroup {
    /// Config field path, for example `health.ignore`.
    pub field: &'static str,
    /// Text placed before each pattern in the warning: `!` for an exception.
    pub prefix: &'static str,
    /// Every pattern of the list, as the user wrote it, without the prefix.
    pub patterns: Vec<String>,
}

/// A config pattern that matches fewer discovered files under the
/// single-segment rule than under the old rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LegacyGlobNarrowing {
    /// Config field path, for example `health.ignore`.
    pub field: &'static str,
    /// The pattern as the user wrote it, with its `!` prefix.
    pub pattern: String,
    /// Count of discovered files that only the old rule matched.
    pub legacy_only_files: usize,
    /// A rewrite with `**` that keeps the old match.
    pub suggestion: String,
}

impl LegacyGlobNarrowing {
    /// The stderr warning for this pattern.
    #[must_use]
    pub fn message(&self) -> String {
        let noun = if self.legacy_only_files == 1 {
            "file"
        } else {
            "files"
        };
        format!(
            "fallow: {} glob \"{}\" no longer matches {} {noun}, because \"*\" and \"?\" no \
             longer match \"/\". To keep the old match, use \"{}\".",
            self.field, self.pattern, self.legacy_only_files, self.suggestion
        )
    }
}

fn normalize(pattern: &str) -> &str {
    pattern.strip_prefix("./").unwrap_or(pattern)
}

fn has_wildcard(segment: &str) -> bool {
    segment != "**" && segment.contains(['*', '?'])
}

/// True when `pattern` has a `*` or `?` that the old rule let match `/`.
///
/// A wildcard segment directly after a `**` segment matches the same files
/// under both rules, so it does not count.
#[must_use]
pub fn has_single_segment_wildcard(pattern: &str) -> bool {
    let mut after_globstar = false;
    for segment in normalize(pattern).split('/') {
        if has_wildcard(segment) && !after_globstar {
            return true;
        }
        after_globstar = segment == "**";
    }
    false
}

/// Rewrite `pattern` with `**` so that it matches nested files again.
///
/// A bare `*` segment becomes `**` (`src/x/*` to `src/x/**`). Another
/// wildcard segment gets a `**/` in front (`*.test.ts` to `**/*.test.ts`).
#[must_use]
pub fn suggest_cross_directory_rewrite(pattern: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut after_globstar = false;
    for segment in pattern.split('/') {
        if segment == "**" {
            if !after_globstar {
                out.push("**");
            }
            after_globstar = true;
            continue;
        }
        if has_wildcard(segment) && !after_globstar {
            out.push("**");
            if segment == "*" {
                after_globstar = true;
                continue;
            }
        }
        out.push(segment);
        after_globstar = false;
    }
    out.join("/")
}

#[derive(Default)]
struct GroupCollector {
    groups: Vec<LegacyGlobGroup>,
}

impl GroupCollector {
    fn push<'a>(
        &mut self,
        field: &'static str,
        prefix: &'static str,
        patterns: impl IntoIterator<Item = &'a str>,
    ) {
        let patterns: Vec<String> = patterns.into_iter().map(str::to_owned).collect();
        if patterns.iter().any(|p| has_single_segment_wildcard(p)) {
            self.groups.push(LegacyGlobGroup {
                field,
                prefix,
                patterns,
            });
        }
    }

    fn push_list(&mut self, field: &'static str, patterns: &[String]) {
        self.push(field, "", patterns.iter().map(String::as_str));
    }

    fn push_detection(&mut self, detection: &PluginDetection) {
        match detection {
            PluginDetection::Dependency { .. } => {}
            PluginDetection::FileExists { pattern } => {
                self.push("framework[].detection.pattern", "", [pattern.as_str()]);
            }
            PluginDetection::All { conditions } | PluginDetection::Any { conditions } => {
                for condition in conditions {
                    self.push_detection(condition);
                }
            }
        }
    }

    fn push_config(&mut self, config: &FallowConfig) {
        self.push_list("entry", &config.entry);
        self.push_list("dynamicallyLoaded", &config.dynamically_loaded);
        let positive = |p: &&String| !p.starts_with('!');
        self.push(
            "ignorePatterns",
            "",
            config
                .ignore_patterns
                .iter()
                .filter(positive)
                .map(String::as_str),
        );
        self.push(
            "ignoreFindings",
            "",
            config
                .ignore_findings
                .iter()
                .filter(positive)
                .map(String::as_str),
        );
        self.push(
            "ignoreFindings",
            "!",
            config
                .ignore_findings
                .iter()
                .filter_map(|p| p.strip_prefix('!')),
        );
        for rule in &config.ignore_exports {
            self.push("ignoreExports[].file", "", [rule.file.as_str()]);
        }
        for rule in &config.ignore_catalog_references {
            if let Some(consumer) = &rule.consumer {
                self.push(
                    "ignoreCatalogReferences[].consumer",
                    "",
                    [consumer.as_str()],
                );
            }
        }
        for entry in &config.overrides {
            self.push_list("overrides[].files", &entry.files);
        }
        self.push_list("duplicates.ignore", &config.duplicates.ignore);
        self.push_list("similarCode.ignore", &config.similar_code.ignore);
        self.push_list("health.ignore", &config.health.ignore);
        for entry in &config.health.threshold_overrides {
            self.push_list("health.thresholdOverrides[].files", &entry.files);
        }
        for zone in &config.boundaries.zones {
            self.push_list("boundaries.zones[].patterns", &zone.patterns);
        }
        self.push_list(
            "boundaries.coverage.allowUnmatched",
            &config.boundaries.coverage.allow_unmatched,
        );
    }

    fn push_plugin(&mut self, plugin: &ExternalPluginDef) {
        for used in &plugin.used_exports {
            self.push(
                "framework[].usedExports[].pattern",
                "",
                [used.pattern.as_str()],
            );
        }
        for rule in &plugin.manifest_entries {
            self.push(
                "framework[].manifestEntries[].manifests",
                "",
                [rule.manifests.as_str()],
            );
        }
        if let Some(detection) = &plugin.detection {
            self.push_detection(detection);
        }
    }

    fn push_rule_pack(&mut self, pack: &RulePackDef) {
        for rule in &pack.rules {
            self.push_list("rulePacks rules[].files", &rule.files);
            self.push_list("rulePacks rules[].exclude", &rule.exclude);
            self.push_list("rulePacks rules[].allowedFiles", &rule.allowed_files);
        }
    }
}

/// Collect every user config path glob list that has a single-segment
/// wildcard, from the config file and from the loaded plugins and rule packs.
///
/// `ignorePatterns` exceptions (`!`) are left out: a file that an exception
/// no longer lifts is not discovered, so source discovery cannot count it.
pub(super) fn collect_config_legacy_glob_groups(config: &FallowConfig) -> Vec<LegacyGlobGroup> {
    let mut collector = GroupCollector::default();
    collector.push_config(config);
    collector.groups
}

/// Add the plugin and rule-pack glob lists to `groups`.
pub(super) fn extend_legacy_glob_groups(
    groups: &mut Vec<LegacyGlobGroup>,
    plugins: &[ExternalPluginDef],
    rule_packs: &[RulePackDef],
) {
    let mut collector = GroupCollector {
        groups: std::mem::take(groups),
    };
    for plugin in plugins {
        collector.push_plugin(plugin);
    }
    for pack in rule_packs {
        collector.push_rule_pack(pack);
    }
    *groups = collector.groups;
}

struct CompiledGroup<'a> {
    group: &'a LegacyGlobGroup,
    strict: GlobSet,
    /// Index into `group.patterns`, and the old-rule matcher of that pattern.
    legacy: Vec<(usize, GlobMatcher)>,
    counts: Vec<usize>,
}

impl CompiledGroup<'_> {
    fn record(&mut self, path: &Path) {
        let mut strict_match: Option<bool> = None;
        for (slot, (_, matcher)) in self.legacy.iter().enumerate() {
            if !matcher.is_match(path) {
                continue;
            }
            if !*strict_match.get_or_insert_with(|| self.strict.is_match(path)) {
                self.counts[slot] += 1;
            }
        }
    }
}

fn compile_group(group: &LegacyGlobGroup) -> Option<CompiledGroup<'_>> {
    let mut strict = GlobSetBuilder::new();
    let mut legacy = Vec::new();
    for (idx, pattern) in group.patterns.iter().enumerate() {
        let normalized = normalize(pattern);
        if let Ok(glob) = compile_path_glob(normalized) {
            strict.add(glob);
        }
        if has_single_segment_wildcard(normalized)
            && let Ok(glob) = Glob::new(normalized)
        {
            legacy.push((idx, glob.compile_matcher()));
        }
    }
    if legacy.is_empty() {
        return None;
    }
    let counts = vec![0; legacy.len()];
    Some(CompiledGroup {
        group,
        strict: strict.build().ok()?,
        legacy,
        counts,
    })
}

/// Compare the old and the new glob rule on the discovered files.
///
/// `relative_paths` are project-root-relative. The result holds one entry
/// for each pattern where the old rule matched a file that no pattern of the
/// same list matches under the new rule, in config order.
#[must_use]
pub fn find_legacy_glob_narrowings(
    groups: &[LegacyGlobGroup],
    relative_paths: &[&Path],
) -> Vec<LegacyGlobNarrowing> {
    let mut compiled: Vec<CompiledGroup<'_>> = groups.iter().filter_map(compile_group).collect();
    if compiled.is_empty() {
        return Vec::new();
    }
    for path in relative_paths {
        for group in &mut compiled {
            group.record(path);
        }
    }
    let mut out = Vec::new();
    for group in compiled {
        let LegacyGlobGroup {
            field,
            prefix,
            patterns,
        } = group.group;
        for ((idx, _), count) in group.legacy.iter().zip(group.counts) {
            if count == 0 {
                continue;
            }
            let pattern = &patterns[*idx];
            out.push(LegacyGlobNarrowing {
                field,
                pattern: format!("{prefix}{pattern}"),
                legacy_only_files: count,
                suggestion: format!("{prefix}{}", suggest_cross_directory_rewrite(pattern)),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_single_segment_wildcards() {
        assert!(has_single_segment_wildcard("*"));
        assert!(has_single_segment_wildcard("*.test.ts"));
        assert!(has_single_segment_wildcard("src/x/*"));
        assert!(has_single_segment_wildcard("./src/x/*"));
        assert!(has_single_segment_wildcard("apps/*/docs/**"));
        assert!(has_single_segment_wildcard("src/?.ts"));
        assert!(has_single_segment_wildcard("src/**/foo/*.ts"));
        assert!(!has_single_segment_wildcard("**/*.test.ts"));
        assert!(!has_single_segment_wildcard("src/**/*.ts"));
        assert!(!has_single_segment_wildcard("src/generated/**"));
        assert!(!has_single_segment_wildcard("src/index.ts"));
    }

    #[test]
    fn suggests_globstar_rewrites() {
        assert_eq!(suggest_cross_directory_rewrite("src/x/*"), "src/x/**");
        assert_eq!(suggest_cross_directory_rewrite("*.test.ts"), "**/*.test.ts");
        assert_eq!(suggest_cross_directory_rewrite("*"), "**");
        assert_eq!(
            suggest_cross_directory_rewrite("apps/*/docs/**"),
            "apps/**/docs/**"
        );
        assert_eq!(
            suggest_cross_directory_rewrite("packages/cli/src/commands/*.ts"),
            "packages/cli/src/commands/**/*.ts"
        );
        assert_eq!(suggest_cross_directory_rewrite("*/*.ts"), "**/*.ts");
    }

    fn group(field: &'static str, patterns: &[&str]) -> LegacyGlobGroup {
        LegacyGlobGroup {
            field,
            prefix: "",
            patterns: patterns.iter().map(|p| (*p).to_owned()).collect(),
        }
    }

    fn paths<'a>(raw: &[&'a str]) -> Vec<&'a Path> {
        raw.iter().map(|p| Path::new(*p)).collect()
    }

    #[test]
    fn counts_files_only_the_old_rule_matched() {
        let files = paths(&[
            "a.test.ts",
            "src/b.test.ts",
            "src/deep/c.test.ts",
            "src/d.ts",
        ]);
        let found = find_legacy_glob_narrowings(&[group("health.ignore", &["*.test.ts"])], &files);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].legacy_only_files, 2);
        assert_eq!(found[0].suggestion, "**/*.test.ts");
        assert_eq!(
            found[0].message(),
            "fallow: health.ignore glob \"*.test.ts\" no longer matches 2 files, because \"*\" \
             and \"?\" no longer match \"/\". To keep the old match, use \"**/*.test.ts\"."
        );
    }

    #[test]
    fn a_sibling_pattern_that_still_matches_suppresses_the_count() {
        let files = paths(&["src/deep/c.test.ts"]);
        let found = find_legacy_glob_narrowings(
            &[group("health.ignore", &["*.test.ts", "src/**/*.test.ts"])],
            &files,
        );
        assert!(found.is_empty());
    }

    #[test]
    fn no_narrowing_when_the_match_set_is_the_same() {
        let files = paths(&["a.test.ts", "src/b.ts"]);
        let found = find_legacy_glob_narrowings(&[group("health.ignore", &["*.test.ts"])], &files);
        assert!(found.is_empty());
    }

    #[test]
    fn exception_patterns_keep_their_prefix() {
        let files = paths(&["src/deep/c.ts"]);
        let mut exceptions = group("ignoreFindings", &["src/*"]);
        exceptions.prefix = "!";
        let found = find_legacy_glob_narrowings(&[exceptions], &files);
        assert_eq!(found[0].pattern, "!src/*");
        assert_eq!(found[0].suggestion, "!src/**");
    }
}
