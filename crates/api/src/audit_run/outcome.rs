//! Comparison of the head run with the base snapshot, and the verdict.

use std::path::{Path, PathBuf};

use fallow_config::{AuditGate, AuditWorsened, ResolvedConfig, RulesConfig, Severity};

use fallow_engine::changed_files::RenamedFile;
use fallow_engine::module_graph::relative_key_path;
use fallow_types::duplicates::CloneGroup;
use rustc_hash::FxHashSet;

use super::clone_containment::base_contained_dupe_group_keys;
use super::{AuditAnalysesView, AuditKeySnapshot};
use crate::audit_keys::{
    AuditComparison, AuditDomainLedger, ChangedPaths, DeadCodeAuditLedger,
    classify_complexity_attribution_against_all_with_moves, complexity_ledger_keys,
    dead_code_audit_ledger, dupe_group_key, preexisting_dupe_group_keys, styling_ledger_keys,
    styling_ledger_keys_with_diff,
};
use crate::audit_moved::{MovedDeadCode, moved_dead_code_keys, moved_dupe_group_keys};
use crate::diff_risk::clones::BaseCloneGroup;
use crate::{AuditAttribution, AuditSummary, AuditVerdict};

/// Which diff decided the new-only duplication demotion, so output can name
/// where a demotion came from (issue #2220).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DupeDemotionDiffSource {
    /// The opt-in shared diff index decided. Carries the user-facing source
    /// label (`--diff-file <path>`, `--diff-stdin`, or
    /// `$FALLOW_DIFF_FILE <path>`).
    Shared(String),
    /// The merge-base worktree diff against the resolved base ref decided.
    Worktree,
    /// No diff was available. The demotion check did not run, and every
    /// introduced clone group kept its gate.
    Skipped,
}

impl DupeDemotionDiffSource {
    /// User-facing label for the diff that decided the demotion.
    #[must_use]
    pub fn label(&self, base_ref: &str) -> String {
        match self {
            Self::Shared(label) => label.clone(),
            Self::Worktree => format!("merge-base worktree diff vs {base_ref}"),
            Self::Skipped => "skipped: no diff available".to_string(),
        }
    }
}

/// An opt-in shared diff that the run already applied, with its source label.
#[derive(Clone, Copy)]
pub struct SharedDiff<'a> {
    /// The parsed diff.
    pub index: &'a fallow_output::DiffIndex,
    /// The user-facing source label, for example `--diff-file <path>`.
    pub label: &'a str,
}

/// The change of one audit run: its files, and the inputs of its diff.
#[derive(Clone, Copy)]
pub struct RunChange<'a> {
    /// Head analysis root.
    pub root: &'a Path,
    /// Resolved base ref, for the merge-base worktree diff.
    pub base_ref: &'a str,
    /// The opt-in shared diff of the run.
    pub shared: Option<SharedDiff<'a>>,
    /// The files that the change touched or deleted.
    pub changed_files: &'a FxHashSet<PathBuf>,
}

impl<'a> RunChange<'a> {
    /// The change of one run.
    #[must_use]
    pub const fn new(
        root: &'a Path,
        base_ref: &'a str,
        shared: Option<SharedDiff<'a>>,
        changed_files: &'a FxHashSet<PathBuf>,
    ) -> Self {
        Self {
            root,
            base_ref,
            shared,
            changed_files,
        }
    }

    /// The diff of the run: the shared diff when one is active, else the
    /// merge-base worktree diff. `None` when git gives no diff.
    fn diff_index(&self) -> Option<std::borrow::Cow<'a, fallow_output::DiffIndex>> {
        if let Some(shared) = self.shared {
            return Some(std::borrow::Cow::Borrowed(shared.index));
        }
        fallow_engine::changed_files::try_get_changed_diff(self.root, self.base_ref)
            .ok()
            .map(|diff| std::borrow::Cow::Owned(fallow_output::DiffIndex::from_unified_diff(&diff)))
    }
}

/// Whether the diff added a line with `symbol` to the head file `path`, a
/// root-relative key path.
fn added_line_names(
    diff: &fallow_output::DiffIndex,
    root: &Path,
    path: &str,
    symbol: &str,
) -> bool {
    let file = root.join(path);
    let Some(added) = diff
        .key_for(&file, root)
        .and_then(|key| diff.added_lines_in(&key))
    else {
        return false;
    };
    let Ok(source) = std::fs::read_to_string(&file) else {
        return false;
    };
    source
        .lines()
        .zip(1_u64..)
        .any(|(line, number)| added.contains(&number) && line.contains(symbol))
}

