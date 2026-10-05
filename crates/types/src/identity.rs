//! Stable public identity for findings.
//!
//! This module owns the FNV-1a 64 hash that CodeClimate fingerprints, SARIF
//! fingerprints and security ids use, and the `finding_id` of dead-code
//! findings. One implementation keeps the surfaces from drifting apart.
//!
//! A dead-code id has the form `dc1:<rule>:<16 hex digits>`. The hash input is
//! `["dc1", rule, parts...]`, where the parts name the subject of the finding:
//! root-relative forward-slash paths and raw symbol names. Line and column are
//! never parts, so a line shift, a reformat or a reorder keeps the id. A
//! rename of the file or the symbol, or another issue type, gives a new id.
//!
//! When several findings of one type have the same parts, they are sorted by
//! line, column, span start and serialized finding. The first one keeps the
//! base id. The finding at sorted position `k` gets the suffix `~k`.
//!
//! The canonical key is the readable form of the same input:
//! `<rule>:<part>:<part>...`, for example `unused-export:src/utils.ts:helper`.
//! Baselines and the audit new-only gate compare findings by this key, so the
//! id, the baseline and the audit can never disagree on what one finding is.
//! The key has no tiebreak suffix: a baseline stores one key for each
//! occurrence, and the audit numbers repeated keys itself.
//!
//! [`stamp_dead_code_finding_ids`](crate::identity::stamp_dead_code_finding_ids) writes the ids onto a full result set. The
//! analysis pipeline calls it before the workspace, scope, changed-file,
//! ignore, baseline and rule filters, so a filter never changes the id of a
//! finding that stays in the report.

use std::path::Path;

use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;

use crate::discover::StableFileKey;
use crate::output_dead_code::{
    AbsentComponentPropFinding, BoundaryCallViolationFinding, BoundaryCoverageViolationFinding,
    BoundaryViolationFinding, CircularDependencyFinding, DeprecatedExportInUseFinding,
    DevDependencyInProductionFinding, DuplicateExportFinding, DuplicatePropShapeFinding,
    DynamicSegmentNameConflictFinding, EmptyCatalogGroupFinding, InvalidClientExportFinding,
    MisconfiguredDependencyOverrideFinding, MisplacedDirectiveFinding,
    MixedClientServerBarrelFinding, PackageCycleFinding, PolicyViolationFinding,
    PrivateTypeLeakFinding, PropDrillingChainFinding, ReExportCycleFinding, RouteCollisionFinding,
    TestOnlyDependencyFinding, ThinWrapperFinding, TypeOnlyDependencyFinding,
    UnlistedDependencyFinding, UnprovidedInjectFinding, UnrenderedComponentFinding,
    UnresolvedCatalogReferenceFinding, UnresolvedImportFinding, UnusedCatalogEntryFinding,
    UnusedClassMemberFinding, UnusedComponentEmitFinding, UnusedComponentInputFinding,
    UnusedComponentOutputFinding, UnusedComponentPropFinding, UnusedDependencyFinding,
    UnusedDependencyOverrideFinding, UnusedDevDependencyFinding, UnusedEnumMemberFinding,
    UnusedExportFinding, UnusedFileFinding, UnusedLoadDataKeyFinding,
    UnusedOptionalDependencyFinding, UnusedServerActionFinding, UnusedStoreMemberFinding,
    UnusedSvelteEventFinding, UnusedTypeFinding,
};
use crate::results::{
    AnalysisResults, DependencyOverrideSource, ReExportCycleKind, StaleSuppression,
    SuppressionOrigin,
};

/// The version prefix of every dead-code finding id. A change to the hash
/// inputs moves this prefix, so an old id never matches a new finding.
pub const DEAD_CODE_ID_SCHEME: &str = "dc1";

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;
/// Written after each part. No UTF-8 string contains this byte, so the parts
/// `["ab", "c"]` and `["a", "bc"]` give different hashes.
const PART_SEPARATOR: u8 = 0xff;
/// Joins the sorted members of a path set into one part.
const SET_SEPARATOR: &str = "|";
/// Stands for "every issue kind" in a suppression identity.
const ANY_KIND: &str = "*";

fn fnv1a64_update(mut hash: u64, bytes: &[u8]) -> u64 {
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// FNV-1a 64 of `bytes`, as 16 lowercase hex digits.
///
/// Security ids use this form: one string, no part separators.
#[must_use]
pub fn fnv1a64_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64_update(FNV_OFFSET_BASIS, bytes))
}

/// FNV-1a 64 of `parts`, with the byte `0xff` after each part, as 16
/// lowercase hex digits.
///
/// CodeClimate and SARIF fingerprints and dead-code finding ids use this form.
/// FNV-1a is used because its output is fixed across Rust versions, which is
/// not true for `DefaultHasher`.
#[must_use]
pub fn fnv1a64_parts(parts: &[&str]) -> String {
    let hash = parts.iter().fold(FNV_OFFSET_BASIS, |hash, part| {
        fnv1a64_update(fnv1a64_update(hash, part.as_bytes()), &[PART_SEPARATOR])
    });
    format!("{hash:016x}")
}

/// The base id of a dead-code finding: `dc1:<rule_token>:<hash>`.
///
/// `rule_token` is the canonical issue code, for example `unused-export`.
/// `parts` name the subject of the finding and never contain a line or a
/// column. The id carries no tiebreak suffix; [`stamp_dead_code_finding_ids`]
/// adds it when two findings share a base id.
#[must_use]
pub fn dead_code_finding_id(rule_token: &str, parts: &[&str]) -> String {
    let mut input = Vec::with_capacity(parts.len() + 2);
    input.push(DEAD_CODE_ID_SCHEME);
    input.push(rule_token);
    input.extend_from_slice(parts);
    format!(
        "{DEAD_CODE_ID_SCHEME}:{rule_token}:{}",
        fnv1a64_parts(&input)
    )
}

/// Joins the rule token and the parts of a canonical key.
const KEY_SEPARATOR: char = ':';
/// Starts the occurrence suffix of an audit key, as in a finding id.
const OCCURRENCE_MARKER: char = '~';

/// Escape `%` and `:` in one part, so the joined key splits back into the
/// same parts. Other characters stay as they are, so the key stays readable.
fn escape_key_part(part: &str, key: &mut String) {
    for character in part.chars() {
        match character {
            '%' => key.push_str("%25"),
            KEY_SEPARATOR => key.push_str("%3A"),
            other => key.push(other),
        }
    }
}

