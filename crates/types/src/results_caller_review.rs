//! Findings that list the inspected callers: the opt-in
//! `absent-component-prop` and `constant-parameter` rules.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::envelope::AuditIntroduced;
use crate::extract::LiteralValue;
use crate::output::{FixActionType, IssueAction};
use crate::output_dead_code::{
    EffectiveSeverity, GatedFinding, deserialize_effective_severity, manual_framework_fix,
    suppress_line,
};
use crate::serde_path;

/// One inspected reachable component invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ComponentPropCallSite {
    /// Caller source path.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based opening-tag line.
    pub line: u32,
    /// 0-based original-source byte column.
    pub col: u32,
}

/// A used optional input that no inspected reachable caller supplies.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AbsentComponentProp {
    /// Source path of the input declaration.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// Semantic declaration name, or SFC filename stem.
    pub component_name: String,
    /// Public framework token.
    pub framework: String,
    /// Public optional input name.
    pub prop_name: String,
    /// 1-based declaration line.
    pub line: u32,
    /// 0-based declaration byte column.
    pub col: u32,
    /// Whether omission has a declared default.
    pub has_default: bool,
    /// Every inspected reachable caller, deterministically ordered.
    pub inspected_call_sites: Vec<ComponentPropCallSite>,
    /// Manual-review meaning and limits of static evidence.
    pub explanation: String,
}

/// Wire-shape envelope for an [`AbsentComponentProp`] finding. There is no safe
/// auto-fix: removing a declared prop is judgement-bearing (the prop may be part
/// of a deliberately-stable public component API). Actions are manual
/// remediation guidance plus a line-level suppress at the prop declaration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AbsentComponentPropFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub prop: AbsentComponentProp,
    /// Stable id of this finding: `dc1:<rule>:<16 hex digits>`, with a
    /// `~<k>` suffix when several findings of one type share an identity.
    /// Line and column are not inputs, so the id survives line shifts,
    /// reformats and reorders. A rename of the file or the symbol gives a
    /// new id. Absent in output from older versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finding_id: Option<String>,
    /// Suggested next steps. Always emitted (possibly empty for
    /// forward-compat).
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
    /// Gate severity of this finding after `rules` and `overrides[].rules`
    /// resolve for its path. CI formats read it for the annotation, SARIF
    /// and CodeClimate level. Absent in output from older versions. Not
    /// part of the finding identity, baseline keys or fingerprints.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_effective_severity"
    )]
    pub effective_severity: Option<EffectiveSeverity>,
}

impl AbsentComponentPropFinding {
    /// Build the wrapper from a raw [`AbsentComponentProp`]. Emits a manual
    /// fix action plus a line-level suppress.
    #[must_use]
    pub fn with_actions(prop: AbsentComponentProp) -> Self {
        let actions = vec![
            manual_framework_fix(
                FixActionType::ReviewComponentProp,
                "Review inspected callers, defaults and API intent",
                "Retain or suppress intentional stable props, or update the component manually; static analysis does not prove runtime unreachability.",
            ),
            suppress_line("// fallow-ignore-next-line absent-component-prop"),
        ];
        Self {
            finding_id: None,
            prop,
            actions,
            introduced: None,
            effective_severity: None,
        }
    }
}

/// The most call sites that one finding lists. `call_site_count` holds the
/// full count.
pub const MAX_INSPECTED_CALL_SITES: usize = 20;

/// Why the rule reports a parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ConstantParameterReason {
    /// Each call passes the same literal value for the parameter.
    IdenticalArgument,
    /// The parameter is optional or has a default, and each call omits it.
    NeverPassed,
    /// Reserved for a later version: the calls pass only some values of a
    /// closed set. Not emitted in this version.
    UnreachedVariant,
}

impl ConstantParameterReason {
    /// The kebab-case wire token.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IdenticalArgument => "identical-argument",
            Self::NeverPassed => "never-passed",
            Self::UnreachedVariant => "unreached-variant",
        }
    }
}

/// The type of a [`ConstantParameterValue`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum ConstantParameterValueKind {
    /// `true` or `false`.
    Boolean,
    /// `null`.
    Null,
    /// `undefined`.
    Undefined,
    /// A number.
    Number,
    /// A string.
    String,
}

/// The literal value that each call passes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ConstantParameterValue {
    /// The type of the value.
    pub kind: ConstantParameterValueKind,
    /// The value as text. A number uses the JavaScript `String(n)` form,
    /// except that negative zero is `-0`. A string is the raw value with no
    /// quotes. `null`, `undefined`, `true` and `false` are the keyword.
    pub text: String,
}

