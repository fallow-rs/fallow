/// Auditable breadcrumb recording when health-finding `suppress-line`
/// action hints were omitted from the report.
///
/// Set at construction time on `HealthReport::actions_meta` (and on
/// each `HealthGroup::actions_meta`
/// when grouped) by the report builder, derived from the active
/// `HealthActionContext`. Lets consumers see "where did the
/// suppress-line hints go?" without having to grep the config or CLI
/// history.
///
/// Stable `reason` codes:
/// - `baseline-active`: a baseline is active and inline ignores would
///   become dead annotations once the baseline regenerates.
/// - `config-disabled`: `health.suggestInlineSuppression` is `false`.
/// - `unspecified`: the caller did not record a reason.
#[derive(Debug, Clone, serde::Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct HealthActionsMeta {
    /// Always `true` when the breadcrumb is emitted. Absent from the wire when
    /// no suppression occurred.
    pub suppression_hints_omitted: bool,
    /// Stable code describing why the suppression occurred.
    pub reason: String,
    /// Scope of the omission. Always `"health-findings"` today.
    pub scope: String,
}