/// The canonical key of a dead-code finding: `<rule_token>:<part>:<part>...`.
///
/// The key holds the same input as [`dead_code_finding_id`], in readable
/// form. Each part escapes `%` as `%25` and `:` as `%3A`. The key never
/// holds a line, a column or a tiebreak suffix.
#[must_use]
pub fn dead_code_canonical_key(rule_token: &str, parts: &[&str]) -> String {
    let mut key = String::with_capacity(
        rule_token.len() + parts.iter().map(|part| part.len() + 1).sum::<usize>(),
    );
    key.push_str(rule_token);
    for part in parts {
        key.push(KEY_SEPARATOR);
        escape_key_part(part, &mut key);
    }
    key
}

/// The canonical keys of `findings`, in input order, with an occurrence
/// suffix on repeated keys.
///
/// The first finding with a key gets the plain key. The finding at
/// occurrence `k` (counted from 0, in input order) gets the extra part `~k`.
/// Two key sets built this way compare by count: when the base has two
/// occurrences and the head has three, only the third head key is absent
/// from the base.
#[must_use]
pub fn dead_code_occurrence_keys<T: IdentifiedFinding>(
    findings: &[T],
    paths: &IdentityPaths<'_>,
) -> Vec<String> {
    let mut seen: FxHashMap<String, usize> = FxHashMap::default();
    findings
        .iter()
        .map(|finding| {
            let key = finding.canonical_key(paths);
            let occurrence = seen.entry(key.clone()).or_default();
            let numbered = if *occurrence == 0 {
                key
            } else {
                format!("{key}{KEY_SEPARATOR}{OCCURRENCE_MARKER}{occurrence}")
            };
            *occurrence += 1;
            numbered
        })
        .collect()
}

/// Turns finding paths into identity parts.
#[derive(Debug, Clone, Copy)]
pub struct IdentityPaths<'a> {
    root: &'a Path,
}

impl<'a> IdentityPaths<'a> {
    /// Paths under `root` become root-relative. Other paths stay as they are.
    #[must_use]
    pub const fn new(root: &'a Path) -> Self {
        Self { root }
    }

    /// The root-relative path with forward slashes.
    #[must_use]
    pub fn key(&self, path: &Path) -> String {
        StableFileKey::from_root_relative(self.root, path)
            .as_str()
            .to_owned()
    }

    /// The sorted, unique keys of `paths`, joined by `|`.
    ///
    /// Each key escapes `%` as `%25` and `|` as `%7C` before the join, so a
    /// file name that contains `|` cannot give the same part as two files.
    /// A key without these characters does not change.
    #[must_use]
    pub fn set<'p>(&self, paths: impl IntoIterator<Item = &'p Path>) -> String {
        let mut keys: Vec<String> = paths
            .into_iter()
            .map(|path| escape_set_member(&self.key(path)))
            .collect();
        keys.sort_unstable();
        keys.dedup();
        keys.join(SET_SEPARATOR)
    }
}

/// Escape the escape character first, then the separator.
fn escape_set_member(key: &str) -> String {
    if !key.contains(['%', '|']) {
        return key.to_owned();
    }
    key.replace('%', "%25").replace('|', "%7C")
}

/// A dead-code finding that carries a stable `finding_id`.
pub trait IdentifiedFinding: Serialize {
    /// The canonical issue code of this finding.
    fn rule_token(&self) -> &'static str;

    /// The parts that name the subject of this finding. Never a line or a
    /// column.
    fn identity_parts(&self, paths: &IdentityPaths<'_>) -> Vec<String>;

    /// The canonical key of this finding: the readable form of the id input.
    /// See [`dead_code_canonical_key`].
    fn canonical_key(&self, paths: &IdentityPaths<'_>) -> String {
        let parts = self.identity_parts(paths);
        let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
        dead_code_canonical_key(self.rule_token(), &parts)
    }

    /// Line, column and span start. Used only to order findings that share
    /// a base id.
    fn tiebreak_position(&self) -> (u32, u32, u32);

    /// The stamped id, or `None` before the stamping pass.
    fn finding_id(&self) -> Option<&str>;

    /// Write the id.
    fn set_finding_id(&mut self, id: Option<String>);
}

/// How a pass treats ids that a finding already carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StampMode {
    /// Compute every id again from the full set.
    All,
    /// Keep every existing id and give an id only to findings without one.
    Missing,
}

/// Write a `finding_id` onto every dead-code finding in `results`.
///
/// `root` is the project root that makes paths root-relative. The function
/// overwrites earlier values, so a second call on the same set gives the same
/// ids. Call it on the full result set, before any filter removes findings:
/// the tiebreak suffix depends on the other findings with the same base id.
pub fn stamp_dead_code_finding_ids(results: &mut AnalysisResults, root: &Path) {
    visit_families(
        results,
        &mut StampPass {
            paths: IdentityPaths::new(root),
            mode: StampMode::All,
        },
    );
}

/// Give an id to each dead-code finding in `results` that has none, and keep
/// every existing id.
///
/// A stage that adds findings after the scope filters (type-aware refinement)
/// calls this. A full restamp there would compute tiebreak suffixes over a
/// filtered set and change the id of a kept finding. A new finding whose base
/// id is taken gets the lowest free `~k` suffix.
pub fn stamp_missing_dead_code_finding_ids(results: &mut AnalysisResults, root: &Path) {
    visit_families(
        results,
        &mut StampPass {
            paths: IdentityPaths::new(root),
            mode: StampMode::Missing,
        },
    );
}

/// Keep only the dead-code findings whose `finding_id` is in `ids`, and
/// return the ids that matched a finding.
///
/// A finding without an id is removed. Fields that are not findings (entry
/// point summary, feature flags, export usages) stay as they are.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn retain_dead_code_findings_by_id(
    results: &mut AnalysisResults,
    ids: &FxHashSet<String>,
) -> FxHashSet<String> {
    let mut pass = RetainPass {
        ids,
        matched: FxHashSet::default(),
        keep_all: false,
    };
    visit_families(results, &mut pass);
    results.security_findings.retain(|finding| {
        let keep = ids.contains(&finding.finding_id);
        if keep {
            pass.matched.insert(finding.finding_id.clone());
        }
        keep
    });
    pass.matched
}

/// The ids in `ids` that a dead-code finding in `results` carries.
///
/// The pass only reads the findings. It takes `results` mutably because it
/// shares the family visitor with the passes that write.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn present_dead_code_finding_ids(
    results: &mut AnalysisResults,
    ids: &FxHashSet<String>,
) -> FxHashSet<String> {
    let mut pass = RetainPass {
        ids,
        matched: FxHashSet::default(),
        keep_all: true,
    };
    visit_families(results, &mut pass);
    pass.matched.extend(
        results
            .security_findings
            .iter()
            .filter(|finding| ids.contains(&finding.finding_id))
            .map(|finding| finding.finding_id.clone()),
    );
    pass.matched
}