/// Classify every head finding once against the base snapshot.
///
/// With `syntactic_fallback` set (the degraded type-aware path), dead code
/// compares against the syntactic keys of the base, and a head finding that
/// only semantic evidence produced stays advisory: it has no syntactic base
/// counterpart to compare with.
///
/// `change` holds the files that the change touched or deleted, and its
/// diff. A complexity finding of a function that the change moved between
/// two of those files compares with its base finding (see
/// [`classify_complexity_attribution_against_all_with_moves`]). An unused
/// symbol or a clone group that the change moved is inherited (see
/// `audit_moved.rs`).
#[must_use]
pub fn compare(
    view: &AuditAnalysesView<'_>,
    base: Option<&AuditKeySnapshot>,
    syntactic_fallback: bool,
    change: &RunChange<'_>,
) -> AuditComparison {
    let dead_code =
        view.dead_code
            .as_ref()
            .map_or_else(DeadCodeAuditLedger::default, |dead_code| {
                // A base that ran without type-aware analysis is already
                // syntactic, so its refined set is also its fallback set.
                let base_keys = base.map(|snapshot| {
                    if syntactic_fallback {
                        snapshot
                            .syntactic_dead_code
                            .as_ref()
                            .unwrap_or(&snapshot.dead_code)
                    } else {
                        &snapshot.dead_code
                    }
                });
                let mut ledger = dead_code_audit_ledger(
                    dead_code.results,
                    dead_code.root,
                    dead_code.config,
                    base_keys,
                );
                if syntactic_fallback && let Some(head_syntactic) = dead_code.syntactic_keys {
                    ledger.demote_unattributable_introductions(head_syntactic);
                }
                if let Some(base_keys) = base_keys {
                    let diff = std::cell::OnceCell::new();
                    let moved = moved_dead_code_keys(
                        &ledger,
                        &MovedDeadCode {
                            base: base_keys,
                            changed: &changed_paths_of(change.changed_files, dead_code.root),
                            also_reported: dead_code.syntactic_keys.filter(|_| syntactic_fallback),
                            added_line_names: &|path, symbol| {
                                let index = diff.get_or_init(|| change.diff_index());
                                index.as_deref().is_some_and(|index| {
                                    added_line_names(index, dead_code.root, path, symbol)
                                })
                            },
                        },
                    );
                    ledger.inherit_introductions(&moved);
                }
                ledger
            });
    let health_classes =
        view.health
            .as_ref()
            .zip(base)
            .map_or_else(Vec::new, |(health, snapshot)| {
                let changed_paths = changed_paths_of(change.changed_files, health.root);
                classify_complexity_attribution_against_all_with_moves(
                    &health.report.findings,
                    health.root,
                    std::iter::once(&snapshot.health).chain(&snapshot.inherited_health),
                    &changed_paths,
                )
            });
    let health = view
        .health
        .as_ref()
        .map_or_else(AuditDomainLedger::default, |health| {
            let findings = &health.report.findings;
            let keys = complexity_ledger_keys(findings, health.root);
            let introduced = (0..findings.len()).map(|index| {
                health_classes
                    .get(index)
                    .is_some_and(|class| class.introduced)
            });
            AuditDomainLedger::from_classified(keys.into_iter().zip(introduced), base.is_some())
        });
    let dupes = dupes_ledger(view, base, change.changed_files);
    let styling = AuditDomainLedger::compare(
        view.health
            .iter()
            .flat_map(|health| styling_ledger_keys(&health.report.styling_findings, health.root)),
        base.map(|snapshot| &snapshot.styling),
    );
    AuditComparison {
        dead_code,
        health,
        health_classes,
        dupes,
        styling,
    }
}

/// Compare the clone groups of the head with the base keys. An introduced
/// group that the change moved to other files is inherited (see
/// [`moved_dupe_group_keys`]).
fn dupes_ledger(
    view: &AuditAnalysesView<'_>,
    base: Option<&AuditKeySnapshot>,
    changed_files: &FxHashSet<PathBuf>,
) -> AuditDomainLedger {
    let Some(duplication) = view.duplication.as_ref() else {
        return AuditDomainLedger::compare(std::iter::empty(), base.map(|base| &base.dupes));
    };
    let keys: Vec<String> = duplication
        .clone_groups
        .iter()
        .map(|group| dupe_group_key(group, duplication.root))
        .collect();
    let mut ledger = AuditDomainLedger::compare(keys.iter().cloned(), base.map(|base| &base.dupes));
    if let Some(base) = base
        && ledger.introduced_count() > 0
    {
        let moved = moved_dupe_group_keys(
            keys.iter()
                .map(String::as_str)
                .filter(|key| ledger.is_introduced(key)),
            &keys.iter().map(String::as_str).collect(),
            &base.dupes,
            &changed_paths_of(changed_files, duplication.root),
        );
        ledger.inherit_introductions(&moved);
    }
    ledger
}

