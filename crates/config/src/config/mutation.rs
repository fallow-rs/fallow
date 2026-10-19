use schemars::JsonSchema;
use serde::{Deserialize, Deserializer, Serialize};

/// The test runner of `fallow mutation`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MutationRunner {
    /// Detect the runner from the project: the `vitest` dependency, the
    /// test module that the test files import, a Bun file at the root, or a
    /// `node --test` script.
    #[default]
    Auto,
    /// Vitest 4.1 to 5 from the project `node_modules`.
    Vitest,
    /// `node:test` of Node.js 22.18 or later. Each test file runs in its own
    /// Node process.
    NodeTest,
    /// `bun test` of Bun 1.4 or later. Each test file runs in its own Bun
    /// process.
    BunTest,
}

/// A mutation operator name, kebab-case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum MutationOperatorName {
    /// `a + b` to `a - b`, and the other arithmetic pairs.
    ArithmeticOperator,
    /// `a < b` to `a <= b`, and the other relational pairs.
    RelationalOperator,
    /// `a && b` to `a || b`, and the reverse.
    LogicalOperator,
    /// A condition to its negation.
    NegateCondition,
    /// A unary operator removed or changed.
    UnaryOperator,
    /// `i++` to `i--`, and the reverse.
    UpdateOperator,
    /// A call statement removed.
    RemovedCall,
    /// A return value changed.
    ReturnValue,
    /// `true` to `false`, and the reverse.
    BooleanLiteral,
    /// A string literal emptied or filled.
    StringLiteral,
    /// `a?.b` to `a.b`.
    OptionalChaining,
    /// One token of a regular expression literal changed: an anchor
    /// removed, a character class negated, or a class escape with the
    /// opposite case.
    RegexLiteral,
}

impl MutationOperatorName {
    /// The kebab-case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ArithmeticOperator => "arithmetic-operator",
            Self::RelationalOperator => "relational-operator",
            Self::LogicalOperator => "logical-operator",
            Self::NegateCondition => "negate-condition",
            Self::UnaryOperator => "unary-operator",
            Self::UpdateOperator => "update-operator",
            Self::RemovedCall => "removed-call",
            Self::ReturnValue => "return-value",
            Self::BooleanLiteral => "boolean-literal",
            Self::StringLiteral => "string-literal",
            Self::OptionalChaining => "optional-chaining",
            Self::RegexLiteral => "regex-literal",
        }
    }
}

fn deserialize_min_score<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<f64>::deserialize(deserializer)?;
    if let Some(score) = value
        && (!score.is_finite() || !(0.0..=100.0).contains(&score))
    {
        return Err(serde::de::Error::custom(format!(
            "mutation.minScore must be between 0 and 100 (got {score})"
        )));
    }
    Ok(value)
}

fn deserialize_concurrency<'de, D>(deserializer: D) -> Result<Option<usize>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<usize>::deserialize(deserializer)?;
    if value == Some(0) {
        return Err(serde::de::Error::custom(
            "mutation.concurrency must be at least 1",
        ));
    }
    Ok(value)
}

/// Project settings for `fallow mutation`.
///
/// Project config cannot widen what a run executes: the override that keeps
/// mutants near file system, process, network or database sinks is a CLI
/// flag only (`--allow-side-effects`), and no key starts a run.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MutationConfig {
    /// The test runner: `auto`, `vitest`, `node-test` or `bun-test`. `auto`
    /// detects it from the project.
    #[serde(default)]
    pub runner: MutationRunner,

    /// The operators to use. Unset uses each operator. `--operators` wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operators: Option<Vec<MutationOperatorName>>,

    /// The minimum mutation score, from 0 to 100. When the score of a run is
    /// lower, `fallow mutation` exits with code 1. Unset never fails a run.
    /// `--min-score` wins.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_min_score"
    )]
    #[schemars(range(min = 0.0, max = 100.0))]
    pub min_score: Option<f64>,

    /// The number of companion processes. Unset uses half the logical cores.
    /// `--concurrency` wins.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_concurrency"
    )]
    #[schemars(range(min = 1))]
    pub concurrency: Option<usize>,

    /// Type-check each survived mutant through the TypeScript-Go companion
    /// of `--type-aware`. A survivor that does not type-check becomes
    /// `compile_error` and does not count in the score. Unset or `false`
    /// checks no survivor. `--typecheck` turns the check on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typecheck: Option<bool>,

    /// The config file of the test runner, relative to the project root, for
    /// example `vitest.integration.config.mjs`. The file must exist inside
    /// the project root. Only the Vitest runner reads it. Unset uses the
    /// default config discovery of the runner. `--runner-config` wins.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_config: Option<String>,
}