impl ConstantParameterValue {
    /// The wire value of a literal argument fact.
    #[must_use]
    pub fn from_literal(value: &LiteralValue) -> Self {
        let (kind, text) = match value {
            LiteralValue::Boolean(value) => {
                (ConstantParameterValueKind::Boolean, value.to_string())
            }
            LiteralValue::Null => (ConstantParameterValueKind::Null, "null".to_string()),
            LiteralValue::Undefined => (
                ConstantParameterValueKind::Undefined,
                "undefined".to_string(),
            ),
            LiteralValue::Number(bits) => (
                ConstantParameterValueKind::Number,
                js_number_text(f64::from_bits(*bits)),
            ),
            LiteralValue::String(value) => (ConstantParameterValueKind::String, value.to_string()),
        };
        Self { kind, text }
    }
}

/// The JavaScript `Number#toString()` text of a number, with `-0` for
/// negative zero.
#[must_use]
pub fn js_number_text(value: f64) -> String {
    if value.is_nan() {
        return "NaN".to_string();
    }
    if value.is_infinite() {
        return if value > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if value == 0.0 {
        return if value.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    let sign = if value < 0.0 { "-" } else { "" };
    // `{:e}` gives the shortest digits that round-trip, as JavaScript does.
    let scientific = format!("{:e}", value.abs());
    let (mantissa, exponent) = scientific
        .split_once('e')
        .unwrap_or((scientific.as_str(), "0"));
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let exponent: i64 = exponent.parse().unwrap_or(0);
    let count = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    let point = exponent + 1;
    let body = if count <= point && point <= 21 {
        format!(
            "{digits}{}",
            "0".repeat(usize::try_from(point - count).unwrap_or(0))
        )
    } else if 0 < point && point <= 21 {
        let (head, tail) = digits.split_at(usize::try_from(point).unwrap_or(0));
        format!("{head}.{tail}")
    } else if -6 < point && point <= 0 {
        format!(
            "0.{}{digits}",
            "0".repeat(usize::try_from(-point).unwrap_or(0))
        )
    } else {
        let (head, tail) = digits.split_at(1);
        let fraction = if tail.is_empty() {
            String::new()
        } else {
            format!(".{tail}")
        };
        let exponent_sign = if point >= 1 { "+" } else { "-" };
        format!("{head}{fraction}e{exponent_sign}{}", (point - 1).abs())
    };
    format!("{sign}{body}")
}

/// One inspected call of the reported function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ConstantParameterCallSite {
    /// Caller source path.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line of the call.
    pub line: u32,
    /// 0-based byte column of the call.
    pub col: u32,
}

/// A parameter that each visible call of its function passes in one way.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ConstantParameter {
    /// Source path of the function.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// The module-scope binding name of the function.
    pub function_name: String,
    /// The parameter name.
    pub parameter_name: String,
    /// 0-based index of the parameter. A TypeScript `this` parameter does
    /// not count.
    pub parameter_index: u32,
    /// 1-based line of the parameter.
    pub line: u32,
    /// 0-based byte column of the parameter.
    pub col: u32,
    /// Why the rule reports the parameter.
    pub reason: ConstantParameterReason,
    /// The value that each call passes. Present if and only if `reason` is
    /// `identical-argument`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<ConstantParameterValue>,
    /// `true` when the parameter has the TypeScript `?` marker.
    pub optional: bool,
    /// `true` when the parameter has a default value.
    pub has_default: bool,
    /// The count of conditional tests in the function whose test is the
    /// parameter, `!parameter` or `!!parameter`: the test of an `if`, a
    /// `?:`, a `while`, a `do` or a `for`. Nested functions count. A
    /// comparison such as `parameter === true` and an operand of `&&` or
    /// `||` do not count.
    pub condition_reads: u32,
    /// The count of calls that the rule read for this parameter. A
    /// recursive call that passes the parameter on unchanged does not count.
    pub call_site_count: u32,
    /// The first calls in source order, at most 20. The list is never longer
    /// than `call_site_count`.
    pub inspected_call_sites: Vec<ConstantParameterCallSite>,
    /// What the finding means and what static analysis does not prove.
    pub explanation: String,
}

impl ConstantParameter {
    /// One clause that says how each call passes the parameter, for human
    /// and CI output: `every call passes true`.
    #[must_use]
    pub fn summary(&self) -> String {
        match (&self.reason, &self.value) {
            (ConstantParameterReason::IdenticalArgument, Some(value)) => {
                format!("every call passes {}", value.display())
            }
            (ConstantParameterReason::NeverPassed, _) if self.has_default => {
                "no call passes it, so the default always applies".to_string()
            }
            (ConstantParameterReason::NeverPassed, _) => {
                "no call passes it, so it is always undefined".to_string()
            }
            _ => "the calls pass only some of its values".to_string(),
        }
    }
}