/// The changed files as key paths, with the files that the head does not have.
fn changed_paths_of(changed_files: &FxHashSet<PathBuf>, root: &Path) -> ChangedPaths {
    let mut paths = ChangedPaths::default();
    for path in changed_files {
        let key = relative_key_path(path, root);
        if !path.exists() {
            paths.deleted.insert(key.clone());
        }
        paths.changed.insert(key);
    }
    paths
}

/// The base data that the clone group reclassification compares with.
#[derive(Clone, Copy)]
pub struct DupeBase<'a> {
    /// Instance locations of the base clone groups, at base paths.
    pub groups: &'a [BaseCloneGroup],
    /// The renamed files of the change.
    pub renames: &'a [RenamedFile],
}

/// Reclassify introduced clone groups that the change did not write.
///
/// Two rules use the run's diff, in this order:
///
/// 1. A group that holds no added line is demoted. Only the attribution key
///    of the group changed, because the changeset removed code in another
///    place. Without this rule, a clone-removal refactor fails the new-only
///    gate on duplication that it did not write (issue #2164).
/// 2. A group where each instance lies inside a different instance of one
///    base clone group, and one instance holds no added line, is inherited.
///    An edit inside one copy of a base clone splits the base group into
///    smaller groups with new keys, and the
///    change did not write their duplication. This rule records no
///    demotion, because the group matches a base group.
///
/// The opt-in shared diff decides when it is present; the merge-base worktree
/// diff decides in the other cases.
pub fn demote_preexisting_dupe_introductions(
    comparison: &mut AuditComparison,
    view: &AuditAnalysesView<'_>,
    root: &Path,
    base_ref: &str,
    shared: Option<SharedDiff<'_>>,
    base: DupeBase<'_>,
) -> Option<DupeDemotionDiffSource> {
    if comparison.dupes.introduced_count() == 0 {
        return None;
    }
    let duplication = view.duplication.as_ref()?;
    let worktree_index;
    let (index, source) = if let Some(shared) = shared {
        (
            shared.index,
            DupeDemotionDiffSource::Shared(shared.label.to_owned()),
        )
    } else if let Ok(diff) = fallow_engine::changed_files::try_get_changed_diff(root, base_ref) {
        worktree_index = fallow_output::DiffIndex::from_unified_diff(&diff);
        (&worktree_index, DupeDemotionDiffSource::Worktree)
    } else {
        return Some(DupeDemotionDiffSource::Skipped);
    };
    let demote = preexisting_dupe_group_keys(
        duplication.clone_groups.iter().copied(),
        duplication.root,
        index,
    );
    comparison.dupes.demote_introductions(&demote);
    // Only an introduced group can become inherited. The filter keeps the
    // containment check, which compares each group with every base group,
    // away from the groups that already match a base key.
    let inherit = base_contained_dupe_group_keys(
        duplication.clone_groups.iter().copied().filter(|group| {
            comparison
                .dupes
                .is_introduced(&dupe_group_key(group, duplication.root))
        }),
        duplication.root,
        index,
        base.groups,
        base.renames,
    );
    comparison.dupes.inherit_introductions(&inherit);
    Some(source)
}

/// Compare the styling findings again with the run's diff, so the finding
/// that the change wrote is the introduced one.
///
/// Equal styling findings share one identity key, and their occurrence
/// suffix follows source order. A new equal finding above an unchanged one
/// would take the occurrence of the unchanged finding. With the diff, the
/// findings on added lines get the last occurrences (issue #3956). The step
/// runs only when a styling finding is introduced, and it changes no count.
pub fn realign_styling_introductions(
    comparison: &mut AuditComparison,
    view: &AuditAnalysesView<'_>,
    base: Option<&AuditKeySnapshot>,
    root: &Path,
    base_ref: &str,
    shared: Option<SharedDiff<'_>>,
) {
    let (Some(base), Some(health)) = (base, view.health.as_ref()) else {
        return;
    };
    if comparison.styling.introduced_count() == 0 {
        return;
    }
    let worktree_index;
    let index = if let Some(shared) = shared {
        shared.index
    } else if let Ok(diff) = fallow_engine::changed_files::try_get_changed_diff(root, base_ref) {
        worktree_index = fallow_output::DiffIndex::from_unified_diff(&diff);
        &worktree_index
    } else {
        return;
    };
    comparison.styling = AuditDomainLedger::compare(
        styling_ledger_keys_with_diff(&health.report.styling_findings, health.root, Some(index)),
        Some(&base.styling),
    );
}

