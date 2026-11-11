//! The `constant-parameter` hint of a `negate-condition` mutant.
//!
//! A `negate-condition` mutant sets a branch or loop test to `true` or
//! `false`. When the test is a parameter `p`, `!p` or `!!p`, and each visible
//! call of the function passes the same literal for `p`, the test always has
//! one value. The mutant that sets the test to that value changes nothing for
//! a visible caller. The `constant-parameter` rule decides the calls, with its
//! call evidence and its closed-world gate, so a function that the gate
//! blocks gets no hint.

use std::path::Path;

use fallow_extract::{ParameterCondition, parameter_conditions};
use fallow_mutation::Operator;
use fallow_types::results::{
    ConstantParameterFinding, ConstantParameterReason, ConstantParameterValueKind,
};
use rustc_hash::FxHashMap;

use super::plan::RankEntry;

/// One parameter whose calls each pass the same literal.
struct ConstantArgument<'a> {
    function: &'a str,
    parameter_index: u32,
    truthy: bool,
}

/// The constant parameters of each file, from the `constant-parameter`
/// findings of the analysis.
#[derive(Default)]
pub(super) struct ConstantParameters<'a> {
    by_file: FxHashMap<&'a Path, Vec<ConstantArgument<'a>>>,
}

impl<'a> ConstantParameters<'a> {
    /// Keeps the findings whose calls each pass the same literal. A
    /// `never-passed` finding has no literal. A passed `undefined` gives a
    /// parameter with a default value the default, which the finding does
    /// not hold.
    pub(super) fn new(findings: &'a [ConstantParameterFinding]) -> Self {
        let mut by_file: FxHashMap<&Path, Vec<ConstantArgument<'_>>> = FxHashMap::default();
        for finding in findings {
            let parameter = &finding.parameter;
            // A finding about one option key says nothing about a test of
            // the whole parameter.
            if parameter.reason != ConstantParameterReason::IdenticalArgument
                || parameter.option_key.is_some()
            {
                continue;
            }
            let Some(value) = &parameter.value else {
                continue;
            };
            if parameter.has_default && value.kind == ConstantParameterValueKind::Undefined {
                continue;
            }
            let Some(truthy) = value.truthiness() else {
                continue;
            };
            by_file
                .entry(parameter.path.as_path())
                .or_default()
                .push(ConstantArgument {
                    function: &parameter.function_name,
                    parameter_index: parameter.parameter_index,
                    truthy,
                });
        }
        Self { by_file }
    }

    /// Marks each `negate-condition` mutant of the file at `path` that sets a
    /// parameter test to the value that the constant argument gives it.
    /// `source` is the whole file, so it holds the mutant spans.
    pub(super) fn mark(&self, path: &Path, source: &str, mutants: &mut [RankEntry]) {
        let Some(arguments) = self.by_file.get(path) else {
            return;
        };
        let is_condition =
            |entry: &RankEntry| entry.planned.mutant.operator == Operator::NegateCondition;
        if !mutants.iter().any(is_condition) {
            return;
        }
        let conditions = parameter_conditions(path, source);
        for entry in mutants.iter_mut().filter(|entry| is_condition(entry)) {
            let mutant = &entry.planned.mutant;
            let constant = match mutant.change.to {
                "true" => true,
                "false" => false,
                _ => continue,
            };
            entry.planned.constant_parameter = conditions
                .iter()
                .filter(|condition| condition.span == mutant.span)
                .any(|condition| test_value(condition, arguments) == Some(constant));
        }
    }
}

