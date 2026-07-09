use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fallow_config::{DetectionMode, DuplicatesConfig};
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LspHealthOptions {
    pub inline_complexity: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LspInitializationOptions {
    pub config_path: Option<String>,
    pub allow_remote_extends: bool,
    pub issue_types: Option<BTreeMap<String, bool>>,
    pub changed_since: Option<String>,
    pub duplication: Option<LspDuplicationOptions>,
    pub production: Option<bool>,
    pub health: Option<LspHealthOptions>,
}

#[must_use]
pub fn parse_initialization_options(opts: Option<&serde_json::Value>) -> LspInitializationOptions {
    let Some(obj) = opts.and_then(serde_json::Value::as_object) else {
        return LspInitializationOptions::default();
    };

    LspInitializationOptions {
        config_path: obj
            .get("configPath")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        allow_remote_extends: obj
            .get("allowRemoteExtends")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
        issue_types: obj.get("issueTypes").and_then(|value| {
            let issue_types = value
                .as_object()?
                .iter()
                .filter_map(|(key, value)| value.as_bool().map(|enabled| (key.clone(), enabled)))
                .collect::<BTreeMap<_, _>>();
            (!issue_types.is_empty()).then_some(issue_types)
        }),
        changed_since: obj
            .get("changedSince")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        duplication: obj
            .get("duplication")
            .and_then(|value| serde_json::from_value(value.clone()).ok()),
        production: obj.get("production").and_then(serde_json::Value::as_bool),
        health: obj.get("health").and_then(|value| {
            let health = value.as_object()?;
            Some(LspHealthOptions {
                inline_complexity: health
                    .get("inlineComplexity")
                    .and_then(serde_json::Value::as_bool),
            })
        }),
    }
}

fn resolve_config_path(raw: Option<&str>, root: Option<&Path>) -> Option<PathBuf> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }

    let path = PathBuf::from(raw);
    let path = if path.is_absolute() {
        path
    } else if let Some(root) = root {
        root.join(path)
    } else {
        path
    };

    Some(path.canonicalize().unwrap_or(path))
}

pub fn initialization_config_path(
    opts: &serde_json::Value,
    root: Option<&Path>,
) -> Option<PathBuf> {
    let parsed = parse_initialization_options(Some(opts));
    resolve_config_path(parsed.config_path.as_deref(), root)
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LspDuplicationOptions {
    pub mode: Option<DetectionMode>,
    pub threshold: Option<f64>,
    pub min_tokens: Option<usize>,
    pub min_lines: Option<usize>,
    pub min_occurrences: Option<usize>,
    pub skip_local: Option<bool>,
    pub cross_language: Option<bool>,
    pub ignore_imports: Option<bool>,
}

impl LspDuplicationOptions {
    pub fn merge_with(&self, config: &DuplicatesConfig) -> DuplicatesConfig {
        DuplicatesConfig {
            enabled: config.enabled,
            mode: self.mode.unwrap_or(config.mode),
            min_tokens: self.min_tokens.unwrap_or(config.min_tokens),
            min_lines: self.min_lines.unwrap_or(config.min_lines),
            min_occurrences: self
                .min_occurrences
                .filter(|min| *min >= 2)
                .unwrap_or(config.min_occurrences),
            threshold: self.threshold.unwrap_or(config.threshold),
            ignore: config.ignore.clone(),
            ignore_defaults: config.ignore_defaults,
            skip_local: self.skip_local.unwrap_or(config.skip_local),
            cross_language: self.cross_language.unwrap_or(config.cross_language),
            ignore_imports: self.ignore_imports.unwrap_or(config.ignore_imports),
            normalization: config.normalization.clone(),
            min_corpus_size_for_shingle_filter: config.min_corpus_size_for_shingle_filter,
            min_corpus_size_for_token_cache: config.min_corpus_size_for_token_cache,
        }
    }
}

#[cfg(test)]
pub fn initialization_duplication_options(
    opts: &serde_json::Value,
) -> Option<LspDuplicationOptions> {
    parse_initialization_options(Some(opts)).duplication
}

/// Read the optional production-mode override from `initializationOptions`.
/// `Some(true)`/`Some(false)` force production on/off; a missing or non-boolean
/// `production` key yields `None`, deferring to the project config (issue
/// #1055). VS Code omits the key for the `"auto"` setting state.
#[cfg(test)]
pub fn initialization_production_override(opts: &serde_json::Value) -> Option<bool> {
    parse_initialization_options(Some(opts)).production
}

#[cfg(test)]
pub fn initialization_inline_complexity_enabled(opts: &serde_json::Value) -> bool {
    parse_initialization_options(Some(opts))
        .health
        .and_then(|health| health.inline_complexity)
        .unwrap_or(false)
}