/// The severity of the rule that owns a styling finding `code`. Styling is
/// verdict-neutral by default (rule `warn`).
#[must_use]
pub fn styling_rule_severity(rules: &RulesConfig, code: &str) -> Severity {
    match code {
        "css-token-drift" => rules.css_token_drift,
        "css-duplicate-block" => rules.css_duplicate_block,
        "css-selector-complexity" => rules.css_selector_complexity,
        "css-dead-surface" => rules.css_dead_surface,
        "css-broken-reference" => rules.css_broken_reference,
        _ => Severity::Warn,
    }
}

/// Whether a styling finding escalates to `error` and so gates the verdict.
#[must_use]
pub fn styling_finding_gates(rules: &RulesConfig, code: &str) -> bool {
    styling_rule_severity(rules, code) == Severity::Error
}

/// The `introduced-clone-group` severity of one clone group.
///
/// With `overrides`, the rule resolves for the file of each instance, and
/// the most severe result applies. So an override that raises the rule for
/// one copy gates the group, and `off` applies only when it matches every
/// copy.
fn clone_group_severity(config: &ResolvedConfig, group: &CloneGroup) -> Severity {
    if config.overrides.is_empty() {
        return config.rules.introduced_clone_group;
    }
    group
        .instances
        .iter()
        .map(|instance| {
            config
                .resolve_rules_for_path(&instance.file)
                .introduced_clone_group
        })
        .max_by_key(|severity| match severity {
            Severity::Error => 2,
            Severity::Warn => 1,
            Severity::Off => 0,
        })
        .unwrap_or(config.rules.introduced_clone_group)
}

/// Attribution counts, verdict and summary of one comparison.
#[must_use]
pub fn outcome(
    gate: AuditGate,
    worsened: AuditWorsened,
    view: &AuditAnalysesView<'_>,
    comparison: &AuditComparison,
    has_base: bool,
) -> (AuditAttribution, AuditVerdict, AuditSummary) {
    let summary = summary(view, comparison);
    (
        attribution(gate, worsened, comparison, has_base),
        verdict(gate, worsened, view, comparison, &summary),
        summary,
    )
}

/// How each complexity finding takes part in the verdict: `(gates, warns)`.
///
/// Under `new-only`, an introduced finding gates. A worsened finding gates
/// under `audit.worsened: error`, and gives at least `warn` under `warn`.
fn complexity_verdict_flags(
    gate: AuditGate,
    worsened: AuditWorsened,
    view: &AuditAnalysesView<'_>,
    comparison: &AuditComparison,
) -> (bool, bool) {
    let new_only = matches!(gate, AuditGate::NewOnly);
    let Some(health) = view.health.as_ref() else {
        return (false, false);
    };
    let mut errors = false;
    let mut warnings = false;
    for (index, (finding, introduced)) in health
        .report
        .findings
        .iter()
        .zip(comparison.health.introduced())
        .enumerate()
    {
        let is_worsened = comparison
            .health_classes
            .get(index)
            .is_some_and(crate::audit_keys::ComplexityClass::is_worsened);
        let counts = !new_only || introduced || (is_worsened && worsened == AuditWorsened::Error);
        if counts {
            if finding.blocks() {
                errors = true;
            } else {
                warnings = true;
            }
        } else if is_worsened {
            warnings = true;
        }
    }
    (errors, warnings)
}

/// How the clone groups take part in the verdict: `(gates, warns)`.
///
/// Under `new-only`, only an introduced group counts. Under `all`, every
/// group counts. The `introduced-clone-group` rule gives the severity of a
/// group that counts.
fn duplication_verdict_flags(
    new_only: bool,
    view: &AuditAnalysesView<'_>,
    comparison: &AuditComparison,
) -> (bool, bool) {
    let Some(duplication) = view.duplication.as_ref() else {
        return (false, false);
    };
    let introduced: Vec<bool> = comparison.dupes.introduced().collect();
    let mut errors = false;
    let mut warnings = false;
    for (index, group) in duplication.clone_groups.iter().enumerate() {
        if new_only && !introduced.get(index).copied().unwrap_or(false) {
            continue;
        }
        match clone_group_severity(duplication.config, group) {
            Severity::Error => errors = true,
            Severity::Warn => warnings = true,
            Severity::Off => {}
        }
    }
    (errors, warnings)
}