/// Whether `id` has the syntax of a current dead-code finding id:
/// `dc1:<rule>:<16 lowercase hex digits>`, with an optional `~<k>` suffix
/// where `k` is a positive decimal number.
#[must_use]
pub fn is_dead_code_finding_id(id: &str) -> bool {
    let Some(rest) = id
        .strip_prefix(DEAD_CODE_ID_SCHEME)
        .and_then(|rest| rest.strip_prefix(':'))
    else {
        return false;
    };
    let Some((rule, tail)) = rest.split_once(':') else {
        return false;
    };
    let rule_ok = !rule.is_empty()
        && rule
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'-');
    let (hash, suffix) = match tail.split_once('~') {
        Some((hash, suffix)) => (hash, Some(suffix)),
        None => (tail, None),
    };
    let hash_ok = hash.len() == HASH_HEX_DIGITS
        && hash
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    let suffix_ok = suffix.is_none_or(|suffix| {
        !suffix.is_empty()
            && !suffix.starts_with('0')
            && suffix.bytes().all(|byte| byte.is_ascii_digit())
    });
    rule_ok && hash_ok && suffix_ok
}

/// The number of hex digits in the hash part of a finding id.
const HASH_HEX_DIGITS: usize = 16;

/// One pass over every dead-code finding family.
trait FamilyVisitor {
    fn visit<T: IdentifiedFinding>(&mut self, findings: &mut Vec<T>);
}

/// Writes ids, see [`StampMode`].
struct StampPass<'a> {
    paths: IdentityPaths<'a>,
    mode: StampMode,
}

impl FamilyVisitor for StampPass<'_> {
    fn visit<T: IdentifiedFinding>(&mut self, findings: &mut Vec<T>) {
        apply(findings, &self.paths, self.mode);
    }
}

/// Records which of `ids` the findings carry. Removes the other findings
/// unless `keep_all` is set.
struct RetainPass<'a> {
    ids: &'a FxHashSet<String>,
    matched: FxHashSet<String>,
    keep_all: bool,
}

impl FamilyVisitor for RetainPass<'_> {
    fn visit<T: IdentifiedFinding>(&mut self, findings: &mut Vec<T>) {
        let keep_all = self.keep_all;
        findings.retain(|finding| {
            let matched = finding
                .finding_id()
                .filter(|id| self.ids.contains(*id))
                .map(|id| self.matched.insert(id.to_owned()))
                .is_some();
            matched || keep_all
        });
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "one exhaustive list of finding families; splitting it would lose the compile-time guard"
)]
fn visit_families<V: FamilyVisitor>(results: &mut AnalysisResults, visitor: &mut V) {
    // No `..` rest pattern: a new field fails to compile here until it is
    // classified as a finding family or as metadata.
    let AnalysisResults {
        unused_files,
        unused_exports,
        unused_types,
        private_type_leaks,
        deprecated_exports_in_use,
        unused_dependencies,
        unused_dev_dependencies,
        unused_optional_dependencies,
        unused_enum_members,
        unused_class_members,
        unused_store_members,
        unresolved_imports,
        unlisted_dependencies,
        duplicate_exports,
        type_only_dependencies,
        test_only_dependencies,
        dev_dependencies_in_production,
        circular_dependencies,
        package_cycles,
        re_export_cycles,
        boundary_violations,
        boundary_coverage_violations,
        boundary_call_violations,
        policy_violations,
        stale_suppressions,
        unused_catalog_entries,
        empty_catalog_groups,
        unresolved_catalog_references,
        unused_dependency_overrides,
        misconfigured_dependency_overrides,
        invalid_client_exports,
        mixed_client_server_barrels,
        misplaced_directives,
        unprovided_injects,
        unrendered_components,
        route_collisions,
        dynamic_segment_name_conflicts,
        unused_component_props,
        absent_component_props,
        unused_component_emits,
        unused_component_inputs,
        unused_component_outputs,
        unused_svelte_events,
        unused_server_actions,
        unused_load_data_keys,
        prop_drilling_chains,
        thin_wrappers,
        duplicate_prop_shapes,
        // Security findings carry their own `finding_id` from
        // `fallow_security::identity`. The other fields are not findings.
        security_findings: _,
        security_unresolved_edge_files: _,
        security_unresolved_callee_sites: _,
        security_unresolved_callee_diagnostics: _,
        unused_load_data_keys_global_abstain: _,
        suppression_count: _,
        unused_component_props_exempted: _,
        active_suppressions: _,
        feature_flags: _,
        export_usages: _,
        entry_point_summary: _,
        render_fan_in: _,
        react_component_intel: _,
        semantic_framework_contracts: _,
    } = results;

    visitor.visit(unused_files);
    visitor.visit(unused_exports);
    visitor.visit(unused_types);
    visitor.visit(private_type_leaks);
    visitor.visit(deprecated_exports_in_use);
    visitor.visit(unused_dependencies);
    visitor.visit(unused_dev_dependencies);
    visitor.visit(unused_optional_dependencies);
    visitor.visit(unused_enum_members);
    visitor.visit(unused_class_members);
    visitor.visit(unused_store_members);
    visitor.visit(unresolved_imports);
    visitor.visit(unlisted_dependencies);
    visitor.visit(duplicate_exports);
    visitor.visit(type_only_dependencies);
    visitor.visit(test_only_dependencies);
    visitor.visit(dev_dependencies_in_production);
    visitor.visit(circular_dependencies);
    visitor.visit(package_cycles);
    visitor.visit(re_export_cycles);
    visitor.visit(boundary_violations);
    visitor.visit(boundary_coverage_violations);
    visitor.visit(boundary_call_violations);
    visitor.visit(policy_violations);
    visitor.visit(stale_suppressions);
    visitor.visit(unused_catalog_entries);
    visitor.visit(empty_catalog_groups);
    visitor.visit(unresolved_catalog_references);
    visitor.visit(unused_dependency_overrides);
    visitor.visit(misconfigured_dependency_overrides);
    visitor.visit(invalid_client_exports);
    visitor.visit(mixed_client_server_barrels);
    visitor.visit(misplaced_directives);
    visitor.visit(unprovided_injects);
    visitor.visit(unrendered_components);
    visitor.visit(route_collisions);
    visitor.visit(dynamic_segment_name_conflicts);
    visitor.visit(unused_component_props);
    visitor.visit(absent_component_props);
    visitor.visit(unused_component_emits);
    visitor.visit(unused_component_inputs);
    visitor.visit(unused_component_outputs);
    visitor.visit(unused_svelte_events);
    visitor.visit(unused_server_actions);
    visitor.visit(unused_load_data_keys);
    visitor.visit(prop_drilling_chains);
    visitor.visit(thin_wrappers);
    visitor.visit(duplicate_prop_shapes);
}