/// The value of the test of `condition` when its parameter gets a constant
/// argument, or `None` when the parameter has none.
fn test_value(condition: &ParameterCondition, arguments: &[ConstantArgument<'_>]) -> Option<bool> {
    let argument = arguments.iter().find(|argument| {
        *argument.function == *condition.function
            && argument.parameter_index == condition.parameter_index
    })?;
    Some(argument.truthy != (condition.negations % 2 == 1))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_mutation::MutationOptions;
    use fallow_types::extract::LiteralValue;
    use fallow_types::results::{ConstantParameter, ConstantParameterValue};

    use super::*;
    use crate::mutation::layout::Layout;
    use crate::mutation::{MutantDisposition, PlannedMutant};

    const PATH: &str = "/project/src/load.ts";
    const SOURCE: &str = "export function load(id, strict) {\n  if (!strict) return id;\n  if (id) return 0;\n  return strict ? 1 : 2;\n}\n";

    fn finding(
        function: &str,
        index: u32,
        reason: ConstantParameterReason,
        value: Option<LiteralValue>,
        has_default: bool,
    ) -> ConstantParameterFinding {
        ConstantParameterFinding::with_actions(ConstantParameter {
            path: PathBuf::from(PATH),
            function_name: function.to_owned(),
            parameter_name: "strict".to_owned(),
            parameter_index: index,
            option_key: None,
            line: 1,
            col: 0,
            reason,
            value: value.map(|value| ConstantParameterValue::from_literal(&value)),
            optional: false,
            has_default,
            condition_reads: 2,
            call_site_count: 3,
            inspected_call_sites: Vec::new(),
            explanation: String::new(),
        })
    }

    fn identical(value: LiteralValue) -> ConstantParameterFinding {
        finding(
            "load",
            1,
            ConstantParameterReason::IdenticalArgument,
            Some(value),
            false,
        )
    }

    /// The text of each marked mutant: the test and its constant.
    fn marked(findings: &[ConstantParameterFinding]) -> Vec<String> {
        let layout = Layout::from_path("src/load.ts").expect("a TypeScript path");
        let options = MutationOptions {
            operators: vec![Operator::NegateCondition, Operator::BooleanLiteral],
            ..MutationOptions::default()
        };
        let mut entries: Vec<RankEntry> = layout
            .instrument(SOURCE, "src/load.ts", &options)
            .expect("the source parses")
            .mutants
            .into_iter()
            .map(|mutant| RankEntry {
                in_diff: true,
                planned: PlannedMutant {
                    mutant,
                    file: "src/load.ts".to_owned(),
                    rank: None,
                    disposition: MutantDisposition::Planned,
                    function: None,
                    test_reach: None,
                    constant_parameter: false,
                },
            })
            .collect();
        ConstantParameters::new(findings).mark(Path::new(PATH), SOURCE, &mut entries);
        entries
            .iter()
            .filter(|entry| entry.planned.constant_parameter)
            .map(|entry| {
                let mutant = &entry.planned.mutant;
                format!("{} -> {}", mutant.original, mutant.replacement)
            })
            .collect()
    }

    #[test]
    fn the_constant_that_the_argument_gives_the_test_is_marked() {
        assert_eq!(
            marked(&[identical(LiteralValue::Boolean(true))]),
            vec!["!strict -> false", "strict -> true"]
        );
        assert_eq!(
            marked(&[identical(LiteralValue::Number(0f64.to_bits()))]),
            vec!["!strict -> true", "strict -> false"]
        );
        assert_eq!(
            marked(&[identical(LiteralValue::String("on".into()))]),
            vec!["!strict -> false", "strict -> true"]
        );
    }

    #[test]
    fn a_parameter_without_one_literal_gets_no_mark() {
        assert!(marked(&[]).is_empty());
        let never = finding("load", 1, ConstantParameterReason::NeverPassed, None, true);
        assert!(marked(&[never]).is_empty());
        let defaulted = finding(
            "load",
            1,
            ConstantParameterReason::IdenticalArgument,
            Some(LiteralValue::Undefined),
            true,
        );
        assert!(marked(&[defaulted]).is_empty());
        let other_function = finding(
            "save",
            1,
            ConstantParameterReason::IdenticalArgument,
            Some(LiteralValue::Boolean(true)),
            false,
        );
        assert!(marked(&[other_function]).is_empty());
        let mut key = identical(LiteralValue::Boolean(true));
        key.parameter.option_key = Some("strict".to_owned());
        assert!(marked(&[key]).is_empty());
        let mut binding = identical(LiteralValue::Boolean(true));
        binding.parameter.value = Some(ConstantParameterValue {
            kind: ConstantParameterValueKind::Binding,
            text: "STRICT".to_owned(),
        });
        assert!(marked(&[binding]).is_empty());
    }
}
