use std::path::{Path, PathBuf};

use fallow_config::{DetectionMode, DuplicatesConfig};
use serde::Deserialize;

pub fn initialization_config_path(
    opts: &serde_json::Value,
    root: Option<&Path>,
) -> Option<PathBuf> {
    let raw = opts.get("configPath").and_then(|v| v.as_str())?.trim();
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

pub fn initialization_duplication_options(
    opts: &serde_json::Value,
) -> Option<LspDuplicationOptions> {
    serde_json::from_value(opts.get("duplication")?.clone()).ok()
}

/// Read the optional production-mode override from `initializationOptions`.
/// `Some(true)`/`Some(false)` force production on/off; a missing or non-boolean
/// `production` key yields `None`, deferring to the project config (issue
/// #1055). VS Code omits the key for the `"auto"` setting state.
pub fn initialization_production_override(opts: &serde_json::Value) -> Option<bool> {
    opts.get("production").and_then(serde_json::Value::as_bool)
}

pub fn initialization_inline_complexity_enabled(opts: &serde_json::Value) -> bool {
    opts.get("health")
        .and_then(|health| health.get("inlineComplexity"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false)
}