fn apply<T: IdentifiedFinding>(findings: &mut [T], paths: &IdentityPaths<'_>, mode: StampMode) {
    match mode {
        StampMode::All => stamp(findings, paths),
        StampMode::Missing => stamp_missing(findings, paths),
    }
}

fn base_id<T: IdentifiedFinding>(finding: &T, paths: &IdentityPaths<'_>) -> String {
    let parts = finding.identity_parts(paths);
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    dead_code_finding_id(finding.rule_token(), &parts)
}

/// The order of findings that share a base id: position first, then the
/// serialized finding, so the order does not depend on the input order.
fn tiebreak_key<T: IdentifiedFinding>(finding: &T) -> ((u32, u32, u32), String) {
    (
        finding.tiebreak_position(),
        serde_json::to_string(finding).unwrap_or_default(),
    )
}

/// Stamp one finding family. The result does not depend on the input order.
fn stamp<T: IdentifiedFinding>(findings: &mut [T], paths: &IdentityPaths<'_>) {
    let mut groups: FxHashMap<String, Vec<usize>> = FxHashMap::default();
    for (index, finding) in findings.iter_mut().enumerate() {
        finding.set_finding_id(None);
        groups
            .entry(base_id(finding, paths))
            .or_default()
            .push(index);
    }
    for (base, mut members) in groups {
        if members.len() > 1 {
            members.sort_by_cached_key(|&index| tiebreak_key(&findings[index]));
        }
        for (position, index) in members.into_iter().enumerate() {
            let id = if position == 0 {
                base.clone()
            } else {
                format!("{base}~{position}")
            };
            findings[index].set_finding_id(Some(id));
        }
    }
}

/// Give an id to each finding of one family that has none.
fn stamp_missing<T: IdentifiedFinding>(findings: &mut [T], paths: &IdentityPaths<'_>) {
    let mut missing: Vec<usize> = (0..findings.len())
        .filter(|&index| findings[index].finding_id().is_none())
        .collect();
    if missing.is_empty() {
        return;
    }
    let mut taken: FxHashSet<String> = findings
        .iter()
        .filter_map(|finding| finding.finding_id().map(str::to_owned))
        .collect();
    missing.sort_by_cached_key(|&index| tiebreak_key(&findings[index]));
    for index in missing {
        let base = base_id(&findings[index], paths);
        let mut id = base.clone();
        let mut suffix = 0_usize;
        while taken.contains(&id) {
            suffix += 1;
            id = format!("{base}~{suffix}");
        }
        taken.insert(id.clone());
        findings[index].set_finding_id(Some(id));
    }
}

/// Implement [`IdentifiedFinding`] for a type with a `finding_id` field.
macro_rules! identified {
    (
        $ty:ty,
        token: |$t:ident| $token:expr,
        parts: |$f:ident, $p:ident| $parts:expr,
        position: |$g:ident| $position:expr $(,)?
    ) => {
        impl IdentifiedFinding for $ty {
            fn rule_token(&self) -> &'static str {
                let $t = self;
                $token
            }

            fn identity_parts(&self, $p: &IdentityPaths<'_>) -> Vec<String> {
                let $f = self;
                $parts
            }

            fn tiebreak_position(&self) -> (u32, u32, u32) {
                let $g = self;
                $position
            }

            fn finding_id(&self) -> Option<&str> {
                self.finding_id.as_deref()
            }

            fn set_finding_id(&mut self, id: Option<String>) {
                self.finding_id = id;
            }
        }
    };
}

identified!(
    UnusedFileFinding,
    token: |_t| "unused-file",
    parts: |f, p| vec![p.key(&f.file.path)],
    position: |_g| (0, 0, 0),
);

identified!(
    UnusedExportFinding,
    token: |_t| "unused-export",
    parts: |f, p| vec![p.key(&f.export.path), f.export.export_name.clone()],
    position: |g| (g.export.line, g.export.col, g.export.span_start),
);

identified!(
    UnusedTypeFinding,
    token: |_t| "unused-type",
    parts: |f, p| vec![p.key(&f.export.path), f.export.export_name.clone()],
    position: |g| (g.export.line, g.export.col, g.export.span_start),
);

identified!(
    PrivateTypeLeakFinding,
    token: |_t| "private-type-leak",
    parts: |f, p| vec![
        p.key(&f.leak.path),
        f.leak.export_name.clone(),
        f.leak.type_name.clone(),
    ],
    position: |g| (g.leak.line, g.leak.col, g.leak.span_start),
);

identified!(
    DeprecatedExportInUseFinding,
    token: |_t| "deprecated-export-in-use",
    parts: |f, p| vec![p.key(&f.export.path), f.export.export_name.clone()],
    position: |g| (g.export.line, g.export.col, g.export.span_start),
);

identified!(
    UnusedDependencyFinding,
    token: |_t| "unused-dependency",
    parts: |f, p| vec![p.key(&f.dep.path), f.dep.package_name.clone()],
    position: |g| (g.dep.line, 0, 0),
);

identified!(
    UnusedDevDependencyFinding,
    token: |_t| "unused-dev-dependency",
    parts: |f, p| vec![p.key(&f.dep.path), f.dep.package_name.clone()],
    position: |g| (g.dep.line, 0, 0),
);

identified!(
    UnusedOptionalDependencyFinding,
    token: |_t| "unused-optional-dependency",
    parts: |f, p| vec![p.key(&f.dep.path), f.dep.package_name.clone()],
    position: |g| (g.dep.line, 0, 0),
);

identified!(
    TypeOnlyDependencyFinding,
    token: |_t| "type-only-dependency",
    parts: |f, p| vec![p.key(&f.dep.path), f.dep.package_name.clone()],
    position: |g| (g.dep.line, 0, 0),
);

identified!(
    TestOnlyDependencyFinding,
    token: |_t| "test-only-dependency",
    parts: |f, p| vec![p.key(&f.dep.path), f.dep.package_name.clone()],
    position: |g| (g.dep.line, 0, 0),
);

identified!(
    DevDependencyInProductionFinding,
    token: |_t| "dev-dependency-in-production",
    parts: |f, p| vec![p.key(&f.dep.path), f.dep.package_name.clone()],
    position: |g| (g.dep.line, 0, 0),
);

identified!(
    UnlistedDependencyFinding,
    token: |_t| "unlisted-dependency",
    parts: |f, _p| vec![f.dep.package_name.clone()],
    position: |_g| (0, 0, 0),
);

identified!(
    UnusedEnumMemberFinding,
    token: |_t| "unused-enum-member",
    parts: |f, p| member_parts(&f.member, p),
    position: |g| (g.member.line, g.member.col, 0),
);

identified!(
    UnusedClassMemberFinding,
    token: |_t| "unused-class-member",
    parts: |f, p| member_parts(&f.member, p),
    position: |g| (g.member.line, g.member.col, 0),
);