impl ConstantParameterValue {
    /// The value as JavaScript source: a string gets quotes.
    #[must_use]
    pub fn display(&self) -> String {
        match self.kind {
            ConstantParameterValueKind::String => {
                serde_json::to_string(&self.text).unwrap_or_else(|_| self.text.clone())
            }
            _ => self.text.clone(),
        }
    }
}

/// Wire-shape envelope for a [`ConstantParameter`] finding. No action is
/// auto-fixable: a change of the function and each call is a refactor that
/// needs the tests, and a default that differs from the constant can change
/// behavior.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ConstantParameterFinding {
    /// The underlying finding.
    #[serde(flatten)]
    pub parameter: ConstantParameter,
    /// Stable id of this finding: `dc1:<rule>:<16 hex digits>`, with a
    /// `~<k>` suffix when several findings of one type share an identity.
    /// Line and column are not inputs, so the id survives line shifts,
    /// reformats and reorders. A rename of the file, the function or the
    /// parameter gives a new id. Absent in output from older versions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finding_id: Option<String>,
    /// Suggested next steps. Always emitted.
    pub actions: Vec<IssueAction>,
    /// Set by the audit pass when this finding is introduced relative to
    /// the merge-base.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub introduced: Option<AuditIntroduced>,
    /// Gate severity of this finding after `rules` and `overrides[].rules`
    /// resolve for its path. Not part of the finding identity, baseline keys
    /// or fingerprints.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_effective_severity"
    )]
    pub effective_severity: Option<EffectiveSeverity>,
}

impl ConstantParameterFinding {
    /// Build the wrapper with a manual review action and a line-level
    /// suppress.
    #[must_use]
    pub fn with_actions(parameter: ConstantParameter) -> Self {
        let actions = vec![
            manual_framework_fix(
                FixActionType::ReviewConstantParameter,
                "Review the calls, the default and the intent of the parameter",
                "Remove the parameter and move the value into the function, or keep or suppress the parameter when the flexibility is deliberate; static analysis does not prove that a caller outside the project does not exist.",
            ),
            suppress_line("// fallow-ignore-next-line constant-parameter"),
        ];
        Self {
            parameter,
            finding_id: None,
            actions,
            introduced: None,
            effective_severity: None,
        }
    }
}

impl GatedFinding for ConstantParameterFinding {
    fn effective_severity(&self) -> Option<EffectiveSeverity> {
        self.effective_severity
    }

    fn set_effective_severity(&mut self, severity: Option<EffectiveSeverity>) {
        self.effective_severity = severity;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_text_follows_javascript() {
        let cases = [
            (1.0, "1"),
            (1.5, "1.5"),
            (-0.0, "-0"),
            (0.0, "0"),
            (-2.0, "-2"),
            (100.0, "100"),
            (0.1, "0.1"),
            (0.000_001, "0.000001"),
            (0.000_000_1, "1e-7"),
            (1.5e-7, "1.5e-7"),
            (1e21, "1e+21"),
            (1.234_5e21, "1.2345e+21"),
            (123_456_789_012_345_680_000.0, "123456789012345680000"),
            (f64::INFINITY, "Infinity"),
            (f64::NEG_INFINITY, "-Infinity"),
            (0.1 + 0.2, "0.30000000000000004"),
        ];
        for (value, text) in cases {
            assert_eq!(js_number_text(value), text, "{value:?}");
        }
    }

    #[test]
    fn literal_values_have_a_kind_and_a_text() {
        let cases = [
            (
                LiteralValue::Boolean(true),
                ConstantParameterValueKind::Boolean,
                "true",
            ),
            (LiteralValue::Null, ConstantParameterValueKind::Null, "null"),
            (
                LiteralValue::Undefined,
                ConstantParameterValueKind::Undefined,
                "undefined",
            ),
            (
                LiteralValue::Number(3.0f64.to_bits()),
                ConstantParameterValueKind::Number,
                "3",
            ),
            (
                LiteralValue::String("a \"b\"".into()),
                ConstantParameterValueKind::String,
                "a \"b\"",
            ),
        ];
        for (literal, kind, text) in cases {
            let value = ConstantParameterValue::from_literal(&literal);
            assert_eq!(value.kind, kind);
            assert_eq!(value.text, text);
        }
    }

    #[test]
    fn reason_tokens_match_the_wire() {
        for reason in [
            ConstantParameterReason::IdenticalArgument,
            ConstantParameterReason::NeverPassed,
            ConstantParameterReason::UnreachedVariant,
        ] {
            let wire = serde_json::to_value(reason).expect("serialize");
            assert_eq!(wire, reason.as_str());
        }
    }
}
