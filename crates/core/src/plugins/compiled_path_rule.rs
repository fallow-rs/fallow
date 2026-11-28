//! Compiled matchers of plugin path rules.

use std::sync::Arc;

use regex::Regex;
use rustc_hash::FxHashMap;

use super::{PathRule, entry_rule_glob};

/// The compiled excludes of a path rule. Plugins such as Jasmine give the
/// same exclude list to each of their include rules, so rules with the same
/// lists share one compiled copy.
#[derive(Debug)]
struct CompiledExcludes {
    globs: Vec<fallow_config::PathMatcher>,
    regexes: Vec<Regex>,
    segment_regexes: Vec<Regex>,
}

/// The exclude lists of a path rule: globs, path regexes and segment
/// regexes.
type ExcludeKey = (Vec<String>, Vec<String>, Vec<String>);

/// Compiled exclude lists, by their text. One cache serves the rules of one
/// compile pass, so each distinct exclude list compiles once.
#[derive(Default)]
pub struct ExcludeCache {
    compiled: FxHashMap<ExcludeKey, Arc<CompiledExcludes>>,
}

impl ExcludeCache {
    fn excludes_of(&mut self, rule: &PathRule, rule_kind: &str) -> Arc<CompiledExcludes> {
        let key = (
            rule.exclude_globs.clone(),
            rule.exclude_regexes.clone(),
            rule.exclude_segment_regexes.clone(),
        );
        if let Some(compiled) = self.compiled.get(&key) {
            return Arc::clone(compiled);
        }
        let compiled = Arc::new(CompiledExcludes {
            globs: compile_excluded_globs(&rule.exclude_globs, rule_kind, &rule.pattern),
            regexes: compile_excluded_regexes(&rule.exclude_regexes, rule_kind, &rule.pattern),
            segment_regexes: compile_excluded_segment_regexes(
                &rule.exclude_segment_regexes,
                rule_kind,
                &rule.pattern,
            ),
        });
        self.compiled.insert(key, Arc::clone(&compiled));
        compiled
    }
}

/// A compiled path rule matcher shared by entry-point and used-export matching.
///
/// Every glob accepts an empty alternative, such as `{,m}js`, because the
/// rewrite of an extglob `?(m)` group gives one.
#[derive(Debug, Clone)]
pub struct CompiledPathRule {
    include: fallow_config::PathMatcher,
    excludes: Arc<CompiledExcludes>,
}

impl CompiledPathRule {
    pub fn for_entry_rule(rule: &PathRule, rule_kind: &str) -> Option<Self> {
        Self::for_entry_rule_cached(rule, rule_kind, &mut ExcludeCache::default())
    }

    /// [`Self::for_entry_rule`] with the excludes from `cache`.
    pub fn for_entry_rule_cached(
        rule: &PathRule,
        rule_kind: &str,
        cache: &mut ExcludeCache,
    ) -> Option<Self> {
        let compiled = entry_rule_glob(&rule.pattern).map_err(|err| err.to_string());
        let include = match compiled.and_then(|glob| {
            fallow_config::try_compile_matcher(&glob).map_err(|err| err.to_string())
        }) {
            Ok(matcher) => matcher,
            Err(err) => {
                tracing::warn!("invalid {rule_kind} '{}': {err}", rule.pattern);
                return None;
            }
        };
        Some(Self {
            include,
            excludes: cache.excludes_of(rule, rule_kind),
        })
    }

    pub fn for_used_export_rule(rule: &PathRule, rule_kind: &str) -> Option<Self> {
        Self::for_entry_rule(rule, rule_kind)
    }

    #[must_use]
    pub fn matches(&self, path: &str) -> bool {
        let excludes = &self.excludes;
        self.include.is_match(path)
            && !excludes.globs.iter().any(|glob| glob.is_match(path))
            && !excludes.regexes.iter().any(|regex| regex.is_match(path))
            && !matches_segment_regex(path, &excludes.segment_regexes)
    }
}

fn compile_excluded_globs(
    patterns: &[String],
    rule_kind: &str,
    rule_pattern: &str,
) -> Vec<fallow_config::PathMatcher> {
    patterns
        .iter()
        .filter_map(|pattern| {
            match globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .empty_alternates(true)
                .build()
                .map_err(|err| err.to_string())
                .and_then(|glob| {
                    fallow_config::try_compile_matcher(&glob).map_err(|err| err.to_string())
                }) {
                Ok(matcher) => Some(matcher),
                Err(err) => {
                    tracing::warn!(
                        "skipping invalid excluded glob '{}' for {} '{}': {err}",
                        pattern,
                        rule_kind,
                        rule_pattern
                    );
                    None
                }
            }
        })
        .collect()
}

fn compile_excluded_regexes(
    patterns: &[String],
    rule_kind: &str,
    rule_pattern: &str,
) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(
            |pattern| match fallow_config::compile_config_regex(pattern) {
                Ok(regex) => Some(regex),
                Err(err) => {
                    tracing::warn!(
                        "skipping invalid excluded regex '{}' for {} '{}': {err}",
                        pattern,
                        rule_kind,
                        rule_pattern
                    );
                    None
                }
            },
        )
        .collect()
}

fn compile_excluded_segment_regexes(
    patterns: &[String],
    rule_kind: &str,
    rule_pattern: &str,
) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(
            |pattern| match fallow_config::compile_config_regex(pattern) {
                Ok(regex) => Some(regex),
                Err(err) => {
                    tracing::warn!(
                        "skipping invalid excluded segment regex '{}' for {} '{}': {err}",
                        pattern,
                        rule_kind,
                        rule_pattern
                    );
                    None
                }
            },
        )
        .collect()
}

/// Whether a segment of `path` matches one of `regexes`. The path can use the
/// native separator: on Windows a caller can pass `src\pages\x.tsx`.
pub(super) fn matches_segment_regex(path: &str, regexes: &[Regex]) -> bool {
    path.split(std::path::is_separator)
        .any(|segment| regexes.iter().any(|regex| regex.is_match(segment)))
}