identified!(
    UnusedStoreMemberFinding,
    token: |_t| "unused-store-member",
    parts: |f, p| member_parts(&f.member, p),
    position: |g| (g.member.line, g.member.col, 0),
);

fn member_parts(member: &crate::results::UnusedMember, paths: &IdentityPaths<'_>) -> Vec<String> {
    vec![
        paths.key(&member.path),
        member.parent_name.clone(),
        member.member_name.clone(),
    ]
}

identified!(
    UnresolvedImportFinding,
    token: |_t| "unresolved-import",
    parts: |f, p| vec![p.key(&f.import.path), f.import.specifier.clone()],
    position: |g| (g.import.line, g.import.col, 0),
);

identified!(
    DuplicateExportFinding,
    token: |_t| "duplicate-export",
    parts: |f, p| vec![
        f.export.export_name.clone(),
        p.set(f.export.locations.iter().map(|location| location.path.as_path())),
    ],
    position: |g| g
        .export
        .locations
        .first()
        .map_or((0, 0, 0), |location| (location.line, location.col, 0)),
);

identified!(
    CircularDependencyFinding,
    token: |_t| "circular-dependency",
    parts: |f, p| vec![p.set(f.cycle.files.iter().map(std::path::PathBuf::as_path))],
    position: |g| (g.cycle.line, g.cycle.col, 0),
);

identified!(
    PackageCycleFinding,
    token: |_t| "package-cycle",
    parts: |f, p| vec![p.set(f.cycle.package_roots.iter().map(std::path::PathBuf::as_path))],
    position: |_g| (0, 0, 0),
);

identified!(
    ReExportCycleFinding,
    token: |_t| "re-export-cycle",
    parts: |f, p| vec![
        re_export_cycle_kind(f.cycle.kind).to_owned(),
        p.set(f.cycle.files.iter().map(std::path::PathBuf::as_path)),
    ],
    position: |_g| (0, 0, 0),
);

/// The wire spelling of the cycle kind, so the part matches the JSON value.
const fn re_export_cycle_kind(kind: ReExportCycleKind) -> &'static str {
    match kind {
        ReExportCycleKind::MultiNode => "multi-node",
        ReExportCycleKind::SelfLoop => "self-loop",
    }
}

identified!(
    BoundaryViolationFinding,
    token: |_t| "boundary-violation",
    parts: |f, p| vec![p.key(&f.violation.from_path), p.key(&f.violation.to_path)],
    position: |g| (g.violation.line, g.violation.col, 0),
);

identified!(
    BoundaryCoverageViolationFinding,
    token: |_t| "boundary-coverage",
    parts: |f, p| vec![p.key(&f.violation.path)],
    position: |g| (g.violation.line, g.violation.col, 0),
);

identified!(
    BoundaryCallViolationFinding,
    token: |_t| "boundary-call-violation",
    parts: |f, p| vec![p.key(&f.violation.path), f.violation.callee.clone()],
    position: |g| (g.violation.line, g.violation.col, 0),
);

identified!(
    PolicyViolationFinding,
    token: |_t| "policy-violation",
    parts: |f, p| vec![
        p.key(&f.violation.path),
        f.violation.pack.clone(),
        f.violation.rule_id.clone(),
        f.violation.matched.clone(),
    ],
    position: |g| (g.violation.line, g.violation.col, 0),
);

identified!(
    StaleSuppression,
    token: |t| if t.missing_reason {
        "missing-suppression-reason"
    } else {
        "stale-suppression"
    },
    parts: |f, p| suppression_parts(f, p),
    position: |g| (g.line, g.col, 0),
);

/// Path, origin kind, issue kind (or `*`), and scope or export name. The
/// reason text is not a part: adding a reason must not change the id.
fn suppression_parts(suppression: &StaleSuppression, paths: &IdentityPaths<'_>) -> Vec<String> {
    let path = paths.key(&suppression.path);
    match &suppression.origin {
        SuppressionOrigin::Comment {
            issue_kind,
            is_file_level,
            ..
        } => vec![
            path,
            "comment".to_owned(),
            issue_kind.clone().unwrap_or_else(|| ANY_KIND.to_owned()),
            if *is_file_level { "file" } else { "line" }.to_owned(),
        ],
        SuppressionOrigin::JsdocTag { export_name, .. } => vec![
            path,
            "jsdoc_tag".to_owned(),
            ANY_KIND.to_owned(),
            export_name.clone(),
        ],
    }
}

identified!(
    UnusedCatalogEntryFinding,
    token: |_t| "unused-catalog-entry",
    parts: |f, p| vec![
        p.key(&f.entry.path),
        f.entry.catalog_name.clone(),
        f.entry.entry_name.clone(),
    ],
    position: |g| (g.entry.line, 0, 0),
);

identified!(
    EmptyCatalogGroupFinding,
    token: |_t| "empty-catalog-group",
    parts: |f, p| vec![p.key(&f.group.path), f.group.catalog_name.clone()],
    position: |g| (g.group.line, 0, 0),
);

identified!(
    UnresolvedCatalogReferenceFinding,
    token: |_t| "unresolved-catalog-reference",
    parts: |f, p| vec![
        p.key(&f.reference.path),
        f.reference.catalog_name.clone(),
        f.reference.entry_name.clone(),
    ],
    position: |g| (g.reference.line, 0, 0),
);

identified!(
    UnusedDependencyOverrideFinding,
    token: |_t| "unused-dependency-override",
    parts: |f, _p| vec![override_source(f.entry.source).to_owned(), f.entry.raw_key.clone()],
    position: |g| (g.entry.line, 0, 0),
);

identified!(
    MisconfiguredDependencyOverrideFinding,
    token: |_t| "misconfigured-dependency-override",
    parts: |f, _p| vec![override_source(f.entry.source).to_owned(), f.entry.raw_key.clone()],
    position: |g| (g.entry.line, 0, 0),
);

/// The wire spelling of the override source, so the part matches the JSON
/// value.
const fn override_source(source: DependencyOverrideSource) -> &'static str {
    match source {
        DependencyOverrideSource::PnpmWorkspaceYaml => "pnpm-workspace.yaml",
        DependencyOverrideSource::PnpmPackageJson => "package.json",
    }
}

identified!(
    InvalidClientExportFinding,
    token: |_t| "invalid-client-export",
    parts: |f, p| vec![p.key(&f.export.path), f.export.export_name.clone()],
    position: |g| (g.export.line, g.export.col, 0),
);

identified!(
    MixedClientServerBarrelFinding,
    token: |_t| "mixed-client-server-barrel",
    parts: |f, p| vec![p.key(&f.barrel.path)],
    position: |g| (g.barrel.line, g.barrel.col, 0),
);

