//! Per-line sampling of the planned mutants (`--mutants-per-line`).

use std::collections::BTreeMap;

use fallow_mutation::Operator;
use sha2::{Digest, Sha256};

use super::MutantDisposition;
use super::plan::RankEntry;

/// The rank of an operator when a line keeps only some of its mutants. Lower
/// ranks stay. Relational, condition, arithmetic and logical mutants change
/// behavior that tests usually check. Literal and structural mutants come
/// after them.
const fn productivity(operator: Operator) -> u8 {
    match operator {
        Operator::RelationalOperator => 0,
        Operator::NegateCondition => 1,
        Operator::ArithmeticOperator => 2,
        Operator::LogicalOperator => 3,
        Operator::UnaryOperator => 4,
        Operator::UpdateOperator => 5,
        Operator::ReturnValue => 6,
        Operator::RemovedCall => 7,
        Operator::OptionalChaining => 8,
        Operator::BooleanLiteral => 9,
        Operator::StringLiteral => 10,
        Operator::RegexLiteral => 11,
        Operator::RemovedChainLink => 12,
    }
}

/// A stable tie-break between mutants of the same operator on one line. The
/// value depends on the mutant id only, so it is the same on every run and on
/// every platform.
fn id_hash(id: &str) -> u64 {
    let digest = Sha256::digest(id.as_bytes());
    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(bytes)
}

/// Keeps at most `per_line` planned mutants on each source line and marks the
/// others [`MutantDisposition::SampledOut`]. A line keeps its mutants in the
/// order of [`productivity`], then of the id hash. Only planned mutants
/// compete, and the choice reads no run state, so a cached result does not
/// change it.
pub(super) fn sample_per_line(mutants: &mut [RankEntry], per_line: usize) {
    let mut lines: BTreeMap<(String, u32), Vec<usize>> = BTreeMap::new();
    for (index, entry) in mutants.iter().enumerate() {
        if entry.planned.disposition == MutantDisposition::Planned {
            lines
                .entry((entry.planned.file.clone(), entry.planned.mutant.line))
                .or_default()
                .push(index);
        }
    }
    for mut indices in lines.into_values() {
        if indices.len() <= per_line {
            continue;
        }
        indices.sort_by_cached_key(|&index| {
            let mutant = &mutants[index].planned.mutant;
            (
                productivity(mutant.operator),
                id_hash(mutant.id.as_str()),
                mutant.id.clone(),
            )
        });
        for &index in &indices[per_line..] {
            mutants[index].planned.disposition = MutantDisposition::SampledOut;
        }
    }
}

#[cfg(test)]
mod tests {
    use fallow_mutation::MutationOptions;

    use super::super::layout::Layout;
    use super::super::{PlannedMutant, plan::RankEntry};
    use super::*;

    /// One line with an arithmetic, a relational and a logical mutant, plus
    /// the condition mutants of the two comparisons.
    const SOURCE: &str =
        "export const f = (a: number, b: number): boolean => a + b > 3 && a < b;\n";

    fn entries(source: &str) -> Vec<RankEntry> {
        let layout = Layout::from_path("src/line.ts").expect("a TypeScript path");
        layout
            .instrument(source, "src/line.ts", &MutationOptions::default())
            .expect("the source parses")
            .mutants
            .into_iter()
            .map(|mutant| RankEntry {
                in_diff: true,
                planned: PlannedMutant {
                    mutant,
                    file: "src/line.ts".to_owned(),
                    rank: None,
                    disposition: MutantDisposition::Planned,
                    function: None,
                    test_reach: None,
                },
            })
            .collect()
    }

    fn kept(entries: &[RankEntry]) -> Vec<String> {
        let mut ids: Vec<String> = entries
            .iter()
            .filter(|entry| entry.planned.disposition == MutantDisposition::Planned)
            .map(|entry| entry.planned.mutant.id.to_string())
            .collect();
        ids.sort();
        ids
    }

    #[test]
    fn a_line_with_three_mutants_keeps_one_and_samples_out_the_others() {
        let mut planned = entries(SOURCE);
        assert!(planned.len() >= 3, "{} mutants", planned.len());
        assert!(planned.iter().all(|entry| entry.planned.mutant.line == 1));
        sample_per_line(&mut planned, 1);
        let kept: Vec<&RankEntry> = planned
            .iter()
            .filter(|entry| entry.planned.disposition == MutantDisposition::Planned)
            .collect();
        assert_eq!(kept.len(), 1);
        assert_eq!(
            kept[0].planned.mutant.operator,
            Operator::RelationalOperator,
            "the relational operator ranks first"
        );
        let sampled = planned
            .iter()
            .filter(|entry| entry.planned.disposition == MutantDisposition::SampledOut)
            .count();
        assert_eq!(sampled, planned.len() - 1);
    }

    #[test]
    fn a_second_run_and_another_input_order_keep_the_same_mutants() {
        let mut first = entries(SOURCE);
        sample_per_line(&mut first, 2);
        let mut second = entries(SOURCE);
        second.reverse();
        sample_per_line(&mut second, 2);
        assert_eq!(kept(&first).len(), 2);
        assert_eq!(kept(&first), kept(&second));
    }

    #[test]
    fn a_limit_above_the_line_count_samples_out_nothing() {
        let mut planned = entries(SOURCE);
        let total = planned.len();
        sample_per_line(&mut planned, total);
        assert_eq!(kept(&planned).len(), total);
    }

    #[test]
    fn each_line_keeps_its_own_mutants() {
        let mut planned = entries(
            "export const f = (a: number, b: number): boolean => a > b;\n\
             export const g = (a: number, b: number): boolean => a < b;\n",
        );
        sample_per_line(&mut planned, 1);
        let mut lines: Vec<u32> = planned
            .iter()
            .filter(|entry| entry.planned.disposition == MutantDisposition::Planned)
            .map(|entry| entry.planned.mutant.line)
            .collect();
        lines.sort_unstable();
        assert_eq!(lines, [1, 2]);
    }
}