fn verdict(
    gate: AuditGate,
    worsened: AuditWorsened,
    view: &AuditAnalysesView<'_>,
    comparison: &AuditComparison,
    summary: &AuditSummary,
) -> AuditVerdict {
    let new_only = matches!(gate, AuditGate::NewOnly);
    let dead_code_errors = if new_only {
        comparison.dead_code.has_introduced_errors()
    } else {
        dead_code_has_errors(view)
    };
    let dead_code_warnings = if new_only {
        comparison.dead_code.has_introduced_warnings()
    } else {
        comparison
            .dead_code
            .records()
            .iter()
            .any(|record| record.effective_severity == Severity::Warn)
    };
    // The `complexity-*` rules decide if a finding blocks: `error` fails the
    // verdict and `warn` gives `warn`. The `new-only` gate reads only the
    // introduced findings.
    let (complexity_errors, complexity_warnings) =
        complexity_verdict_flags(gate, worsened, view, comparison);
    let styling_errors = view.health.as_ref().is_some_and(|health| {
        health
            .report
            .styling_findings
            .iter()
            .zip(comparison.styling.introduced())
            .any(|(finding, introduced)| {
                (!new_only || introduced) && styling_finding_gates(health.rules, &finding.code)
            })
    });
    let (clone_errors, clone_warnings) = duplication_verdict_flags(new_only, view, comparison);
    // The threshold reads every counted group, also a group that the
    // `introduced-clone-group` rule turns off: the two gates are independent.
    let duplication_findings = if new_only {
        comparison.dupes.introduced_count()
    } else {
        summary.duplication_clone_groups
    };
    let duplication_errors = view.duplication.as_ref().is_some_and(|duplication| {
        duplication_findings > 0
            && duplication.threshold > 0.0
            && duplication.duplication_percentage > duplication.threshold
    });
    if dead_code_errors || complexity_errors || styling_errors || duplication_errors || clone_errors
    {
        AuditVerdict::Fail
    } else if dead_code_warnings || complexity_warnings || clone_warnings {
        AuditVerdict::Warn
    } else {
        AuditVerdict::Pass
    }
}

/// Whether the head dead-code findings hold an error-severity finding, by the
/// same rule that decides the exit code of `fallow dead-code`.
fn dead_code_has_errors(view: &AuditAnalysesView<'_>) -> bool {
    view.dead_code.as_ref().is_some_and(|dead_code| {
        fallow_engine::error_severity::has_error_severity_issues(
            dead_code.results,
            &dead_code.config.rules,
            Some(dead_code.config),
            false,
        )
    })
}

fn attribution(
    gate: AuditGate,
    worsened: AuditWorsened,
    comparison: &AuditComparison,
    has_base: bool,
) -> AuditAttribution {
    if !has_base {
        return AuditAttribution {
            gate,
            worsened,
            ..AuditAttribution::default()
        };
    }
    AuditAttribution {
        gate,
        worsened,
        complexity_worsened: comparison.complexity_worsened_count(),
        dead_code_introduced: comparison.dead_code.introduced_count(),
        dead_code_inherited: comparison.dead_code.inherited_count(),
        complexity_introduced: comparison.health.introduced_count(),
        complexity_inherited: comparison.health.inherited_count(),
        duplication_introduced: comparison.dupes.introduced_count(),
        duplication_inherited: comparison.dupes.inherited_count(),
    }
}

fn summary(view: &AuditAnalysesView<'_>, comparison: &AuditComparison) -> AuditSummary {
    AuditSummary {
        dead_code_issues: comparison.dead_code.visible_count(),
        dead_code_has_errors: dead_code_has_errors(view),
        dead_code_skipped: view.dead_code.is_none(),
        complexity_skipped: view.health.is_none(),
        duplication_skipped: view.duplication.is_none(),
        complexity_findings: view
            .health
            .as_ref()
            .map_or(0, |health| health.report.findings.len()),
        max_cyclomatic: view.health.as_ref().and_then(|health| {
            health
                .report
                .findings
                .iter()
                .map(|finding| finding.cyclomatic)
                .max()
        }),
        duplication_clone_groups: view
            .duplication
            .as_ref()
            .map_or(0, |duplication| duplication.clone_groups.len()),
    }
}