identified!(
    MisplacedDirectiveFinding,
    token: |_t| "misplaced-directive",
    parts: |f, p| vec![
        p.key(&f.directive_site.path),
        f.directive_site.directive.clone(),
    ],
    position: |g| (g.directive_site.line, g.directive_site.col, 0),
);

identified!(
    UnprovidedInjectFinding,
    token: |_t| "unprovided-inject",
    parts: |f, p| vec![p.key(&f.inject.path), f.inject.key_name.clone()],
    position: |g| (g.inject.line, g.inject.col, 0),
);

identified!(
    UnrenderedComponentFinding,
    token: |_t| "unrendered-component",
    parts: |f, p| vec![p.key(&f.component.path), f.component.component_name.clone()],
    position: |g| (g.component.line, g.component.col, 0),
);

identified!(
    RouteCollisionFinding,
    token: |_t| "route-collision",
    parts: |f, p| vec![p.key(&f.collision.path), f.collision.url.clone()],
    position: |g| (g.collision.line, g.collision.col, 0),
);

identified!(
    DynamicSegmentNameConflictFinding,
    token: |_t| "dynamic-segment-name-conflict",
    parts: |f, p| vec![p.key(&f.conflict.path), f.conflict.position.clone()],
    position: |g| (g.conflict.line, g.conflict.col, 0),
);

identified!(
    UnusedComponentPropFinding,
    token: |_t| "unused-component-prop",
    parts: |f, p| vec![
        p.key(&f.prop.path),
        f.prop.component_name.clone(),
        f.prop.prop_name.clone(),
    ],
    position: |g| (g.prop.line, g.prop.col, 0),
);
identified!(
    AbsentComponentPropFinding,
    token: |_t| "absent-component-prop",
    parts: |f, p| vec![
        p.key(&f.prop.path),
        f.prop.component_name.clone(),
        f.prop.prop_name.clone(),
    ],
    position: |g| (g.prop.line, g.prop.col, 0),
);

identified!(
    UnusedComponentEmitFinding,
    token: |_t| "unused-component-emit",
    parts: |f, p| vec![
        p.key(&f.emit.path),
        f.emit.component_name.clone(),
        f.emit.emit_name.clone(),
    ],
    position: |g| (g.emit.line, g.emit.col, 0),
);

identified!(
    UnusedComponentInputFinding,
    token: |_t| "unused-component-input",
    parts: |f, p| vec![
        p.key(&f.input.path),
        f.input.component_name.clone(),
        f.input.input_name.clone(),
    ],
    position: |g| (g.input.line, g.input.col, 0),
);

identified!(
    UnusedComponentOutputFinding,
    token: |_t| "unused-component-output",
    parts: |f, p| vec![
        p.key(&f.output.path),
        f.output.component_name.clone(),
        f.output.output_name.clone(),
    ],
    position: |g| (g.output.line, g.output.col, 0),
);

identified!(
    UnusedSvelteEventFinding,
    token: |_t| "unused-svelte-event",
    parts: |f, p| vec![
        p.key(&f.event.path),
        f.event.component_name.clone(),
        f.event.event_name.clone(),
    ],
    position: |g| (g.event.line, g.event.col, 0),
);

identified!(
    UnusedServerActionFinding,
    token: |_t| "unused-server-action",
    parts: |f, p| vec![p.key(&f.action.path), f.action.action_name.clone()],
    position: |g| (g.action.line, g.action.col, 0),
);

identified!(
    UnusedLoadDataKeyFinding,
    token: |_t| "unused-load-data-key",
    parts: |f, p| vec![p.key(&f.key.path), f.key.key_name.clone()],
    position: |g| (g.key.line, g.key.col, 0),
);

identified!(
    PropDrillingChainFinding,
    token: |_t| "prop-drilling",
    parts: |f, p| {
        let origin = f.chain.hops.first();
        vec![
            origin.map(|hop| p.key(&hop.file)).unwrap_or_default(),
            origin.map(|hop| hop.component.clone()).unwrap_or_default(),
            f.chain.prop.clone(),
        ]
    },
    position: |g| (g.chain.hops.first().map_or(0, |hop| hop.line), 0, 0),
);

identified!(
    ThinWrapperFinding,
    token: |_t| "thin-wrapper",
    parts: |f, p| vec![p.key(&f.wrapper.file), f.wrapper.component.clone()],
    position: |g| (g.wrapper.line, 0, 0),
);

