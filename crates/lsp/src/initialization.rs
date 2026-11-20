use std::collections::BTreeMap;

use fallow_config::{DetectionMode, DupesInputs};
use serde::Deserialize;

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LspHealthOptions {
    pub inline_complexity: Option<bool>,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LspTypeAwareOptions {
    pub enabled: bool,
    pub projects: Vec<String>,
    pub require: Option<fallow_config::TypeAwareRequire>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LspInitializationOptions {
    pub config_path: Option<String>,
    pub allow_remote_extends: bool,
    pub issue_types: Option<BTreeMap<String, bool>>,
    pub muted_categories: Option<Vec<String>>,
    pub changed_since: Option<String>,
    /// `false` ignores `workspaces.changedSince`, as `--no-package-baselines`
    /// does on the CLI. Absent keeps the map.
    pub package_baselines: Option<bool>,
    pub duplication: Option<LspDuplicationOptions>,
    pub production: Option<bool>,
    pub health: Option<LspHealthOptions>,
    pub type_aware: Option<LspTypeAwareOptions>,
    /// Parse the project at `initialized`, before the first open, without a
    /// publish. Off by default.
    pub prewarm: bool,
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
        muted_categories: obj.get("mutedCategories").and_then(|value| {
            let categories = value
                .as_array()?
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>();
            (!categories.is_empty()).then_some(categories)
        }),
        changed_since: obj
            .get("changedSince")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        package_baselines: obj
            .get("packageBaselines")
            .and_then(serde_json::Value::as_bool),
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
        type_aware: obj
            .get("typeAware")
            .and_then(|value| serde_json::from_value(value.clone()).ok()),
        prewarm: obj
            .get("prewarm")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    }
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LspDuplicationOptions {
    pub mode: Option<DetectionMode>,
    pub near: Option<bool>,
    pub threshold: Option<f64>,
    pub min_tokens: Option<usize>,
    pub min_lines: Option<usize>,
    pub min_occurrences: Option<usize>,
    pub skip_local: Option<bool>,
    pub cross_language: Option<bool>,
    pub ignore_imports: Option<bool>,
    pub ignore_symlinks: Option<bool>,
}

impl LspDuplicationOptions {
    /// The duplication inputs of the editor. A `minOccurrences` below 2 is
    /// not an input, so the config value stays.
    pub fn inputs(&self) -> DupesInputs {
        DupesInputs {
            mode: self.mode,
            near: self.near,
            min_tokens: self.min_tokens,
            min_lines: self.min_lines,
            min_occurrences: self.min_occurrences.filter(|min| *min >= 2),
            threshold: self.threshold,
            skip_local: self.skip_local,
            cross_language: self.cross_language,
            ignore_imports: self.ignore_imports,
            ignore_symlinks: self.ignore_symlinks,
        }
    }
}

/// The initialization options that the client sends but that do not parse,
/// by name. The server then runs without them, so it names each one in a
/// warning.
#[must_use]
pub fn invalid_initialization_options(opts: Option<&serde_json::Value>) -> Vec<&'static str> {
    let Some(obj) = opts.and_then(serde_json::Value::as_object) else {
        return Vec::new();
    };
    let invalid = |name: &'static str, parses: fn(serde_json::Value) -> bool| {
        obj.get(name)
            .filter(|value| !value.is_null() && !parses((*value).clone()))
            .map(|_| name)
    };
    [
        invalid("duplication", |value| {
            serde_json::from_value::<LspDuplicationOptions>(value).is_ok()
        }),
        invalid("typeAware", |value| {
            serde_json::from_value::<LspTypeAwareOptions>(value).is_ok()
        }),
    ]
    .into_iter()
    .flatten()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_option_that_does_not_parse_is_named() {
        let opts = serde_json::json!({
            "duplication": { "near": "yes" },
            "typeAware": { "enabled": true, "projects": [] },
        });
        assert_eq!(
            invalid_initialization_options(Some(&opts)),
            vec!["duplication"]
        );
        let valid = serde_json::json!({ "duplication": { "near": false }, "typeAware": null });
        assert!(invalid_initialization_options(Some(&valid)).is_empty());
    }
}
