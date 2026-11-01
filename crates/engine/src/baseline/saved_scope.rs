//! What a saved baseline file records about the run that saved it.

use super::{BaselineData, DuplicationBaselineData, HealthBaselineData};

impl BaselineData {
    /// Record the channels that narrowed the saving run.
    #[must_use]
    pub fn with_scope_reasons(mut self, reasons: fallow_output::BaselineScopeReasons) -> Self {
        self.scope_reasons = reasons
            .iter()
            .map(|reason| reason.as_str().to_owned())
            .collect();
        self
    }
}

impl DuplicationBaselineData {
    /// Record whether the saving run used production mode.
    #[must_use]
    pub fn with_production(mut self, production: bool) -> Self {
        self.scope_reasons = production_reason(production);
        self
    }
}

impl HealthBaselineData {
    /// Record whether the saving run used production mode.
    #[must_use]
    pub(crate) fn with_production(mut self, production: bool) -> Self {
        self.scope_reasons = production_reason(production);
        self
    }
}

/// The reasons that a health or duplication file records: production mode only,
/// because the prune guard reads that channel and no gate reads the file.
fn production_reason(production: bool) -> Vec<String> {
    if production {
        vec![fallow_output::ScopeReason::Production.as_str().to_owned()]
    } else {
        Vec::new()
    }
}

/// The non-empty finding categories of a dead-code baseline file, by field
/// name. Empty for a file that is not a JSON object.
#[must_use]
pub fn saved_finding_categories(json: &str) -> Vec<String> {
    macro_rules! field_names {
        ($($field:ident),* $(,)?) => {
            [$(stringify!($field)),*]
        };
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };
    with_baseline_fields!(field_names)
        .into_iter()
        .filter(|field| {
            value
                .get(field)
                .and_then(serde_json::Value::as_array)
                .is_some_and(|entries| !entries.is_empty())
        })
        .map(str::to_owned)
        .collect()
}

/// The `scope_reasons` a baseline file recorded when it was saved. Empty for a
/// whole-project save, a file saved before the file recorded its scope, or a
/// file that is not JSON.
#[must_use]
pub fn saved_scope_reasons(json: &str) -> Vec<String> {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|value| {
            value.get("scope_reasons")?.as_array().map(|reasons| {
                reasons
                    .iter()
                    .filter_map(serde_json::Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
        })
        .unwrap_or_default()
}