identified!(
    DuplicatePropShapeFinding,
    token: |_t| "duplicate-prop-shape",
    parts: |f, p| vec![p.key(&f.shape.file), f.shape.component.clone()],
    position: |g| (g.shape.line, 0, 0),
);

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::results::{UnusedExport, UnusedFile, UnusedMember};

    // Every expected value below comes from an independent FNV-1a 64 script,
    // not from this module.

    #[test]
    fn fnv1a64_parts_golden_values() {
        assert_eq!(
            fnv1a64_parts(&["src/index.ts", "FEATURE_X", "3"]),
            "2278c9d9bd9d2dd2"
        );
        assert_eq!(
            fnv1a64_parts(&["fallow/unused-file", "src/orphan.ts"]),
            "c03b925ddb7d871a"
        );
        assert_eq!(fnv1a64_parts(&[]), "cbf29ce484222325");
    }

    #[test]
    fn fnv1a64_hex_golden_value() {
        assert_eq!(
            fnv1a64_hex(b"security/client-server-leak:src/a.ts:3:7"),
            "89b6dece9d8b96d2"
        );
    }

    #[test]
    fn finding_id_syntax_accepts_base_and_tiebreak_ids() {
        assert!(is_dead_code_finding_id(
            "dc1:unused-export:81a349a3b9ea3b15"
        ));
        assert!(is_dead_code_finding_id(
            "dc1:unused-class-member:0123456789abcdef~1"
        ));
        assert!(is_dead_code_finding_id(
            "dc1:unused-file:0123456789abcdef~12"
        ));
    }

    #[test]
    fn finding_id_syntax_refuses_other_shapes() {
        for bad in [
            "",
            "dc1",
            "dc1:unused-export",
            "dc1::0123456789abcdef",
            "dc2:unused-export:0123456789abcdef",
            "dc1:unused-export:0123456789ABCDEF",
            "dc1:unused-export:0123456789abcde",
            "dc1:unused-export:0123456789abcdef0",
            "dc1:unused-export:0123456789abcdef~",
            "dc1:unused-export:0123456789abcdef~0",
            "dc1:unused-export:0123456789abcdef~01",
            "dc1:unused-export:0123456789abcdef~x",
            "dc1:Unused-Export:0123456789abcdef",
            "dc1:unused-export:0123456789abcdef:extra",
        ] {
            assert!(!is_dead_code_finding_id(bad), "{bad:?} was accepted");
        }
    }

    #[test]
    fn retain_by_id_keeps_only_the_requested_findings() {
        let root = PathBuf::from("/repo");
        let mut results = AnalysisResults {
            unused_files: vec![
                UnusedFileFinding::with_actions(UnusedFile {
                    path: root.join("src/a.ts"),
                }),
                UnusedFileFinding::with_actions(UnusedFile {
                    path: root.join("src/b.ts"),
                }),
            ],
            ..AnalysisResults::default()
        };
        stamp_dead_code_finding_ids(&mut results, &root);
        let kept = results.unused_files[1]
            .finding_id
            .clone()
            .expect("stamped id");
        let ids: FxHashSet<String> = [kept.clone(), "dc1:unused-file:0000000000000000".to_owned()]
            .into_iter()
            .collect();

        let present = present_dead_code_finding_ids(&mut results, &ids);
        assert_eq!(results.unused_files.len(), 2, "present only reads");
        let matched = retain_dead_code_findings_by_id(&mut results, &ids);

        assert_eq!(present, matched);
        assert_eq!(matched, std::iter::once(kept.clone()).collect());
        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(
            results.unused_files[0].finding_id.as_deref(),
            Some(kept.as_str())
        );
    }

    #[test]
    fn dead_code_finding_id_golden_values() {
        assert_eq!(
            dead_code_finding_id("unused-export", &["src/utils.ts", "helper"]),
            "dc1:unused-export:81a349a3b9ea3b15"
        );
        assert_eq!(
            dead_code_finding_id("unused-file", &["src/orphan.ts"]),
            "dc1:unused-file:9fd2d414a2a9e611"
        );
        assert_eq!(
            dead_code_finding_id("unused-class-member", &["src/service.ts", "Service", "run"]),
            "dc1:unused-class-member:675fa79a4c2f244f"
        );
        assert_eq!(
            dead_code_finding_id("unused-dependency", &["package.json", "lodash"]),
            "dc1:unused-dependency:e2f217ff5a209568"
        );
        assert_eq!(
            dead_code_finding_id("duplicate-export", &["Button", "src/a.ts|src/b.ts"]),
            "dc1:duplicate-export:17c140e16d40660a"
        );
    }

    fn export(root: &Path, name: &str, line: u32) -> UnusedExportFinding {
        UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: name.to_owned(),
            is_type_only: false,
            line,
            col: 7,
            span_start: line * 10,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        })
    }

    fn member(root: &Path, line: u32) -> UnusedClassMemberFinding {
        UnusedClassMemberFinding::with_actions(UnusedMember {
            path: root.join("src/service.ts"),
            parent_name: "Service".to_owned(),
            member_name: "run".to_owned(),
            kind: crate::extract::MemberKind::ClassMethod,
            line,
            col: 2,
        })
    }

    fn ids<T: IdentifiedFinding>(findings: &[T]) -> Vec<Option<String>> {
        findings
            .iter()
            .map(|finding| finding.finding_id().map(str::to_owned))
            .collect()
    }

    #[test]
    fn stamping_uses_root_relative_paths_and_ignores_lines() {
        let root = PathBuf::from("/repo");
        let mut results = AnalysisResults {
            unused_files: vec![UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/orphan.ts"),
            })],
            unused_exports: vec![export(&root, "helper", 40)],
            ..AnalysisResults::default()
        };

        stamp_dead_code_finding_ids(&mut results, &root);

        assert_eq!(
            ids(&results.unused_files),
            vec![Some("dc1:unused-file:9fd2d414a2a9e611".to_owned())]
        );
        assert_eq!(
            ids(&results.unused_exports),
            vec![Some("dc1:unused-export:81a349a3b9ea3b15".to_owned())]
        );
    }

    #[test]
    fn duplicate_subjects_get_a_tiebreak_suffix_in_line_order() {
        let root = PathBuf::from("/repo");
        let mut results = AnalysisResults {
            unused_class_members: vec![member(&root, 30), member(&root, 10), member(&root, 20)],
            ..AnalysisResults::default()
        };

        stamp_dead_code_finding_ids(&mut results, &root);

        let base = "dc1:unused-class-member:675fa79a4c2f244f";
        assert_eq!(
            ids(&results.unused_class_members),
            vec![
                Some(format!("{base}~2")),
                Some(base.to_owned()),
                Some(format!("{base}~1")),
            ]
        );
    }

    #[test]
    fn stamping_does_not_depend_on_input_order() {
        let root = PathBuf::from("/repo");
        let forward = vec![member(&root, 10), member(&root, 20), member(&root, 30)];
        let mut reversed = forward.clone();
        reversed.reverse();
        let mut a = AnalysisResults {
            unused_class_members: forward,
            ..AnalysisResults::default()
        };
        let mut b = AnalysisResults {
            unused_class_members: reversed,
            ..AnalysisResults::default()
        };

        stamp_dead_code_finding_ids(&mut a, &root);
        stamp_dead_code_finding_ids(&mut b, &root);

        let mut a_pairs: Vec<(u32, Option<String>)> = a
            .unused_class_members
            .iter()
            .map(|finding| (finding.member.line, finding.finding_id.clone()))
            .collect();
        let mut b_pairs: Vec<(u32, Option<String>)> = b
            .unused_class_members
            .iter()
            .map(|finding| (finding.member.line, finding.finding_id.clone()))
            .collect();
        a_pairs.sort();
        b_pairs.sort();
        assert_eq!(a_pairs, b_pairs);
    }

    #[test]
    fn stamping_twice_gives_the_same_ids() {
        let root = PathBuf::from("/repo");
        let mut results = AnalysisResults {
            unused_class_members: vec![member(&root, 10), member(&root, 20)],
            unused_exports: vec![export(&root, "helper", 3)],
            ..AnalysisResults::default()
        };

        stamp_dead_code_finding_ids(&mut results, &root);
        let first = (
            ids(&results.unused_class_members),
            ids(&results.unused_exports),
        );
        stamp_dead_code_finding_ids(&mut results, &root);

        assert_eq!(
            first,
            (
                ids(&results.unused_class_members),
                ids(&results.unused_exports),
            )
        );
    }

    #[test]
    fn stamping_missing_ids_keeps_existing_ids_and_takes_a_free_suffix() {
        let root = PathBuf::from("/repo");
        let mut results = AnalysisResults {
            unused_class_members: vec![member(&root, 10), member(&root, 20)],
            ..AnalysisResults::default()
        };
        stamp_dead_code_finding_ids(&mut results, &root);
        // A filter removed the base finding; a later stage adds a new one.
        results.unused_class_members.remove(0);
        results.unused_class_members.push(member(&root, 5));

        stamp_missing_dead_code_finding_ids(&mut results, &root);

        let base = "dc1:unused-class-member:675fa79a4c2f244f";
        assert_eq!(
            ids(&results.unused_class_members),
            vec![Some(format!("{base}~1")), Some(base.to_owned())]
        );
    }

    fn duplicate_export(root: &Path, files: &[&str]) -> DuplicateExportFinding {
        DuplicateExportFinding::with_actions(crate::results::DuplicateExport {
            export_name: "Button".to_owned(),
            locations: files
                .iter()
                .map(|file| crate::results::DuplicateLocation {
                    path: root.join(file),
                    line: 1,
                    col: 0,
                })
                .collect(),
        })
    }

    #[test]
    fn a_path_set_without_special_characters_keeps_its_golden_id() {
        let root = PathBuf::from("/repo");
        let mut results = AnalysisResults {
            duplicate_exports: vec![duplicate_export(&root, &["src/b.ts", "src/a.ts"])],
            ..AnalysisResults::default()
        };

        stamp_dead_code_finding_ids(&mut results, &root);

        assert_eq!(
            ids(&results.duplicate_exports),
            vec![Some("dc1:duplicate-export:17c140e16d40660a".to_owned())]
        );
    }

    #[test]
    fn a_pipe_in_a_file_name_does_not_collide_with_two_files() {
        let root = PathBuf::from("/repo");
        let paths = IdentityPaths::new(&root);
        let one = root.join("src/a.ts|src/b.ts");
        let first = root.join("src/a.ts");
        let second = root.join("src/b.ts");

        assert_eq!(paths.set([one.as_path()]), "src/a.ts%7Csrc/b.ts");
        assert_eq!(
            paths.set([first.as_path(), second.as_path()]),
            "src/a.ts|src/b.ts"
        );
        assert_eq!(paths.set([root.join("100%.ts").as_path()]), "100%25.ts");

        let mut results = AnalysisResults {
            duplicate_exports: vec![
                duplicate_export(&root, &["src/a.ts|src/b.ts"]),
                duplicate_export(&root, &["src/a.ts", "src/b.ts"]),
            ],
            ..AnalysisResults::default()
        };
        stamp_dead_code_finding_ids(&mut results, &root);

        let stamped = ids(&results.duplicate_exports);
        assert_ne!(stamped[0], stamped[1]);
        assert!(
            stamped.iter().flatten().all(|id| !id.contains('~')),
            "the two findings must not share a base id: {stamped:?}"
        );
    }

    #[test]
    fn a_package_cycle_gets_a_golden_id_from_its_sorted_package_roots() {
        let root = PathBuf::from("/repo");
        let cycle = |roots: &[&str]| {
            PackageCycleFinding::with_actions(crate::results::PackageCycle {
                packages: vec!["@x/a".to_owned(), "@x/b".to_owned()],
                package_roots: roots.iter().map(|dir| root.join(dir)).collect(),
                length: 2,
                edges: Vec::new(),
                group_truncated: false,
            })
        };
        let mut results = AnalysisResults {
            package_cycles: vec![cycle(&["packages/b", "packages/a"])],
            ..AnalysisResults::default()
        };

        stamp_dead_code_finding_ids(&mut results, &root);

        assert_eq!(
            ids(&results.package_cycles),
            vec![Some("dc1:package-cycle:fed127e4525389ac".to_owned())]
        );
    }

    #[test]
    fn windows_separators_give_the_same_id() {
        let mut windows = AnalysisResults {
            unused_files: vec![UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("src\\orphan.ts"),
            })],
            ..AnalysisResults::default()
        };

        stamp_dead_code_finding_ids(&mut windows, Path::new(""));

        assert_eq!(
            ids(&windows.unused_files),
            vec![Some("dc1:unused-file:9fd2d414a2a9e611".to_owned())]
        );
    }

    #[test]
    fn canonical_keys_are_readable_and_escape_the_separator() {
        assert_eq!(
            dead_code_canonical_key("unused-export", &["src/utils.ts", "helper"]),
            "unused-export:src/utils.ts:helper"
        );
        assert_eq!(
            dead_code_canonical_key("unused-file", &["C:/repo/a%b.ts"]),
            "unused-file:C%3A/repo/a%25b.ts"
        );
        assert_ne!(
            dead_code_canonical_key("unused-export", &["a:b", "c"]),
            dead_code_canonical_key("unused-export", &["a", "b:c"])
        );
    }

    #[test]
    fn the_canonical_key_and_the_id_use_the_same_parts() {
        let root = PathBuf::from("/repo");
        let paths = IdentityPaths::new(&root);
        let finding = member(&root, 10);

        assert_eq!(
            finding.canonical_key(&paths),
            "unused-class-member:src/service.ts:Service:run"
        );
        assert_eq!(
            base_id(&finding, &paths),
            dead_code_finding_id("unused-class-member", &["src/service.ts", "Service", "run"])
        );
        assert_eq!(
            member(&root, 99).canonical_key(&paths),
            finding.canonical_key(&paths)
        );
    }

    #[test]
    fn occurrence_keys_number_repeated_keys_in_input_order() {
        let root = PathBuf::from("/repo");
        let paths = IdentityPaths::new(&root);
        let findings = vec![member(&root, 10), member(&root, 20), member(&root, 30)];

        assert_eq!(
            dead_code_occurrence_keys(&findings, &paths),
            vec![
                "unused-class-member:src/service.ts:Service:run".to_owned(),
                "unused-class-member:src/service.ts:Service:run:~1".to_owned(),
                "unused-class-member:src/service.ts:Service:run:~2".to_owned(),
            ]
        );
    }

    #[test]
    fn the_suppression_reason_is_not_part_of_the_id() {
        let suppression = |reason: Option<&str>| StaleSuppression {
            path: PathBuf::from("src/a.ts"),
            line: 3,
            col: 0,
            origin: SuppressionOrigin::Comment {
                issue_kind: Some("unused-export".to_owned()),
                reason: reason.map(str::to_owned),
                is_file_level: false,
                kind_known: true,
            },
            missing_reason: false,
            finding_id: None,
            actions: Vec::new(),
            effective_severity: None,
        };
        let mut results = AnalysisResults {
            stale_suppressions: vec![suppression(None)],
            ..AnalysisResults::default()
        };
        let mut with_reason = AnalysisResults {
            stale_suppressions: vec![suppression(Some("kept for the plugin API"))],
            ..AnalysisResults::default()
        };

        stamp_dead_code_finding_ids(&mut results, Path::new(""));
        stamp_dead_code_finding_ids(&mut with_reason, Path::new(""));

        assert_eq!(
            ids(&results.stale_suppressions),
            ids(&with_reason.stale_suppressions)
        );
        assert!(
            ids(&results.stale_suppressions)[0]
                .as_deref()
                .is_some_and(|id| id.starts_with("dc1:stale-suppression:"))
        );
    }
}