impl MutationConfig {
    /// True when no key is set.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(json: &str) -> Result<MutationConfig, String> {
        serde_json::from_str::<MutationConfig>(json).map_err(|error| error.to_string())
    }

    #[test]
    fn every_key_parses_in_camel_case() {
        let config = parse(
            r#"{"runner":"vitest","operators":["arithmetic-operator","removed-call"],
                "minScore":80,"concurrency":4,"typecheck":true,
                "runnerConfig":"vitest.integration.config.mjs"}"#,
        )
        .expect("valid config");
        assert_eq!(config.runner, MutationRunner::Vitest);
        assert_eq!(
            config.operators,
            Some(vec![
                MutationOperatorName::ArithmeticOperator,
                MutationOperatorName::RemovedCall
            ])
        );
        assert_eq!(config.min_score, Some(80.0));
        assert_eq!(config.concurrency, Some(4));
        assert_eq!(config.typecheck, Some(true));
        assert_eq!(
            config.runner_config.as_deref(),
            Some("vitest.integration.config.mjs")
        );
    }

    #[test]
    fn the_runner_names_node_test_and_bun_test() {
        let node = parse(r#"{"runner":"node-test"}"#).expect("node-test");
        assert_eq!(node.runner, MutationRunner::NodeTest);
        let bun = parse(r#"{"runner":"bun-test"}"#).expect("bun-test");
        assert_eq!(bun.runner, MutationRunner::BunTest);
        assert!(parse(r#"{"runner":"jest"}"#).is_err());
    }

    #[test]
    fn an_empty_section_is_the_default() {
        let config = parse("{}").expect("empty section");
        assert!(config.is_default());
        assert_eq!(config.runner, MutationRunner::Auto);
        assert!(!parse(r#"{"minScore":10}"#).expect("set").is_default());
    }

    #[test]
    fn side_effect_override_is_not_a_config_key() {
        let error = parse(r#"{"allowSideEffects":true}"#).expect_err("flag only");
        assert!(error.contains("allowSideEffects"), "{error}");
    }

    #[test]
    fn keys_that_the_run_does_not_apply_are_rejected() {
        for key in ["include", "exclude", "timeoutFactor", "timeoutMs"] {
            let json = if key.starts_with("timeout") {
                format!(r#"{{"{key}":2}}"#)
            } else {
                format!(r#"{{"{key}":["src/**"]}}"#)
            };
            let error = parse(&json).expect_err(&json);
            assert!(error.contains("unknown field"), "{json}: {error}");
        }
    }

    #[test]
    fn out_of_range_values_fail_loud() {
        for (json, needle) in [
            (r#"{"minScore":101}"#, "between 0 and 100"),
            (r#"{"minScore":-1}"#, "between 0 and 100"),
            (r#"{"concurrency":0}"#, "concurrency must be at least 1"),
            (r#"{"runner":"jest"}"#, "unknown variant"),
            (r#"{"operators":["bogus"]}"#, "unknown variant"),
        ] {
            let error = parse(json).expect_err(json);
            assert!(error.contains(needle), "{json}: {error}");
        }
        assert_eq!(
            parse(r#"{"minScore":0}"#).expect("zero").min_score,
            Some(0.0)
        );
        assert_eq!(
            parse(r#"{"minScore":100}"#).expect("max").min_score,
            Some(100.0)
        );
    }

    #[test]
    fn operator_names_round_trip_through_serde() {
        for name in [
            MutationOperatorName::ArithmeticOperator,
            MutationOperatorName::RelationalOperator,
            MutationOperatorName::LogicalOperator,
            MutationOperatorName::NegateCondition,
            MutationOperatorName::UnaryOperator,
            MutationOperatorName::UpdateOperator,
            MutationOperatorName::RemovedCall,
            MutationOperatorName::ReturnValue,
            MutationOperatorName::BooleanLiteral,
            MutationOperatorName::StringLiteral,
            MutationOperatorName::OptionalChaining,
        ] {
            let json = serde_json::to_value(name).expect("serializes");
            assert_eq!(json, serde_json::Value::String(name.as_str().to_owned()));
        }
    }
}
