use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::BTreeMap;
use std::path::Path;

use fallow_types::extract::is_anonymous_function_name;
use fallow_types::identity::{IdentifiedFinding, IdentityPaths};

use crate::duplicates::DuplicationReport;

/// Strip the project root from a path to produce a portable relative key.
///
/// Both `path` and `root` must be in the same form (both canonicalized or both
/// not) for `strip_prefix` to succeed. The analysis pipeline keeps all paths
/// non-canonicalized, so this invariant holds in practice.
fn relative_path(path: &Path, root: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
        Err(_) => {
            tracing::debug!(
                path = %path.display(),
                root = %root.display(),
                "baseline key: path is not under project root, using absolute path as key"
            );
            path.to_string_lossy().replace('\\', "/")
        }
    }
}

fn package_json_dependency_key(package_name: &str, path: &Path, root: &Path) -> String {
    format!("{}:{package_name}", relative_path(path, root))
}

fn baseline_contains_dependency(
    baseline_keys: &FxHashSet<&str>,
    package_name: &str,
    path_key: &str,
) -> bool {
    baseline_keys.contains(path_key) || baseline_keys.contains(package_name)
}

fn retain_new_by_keys<T>(
    items: &mut Vec<T>,
    baseline_keys: &[String],
    root: &Path,
    key_builder: fn(&[T], &Path) -> Vec<String>,
) {
    let baseline_keys: FxHashSet<&str> = baseline_keys.iter().map(String::as_str).collect();
    let item_keys = key_builder(items, root);
    let mut key_iter = item_keys.into_iter();
    items.retain(|_| match key_iter.next() {
        Some(key) => !baseline_keys.contains(key.as_str()),
        None => true,
    });
}

/// Call `$callback!` with every finding field of a dead-code baseline. The
/// save path and the canonical filter read this one list, so a field cannot
/// be saved without being matched.
macro_rules! with_baseline_fields {
    ($callback:ident) => {
        $callback!(
            unused_files,
            unused_exports,
            unused_types,
            private_type_leaks,
            deprecated_exports_in_use,
            unused_dependencies,
            unused_dev_dependencies,
            circular_dependencies,
            re_export_cycles,
            package_cycles,
            unused_optional_dependencies,
            unused_enum_members,
            unused_class_members,
            unused_store_members,
            unprovided_injects,
            unrendered_components,
            unused_component_props,
            absent_component_props,
            unused_component_emits,
            unused_component_inputs,
            unused_component_outputs,
            unused_svelte_events,
            unused_effect_layers,
            unused_tagged_errors,
            unproduced_catch_tags,
            unused_server_actions,
            unused_load_data_keys,
            unresolved_imports,
            unlisted_dependencies,
            duplicate_exports,
            type_only_dependencies,
            test_only_dependencies,
            dev_dependencies_in_production,
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
            route_collisions,
            dynamic_segment_name_conflicts,
            prop_drilling_chains,
            thin_wrappers,
            duplicate_prop_shapes,
        )
    };
}

mod finding_counts;
mod install_state;

use finding_counts::{baseline_finding_count, cascade_hidden_matches};
mod prune;

pub use install_state::{install_state_diagnostic, saved_node_modules_state};
pub use prune::{
    BaselinePrune, BaselinePruneRefusal, PrunedEntry, prune_dead_code_baseline,
    prune_dupes_baseline, prune_health_baseline,
};

/// The sorted canonical keys of `items`, one for each occurrence.
fn canonical_keys<T: IdentifiedFinding>(items: &[T], paths: &IdentityPaths<'_>) -> Vec<String> {
    let mut keys: Vec<String> = items.iter().map(|item| item.canonical_key(paths)).collect();
    keys.sort_unstable();
    keys
}

/// Remove the findings that a saved occurrence covers. Each saved key hides
/// one current finding with the same canonical key, in input order.
fn retain_unsaved<T: IdentifiedFinding>(
    items: &mut Vec<T>,
    saved: &[String],
    paths: &IdentityPaths<'_>,
) {
    if saved.is_empty() {
        return;
    }
    let mut remaining: FxHashMap<&str, usize> = FxHashMap::default();
    for key in saved {
        *remaining.entry(key.as_str()).or_default() += 1;
    }
    items.retain(|item| !consume_baseline_key(&mut remaining, &item.canonical_key(paths)));
}

/// Stale fraction (in percent) at which a partial-staleness warning fires.
///
/// A little drift is the normal state of a living baseline, so warning on any
/// stale entry would train people to ignore the note. A quarter of the
/// baseline matching nothing means the gate protects meaningfully less than
/// what was saved. Shared by the dead-code and health baselines so the two
/// warnings cannot drift apart.
const STALE_WARN_PERCENT: usize = 25;

/// True when `stale_entries` out of `baseline_entries` is a large enough share
/// to be worth warning about.
///
/// The threshold alone; [`BaselineStaleness::warning`] owns the surrounding
/// guards and is what commands call.
#[must_use]
pub const fn stale_share_warrants_warning(baseline_entries: usize, stale_entries: usize) -> bool {
    stale_entries > 0 && stale_entries * 100 >= baseline_entries * STALE_WARN_PERCENT
}

/// One run's view of a loaded baseline: everything needed to decide whether the
/// baseline still describes the project.
///
/// Every command that accepts `--baseline` builds one of these and asks it the
/// same two questions, so `dead-code`, `dupes` and `health` cannot answer
/// "is this baseline stale enough to say something" three different ways.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BaselineStaleness {
    /// Entries saved in the baseline file.
    pub entries: usize,
    /// Baseline entries that matched something in this run.
    pub matched: usize,
    /// Findings this run produced before the baseline filtered them. Zero means
    /// there was nothing to compare, either because the project is clean or the
    /// scope was empty, so staleness cannot be judged.
    pub current_findings: usize,
    /// True when this run analyzed only part of the project, so a
    /// whole-project baseline matches less of it for reasons that are not rot.
    pub change_scoped: bool,
}

/// Which advisory warning a loaded baseline earns, if any.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BaselineStalenessWarning {
    /// Say nothing: the baseline is fresh enough, or this run cannot judge it.
    None,
    /// Nothing in the baseline matched, and there were findings to match.
    ZeroOverlap,
    /// A large enough share of the baseline matched nothing.
    Partial,
}

impl BaselineStaleness {
    /// Entries that matched no current finding on this run.
    #[must_use]
    pub const fn stale_entries(&self) -> usize {
        self.entries.saturating_sub(self.matched)
    }

    /// The advisory warning this run prints on stderr by default.
    ///
    /// Silent for a run narrowed to part of the project, because such a run
    /// legitimately sees only a slice of a whole-project baseline and re-saving
    /// from it would drop every entry the run never looked at. Silent as well
    /// when the run produced no findings: a cleaned project and a rotted
    /// baseline look identical from here, and the re-save advice is wrong when
    /// the right move is deleting the file.
    #[must_use]
    pub const fn warning(&self) -> BaselineStalenessWarning {
        if self.change_scoped || self.entries == 0 || self.current_findings == 0 {
            return BaselineStalenessWarning::None;
        }
        if self.matched == 0 {
            return BaselineStalenessWarning::ZeroOverlap;
        }
        if stale_share_warrants_warning(self.entries, self.stale_entries()) {
            return BaselineStalenessWarning::Partial;
        }
        BaselineStalenessWarning::None
    }

    /// Whether the opt-in `--fail-on-stale-baseline` gate fires.
    ///
    /// Deliberately stricter than [`Self::warning`]: any stale entry counts.
    /// The quarter threshold exists to keep an unasked-for line from training
    /// people to ignore it, and the empty-run silence exists because an
    /// advisory cannot tell rot from success; a repository that passes the flag
    /// has asked for both. Change-scope stays the one shared suppression,
    /// because a narrowed run still cannot judge a whole-project baseline.
    #[must_use]
    pub const fn trips_gate(&self) -> bool {
        stale_baseline_gate_trips(self.entries, self.matched, self.change_scoped)
    }
    /// This run's machine-readable view of the baseline, for the JSON envelope.
    ///
    /// Every member is derived here rather than in a consumer, so the advisory
    /// threshold and the gate rule have exactly one implementation. Only
    /// `health` can follow a file move; the commands that match entries by
    /// fingerprint pass `0`.
    ///
    /// `scope_reasons` and `unrecognised_format` come from the caller because
    /// only the command knows which channels it read and which format it read
    /// the file as; this struct carries the counts the analysis needs and
    /// nothing more. `change_scoped` and `scope_reasons` must agree, which is
    /// why every caller derives the boolean from the same reason set it passes
    /// here.
    ///
    /// `unrecognised_format` widens `gate_trips` here, at the single site that
    /// builds it, so the opt-in gate, the exit code, every CI surface that reads
    /// the boolean and the MCP sentences move together: a file this command
    /// cannot read as its own suppresses nothing, which no count can express.
    /// Unlike the count rule it is not suppressed by `change_scoped`, because
    /// telling a foreign file from this command's own needs no project-wide run.
    #[must_use]
    pub fn to_envelope(
        &self,
        moved_entries: usize,
        scope_reasons: fallow_output::BaselineScopeReasons,
        unrecognised_format: bool,
    ) -> fallow_output::BaselineStaleness {
        debug_assert_eq!(
            self.change_scoped,
            !scope_reasons.is_empty(),
            "change_scoped and scope_reasons must be derived from the same predicate"
        );
        let warning = self.warning();
        fallow_output::BaselineStaleness {
            baseline_entries: self.entries,
            matched_entries: self.matched,
            stale_entries: self.stale_entries(),
            current_findings: self.current_findings,
            remaining_findings: None,
            change_scoped: self.change_scoped,
            stale: warning != BaselineStalenessWarning::None,
            warning: match warning {
                BaselineStalenessWarning::None => fallow_output::BaselineStalenessAdvisory::None,
                BaselineStalenessWarning::ZeroOverlap => {
                    fallow_output::BaselineStalenessAdvisory::ZeroOverlap
                }
                BaselineStalenessWarning::Partial => {
                    fallow_output::BaselineStalenessAdvisory::Partial
                }
            },
            gate_trips: self.trips_gate() || unrecognised_format,
            moved_entries,
            unrecognised_format,
            saved_by: None,
            format: None,
            scope_reasons,
        }
    }
}

/// [`BaselineStaleness::trips_gate`] over the three counts it reads, for
/// callers that carry the numbers in their own output type.
#[must_use]
pub const fn stale_baseline_gate_trips(
    entries: usize,
    matched: usize,
    change_scoped: bool,
) -> bool {
    !change_scoped && entries > 0 && matched < entries
}

/// Whether a baseline file is written in the format it was loaded as.
///
/// The duplication and health formats give every field a serde default, so a
/// baseline another command saved, and an object with nothing in it, both
/// deserialize into zero entries and are indistinguishable from this command's
/// own baseline saved from a project that had nothing to record. Asking which
/// keys the file actually carries separates them: a file that declares at
/// least one key of the format it was read as is that command's baseline,
/// however empty it is.
///
/// False for anything that is not a JSON object, including a file this crate
/// would refuse to deserialize; those paths report their own error before a
/// caller gets here.
#[must_use]
pub fn declares_baseline_format(json: &str, declared_keys: &[&str]) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return false;
    };
    value_declares_baseline_format(&value, declared_keys)
}

/// [`declares_baseline_format`] for a caller that has already parsed the file.
fn value_declares_baseline_format(value: &serde_json::Value, declared_keys: &[&str]) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    declared_keys.iter().any(|key| object.contains_key(*key))
}

/// Which command saved a baseline file.
///
/// Written as the top-level `kind` of every baseline this version saves, spelled
/// exactly as the envelope root kinds, so a file states which command can read
/// it instead of leaving three formats to be told apart by the keys they happen
/// to carry. A file saved before the member exists carries no `kind`, which
/// [`classify_baseline_file`] falls back from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BaselineKind {
    /// Saved by `fallow dead-code` (or `fallow check`), and by
    /// `fallow architecture`.
    ///
    /// The kind names the baseline format, not the command: `fallow
    /// architecture` runs the dead-code analysis, so it writes and reads this
    /// format although its JSON envelope has `kind: "architecture"`.
    DeadCode,
    /// Saved by `fallow dupes`.
    Dupes,
    /// Saved by `fallow health`.
    Health,
}

impl BaselineKind {
    /// The command that removes only the stale entries of the baseline at
    /// `path`.
    #[must_use]
    pub fn prune_command(self, path: &Path) -> String {
        // The prune flags resolve a relative path against the project root,
        // while `--baseline` resolves it against the working directory. An
        // absolute path names the same file in both.
        let absolute = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
        let quoted = crate::shell_quote::shell_quote(&absolute.display().to_string());
        format!(
            "fallow baselines prune --{}-baseline {quoted}",
            self.as_str()
        )
    }

    /// The token this kind is written as, which is also the command to run.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DeadCode => "dead-code",
            Self::Dupes => "dupes",
            Self::Health => "health",
        }
    }

    /// The kind a `kind` token names, or `None` for a token this version does
    /// not know (for example one that only a newer fallow writes).
    #[must_use]
    pub fn from_token(token: &str) -> Option<Self> {
        [Self::DeadCode, Self::Dupes, Self::Health]
            .into_iter()
            .find(|kind| kind.as_str() == token)
    }

    /// The keys that identify this format in a file carrying no `kind`.
    #[must_use]
    pub const fn declared_keys(self) -> &'static [&'static str] {
        match self {
            Self::DeadCode => BaselineData::REQUIRED_KEYS,
            Self::Dupes => DuplicationBaselineData::DECLARED_KEYS,
            Self::Health => HealthBaselineData::DECLARED_KEYS,
        }
    }
}

/// What a saved baseline file is, read as one command's format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaselineFileKind {
    /// This command's own baseline: it names this command in `kind`, or it
    /// carries no `kind` and at least one key of this command's format.
    Own,
    /// A baseline another command saved, carrying the `kind` as written so a
    /// message can quote it, including a token only a newer fallow writes.
    Foreign(String),
    /// No `kind` and no key of this command's format: a baseline another
    /// command saved before `kind` existed, or an object with nothing of this
    /// command's in it.
    Unrecognised,
    /// Not a JSON object, so it says nothing about which command wrote it and
    /// the caller's own parse error is the honest report.
    NotAnObject,
}

impl BaselineFileKind {
    /// The known command that saved a file another command wrote.
    ///
    /// `None` for this command's own file, for a file that names no writer, and
    /// for a `kind` token this version does not know. The CLI computes this one
    /// time per loaded baseline and uses it for the stderr note and for
    /// `baseline_staleness.saved_by`, so the two cannot disagree.
    #[must_use]
    pub fn saved_by(&self) -> Option<BaselineKind> {
        match self {
            Self::Foreign(token) => BaselineKind::from_token(token),
            Self::Own | Self::Unrecognised | Self::NotAnObject => None,
        }
    }
}

/// Why a `--save-baseline` must not overwrite the file at `save_path`, or `None`
/// when the save may proceed.
///
/// A save serializes a fresh struct over the whole file, so one command's save
/// aimed at another command's baseline destroys it with nothing left to recover
/// from. Read from the destination's own `kind`: a file saved before that member
/// existed carries nothing to identify it and is still overwritten silently, and
/// an unreadable or absent destination is not a guard condition, exactly as the
/// health identity-overwrite guard treats one.
#[must_use]
pub fn refuse_baseline_kind_overwrite(save_path: &Path, saving: BaselineKind) -> Option<String> {
    let existing = std::fs::read_to_string(save_path).ok()?;
    let BaselineFileKind::Foreign(found) = classify_baseline_file(&existing, saving) else {
        return None;
    };
    Some(format!(
        "refusing to overwrite the baseline at {}: it was saved by `fallow {found}` and this is a \
         `fallow {}` save, which would destroy it. Save each command's baseline to its own path.",
        save_path.display(),
        saving.as_str(),
    ))
}

/// The `scope_reasons` a dead-code baseline file recorded when it was saved.
/// Empty for a whole-project save, a legacy file, or a file that is not JSON.
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

/// Which command a saved baseline file belongs to, read as `expected`'s format.
///
/// `kind` decides whenever the file carries one, which is every baseline saved
/// from this version onward. A file without one predates the member, so the keys
/// it carries decide instead, exactly as [`declares_baseline_format`] documents.
///
/// Reads the raw file rather than a deserialized struct, because the one format
/// with required fields (`dead-code`) has to answer this before its own parse
/// error hides the answer.
#[must_use]
pub fn classify_baseline_file(json: &str, expected: BaselineKind) -> BaselineFileKind {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return BaselineFileKind::NotAnObject;
    };
    classify_baseline_value(&value, expected)
}

/// [`classify_baseline_file`] for a caller that parses the file itself.
///
/// The `dead-code` read needs both this answer and the deserialized struct, and
/// its format is the one with required fields, so it parses once into a value and
/// asks here rather than paying for a second parse of the same bytes.
#[must_use]
pub fn classify_baseline_value(
    value: &serde_json::Value,
    expected: BaselineKind,
) -> BaselineFileKind {
    let Some(object) = value.as_object() else {
        return BaselineFileKind::NotAnObject;
    };
    match object.get("kind").and_then(serde_json::Value::as_str) {
        Some(token) if token == expected.as_str() => BaselineFileKind::Own,
        Some(token) => BaselineFileKind::Foreign(token.to_owned()),
        // A `kind` that is not a string is no statement about the writer, so the
        // keys answer as they do for a file that carries no `kind` at all.
        None => {
            if value_declares_baseline_format(value, expected.declared_keys()) {
                BaselineFileKind::Own
            } else {
                BaselineFileKind::Unrecognised
            }
        }
    }
}

/// The key scheme of a dead-code baseline that stores canonical keys.
///
/// A baseline with `"identity": "dc1"` stores, for each finding, the canonical
/// key from [`fallow_types::identity`] (`<rule>:<path>:<name>...`, never a
/// line), once for each occurrence. N saved occurrences of a key hide at most
/// N current findings with that key. A baseline without `identity` is a legacy
/// baseline: each entry keeps its old form (some old forms hold a line) and
/// hides every current finding with that exact key.
pub const BASELINE_KEY_SCHEME: &str = fallow_types::identity::DEAD_CODE_ID_SCHEME;

/// Baseline data for comparison.
///
/// The per-field notes below give the legacy key forms. A baseline with an
/// `identity` stores canonical keys in every field; see
/// [`BASELINE_KEY_SCHEME`].
#[derive(serde::Serialize, serde::Deserialize)]
pub struct BaselineData {
    /// The command that saved this file, written on every save and never read
    /// back through this struct: [`classify_baseline_file`] reads it off the raw
    /// file, before the required fields below can reject another command's
    /// baseline. `skip_deserializing` keeps a `kind` only a newer fallow writes
    /// from turning a loadable file into a parse error.
    #[serde(default, skip_deserializing, skip_serializing_if = "Option::is_none")]
    kind: Option<BaselineKind>,
    /// Analysis identity plus install state; legacy files read as syntactic,
    /// never silently compared with type-aware output.
    #[serde(default)]
    analysis_identity: install_state::BaselineAnalysisIdentity,
    /// The key scheme of the entries: [`BASELINE_KEY_SCHEME`] for canonical,
    /// count-matched keys, absent for a legacy baseline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identity: Option<String>,
    /// The `scope_reasons` of the run that saved this file, absent for a
    /// whole-project save. A listed reason means the file holds only the
    /// findings in that scope, so a wider run that loads it can report old
    /// findings as new. Read back with [`saved_scope_reasons`].
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    scope_reasons: Vec<String>,
    unused_files: Vec<String>,
    unused_exports: Vec<String>,
    unused_types: Vec<String>,
    #[serde(default)]
    private_type_leaks: Vec<String>,
    /// Deprecated exports still in use, keyed by `path:export_name`.
    #[serde(default)]
    deprecated_exports_in_use: Vec<String>,
    /// Unused dependencies, keyed by `package.json:package_name`. Legacy
    /// bare `package_name` keys are still matched for back-compat with
    /// baselines saved by older fallow versions.
    unused_dependencies: Vec<String>,
    /// Unused dev dependencies, keyed by `package.json:package_name`. Legacy
    /// bare `package_name` keys are still matched for back-compat with
    /// baselines saved by older fallow versions.
    unused_dev_dependencies: Vec<String>,
    /// Circular dependency chains, keyed by sorted file paths joined with `->`.
    #[serde(default)]
    circular_dependencies: Vec<String>,
    /// Re-export cycles, keyed by `kind:sorted_file_paths_joined_with_<->`
    /// (where `kind` is `multi-node` or `self-loop`). The kind prefix keeps
    /// self-loops from keyspace-colliding with future single-file multi-node
    /// shapes.
    #[serde(default)]
    re_export_cycles: Vec<String>,
    /// Package cycles, keyed by package names in canonical cycle order joined
    /// with `->`.
    #[serde(default)]
    package_cycles: Vec<String>,
    /// Unused optional dependencies, keyed by `package.json:package_name`.
    /// Legacy bare `package_name` keys are still matched for back-compat
    /// with baselines saved by older fallow versions.
    #[serde(default)]
    unused_optional_dependencies: Vec<String>,
    /// Unused enum members, keyed by `file:parent.member`.
    #[serde(default)]
    unused_enum_members: Vec<String>,
    /// Unused class members, keyed by `file:parent.member`.
    #[serde(default)]
    unused_class_members: Vec<String>,
    /// Unused store members, keyed by `file:parent.member`.
    #[serde(default)]
    unused_store_members: Vec<String>,
    /// Unprovided injects, keyed by `file:key_name`.
    #[serde(default)]
    unprovided_injects: Vec<String>,
    /// Unrendered components, keyed by `file:component_name`.
    #[serde(default)]
    unrendered_components: Vec<String>,
    /// Unused component props, keyed by `file:prop_name`.
    #[serde(default)]
    unused_component_props: Vec<String>,
    #[serde(default)]
    absent_component_props: Vec<String>,
    /// Unused component emits, keyed by `file:emit_name`.
    #[serde(default)]
    unused_component_emits: Vec<String>,
    /// Unused component inputs, keyed by `file:input_name`.
    #[serde(default)]
    unused_component_inputs: Vec<String>,
    /// Unused component outputs, keyed by `file:output_name`.
    #[serde(default)]
    unused_component_outputs: Vec<String>,
    /// Unused Svelte dispatched events, keyed by `file:event_name`.
    #[serde(default)]
    unused_svelte_events: Vec<String>,
    /// Unused Effect layers, keyed by `file:layer_name`.
    #[serde(default)]
    unused_effect_layers: Vec<String>,
    /// Unused tagged errors, keyed by `file:class_name`.
    #[serde(default)]
    unused_tagged_errors: Vec<String>,
    /// Unproduced catch tags, keyed by `file:tag`.
    #[serde(default)]
    unproduced_catch_tags: Vec<String>,
    /// Unused server actions, keyed by `file:action_name`.
    #[serde(default)]
    unused_server_actions: Vec<String>,
    /// Unused SvelteKit load() data keys, keyed by `file:key_name`.
    #[serde(default)]
    unused_load_data_keys: Vec<String>,
    /// Unresolved imports, keyed by `file:specifier`.
    #[serde(default)]
    unresolved_imports: Vec<String>,
    /// Unlisted dependencies, keyed by package name.
    #[serde(default)]
    unlisted_dependencies: Vec<String>,
    /// Duplicate exports, keyed by export name.
    #[serde(default)]
    duplicate_exports: Vec<String>,
    /// Type-only dependencies, keyed by `package.json:package_name`. Legacy
    /// bare `package_name` keys are still matched for back-compat with
    /// baselines saved by older fallow versions.
    #[serde(default)]
    type_only_dependencies: Vec<String>,
    /// Test-only dependencies, keyed by `package.json:package_name`. Legacy
    /// bare `package_name` keys are still matched for back-compat with
    /// baselines saved by older fallow versions.
    #[serde(default)]
    test_only_dependencies: Vec<String>,
    /// Dev dependencies used in production, keyed by `package.json:package_name`.
    #[serde(default)]
    dev_dependencies_in_production: Vec<String>,
    /// Boundary violations, keyed by `from_path->to_path`.
    #[serde(default)]
    boundary_violations: Vec<String>,
    /// Boundary coverage violations, keyed by `path`.
    #[serde(default)]
    boundary_coverage_violations: Vec<String>,
    /// Boundary call violations, keyed by `path:callee`.
    #[serde(default)]
    boundary_call_violations: Vec<String>,
    /// Rule-pack policy violations, keyed by `path:pack/rule_id:matched`.
    #[serde(default)]
    policy_violations: Vec<String>,
    /// Stale suppressions, keyed by `file:line`.
    #[serde(default)]
    stale_suppressions: Vec<String>,
    /// Unused pnpm catalog entries, keyed by `catalog_name:entry_name`.
    #[serde(default)]
    unused_catalog_entries: Vec<String>,
    /// Empty pnpm catalog groups, keyed by `catalog_name`.
    #[serde(default)]
    empty_catalog_groups: Vec<String>,
    /// Unresolved catalog references, keyed by `path:line:catalog_name:entry_name`.
    #[serde(default)]
    unresolved_catalog_references: Vec<String>,
    /// Unused package-manager dependency overrides, keyed by `source:raw_key`.
    #[serde(default)]
    unused_dependency_overrides: Vec<String>,
    /// Misconfigured package-manager dependency overrides, keyed by `source:raw_key`.
    #[serde(default)]
    misconfigured_dependency_overrides: Vec<String>,
    /// Invalid `"use client"` exports, keyed by `path:export_name`.
    #[serde(default)]
    invalid_client_exports: Vec<String>,
    /// Mixed client/server barrels, keyed by `path:client_origin:server_origin`.
    #[serde(default)]
    mixed_client_server_barrels: Vec<String>,
    /// Misplaced `"use client"` / `"use server"` directives, keyed by
    /// `path:line:directive`.
    #[serde(default)]
    misplaced_directives: Vec<String>,
    /// Next.js route collisions, keyed by `path:url`.
    #[serde(default)]
    route_collisions: Vec<String>,
    /// Next.js dynamic-segment name conflicts, keyed by `path:position`.
    #[serde(default)]
    dynamic_segment_name_conflicts: Vec<String>,
    /// Reviewed prop chains, keyed by source component and prop with occurrence matching.
    #[serde(default)]
    prop_drilling_chains: Vec<String>,
    /// Reviewed thin wrappers, keyed by source component with occurrence matching.
    #[serde(default)]
    thin_wrappers: Vec<String>,
    /// Reviewed duplicate prop shapes, keyed by component and significant shape.
    #[serde(default)]
    duplicate_prop_shapes: Vec<String>,
}

impl BaselineData {
    /// The keys a dead-code baseline cannot load without. They carry no serde
    /// default, so a file that deserializes into this struct carries all of
    /// them, and a file carrying none of them is not a dead-code baseline
    /// however it is spelled. Deliberately a subset of what the format writes:
    /// the rest are optional and say nothing about which command wrote the file.
    pub const REQUIRED_KEYS: &'static [&'static str] = &[
        "unused_files",
        "unused_exports",
        "unused_types",
        "unused_dependencies",
        "unused_dev_dependencies",
    ];

    /// Build baseline keys from analysis results under the syntactic identity.
    pub fn from_results(results: &crate::results::AnalysisResults, root: &Path) -> Self {
        Self::from_results_with_identity(
            results,
            root,
            fallow_types::semantic::SemanticAnalysisIdentity::syntactic(),
        )
    }

    /// Build baseline keys from analysis results, stamping the given analysis
    /// identity so later loads can reject baselines produced under an
    /// incompatible analysis mode.
    ///
    /// Every entry is a canonical key, written once for each occurrence and
    /// sorted, so the file does not change when findings move.
    pub fn from_results_with_identity(
        results: &crate::results::AnalysisResults,
        root: &Path,
        analysis_identity: fallow_types::semantic::SemanticAnalysisIdentity,
    ) -> Self {
        let paths = IdentityPaths::new(root);
        macro_rules! build {
            ($($field:ident),* $(,)?) => {
                Self {
                    kind: Some(BaselineKind::DeadCode),
                    analysis_identity: analysis_identity.into(),
                    identity: Some(BASELINE_KEY_SCHEME.to_owned()),
                    scope_reasons: Vec::new(),
                    $($field: canonical_keys(&results.$field, &paths),)*
                }
            };
        }
        with_baseline_fields!(build)
    }

    /// Record the channels that narrowed the saving run.
    #[must_use]
    pub fn with_scope_reasons(mut self, reasons: fallow_output::BaselineScopeReasons) -> Self {
        self.scope_reasons = reasons
            .iter()
            .map(|reason| reason.as_str().to_owned())
            .collect();
        self
    }

    /// Build a baseline in the legacy key forms that fallow wrote before
    /// canonical keys. Tests use it to prove that legacy files still load.
    #[cfg(test)]
    pub(crate) fn legacy_from_results(
        results: &crate::results::AnalysisResults,
        root: &Path,
    ) -> Self {
        let analysis_identity = fallow_types::semantic::SemanticAnalysisIdentity::syntactic();
        let file_exports = baseline_file_export_keys(results, root);
        let member_imports = baseline_member_import_keys(results, root);
        let dependencies = baseline_dependency_keys(results, root);
        let graph = baseline_graph_keys(results, root);
        let catalog = baseline_catalog_keys(results, root);

        Self {
            kind: Some(BaselineKind::DeadCode),
            analysis_identity: analysis_identity.into(),
            identity: None,
            scope_reasons: Vec::new(),
            unused_files: file_exports.unused_files,
            unused_exports: file_exports.unused_exports,
            unused_types: file_exports.unused_types,
            private_type_leaks: file_exports.private_type_leaks,
            deprecated_exports_in_use: file_exports.deprecated_exports_in_use,
            unused_dependencies: dependencies.unused,
            unused_dev_dependencies: dependencies.unused_dev,
            circular_dependencies: graph.circular_dependencies,
            re_export_cycles: graph.re_export_cycles,
            package_cycles: graph.package_cycles,
            unused_optional_dependencies: dependencies.unused_optional,
            unused_enum_members: member_imports.unused_enum_members,
            unused_class_members: member_imports.unused_class_members,
            unused_store_members: member_imports.unused_store_members,
            unprovided_injects: member_imports.unprovided_injects,
            unrendered_components: member_imports.unrendered_components,
            unused_component_props: member_imports.unused_component_props,
            absent_component_props: member_imports.absent_component_props,
            unused_component_emits: member_imports.unused_component_emits,
            unused_component_inputs: member_imports.unused_component_inputs,
            unused_component_outputs: member_imports.unused_component_outputs,
            unused_svelte_events: member_imports.unused_svelte_events,
            unused_effect_layers: member_imports.unused_effect_layers,
            unused_tagged_errors: member_imports.unused_tagged_errors,
            unproduced_catch_tags: member_imports.unproduced_catch_tags,
            unused_server_actions: member_imports.unused_server_actions,
            unused_load_data_keys: member_imports.unused_load_data_keys,
            unresolved_imports: member_imports.unresolved_imports,
            unlisted_dependencies: dependencies.unlisted,
            duplicate_exports: member_imports.duplicate_exports,
            type_only_dependencies: dependencies.type_only,
            test_only_dependencies: dependencies.test_only,
            dev_dependencies_in_production: dependencies.dev_in_prod,
            boundary_violations: graph.boundary_violations,
            boundary_coverage_violations: graph.boundary_coverage_violations,
            boundary_call_violations: graph.boundary_call_violations,
            policy_violations: graph.policy_violations,
            stale_suppressions: member_imports.stale_suppressions,
            unused_catalog_entries: catalog.unused_catalog_entries,
            empty_catalog_groups: catalog.empty_catalog_groups,
            unresolved_catalog_references: catalog.unresolved_catalog_references,
            unused_dependency_overrides: catalog.unused_dependency_overrides,
            misconfigured_dependency_overrides: catalog.misconfigured_dependency_overrides,
            invalid_client_exports: file_exports.invalid_client_exports,
            mixed_client_server_barrels: file_exports.mixed_client_server_barrels,
            misplaced_directives: file_exports.misplaced_directives,
            route_collisions: file_exports.route_collisions,
            dynamic_segment_name_conflicts: file_exports.dynamic_segment_name_conflicts,
            prop_drilling_chains: Vec::new(),
            thin_wrappers: Vec::new(),
            duplicate_prop_shapes: Vec::new(),
        }
    }

    /// The analysis identity this baseline was captured under.
    #[must_use]
    pub const fn analysis_identity(&self) -> &fallow_types::semantic::SemanticAnalysisIdentity {
        &self.analysis_identity.semantic
    }

    /// Total number of entries across all categories.
    pub fn total_entries(&self) -> usize {
        self.unused_files.len()
            + self.unused_exports.len()
            + self.unused_types.len()
            + self.private_type_leaks.len()
            + self.deprecated_exports_in_use.len()
            + self.unused_dependencies.len()
            + self.unused_dev_dependencies.len()
            + self.circular_dependencies.len()
            + self.re_export_cycles.len()
            + self.package_cycles.len()
            + self.unused_optional_dependencies.len()
            + self.unused_enum_members.len()
            + self.unused_class_members.len()
            + self.unused_store_members.len()
            + self.unprovided_injects.len()
            + self.unrendered_components.len()
            + self.unused_component_props.len()
            + self.absent_component_props.len()
            + self.unused_component_emits.len()
            + self.unused_component_inputs.len()
            + self.unused_component_outputs.len()
            + self.unused_svelte_events.len()
            + self.unused_effect_layers.len()
            + self.unused_tagged_errors.len()
            + self.unproduced_catch_tags.len()
            + self.unused_server_actions.len()
            + self.unused_load_data_keys.len()
            + self.unresolved_imports.len()
            + self.unlisted_dependencies.len()
            + self.duplicate_exports.len()
            + self.type_only_dependencies.len()
            + self.test_only_dependencies.len()
            + self.dev_dependencies_in_production.len()
            + self.boundary_violations.len()
            + self.boundary_coverage_violations.len()
            + self.boundary_call_violations.len()
            + self.policy_violations.len()
            + self.stale_suppressions.len()
            + self.unused_catalog_entries.len()
            + self.empty_catalog_groups.len()
            + self.unresolved_catalog_references.len()
            + self.unused_dependency_overrides.len()
            + self.misconfigured_dependency_overrides.len()
            + self.invalid_client_exports.len()
            + self.mixed_client_server_barrels.len()
            + self.misplaced_directives.len()
            + self.route_collisions.len()
            + self.dynamic_segment_name_conflicts.len()
            + self.prop_drilling_chains.len()
            + self.thin_wrappers.len()
            + self.duplicate_prop_shapes.len()
    }
}

#[cfg(test)]
struct BaselineFileExportKeys {
    unused_files: Vec<String>,
    unused_exports: Vec<String>,
    unused_types: Vec<String>,
    private_type_leaks: Vec<String>,
    deprecated_exports_in_use: Vec<String>,
    invalid_client_exports: Vec<String>,
    mixed_client_server_barrels: Vec<String>,
    misplaced_directives: Vec<String>,
    route_collisions: Vec<String>,
    dynamic_segment_name_conflicts: Vec<String>,
}

#[cfg(test)]
fn baseline_file_export_keys(
    results: &crate::results::AnalysisResults,
    root: &Path,
) -> BaselineFileExportKeys {
    BaselineFileExportKeys {
        unused_files: results
            .unused_files
            .iter()
            .map(|f| relative_path(&f.file.path, root))
            .collect(),
        unused_exports: unused_export_baseline_keys(&results.unused_exports, root),
        unused_types: unused_type_baseline_keys(&results.unused_types, root),
        private_type_leaks: private_type_leak_baseline_keys(&results.private_type_leaks, root),
        deprecated_exports_in_use: results
            .deprecated_exports_in_use
            .iter()
            .map(|e| deprecated_export_key(&e.export, root))
            .collect(),
        invalid_client_exports: invalid_client_export_baseline_keys(
            &results.invalid_client_exports,
            root,
        ),
        mixed_client_server_barrels: barrel_baseline_keys(
            &results.mixed_client_server_barrels,
            root,
        ),
        misplaced_directives: directive_baseline_keys(&results.misplaced_directives, root),
        route_collisions: route_collision_baseline_keys(&results.route_collisions, root),
        dynamic_segment_name_conflicts: results
            .dynamic_segment_name_conflicts
            .iter()
            .map(|c| {
                format!(
                    "{}:{}",
                    relative_path(&c.conflict.path, root),
                    c.conflict.position
                )
            })
            .collect(),
    }
}

#[cfg(test)]
fn unused_export_baseline_keys(
    items: &[crate::results::UnusedExportFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|e| {
            format!(
                "{}:{}",
                relative_path(&e.export.path, root),
                e.export.export_name
            )
        })
        .collect()
}

#[cfg(test)]
fn unused_type_baseline_keys(
    items: &[crate::results::UnusedTypeFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|e| {
            format!(
                "{}:{}",
                relative_path(&e.export.path, root),
                e.export.export_name
            )
        })
        .collect()
}

#[cfg(test)]
fn invalid_client_export_baseline_keys(
    items: &[crate::results::InvalidClientExportFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|e| {
            format!(
                "{}:{}",
                relative_path(&e.export.path, root),
                e.export.export_name
            )
        })
        .collect()
}

#[cfg(test)]
fn private_type_leak_baseline_keys(
    items: &[crate::results::PrivateTypeLeakFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|e| {
            format!(
                "{}:{}->{}",
                relative_path(&e.leak.path, root),
                e.leak.export_name,
                e.leak.type_name
            )
        })
        .collect()
}

#[cfg(test)]
fn barrel_baseline_keys(
    items: &[crate::results::MixedClientServerBarrelFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|b| {
            format!(
                "{}:{}:{}",
                relative_path(&b.barrel.path, root),
                b.barrel.client_origin,
                b.barrel.server_origin
            )
        })
        .collect()
}

#[cfg(test)]
fn directive_baseline_keys(
    items: &[crate::results::MisplacedDirectiveFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|d| {
            format!(
                "{}:{}:{}",
                relative_path(&d.directive_site.path, root),
                d.directive_site.line,
                d.directive_site.directive
            )
        })
        .collect()
}

#[cfg(test)]
fn route_collision_baseline_keys(
    items: &[crate::results::RouteCollisionFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|c| {
            format!(
                "{}:{}",
                relative_path(&c.collision.path, root),
                c.collision.url
            )
        })
        .collect()
}

#[cfg(test)]
struct BaselineMemberImportKeys {
    unused_enum_members: Vec<String>,
    unused_class_members: Vec<String>,
    unused_store_members: Vec<String>,
    unprovided_injects: Vec<String>,
    unrendered_components: Vec<String>,
    unused_component_props: Vec<String>,
    absent_component_props: Vec<String>,
    unused_component_emits: Vec<String>,
    unused_component_inputs: Vec<String>,
    unused_component_outputs: Vec<String>,
    unused_svelte_events: Vec<String>,
    unused_effect_layers: Vec<String>,
    unused_tagged_errors: Vec<String>,
    unproduced_catch_tags: Vec<String>,
    unused_server_actions: Vec<String>,
    unused_load_data_keys: Vec<String>,
    unresolved_imports: Vec<String>,
    duplicate_exports: Vec<String>,
    stale_suppressions: Vec<String>,
}

#[cfg(test)]
fn baseline_member_import_keys(
    results: &crate::results::AnalysisResults,
    root: &Path,
) -> BaselineMemberImportKeys {
    BaselineMemberImportKeys {
        unused_enum_members: enum_member_baseline_keys(&results.unused_enum_members, root),
        unused_class_members: class_member_baseline_keys(&results.unused_class_members, root),
        unused_store_members: store_member_baseline_keys(&results.unused_store_members, root),
        unprovided_injects: inject_baseline_keys(&results.unprovided_injects, root),
        unrendered_components: component_baseline_keys(&results.unrendered_components, root),
        unused_component_props: component_prop_baseline_keys(&results.unused_component_props, root),
        absent_component_props: absent_component_prop_baseline_keys(
            &results.absent_component_props,
            root,
        ),
        unused_component_emits: component_emit_baseline_keys(&results.unused_component_emits, root),
        unused_component_inputs: component_input_baseline_keys(
            &results.unused_component_inputs,
            root,
        ),
        unused_component_outputs: component_output_baseline_keys(
            &results.unused_component_outputs,
            root,
        ),
        unused_svelte_events: svelte_event_baseline_keys(&results.unused_svelte_events, root),
        unused_effect_layers: effect_layer_baseline_keys(&results.unused_effect_layers, root),
        unused_tagged_errors: tagged_error_baseline_keys(&results.unused_tagged_errors, root),
        unproduced_catch_tags: catch_tag_baseline_keys(&results.unproduced_catch_tags, root),
        unused_server_actions: server_action_baseline_keys(&results.unused_server_actions, root),
        unused_load_data_keys: load_data_key_baseline_keys(&results.unused_load_data_keys, root),
        unresolved_imports: unresolved_import_baseline_keys(&results.unresolved_imports, root),
        duplicate_exports: results
            .duplicate_exports
            .iter()
            .map(|d| duplicate_export_key(&d.export, root))
            .collect(),
        stale_suppressions: results
            .stale_suppressions
            .iter()
            .map(|s| stale_suppression_baseline_key(s, root))
            .collect(),
    }
}

fn stale_suppression_baseline_key(
    suppression: &crate::results::StaleSuppression,
    root: &Path,
) -> String {
    let rule_id = if suppression.missing_reason {
        "missing-suppression-reason"
    } else {
        "stale-suppression"
    };
    format!(
        "{rule_id}:{}:{}",
        relative_path(&suppression.path, root),
        suppression.line
    )
}

#[cfg(test)]
fn enum_member_baseline_keys(
    items: &[crate::results::UnusedEnumMemberFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|m| unused_member_baseline_key(&m.member, root))
        .collect()
}

#[cfg(test)]
fn class_member_baseline_keys(
    items: &[crate::results::UnusedClassMemberFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|m| unused_member_baseline_key(&m.member, root))
        .collect()
}

#[cfg(test)]
fn store_member_baseline_keys(
    items: &[crate::results::UnusedStoreMemberFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|m| unused_member_baseline_key(&m.member, root))
        .collect()
}

#[cfg(test)]
fn unused_member_baseline_key(member: &crate::results::UnusedMember, root: &Path) -> String {
    format!(
        "{}:{}.{}",
        relative_path(&member.path, root),
        member.parent_name,
        member.member_name
    )
}

fn inject_baseline_keys(
    items: &[crate::results::UnprovidedInjectFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|f| {
            format!(
                "{}:{}",
                relative_path(&f.inject.path, root),
                f.inject.key_name
            )
        })
        .collect()
}

fn component_baseline_keys(
    items: &[crate::results::UnrenderedComponentFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|c| {
            format!(
                "{}:{}",
                relative_path(&c.component.path, root),
                c.component.component_name
            )
        })
        .collect()
}

fn absent_component_prop_baseline_keys(
    items: &[crate::results::AbsentComponentPropFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|finding| {
            format!(
                "{}:{}:{}",
                relative_path(&finding.prop.path, root),
                finding.prop.component_name,
                finding.prop.prop_name
            )
        })
        .collect()
}

fn component_prop_baseline_keys(
    items: &[crate::results::UnusedComponentPropFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|p| format!("{}:{}", relative_path(&p.prop.path, root), p.prop.prop_name))
        .collect()
}

fn component_emit_baseline_keys(
    items: &[crate::results::UnusedComponentEmitFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|e| format!("{}:{}", relative_path(&e.emit.path, root), e.emit.emit_name))
        .collect()
}

fn component_input_baseline_keys(
    items: &[crate::results::UnusedComponentInputFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|i| {
            format!(
                "{}:{}",
                relative_path(&i.input.path, root),
                i.input.input_name
            )
        })
        .collect()
}

fn component_output_baseline_keys(
    items: &[crate::results::UnusedComponentOutputFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|o| {
            format!(
                "{}:{}",
                relative_path(&o.output.path, root),
                o.output.output_name
            )
        })
        .collect()
}

fn effect_layer_baseline_keys(
    items: &[crate::results::UnusedEffectLayerFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|f| format!("{}:{}", relative_path(&f.layer.path, root), f.layer.name))
        .collect()
}

fn tagged_error_baseline_keys(
    items: &[crate::results::UnusedTaggedErrorFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|f| format!("{}:{}", relative_path(&f.error.path, root), f.error.name))
        .collect()
}

fn catch_tag_baseline_keys(
    items: &[crate::results::UnproducedCatchTagFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|f| {
            format!(
                "{}:{}",
                relative_path(&f.catch_tag.path, root),
                f.catch_tag.tag
            )
        })
        .collect()
}

fn svelte_event_baseline_keys(
    items: &[crate::results::UnusedSvelteEventFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|e| {
            format!(
                "{}:{}",
                relative_path(&e.event.path, root),
                e.event.event_name
            )
        })
        .collect()
}

#[cfg(test)]
fn server_action_baseline_keys(
    items: &[crate::results::UnusedServerActionFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|a| {
            format!(
                "{}:{}",
                relative_path(&a.action.path, root),
                a.action.action_name
            )
        })
        .collect()
}

#[cfg(test)]
fn load_data_key_baseline_keys(
    items: &[crate::results::UnusedLoadDataKeyFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|k| format!("{}:{}", relative_path(&k.key.path, root), k.key.key_name))
        .collect()
}

#[cfg(test)]
fn unresolved_import_baseline_keys(
    items: &[crate::results::UnresolvedImportFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|i| {
            format!(
                "{}:{}",
                relative_path(&i.import.path, root),
                i.import.specifier
            )
        })
        .collect()
}

#[cfg(test)]
struct BaselineDependencyKeys {
    unused: Vec<String>,
    unused_dev: Vec<String>,
    unused_optional: Vec<String>,
    unlisted: Vec<String>,
    type_only: Vec<String>,
    test_only: Vec<String>,
    dev_in_prod: Vec<String>,
}

#[cfg(test)]
fn baseline_dependency_keys(
    results: &crate::results::AnalysisResults,
    root: &Path,
) -> BaselineDependencyKeys {
    BaselineDependencyKeys {
        unused: results
            .unused_dependencies
            .iter()
            .map(|d| package_json_dependency_key(&d.dep.package_name, &d.dep.path, root))
            .collect(),
        unused_dev: results
            .unused_dev_dependencies
            .iter()
            .map(|d| package_json_dependency_key(&d.dep.package_name, &d.dep.path, root))
            .collect(),
        unused_optional: results
            .unused_optional_dependencies
            .iter()
            .map(|d| package_json_dependency_key(&d.dep.package_name, &d.dep.path, root))
            .collect(),
        unlisted: results
            .unlisted_dependencies
            .iter()
            .map(|d| d.dep.package_name.clone())
            .collect(),
        type_only: results
            .type_only_dependencies
            .iter()
            .map(|d| package_json_dependency_key(&d.dep.package_name, &d.dep.path, root))
            .collect(),
        test_only: results
            .test_only_dependencies
            .iter()
            .map(|d| package_json_dependency_key(&d.dep.package_name, &d.dep.path, root))
            .collect(),
        dev_in_prod: results
            .dev_dependencies_in_production
            .iter()
            .map(|d| package_json_dependency_key(&d.dep.package_name, &d.dep.path, root))
            .collect(),
    }
}

#[cfg(test)]
struct BaselineGraphKeys {
    circular_dependencies: Vec<String>,
    re_export_cycles: Vec<String>,
    package_cycles: Vec<String>,
    boundary_violations: Vec<String>,
    boundary_coverage_violations: Vec<String>,
    boundary_call_violations: Vec<String>,
    policy_violations: Vec<String>,
}

#[cfg(test)]
fn baseline_graph_keys(
    results: &crate::results::AnalysisResults,
    root: &Path,
) -> BaselineGraphKeys {
    BaselineGraphKeys {
        circular_dependencies: results
            .circular_dependencies
            .iter()
            .map(|c| circular_dep_key(&c.cycle, root))
            .collect(),
        re_export_cycles: results
            .re_export_cycles
            .iter()
            .map(|c| re_export_cycle_key(&c.cycle, root))
            .collect(),
        package_cycles: results
            .package_cycles
            .iter()
            .map(|c| package_cycle_key(&c.cycle))
            .collect(),
        boundary_violations: results
            .boundary_violations
            .iter()
            .map(|v| boundary_violation_key(&v.violation, root))
            .collect(),
        boundary_coverage_violations: results
            .boundary_coverage_violations
            .iter()
            .map(|v| relative_path(&v.violation.path, root))
            .collect(),
        boundary_call_violations: results
            .boundary_call_violations
            .iter()
            .map(|v| boundary_call_violation_key(&v.violation, root))
            .collect(),
        policy_violations: results
            .policy_violations
            .iter()
            .map(|v| policy_violation_key(&v.violation, root))
            .collect(),
    }
}

#[cfg(test)]
struct BaselineCatalogKeys {
    unused_catalog_entries: Vec<String>,
    empty_catalog_groups: Vec<String>,
    unresolved_catalog_references: Vec<String>,
    unused_dependency_overrides: Vec<String>,
    misconfigured_dependency_overrides: Vec<String>,
}

#[cfg(test)]
fn baseline_catalog_keys(
    results: &crate::results::AnalysisResults,
    root: &Path,
) -> BaselineCatalogKeys {
    BaselineCatalogKeys {
        unused_catalog_entries: results
            .unused_catalog_entries
            .iter()
            .map(|e| format!("{}:{}", e.entry.catalog_name, e.entry.entry_name))
            .collect(),
        empty_catalog_groups: results
            .empty_catalog_groups
            .iter()
            .map(|g| g.group.catalog_name.clone())
            .collect(),
        unresolved_catalog_references: results
            .unresolved_catalog_references
            .iter()
            .map(|r| {
                format!(
                    "{}:{}:{}:{}",
                    relative_path(&r.reference.path, root),
                    r.reference.line,
                    r.reference.catalog_name,
                    r.reference.entry_name,
                )
            })
            .collect(),
        unused_dependency_overrides: results
            .unused_dependency_overrides
            .iter()
            .map(|o| format!("{}:{}", o.entry.source, o.entry.raw_key))
            .collect(),
        misconfigured_dependency_overrides: results
            .misconfigured_dependency_overrides
            .iter()
            .map(|o| format!("{}:{}", o.entry.source, o.entry.raw_key))
            .collect(),
    }
}

/// Generate a stable key for a boundary violation: `from_path->to_path`.
fn boundary_violation_key(v: &crate::results::BoundaryViolation, root: &Path) -> String {
    format!(
        "{}->{}",
        relative_path(&v.from_path, root),
        relative_path(&v.to_path, root),
    )
}

/// Generate a stable key for a boundary call violation: `path:callee`.
fn boundary_call_violation_key(v: &crate::results::BoundaryCallViolation, root: &Path) -> String {
    format!("{}:{}", relative_path(&v.path, root), v.callee)
}

/// Generate a stable key for a rule-pack policy violation:
/// `path:pack/rule_id:matched`. Line numbers are deliberately excluded so a
/// baselined finding survives unrelated edits above it.
#[cfg(test)]
fn policy_violation_key(v: &crate::results::PolicyViolation, root: &Path) -> String {
    format!(
        "{}:{}/{}:{}",
        relative_path(&v.path, root),
        v.pack,
        v.rule_id,
        v.matched
    )
}

/// Generate a stable key for a duplicate export: `name|sorted_paths`.
fn duplicate_export_key(dup: &crate::results::DuplicateExport, root: &Path) -> String {
    let mut locs: Vec<String> = dup
        .locations
        .iter()
        .map(|l| relative_path(&l.path, root))
        .collect();
    locs.sort();
    format!("{}|{}", dup.export_name, locs.join("|"))
}

/// Generate a stable key for a circular dependency based on sorted file paths.
fn circular_dep_key(dep: &crate::results::CircularDependency, root: &Path) -> String {
    let mut paths: Vec<String> = dep.files.iter().map(|f| relative_path(f, root)).collect();
    paths.sort();
    paths.join("->")
}

/// Generate a stable key for a re-export cycle based on its discriminator
/// kind plus sorted member paths. The `kind` prefix is mandatory: without
/// it a self-loop on `src/foo.ts` would keyspace-collide with any future
/// single-file multi-node shape, and the `--baseline new` filter would
/// silently drop the new one as already-seen (panel catch #7).
fn re_export_cycle_key(cycle: &crate::results::ReExportCycle, root: &Path) -> String {
    let kind = match cycle.kind {
        crate::results::ReExportCycleKind::MultiNode => "multi-node",
        crate::results::ReExportCycleKind::SelfLoop => "self-loop",
    };
    let mut paths: Vec<String> = cycle.files.iter().map(|f| relative_path(f, root)).collect();
    paths.sort();
    format!("{kind}:{}", paths.join("<->"))
}

/// Generate a stable key for a package cycle. The package names are already
/// in canonical cycle order, and they do not depend on the example import, so
/// the key survives a change of example import.
fn package_cycle_key(cycle: &crate::results::PackageCycle) -> String {
    cycle.packages.join("->")
}

fn private_type_leak_key(leak: &crate::results::PrivateTypeLeak, root: &Path) -> String {
    format!(
        "{}:{}->{}",
        relative_path(&leak.path, root),
        leak.export_name,
        leak.type_name
    )
}

fn deprecated_export_key(
    export: &fallow_types::results::DeprecatedExportInUse,
    root: &Path,
) -> String {
    format!(
        "{}:{}",
        relative_path(&export.path, root),
        export.export_name
    )
}

fn filter_private_type_leaks(
    leaks: &mut Vec<fallow_types::output_dead_code::PrivateTypeLeakFinding>,
    baseline_keys: &[String],
    root: &Path,
) {
    let baseline_private_type_leaks: FxHashSet<&str> =
        baseline_keys.iter().map(String::as_str).collect();
    leaks.retain(|entry| {
        let key = private_type_leak_key(&entry.leak, root);
        !baseline_private_type_leaks.contains(key.as_str())
    });
}

struct BaselineFilterContext<'a> {
    baseline: &'a BaselineData,
    root: &'a Path,
}

impl BaselineFilterContext<'_> {
    fn filter_cycles_and_members(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_circular: FxHashSet<&str> = self
            .baseline
            .circular_dependencies
            .iter()
            .map(String::as_str)
            .collect();
        results.circular_dependencies.retain(|cycle| {
            let key = circular_dep_key(&cycle.cycle, self.root);
            !baseline_circular.contains(key.as_str())
        });

        let baseline_re_export_cycles: FxHashSet<&str> = self
            .baseline
            .re_export_cycles
            .iter()
            .map(String::as_str)
            .collect();
        results.re_export_cycles.retain(|cycle| {
            let key = re_export_cycle_key(&cycle.cycle, self.root);
            !baseline_re_export_cycles.contains(key.as_str())
        });

        let baseline_package_cycles: FxHashSet<&str> = self
            .baseline
            .package_cycles
            .iter()
            .map(String::as_str)
            .collect();
        results.package_cycles.retain(|cycle| {
            !baseline_package_cycles.contains(package_cycle_key(&cycle.cycle).as_str())
        });

        self.filter_unused_members(results);
        self.filter_unresolved_and_exports(results);
    }

    fn filter_unused_members(&self, results: &mut crate::results::AnalysisResults) {
        self.filter_enum_class_store_members(results);
        self.filter_component_surface_members(results);
        self.filter_route_action_members(results);
    }

    fn filter_enum_class_store_members(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_enum_members: FxHashSet<&str> = self
            .baseline
            .unused_enum_members
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_enum_members.retain(|member| {
            let key = format!(
                "{}:{}.{}",
                relative_path(&member.member.path, self.root),
                member.member.parent_name,
                member.member.member_name
            );
            !baseline_enum_members.contains(key.as_str())
        });

        let baseline_class_members: FxHashSet<&str> = self
            .baseline
            .unused_class_members
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_class_members.retain(|member| {
            let key = format!(
                "{}:{}.{}",
                relative_path(&member.member.path, self.root),
                member.member.parent_name,
                member.member.member_name
            );
            !baseline_class_members.contains(key.as_str())
        });

        let baseline_store_members: FxHashSet<&str> = self
            .baseline
            .unused_store_members
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_store_members.retain(|member| {
            let key = format!(
                "{}:{}.{}",
                relative_path(&member.member.path, self.root),
                member.member.parent_name,
                member.member.member_name
            );
            !baseline_store_members.contains(key.as_str())
        });
    }

    fn filter_component_surface_members(&self, results: &mut crate::results::AnalysisResults) {
        retain_new_by_keys(
            &mut results.unprovided_injects,
            &self.baseline.unprovided_injects,
            self.root,
            inject_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unrendered_components,
            &self.baseline.unrendered_components,
            self.root,
            component_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unused_component_props,
            &self.baseline.unused_component_props,
            self.root,
            component_prop_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.absent_component_props,
            &self.baseline.absent_component_props,
            self.root,
            absent_component_prop_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unused_component_emits,
            &self.baseline.unused_component_emits,
            self.root,
            component_emit_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unused_component_inputs,
            &self.baseline.unused_component_inputs,
            self.root,
            component_input_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unused_component_outputs,
            &self.baseline.unused_component_outputs,
            self.root,
            component_output_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unused_svelte_events,
            &self.baseline.unused_svelte_events,
            self.root,
            svelte_event_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unused_effect_layers,
            &self.baseline.unused_effect_layers,
            self.root,
            effect_layer_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unused_tagged_errors,
            &self.baseline.unused_tagged_errors,
            self.root,
            tagged_error_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unproduced_catch_tags,
            &self.baseline.unproduced_catch_tags,
            self.root,
            catch_tag_baseline_keys,
        );
    }

    fn filter_route_action_members(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_unused_server_actions: FxHashSet<&str> = self
            .baseline
            .unused_server_actions
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_server_actions.retain(|finding| {
            let key = format!(
                "{}:{}",
                relative_path(&finding.action.path, self.root),
                finding.action.action_name
            );
            !baseline_unused_server_actions.contains(key.as_str())
        });

        let baseline_unused_load_data_keys: FxHashSet<&str> = self
            .baseline
            .unused_load_data_keys
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_load_data_keys.retain(|finding| {
            let key = format!(
                "{}:{}",
                relative_path(&finding.key.path, self.root),
                finding.key.key_name
            );
            !baseline_unused_load_data_keys.contains(key.as_str())
        });
    }

    fn filter_unresolved_and_exports(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_unresolved: FxHashSet<&str> = self
            .baseline
            .unresolved_imports
            .iter()
            .map(String::as_str)
            .collect();
        results.unresolved_imports.retain(|import| {
            let key = format!(
                "{}:{}",
                relative_path(&import.import.path, self.root),
                import.import.specifier
            );
            !baseline_unresolved.contains(key.as_str())
        });

        let baseline_unlisted: FxHashSet<&str> = self
            .baseline
            .unlisted_dependencies
            .iter()
            .map(String::as_str)
            .collect();
        results
            .unlisted_dependencies
            .retain(|dep| !baseline_unlisted.contains(dep.dep.package_name.as_str()));

        let baseline_dup_exports: FxHashSet<&str> = self
            .baseline
            .duplicate_exports
            .iter()
            .map(String::as_str)
            .collect();
        results.duplicate_exports.retain(|duplicate| {
            let key = duplicate_export_key(&duplicate.export, self.root);
            !baseline_dup_exports.contains(key.as_str())
        });
    }

    fn filter_dependency_variants(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_optional_deps: FxHashSet<&str> = self
            .baseline
            .unused_optional_dependencies
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_optional_dependencies.retain(|dep| {
            let key = package_json_dependency_key(&dep.dep.package_name, &dep.dep.path, self.root);
            !baseline_contains_dependency(
                &baseline_optional_deps,
                &dep.dep.package_name,
                key.as_str(),
            )
        });

        self.filter_type_and_test_only_dependencies(results);
    }

    fn filter_type_and_test_only_dependencies(
        &self,
        results: &mut crate::results::AnalysisResults,
    ) {
        let baseline_type_only: FxHashSet<&str> = self
            .baseline
            .type_only_dependencies
            .iter()
            .map(String::as_str)
            .collect();
        results.type_only_dependencies.retain(|dep| {
            let key = package_json_dependency_key(&dep.dep.package_name, &dep.dep.path, self.root);
            !baseline_contains_dependency(&baseline_type_only, &dep.dep.package_name, key.as_str())
        });

        let baseline_test_only: FxHashSet<&str> = self
            .baseline
            .test_only_dependencies
            .iter()
            .map(String::as_str)
            .collect();
        results.test_only_dependencies.retain(|dep| {
            let key = package_json_dependency_key(&dep.dep.package_name, &dep.dep.path, self.root);
            !baseline_contains_dependency(&baseline_test_only, &dep.dep.package_name, key.as_str())
        });

        let baseline_dev_in_prod: FxHashSet<&str> = self
            .baseline
            .dev_dependencies_in_production
            .iter()
            .map(String::as_str)
            .collect();
        results.dev_dependencies_in_production.retain(|dep| {
            let key = package_json_dependency_key(&dep.dep.package_name, &dep.dep.path, self.root);
            !baseline_contains_dependency(
                &baseline_dev_in_prod,
                &dep.dep.package_name,
                key.as_str(),
            )
        });
    }

    fn filter_boundaries_and_suppressions(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_boundary: FxHashSet<&str> = self
            .baseline
            .boundary_violations
            .iter()
            .map(String::as_str)
            .collect();
        results.boundary_violations.retain(|violation| {
            let key = boundary_violation_key(&violation.violation, self.root);
            !baseline_boundary.contains(key.as_str())
        });

        self.filter_boundary_details(results);
        self.filter_stale_suppressions(results);
        self.filter_invalid_client_exports(results);
        self.filter_mixed_client_server_barrels(results);
        self.filter_misplaced_directives(results);
        self.filter_route_collisions(results);
        self.filter_dynamic_segment_name_conflicts(results);
    }

    fn filter_invalid_client_exports(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_invalid: FxHashSet<&str> = self
            .baseline
            .invalid_client_exports
            .iter()
            .map(String::as_str)
            .collect();
        results.invalid_client_exports.retain(|finding| {
            let key = format!(
                "{}:{}",
                relative_path(&finding.export.path, self.root),
                finding.export.export_name
            );
            !baseline_invalid.contains(key.as_str())
        });
    }

    fn filter_mixed_client_server_barrels(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_barrels: FxHashSet<&str> = self
            .baseline
            .mixed_client_server_barrels
            .iter()
            .map(String::as_str)
            .collect();
        results.mixed_client_server_barrels.retain(|finding| {
            let key = format!(
                "{}:{}:{}",
                relative_path(&finding.barrel.path, self.root),
                finding.barrel.client_origin,
                finding.barrel.server_origin
            );
            !baseline_barrels.contains(key.as_str())
        });
    }

    fn filter_misplaced_directives(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_directives: FxHashSet<&str> = self
            .baseline
            .misplaced_directives
            .iter()
            .map(String::as_str)
            .collect();
        results.misplaced_directives.retain(|finding| {
            let key = format!(
                "{}:{}:{}",
                relative_path(&finding.directive_site.path, self.root),
                finding.directive_site.line,
                finding.directive_site.directive
            );
            !baseline_directives.contains(key.as_str())
        });
    }

    fn filter_route_collisions(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_collisions: FxHashSet<&str> = self
            .baseline
            .route_collisions
            .iter()
            .map(String::as_str)
            .collect();
        results.route_collisions.retain(|finding| {
            let key = format!(
                "{}:{}",
                relative_path(&finding.collision.path, self.root),
                finding.collision.url
            );
            !baseline_collisions.contains(key.as_str())
        });
    }

    fn filter_dynamic_segment_name_conflicts(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_conflicts: FxHashSet<&str> = self
            .baseline
            .dynamic_segment_name_conflicts
            .iter()
            .map(String::as_str)
            .collect();
        results.dynamic_segment_name_conflicts.retain(|finding| {
            let key = format!(
                "{}:{}",
                relative_path(&finding.conflict.path, self.root),
                finding.conflict.position
            );
            !baseline_conflicts.contains(key.as_str())
        });
    }

    fn filter_boundary_details(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_boundary_coverage: FxHashSet<&str> = self
            .baseline
            .boundary_coverage_violations
            .iter()
            .map(String::as_str)
            .collect();
        results.boundary_coverage_violations.retain(|violation| {
            let key = relative_path(&violation.violation.path, self.root);
            !baseline_boundary_coverage.contains(key.as_str())
        });

        let baseline_boundary_calls: FxHashSet<&str> = self
            .baseline
            .boundary_call_violations
            .iter()
            .map(String::as_str)
            .collect();
        results.boundary_call_violations.retain(|violation| {
            let key = boundary_call_violation_key(&violation.violation, self.root);
            !baseline_boundary_calls.contains(key.as_str())
        });
    }

    fn filter_stale_suppressions(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_stale: FxHashSet<&str> = self
            .baseline
            .stale_suppressions
            .iter()
            .map(String::as_str)
            .collect();
        results.stale_suppressions.retain(|suppression| {
            let key = stale_suppression_baseline_key(suppression, self.root);
            let legacy_key = format!(
                "{}:{}",
                relative_path(&suppression.path, self.root),
                suppression.line
            );
            !baseline_stale.contains(key.as_str()) && !baseline_stale.contains(legacy_key.as_str())
        });
    }

    fn filter_pnpm_entries(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_catalog: FxHashSet<&str> = self
            .baseline
            .unused_catalog_entries
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_catalog_entries.retain(|entry| {
            let key = format!("{}:{}", entry.entry.catalog_name, entry.entry.entry_name);
            !baseline_catalog.contains(key.as_str())
        });

        let baseline_empty_catalog_groups: FxHashSet<&str> = self
            .baseline
            .empty_catalog_groups
            .iter()
            .map(String::as_str)
            .collect();
        results.empty_catalog_groups.retain(|group| {
            !baseline_empty_catalog_groups.contains(group.group.catalog_name.as_str())
        });

        self.filter_pnpm_references_and_overrides(results);
    }

    fn filter_pnpm_references_and_overrides(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_unresolved: FxHashSet<&str> = self
            .baseline
            .unresolved_catalog_references
            .iter()
            .map(String::as_str)
            .collect();
        results.unresolved_catalog_references.retain(|reference| {
            let key = format!(
                "{}:{}:{}:{}",
                relative_path(&reference.reference.path, self.root),
                reference.reference.line,
                reference.reference.catalog_name,
                reference.reference.entry_name,
            );
            !baseline_unresolved.contains(key.as_str())
        });

        self.filter_pnpm_overrides(results);
    }

    fn filter_pnpm_overrides(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_unused_overrides: FxHashSet<&str> = self
            .baseline
            .unused_dependency_overrides
            .iter()
            .map(String::as_str)
            .collect();
        results
            .unused_dependency_overrides
            .retain(|override_entry| {
                let key = format!(
                    "{}:{}",
                    override_entry.entry.source, override_entry.entry.raw_key
                );
                !baseline_unused_overrides.contains(key.as_str())
            });

        let baseline_misconfigured_overrides: FxHashSet<&str> = self
            .baseline
            .misconfigured_dependency_overrides
            .iter()
            .map(String::as_str)
            .collect();
        results
            .misconfigured_dependency_overrides
            .retain(|override_entry| {
                let key = format!(
                    "{}:{}",
                    override_entry.entry.source, override_entry.entry.raw_key
                );
                !baseline_misconfigured_overrides.contains(key.as_str())
            });
    }
}

/// Filter results to only include issues not present in the baseline.
///
/// A baseline with an `identity` matches canonical keys by count. A legacy
/// baseline matches each entry in its old form, as older fallow versions did.
pub fn filter_new_issues(
    results: crate::results::AnalysisResults,
    baseline: &BaselineData,
    root: &Path,
) -> crate::results::AnalysisResults {
    if baseline.identity.is_some() {
        filter_new_issues_by_canonical_keys(results, baseline, root)
    } else {
        filter_new_issues_by_legacy_keys(results, baseline, root)
    }
}

fn filter_new_issues_by_canonical_keys(
    mut results: crate::results::AnalysisResults,
    baseline: &BaselineData,
    root: &Path,
) -> crate::results::AnalysisResults {
    let paths = IdentityPaths::new(root);
    macro_rules! retain {
        ($($field:ident),* $(,)?) => {
            $(retain_unsaved(&mut results.$field, &baseline.$field, &paths);)*
        };
    }
    with_baseline_fields!(retain);
    results
}

fn filter_new_issues_by_legacy_keys(
    mut results: crate::results::AnalysisResults,
    baseline: &BaselineData,
    root: &Path,
) -> crate::results::AnalysisResults {
    let baseline_files: FxHashSet<&str> =
        baseline.unused_files.iter().map(String::as_str).collect();
    let baseline_exports: FxHashSet<&str> =
        baseline.unused_exports.iter().map(String::as_str).collect();
    let baseline_types: FxHashSet<&str> =
        baseline.unused_types.iter().map(String::as_str).collect();
    let baseline_deps: FxHashSet<&str> = baseline
        .unused_dependencies
        .iter()
        .map(String::as_str)
        .collect();
    let baseline_dev_deps: FxHashSet<&str> = baseline
        .unused_dev_dependencies
        .iter()
        .map(String::as_str)
        .collect();

    results
        .unused_files
        .retain(|f| !baseline_files.contains(relative_path(&f.file.path, root).as_str()));
    results.unused_exports.retain(|e| {
        let key = format!(
            "{}:{}",
            relative_path(&e.export.path, root),
            e.export.export_name
        );
        !baseline_exports.contains(key.as_str())
    });
    results.unused_types.retain(|e| {
        let key = format!(
            "{}:{}",
            relative_path(&e.export.path, root),
            e.export.export_name
        );
        !baseline_types.contains(key.as_str())
    });
    filter_private_type_leaks(
        &mut results.private_type_leaks,
        &baseline.private_type_leaks,
        root,
    );
    let baseline_deprecated: FxHashSet<&str> = baseline
        .deprecated_exports_in_use
        .iter()
        .map(String::as_str)
        .collect();
    results
        .deprecated_exports_in_use
        .retain(|e| !baseline_deprecated.contains(deprecated_export_key(&e.export, root).as_str()));
    results.unused_dependencies.retain(|d| {
        let key = package_json_dependency_key(&d.dep.package_name, &d.dep.path, root);
        !baseline_contains_dependency(&baseline_deps, &d.dep.package_name, key.as_str())
    });
    results.unused_dev_dependencies.retain(|d| {
        let key = package_json_dependency_key(&d.dep.package_name, &d.dep.path, root);
        !baseline_contains_dependency(&baseline_dev_deps, &d.dep.package_name, key.as_str())
    });

    let filter = BaselineFilterContext { baseline, root };
    filter.filter_cycles_and_members(&mut results);
    filter.filter_dependency_variants(&mut results);
    filter.filter_boundaries_and_suppressions(&mut results);
    filter.filter_pnpm_entries(&mut results);

    results
}

/// What a loaded dead-code baseline did to a result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeadCodeBaselineOutcome {
    /// The file is a dead-code baseline. The findings it holds are removed.
    Applied {
        /// This run's view of the baseline.
        staleness: BaselineStaleness,
        /// True when the file has no `identity`, so its entries use the legacy
        /// key forms. `--save-baseline` rewrites it with canonical keys.
        legacy_keys: bool,
    },
    /// The file is not a dead-code baseline: another command saved it
    /// (`saved_by` names that command), or nothing in it names a writer. It
    /// suppresses nothing, so every finding stays.
    NotDeadCode {
        /// This run's view of the file: zero entries, zero matches.
        staleness: BaselineStaleness,
        /// The known command that saved the file, when the file names one.
        saved_by: Option<BaselineKind>,
    },
}

/// Why a dead-code baseline could not be applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeadCodeBaselineError {
    /// The file is not valid JSON, or a dead-code baseline with a missing or
    /// wrong field.
    Parse(String),
    /// The baseline was saved with another analysis identity, for example a
    /// type-aware run. The fields that differ are listed.
    IncompatibleIdentity(Vec<&'static str>),
}

/// Apply a dead-code baseline to a result.
///
/// The CLI (`--baseline`) and the programmatic API read a baseline through this
/// one function, so a baseline hides the same findings on every surface.
/// `content` is the text of the baseline file. `change_scoped` is true when the
/// run analyzed only part of the project; see [`BaselineStaleness`].
///
/// The file kind is read before the strict parse. Several fields of the
/// dead-code format have no default, so another command's baseline would fail
/// the parse before anything could name its writer.
///
/// # Errors
///
/// Returns [`DeadCodeBaselineError`] when the file is not valid JSON, is a
/// broken dead-code baseline, or was saved with an incompatible analysis
/// identity.
pub fn apply_dead_code_baseline(
    results: &mut crate::results::AnalysisResults,
    content: &str,
    root: &Path,
    identity: &fallow_types::semantic::SemanticAnalysisIdentity,
    change_scoped: bool,
) -> Result<DeadCodeBaselineOutcome, DeadCodeBaselineError> {
    let parsed = serde_json::from_str::<serde_json::Value>(content)
        .map_err(|err| DeadCodeBaselineError::Parse(err.to_string()))?;
    let saved_by = match classify_baseline_value(&parsed, BaselineKind::DeadCode) {
        BaselineFileKind::Own | BaselineFileKind::NotAnObject => None,
        foreign @ BaselineFileKind::Foreign(_) => Some(foreign.saved_by()),
        BaselineFileKind::Unrecognised => Some(None),
    };
    if let Some(saved_by) = saved_by {
        return Ok(DeadCodeBaselineOutcome::NotDeadCode {
            staleness: BaselineStaleness {
                entries: 0,
                matched: 0,
                current_findings: results.total_issues(),
                change_scoped,
            },
            saved_by,
        });
    }
    let baseline = serde_json::from_value::<BaselineData>(parsed)
        .map_err(|err| DeadCodeBaselineError::Parse(err.to_string()))?;
    if let Some(scheme) = baseline.identity.as_deref()
        && scheme != BASELINE_KEY_SCHEME
    {
        return Err(DeadCodeBaselineError::Parse(format!(
            "unknown baseline identity `{scheme}`: this fallow version reads `{BASELINE_KEY_SCHEME}` and legacy baselines. Regenerate the file with --save-baseline"
        )));
    }
    let incompatible = baseline.analysis_identity().incompatible_fields(identity);
    if !incompatible.is_empty() {
        return Err(DeadCodeBaselineError::IncompatibleIdentity(incompatible));
    }
    let before = baseline_finding_count(results);
    let hidden_matched = cascade_hidden_matches(results, &baseline, root);
    *results = filter_new_issues(std::mem::take(results), &baseline, root);
    Ok(DeadCodeBaselineOutcome::Applied {
        staleness: BaselineStaleness {
            entries: baseline.total_entries(),
            matched: before.saturating_sub(baseline_finding_count(results)) + hidden_matched,
            current_findings: before,
            change_scoped,
        },
        legacy_keys: baseline.identity.is_none(),
    })
}

/// Baseline data for duplication comparison.
///
/// New baselines key every clone group by `<fingerprint>:<instance count>` in
/// `normalized_clone_fingerprints`. The fingerprint hashes normalized clone
/// content, so an unrelated line shift or formatting-only edit keeps it matched,
/// while a token edit or extra copy reports a new finding.
///
/// `clone_groups` keeps the oldest location keys, while `clone_fingerprints`
/// keeps the raw-fragment keys expected by older binaries. New readers prefer
/// the normalized field. This makes baselines readable in both directions.
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct DuplicationBaselineData {
    /// The command that saved this file. See [`BaselineData::kind`] for why it
    /// is written but never read back through this struct.
    #[serde(default, skip_deserializing, skip_serializing_if = "Option::is_none")]
    kind: Option<BaselineKind>,
    /// Legacy clone group keys: sorted list of `file:start-end` per group.
    #[serde(default)]
    pub clone_groups: Vec<String>,
    /// Legacy raw-fragment keys expected by older binaries.
    #[serde(default)]
    pub clone_fingerprints: Vec<String>,
    /// Normalized content keys used by current binaries.
    #[serde(default)]
    pub normalized_clone_fingerprints: Vec<String>,
}

impl DuplicationBaselineData {
    /// The keys this format writes, for [`declares_baseline_format`].
    pub const DECLARED_KEYS: &'static [&'static str] = &[
        "clone_groups",
        "clone_fingerprints",
        "normalized_clone_fingerprints",
    ];

    /// Build a duplication baseline from the current report.
    pub fn from_report(report: &DuplicationReport, root: &Path) -> Self {
        let fingerprints =
            crate::duplicates::CloneFingerprintSet::from_groups(&report.clone_groups);
        Self {
            kind: Some(BaselineKind::Dupes),
            clone_groups: report
                .clone_groups
                .iter()
                .map(|g| clone_group_key(g, root))
                .collect(),
            clone_fingerprints: report
                .clone_groups
                .iter()
                .map(legacy_clone_group_fingerprint_key)
                .collect(),
            normalized_clone_fingerprints: report
                .clone_groups
                .iter()
                .map(|group| clone_group_fingerprint_key(group, &fingerprints))
                .collect(),
        }
    }

    /// True when the baseline holds a key that older versions gave to every
    /// clone group whose code did not parse on its own. A re-save replaces it.
    #[must_use]
    pub fn has_unparsed_collision_keys(&self) -> bool {
        self.normalized_clone_fingerprints
            .iter()
            .any(|key| crate::duplicates::is_unparsed_collision_key(key))
    }

    /// Number of baseline entries actually used for comparison.
    #[must_use]
    pub fn entry_count(&self) -> usize {
        if !self.normalized_clone_fingerprints.is_empty() {
            self.normalized_clone_fingerprints.len()
        } else if !self.clone_fingerprints.is_empty() {
            self.clone_fingerprints.len()
        } else {
            self.clone_groups.len()
        }
    }
}

/// Generate a stable key for a clone group based on its instance locations.
fn clone_group_key(group: &crate::duplicates::CloneGroup, root: &Path) -> String {
    let mut parts: Vec<String> = group
        .instances
        .iter()
        .map(|i| {
            format!(
                "{}:{}-{}",
                relative_path(&i.file, root),
                i.start_line,
                i.end_line
            )
        })
        .collect();
    parts.sort();
    parts.join("|")
}

/// Generate the normalized, location-independent key written by new baselines.
fn clone_group_fingerprint_key(
    group: &crate::duplicates::CloneGroup,
    fingerprints: &crate::duplicates::CloneFingerprintSet,
) -> String {
    fingerprints.ignored_clone_key_for_group(group)
}

/// Recreate the raw-fragment key written by baseline versions before normalized
/// clone identity. Kept only as a read fallback during migration.
fn legacy_clone_group_fingerprint_key(group: &crate::duplicates::CloneGroup) -> String {
    let representative = group
        .instances
        .iter()
        .min_by(|a, b| (a.file.as_path(), a.start_line).cmp(&(b.file.as_path(), b.start_line)))
        .map_or("", |i| i.fragment.as_str());
    let hash = if representative.as_bytes().contains(&b'\r') {
        xxhash_rust::xxh3::xxh3_64(representative.replace('\r', "").as_bytes())
    } else {
        xxhash_rust::xxh3::xxh3_64(representative.as_bytes())
    };
    format!(
        "{}{:08x}:{}",
        crate::duplicates::FINGERPRINT_PREFIX,
        hash as u32,
        group.instances.len()
    )
}

fn consume_baseline_key(remaining: &mut FxHashMap<&str, usize>, key: &str) -> bool {
    match remaining.get_mut(key) {
        Some(count) if *count > 0 => {
            *count -= 1;
            true
        }
        _ => false,
    }
}

/// Filter a duplication report to only include clone groups not present in the baseline.
///
/// Baselines carrying `normalized_clone_fingerprints` compare on normalized
/// content plus instance count. Older raw fingerprints and location keys remain
/// supported as fallbacks.
pub fn filter_new_clone_groups(
    mut report: DuplicationReport,
    baseline: &DuplicationBaselineData,
    root: &Path,
) -> DuplicationReport {
    if !baseline.normalized_clone_fingerprints.is_empty() {
        let fingerprints =
            crate::duplicates::CloneFingerprintSet::from_groups(&report.clone_groups);
        let mut remaining: FxHashMap<&str, usize> = FxHashMap::default();
        for key in &baseline.normalized_clone_fingerprints {
            *remaining.entry(key.as_str()).or_insert(0) += 1;
        }
        report.clone_groups.retain(|group| {
            let key = clone_group_fingerprint_key(group, &fingerprints);
            !consume_baseline_key(&mut remaining, &key)
        });
    } else if baseline.clone_fingerprints.is_empty() {
        let baseline_keys: FxHashSet<&str> =
            baseline.clone_groups.iter().map(String::as_str).collect();
        report.clone_groups.retain(|g| {
            let key = clone_group_key(g, root);
            !baseline_keys.contains(key.as_str())
        });
    } else {
        let mut remaining: FxHashMap<&str, usize> = FxHashMap::default();
        for key in &baseline.clone_fingerprints {
            *remaining.entry(key.as_str()).or_insert(0) += 1;
        }
        report.clone_groups.retain(|group| {
            let key = legacy_clone_group_fingerprint_key(group);
            !consume_baseline_key(&mut remaining, &key)
        });
    }

    crate::duplicates::refresh_scoped_report(&mut report, root);

    report
}

/// Recompute duplication statistics after filtering (baseline or `--changed-since`).
///
/// Uses per-file line deduplication (matching `compute_stats` in `detect.rs`)
/// so overlapping clone instances don't inflate the duplicated line count.
pub fn recompute_stats(report: &DuplicationReport) -> crate::duplicates::DuplicationStats {
    crate::duplicates::recompute_stats(report)
}

/// Baseline data for health (complexity) comparison.
///
/// New baselines store count-per-category-per-file data in `finding_counts` so
/// line shifts do not leak pre-existing findings. Legacy baselines with
/// `findings: ["path:name:line"]` still load so users can refresh them in
/// place with `--save-baseline`.
///
/// `identity_finding_counts` carries the same counts bucketed per function
/// identity instead of per file, for the stricter [`HealthBaselineMode::Identity`]
/// comparison. It is written only when the baseline is saved in identity mode,
/// so default baselines keep their count-only shape. Identity baselines still
/// carry `finding_counts`, so they also work in count mode and with older
/// binaries.
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct HealthBaselineData {
    /// The command that saved this file. See [`BaselineData::kind`] for why it
    /// is written but never read back through this struct.
    #[serde(default, skip_deserializing, skip_serializing_if = "Option::is_none")]
    pub(crate) kind: Option<BaselineKind>,
    /// Legacy health baseline keys: `relative_path:function_name:line`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) findings: Vec<String>,
    /// Count-per-category-per-file baseline buckets.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) finding_counts: HealthFindingCountMap,
    /// Count-per-category buckets keyed by `relative_path\0function_name`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) identity_finding_counts: HealthFindingCountMap,
    /// Stable runtime-coverage finding IDs from the sidecar.
    #[serde(default)]
    pub(crate) runtime_coverage_findings: Vec<String>,
    /// Line-move-tolerant runtime-coverage suppression keys of the form
    /// `path\0name\0source_hash`. Unlike `runtime_coverage_findings` (whose
    /// keys hash the start line and so churn when a function moves), the
    /// `source_hash` component is the content digest of the function body, so a
    /// moved-but-unedited function keeps the same key and stays suppressed.
    /// Only findings whose `source_hash` is present contribute an entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) runtime_coverage_source_hashes: Vec<String>,
    /// Refactoring target keys: `relative_path:category`.
    #[serde(default)]
    pub(crate) target_keys: Vec<String>,
}

/// Serialized per-bucket finding tally inside a health baseline file.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HealthBaselineCount {
    count: usize,
}

type HealthFindingCountMap = BTreeMap<String, BTreeMap<String, HealthBaselineCount>>;

/// How a saved health baseline is matched against current findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HealthBaselineMode {
    /// Match per file and finding category. Resilient to renames and line
    /// shifts, but a replacement hotspot consumes the allowance of the hotspot
    /// it replaced.
    ///
    /// Count buckets do not survive file moves: the bucket key is the path and
    /// the payload is category tallies alone, so a bucket whose file moved has
    /// no surviving identity component to re-match on. Guessing a new path
    /// from count shapes could silently transfer allowance between unrelated
    /// files, so no move tolerance is attempted in this mode.
    #[default]
    Count,
    /// Match per function identity (path plus function name) and finding
    /// category. A hotspot that replaces another hotspot in the same file is
    /// reported, while line shifts and in-place edits stay suppressed.
    ///
    /// Identity buckets tolerate file moves conservatively: a bucket whose
    /// path no longer exists on disk follows its function name to a new path
    /// when exactly one unclaimed current bucket carries that name, resolved
    /// by `moved_identity_bucket_remaps`.
    Identity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HealthFindingDimension {
    Complexity,
    Crap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HealthFindingCategory {
    dimension: HealthFindingDimension,
    severity: fallow_output::FindingSeverity,
}

impl HealthFindingCategory {
    const fn key(self) -> &'static str {
        match (self.dimension, self.severity) {
            (HealthFindingDimension::Complexity, fallow_output::FindingSeverity::Moderate) => {
                "complexity_moderate"
            }
            (HealthFindingDimension::Complexity, fallow_output::FindingSeverity::High) => {
                "complexity_high"
            }
            (HealthFindingDimension::Complexity, fallow_output::FindingSeverity::Critical) => {
                "complexity_critical"
            }
            (HealthFindingDimension::Crap, fallow_output::FindingSeverity::Moderate) => {
                "crap_moderate"
            }
            (HealthFindingDimension::Crap, fallow_output::FindingSeverity::High) => "crap_high",
            (HealthFindingDimension::Crap, fallow_output::FindingSeverity::Critical) => {
                "crap_critical"
            }
        }
    }
}

const HEALTH_FINDING_DIMENSIONS: [HealthFindingDimension; 2] = [
    HealthFindingDimension::Complexity,
    HealthFindingDimension::Crap,
];

impl HealthBaselineData {
    /// The keys this format writes, for [`declares_baseline_format`]. A health
    /// baseline omits the buckets it has nothing for, so a file carrying any
    /// one of these is health's own.
    pub const DECLARED_KEYS: &'static [&'static str] = &[
        "findings",
        "finding_counts",
        "identity_finding_counts",
        "runtime_coverage_findings",
        "runtime_coverage_source_hashes",
        "target_keys",
    ];

    /// Build a health baseline from findings and targets.
    pub(crate) fn from_findings(
        findings: &[fallow_output::ComplexityViolation],
        runtime_coverage_findings: &[fallow_output::RuntimeCoverageFinding],
        targets: &[fallow_output::RefactoringTarget],
        root: &Path,
    ) -> Self {
        Self {
            kind: Some(BaselineKind::Health),
            findings: Vec::new(),
            finding_counts: health_finding_counts(findings, root, HealthBaselineMode::Count),
            identity_finding_counts: HealthFindingCountMap::new(),
            runtime_coverage_findings: runtime_coverage_findings
                .iter()
                .map(|f| runtime_coverage_finding_key(f, root))
                .collect(),
            runtime_coverage_source_hashes: runtime_coverage_findings
                .iter()
                .filter_map(|f| runtime_coverage_source_hash_key(f, root))
                .collect(),
            target_keys: targets
                .iter()
                .map(|t| target_baseline_key(t, root))
                .collect(),
        }
    }

    pub(crate) fn finding_entry_count(&self) -> usize {
        if self.finding_counts.is_empty() {
            self.findings.len()
        } else {
            self.finding_counts
                .values()
                .flat_map(BTreeMap::values)
                .map(|entry| entry.count)
                .sum()
        }
    }

    /// Add per-function identity buckets to a saved baseline.
    ///
    /// Only [`HealthBaselineMode::Identity`] saves record these, so a default
    /// baseline keeps carrying counts alone and stays free of function names.
    #[must_use]
    pub(crate) fn with_identity(
        mut self,
        findings: &[fallow_output::ComplexityViolation],
        root: &Path,
    ) -> Self {
        self.identity_finding_counts =
            health_finding_counts(findings, root, HealthBaselineMode::Identity);
        self
    }

    /// `true` when identity matching would silently degrade because the saved
    /// baseline predates `identity_finding_counts` yet does carry findings.
    pub(crate) fn lacks_identity_data(&self) -> bool {
        self.identity_finding_counts.is_empty()
            && (!self.finding_counts.is_empty() || !self.findings.is_empty())
    }

    fn counts_for(&self, mode: HealthBaselineMode) -> &HealthFindingCountMap {
        match mode {
            HealthBaselineMode::Count => &self.finding_counts,
            HealthBaselineMode::Identity => &self.identity_finding_counts,
        }
    }

    pub(crate) fn overlap_entries(
        &self,
        findings: &[fallow_output::ComplexityViolation],
        root: &Path,
        mode: HealthBaselineMode,
    ) -> HealthBaselineOverlap {
        let baseline_counts = self.counts_for(mode);
        if baseline_counts.is_empty() {
            let baseline_keys: FxHashSet<&str> = self.findings.iter().map(String::as_str).collect();
            HealthBaselineOverlap {
                matched_entries: findings
                    .iter()
                    .filter(|finding| {
                        baseline_keys.contains(health_finding_key(finding, root).as_str())
                    })
                    .count(),
                moved_entries: 0,
            }
        } else {
            let current_counts = health_finding_counts(findings, root, mode);
            let direct = health_overlap_entry_count(&current_counts, baseline_counts);
            if mode != HealthBaselineMode::Identity {
                return HealthBaselineOverlap {
                    matched_entries: direct,
                    moved_entries: 0,
                };
            }
            // `moved_entries` counts only followed file moves, so the overlap
            // is computed before and after the placeholder transfer.
            let moved = identity_counts_with_move_tolerance(baseline_counts, &current_counts, root);
            let moved_matched = moved.as_ref().map_or(direct, |counts| {
                health_overlap_entry_count(&current_counts, counts)
            });
            let matched = identity_counts_with_placeholder_allowance(
                moved.as_ref().unwrap_or(baseline_counts),
                &current_counts,
            )
            .map_or(moved_matched, |counts| {
                health_overlap_entry_count(&current_counts, &counts)
            });
            HealthBaselineOverlap {
                matched_entries: matched,
                moved_entries: moved_matched.saturating_sub(direct),
            }
        }
    }
}

/// Baseline entry overlap for one run, split so followed file moves stay
/// observable in output rather than silently absorbed into the match count.
pub(crate) struct HealthBaselineOverlap {
    /// Entries that matched a current finding, including via followed moves.
    pub(crate) matched_entries: usize,
    /// Entries that matched only because a retired identity bucket was
    /// re-keyed to a moved file.
    pub(crate) moved_entries: usize,
}

/// Generate a stable key for a refactoring target: `relative_path:category`.
fn target_baseline_key(target: &fallow_output::RefactoringTarget, root: &Path) -> String {
    format!(
        "{}:{}",
        relative_path(&target.path, root),
        target.category.label()
    )
}

/// Generate a stable key for a health finding.
fn health_finding_key(finding: &fallow_output::ComplexityViolation, root: &Path) -> String {
    format!(
        "{}:{}:{}",
        relative_path(&finding.path, root),
        finding.name,
        finding.line
    )
}

/// Bucket a finding belongs to for the given comparison mode.
///
/// The NUL separator keeps the identity bucket unambiguous for paths and
/// function names that contain `:`.
fn health_bucket_key(
    finding: &fallow_output::ComplexityViolation,
    root: &Path,
    mode: HealthBaselineMode,
) -> String {
    let path = relative_path(&finding.path, root);
    match mode {
        HealthBaselineMode::Count => path,
        HealthBaselineMode::Identity => format!("{path}\0{}", finding.name),
    }
}

fn identity_bucket_parts(key: &str) -> Option<(&str, &str)> {
    key.split_once('\0')
}

/// Conservative file-move tolerance for identity baseline buckets.
///
/// A baseline bucket follows its function to a new path only when every one of
/// these holds:
///
/// - the bucket matched no current bucket at its saved path,
/// - the saved path no longer exists on disk under the project root, so the
///   file was moved or deleted rather than merely fixed,
/// - exactly one current bucket carries the same function name at a path the
///   baseline does not already cover,
/// - no other retired baseline bucket claims that same candidate.
///
/// The function name is the only identity component that survives a move, so
/// anything more permissive would risk transferring allowance between
/// unrelated functions. Anonymous placeholders never match. The result is
/// deterministic: both maps iterate in `BTreeMap` order and the
/// exactly-one rules make the outcome independent of iteration order.
fn moved_identity_bucket_remaps(
    baseline_counts: &HealthFindingCountMap,
    current_counts: &HealthFindingCountMap,
    root: &Path,
) -> Vec<(String, String)> {
    let mut candidates_by_name: FxHashMap<&str, Vec<&str>> = FxHashMap::default();
    for key in current_counts.keys() {
        if baseline_counts.contains_key(key) {
            continue;
        }
        if let Some((_, name)) = identity_bucket_parts(key)
            && !is_anonymous_function_name(name)
        {
            candidates_by_name.entry(name).or_default().push(key);
        }
    }

    let mut proposals: Vec<(&str, &str)> = Vec::new();
    let mut claims: FxHashMap<&str, usize> = FxHashMap::default();
    for key in baseline_counts.keys() {
        if current_counts.contains_key(key.as_str()) {
            continue;
        }
        let Some((path, name)) = identity_bucket_parts(key) else {
            continue;
        };
        if is_anonymous_function_name(name) || root.join(path).exists() {
            continue;
        }
        if let Some(candidates) = candidates_by_name.get(name)
            && let [only_candidate] = candidates.as_slice()
        {
            proposals.push((key.as_str(), only_candidate));
            *claims.entry(only_candidate).or_default() += 1;
        }
    }

    proposals
        .into_iter()
        .filter(|(_, candidate)| claims.get(candidate) == Some(&1))
        .map(|(old, new)| (old.to_string(), new.to_string()))
        .collect()
}

/// A current identity bucket with a function name: the key and its counts.
type NamedBucket<'a> = (&'a String, &'a BTreeMap<String, HealthBaselineCount>);

/// Baseline identity counts where a placeholder bucket (`<anonymous>`,
/// `<arrow>`) gives its unused allowance to new function names in the same
/// file.
///
/// A function without a declared name can get a context name in a later
/// version (`Command.make("setup")`, an `Effect.fn` span name). Its findings
/// then move from the placeholder bucket to a named bucket that has no
/// allowance for them. This transfer keeps such a baseline valid. The
/// allowance moves per category, only to named buckets in the same file whose
/// current count is above their own allowance, and only up to the count that
/// the placeholder bucket no longer uses. After a re-save, a placeholder bucket has unused allowance only when
/// an unnamed finding was fixed. Count mode has the same tolerance per file.
///
/// Returns `None` when no allowance moves, so callers can keep borrowing the
/// original map.
fn identity_counts_with_placeholder_allowance(
    baseline_counts: &HealthFindingCountMap,
    current_counts: &HealthFindingCountMap,
) -> Option<HealthFindingCountMap> {
    let mut named_by_path: FxHashMap<&str, Vec<NamedBucket<'_>>> = FxHashMap::default();
    for (key, counts) in current_counts {
        if let Some((path, name)) = identity_bucket_parts(key)
            && !is_anonymous_function_name(name)
        {
            named_by_path.entry(path).or_default().push((key, counts));
        }
    }
    let mut transferred: HealthFindingCountMap = BTreeMap::new();
    for (key, placeholder_counts) in baseline_counts {
        let Some((path, name)) = identity_bucket_parts(key) else {
            continue;
        };
        if !is_anonymous_function_name(name) {
            continue;
        }
        let Some(recipients) = named_by_path.get(path) else {
            continue;
        };
        for (category, placeholder_count) in placeholder_counts {
            let mut unused = placeholder_count
                .count
                .saturating_sub(bucket_category_count(current_counts, key, category));
            for (recipient_key, recipient_counts) in recipients {
                if unused == 0 {
                    break;
                }
                let current = recipient_counts
                    .get(category)
                    .map_or(0, |entry| entry.count);
                let own_allowance = bucket_category_count(baseline_counts, recipient_key, category);
                let given = transferred
                    .entry((*recipient_key).clone())
                    .or_default()
                    .entry(category.clone())
                    .or_insert(HealthBaselineCount { count: 0 });
                let take = current
                    .saturating_sub(own_allowance + given.count)
                    .min(unused);
                given.count += take;
                unused -= take;
            }
        }
    }
    transferred.retain(|_, counts| {
        counts.retain(|_, entry| entry.count > 0);
        !counts.is_empty()
    });
    if transferred.is_empty() {
        return None;
    }
    let mut effective = baseline_counts.clone();
    for (key, counts) in transferred {
        let bucket = effective.entry(key).or_default();
        for (category, given) in counts {
            bucket
                .entry(category)
                .and_modify(|entry| entry.count += given.count)
                .or_insert(given);
        }
    }
    Some(effective)
}

fn bucket_category_count(counts: &HealthFindingCountMap, key: &str, category: &str) -> usize {
    counts
        .get(key)
        .and_then(|bucket| bucket.get(category))
        .map_or(0, |entry| entry.count)
}

/// Baseline identity counts after file-move tolerance and the placeholder
/// allowance transfer. Returns `None` when neither changes the baseline.
fn effective_identity_counts(
    baseline_counts: &HealthFindingCountMap,
    current_counts: &HealthFindingCountMap,
    root: &Path,
) -> Option<HealthFindingCountMap> {
    let moved = identity_counts_with_move_tolerance(baseline_counts, current_counts, root);
    identity_counts_with_placeholder_allowance(
        moved.as_ref().unwrap_or(baseline_counts),
        current_counts,
    )
    .or(moved)
}

/// Baseline identity counts with retired buckets re-keyed to moved files.
///
/// Returns `None` when no bucket qualifies, so callers can keep borrowing the
/// original map.
fn identity_counts_with_move_tolerance(
    baseline_counts: &HealthFindingCountMap,
    current_counts: &HealthFindingCountMap,
    root: &Path,
) -> Option<HealthFindingCountMap> {
    let remaps = moved_identity_bucket_remaps(baseline_counts, current_counts, root);
    if remaps.is_empty() {
        return None;
    }
    let mut remapped = baseline_counts.clone();
    for (old_key, new_key) in remaps {
        if let Some(entry) = remapped.remove(&old_key) {
            remapped.insert(new_key, entry);
        }
    }
    Some(remapped)
}

fn health_finding_counts(
    findings: &[fallow_output::ComplexityViolation],
    root: &Path,
    mode: HealthBaselineMode,
) -> HealthFindingCountMap {
    let mut counts = BTreeMap::new();
    for finding in findings {
        let bucket = health_bucket_key(finding, root, mode);
        let file_counts = counts.entry(bucket).or_insert_with(BTreeMap::new);
        for category in health_finding_categories(finding).into_iter().flatten() {
            file_counts
                .entry(category.key().to_string())
                .and_modify(|entry: &mut HealthBaselineCount| entry.count += 1)
                .or_insert(HealthBaselineCount { count: 1 });
        }
    }
    counts
}

fn health_finding_categories(
    finding: &fallow_output::ComplexityViolation,
) -> [Option<HealthFindingCategory>; 2] {
    let complexity_category = HealthFindingCategory {
        dimension: HealthFindingDimension::Complexity,
        severity: finding.severity,
    };
    let crap_category = HealthFindingCategory {
        dimension: HealthFindingDimension::Crap,
        severity: finding.severity,
    };
    // A file-length finding counts as complexity: it has no CRAP part, and a
    // finding without a category would never match a baseline entry.
    let has_complexity = finding.exceeded.includes_cyclomatic()
        || finding.exceeded.includes_cognitive()
        || finding.exceeded.is_file_lines();
    let has_crap = finding.exceeded.includes_crap();
    [
        has_complexity.then_some(complexity_category),
        has_crap.then_some(crap_category),
    ]
}

fn severity_index(severity: fallow_output::FindingSeverity) -> usize {
    match severity {
        fallow_output::FindingSeverity::Moderate => 0,
        fallow_output::FindingSeverity::High => 1,
        fallow_output::FindingSeverity::Critical => 2,
    }
}

fn severity_counts_for_dimension(
    file_counts: Option<&BTreeMap<String, HealthBaselineCount>>,
    dimension: HealthFindingDimension,
) -> [usize; 3] {
    let mut counts = [0; 3];
    for severity in [
        fallow_output::FindingSeverity::Moderate,
        fallow_output::FindingSeverity::High,
        fallow_output::FindingSeverity::Critical,
    ] {
        let category = HealthFindingCategory {
            dimension,
            severity,
        };
        counts[severity_index(severity)] = file_counts
            .and_then(|entries| entries.get(category.key()))
            .map_or(0, |entry| entry.count);
    }
    counts
}

fn overflowing_severities(current: [usize; 3], baseline: [usize; 3]) -> [bool; 3] {
    let mut available = baseline;
    let mut overflow = [false; 3];

    for severity_idx in 0..3 {
        let compatible = available[severity_idx..].iter().sum::<usize>();
        overflow[severity_idx] = compatible < current[severity_idx];

        let mut matched = current[severity_idx].min(compatible);
        for slot in available.iter_mut().skip(severity_idx) {
            let taken = matched.min(*slot);
            *slot -= taken;
            matched -= taken;
            if matched == 0 {
                break;
            }
        }
    }

    overflow
}

fn health_overflow_categories(
    current_counts: &HealthFindingCountMap,
    baseline_counts: &HealthFindingCountMap,
) -> FxHashMap<String, FxHashSet<&'static str>> {
    let mut overflow_by_path = FxHashMap::default();

    for (path, current_file_counts) in current_counts {
        let mut overflow_categories: FxHashSet<&'static str> = FxHashSet::default();
        let baseline_file_counts = baseline_counts.get(path);

        for dimension in HEALTH_FINDING_DIMENSIONS {
            let current = severity_counts_for_dimension(Some(current_file_counts), dimension);
            let baseline = severity_counts_for_dimension(baseline_file_counts, dimension);
            let overflow = overflowing_severities(current, baseline);

            for severity in [
                fallow_output::FindingSeverity::Moderate,
                fallow_output::FindingSeverity::High,
                fallow_output::FindingSeverity::Critical,
            ] {
                if overflow[severity_index(severity)] {
                    overflow_categories.insert(
                        HealthFindingCategory {
                            dimension,
                            severity,
                        }
                        .key(),
                    );
                }
            }
        }

        if !overflow_categories.is_empty() {
            overflow_by_path.insert(path.clone(), overflow_categories);
        }
    }

    overflow_by_path
}

fn health_overlap_entry_count(
    current_counts: &HealthFindingCountMap,
    baseline_counts: &HealthFindingCountMap,
) -> usize {
    let mut overlap = 0;

    for (path, baseline_file_counts) in baseline_counts {
        let current_file_counts = current_counts.get(path);

        for dimension in HEALTH_FINDING_DIMENSIONS {
            let current_total: usize =
                severity_counts_for_dimension(current_file_counts, dimension)
                    .into_iter()
                    .sum();
            let baseline_total: usize =
                severity_counts_for_dimension(Some(baseline_file_counts), dimension)
                    .into_iter()
                    .sum();
            overlap += current_total.min(baseline_total);
        }
    }

    overlap
}

fn runtime_coverage_finding_key(
    finding: &fallow_output::RuntimeCoverageFinding,
    _root: &Path,
) -> String {
    finding
        .stable_id
        .clone()
        .unwrap_or_else(|| finding.id.clone())
}

/// Line-move-tolerant writer key: `path\0name\0source_hash`.
///
/// Returns `None` when the finding carries no `source_hash` (e.g. a 0.5-shape
/// sidecar or an un-migrated producer); such findings fall back to the
/// line-sensitive `runtime_coverage_finding_key` for suppression. The NUL
/// separator avoids collisions with paths/names that contain `:`.
fn runtime_coverage_source_hash_key(
    finding: &fallow_output::RuntimeCoverageFinding,
    root: &Path,
) -> Option<String> {
    finding.source_hash.as_deref().map(|hash| {
        format!(
            "{}\0{}\0{}",
            relative_path(&finding.path, root),
            finding.function,
            hash
        )
    })
}

/// Filter health findings to only include those not present in the baseline.
pub(crate) fn filter_new_health_findings(
    mut findings: Vec<fallow_output::ComplexityViolation>,
    baseline: &HealthBaselineData,
    root: &Path,
    mode: HealthBaselineMode,
) -> Vec<fallow_output::ComplexityViolation> {
    let baseline_counts = baseline.counts_for(mode);
    if !baseline_counts.is_empty() {
        let current_counts = health_finding_counts(&findings, root, mode);
        let remapped = (mode == HealthBaselineMode::Identity)
            .then(|| effective_identity_counts(baseline_counts, &current_counts, root))
            .flatten();
        let overflow_categories = health_overflow_categories(
            &current_counts,
            remapped.as_ref().unwrap_or(baseline_counts),
        );
        findings.retain(|finding| {
            let bucket = health_bucket_key(finding, root, mode);
            overflow_categories.get(&bucket).is_some_and(|categories| {
                health_finding_categories(finding)
                    .into_iter()
                    .flatten()
                    .any(|category| categories.contains(category.key()))
            })
        });
        return findings;
    }

    let baseline_keys: FxHashSet<&str> = baseline.findings.iter().map(String::as_str).collect();
    findings.retain(|f| {
        let key = health_finding_key(f, root);
        !baseline_keys.contains(key.as_str())
    });
    findings
}

pub(crate) fn filter_new_runtime_coverage_findings(
    mut findings: Vec<fallow_output::RuntimeCoverageFinding>,
    baseline: &HealthBaselineData,
    root: &Path,
) -> Vec<fallow_output::RuntimeCoverageFinding> {
    let baseline_keys: FxHashSet<&str> = baseline
        .runtime_coverage_findings
        .iter()
        .map(String::as_str)
        .collect();
    let baseline_source_hash_keys: FxHashSet<&str> = baseline
        .runtime_coverage_source_hashes
        .iter()
        .map(String::as_str)
        .collect();
    findings.retain(|finding| {
        let suppressed_by_stable_id = finding
            .stable_id
            .as_deref()
            .is_some_and(|id| baseline_keys.contains(id));
        let suppressed_by_legacy_id = baseline_keys.contains(finding.id.as_str());
        let suppressed_by_source_hash = runtime_coverage_source_hash_key(finding, root)
            .is_some_and(|key| baseline_source_hash_keys.contains(key.as_str()));
        !(suppressed_by_stable_id || suppressed_by_legacy_id || suppressed_by_source_hash)
    });
    findings
}

/// Filter refactoring targets to only include those not present in the baseline.
pub(crate) fn filter_new_health_targets(
    mut targets: Vec<fallow_output::RefactoringTarget>,
    baseline: &HealthBaselineData,
    root: &Path,
) -> Vec<fallow_output::RefactoringTarget> {
    let baseline_keys: FxHashSet<&str> = baseline.target_keys.iter().map(String::as_str).collect();
    targets.retain(|t| {
        let key = target_baseline_key(t, root);
        !baseline_keys.contains(key.as_str())
    });
    targets
}

/// Per-category delta between current results and a baseline.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CategoryDelta {
    /// Finding count in the current run.
    pub current: usize,
    /// Finding count recorded in the baseline.
    pub baseline: usize,
    /// `current - baseline`; positive means new findings appeared.
    pub delta: i64,
}

/// Deltas between current analysis results and a saved baseline.
///
/// Used in combined mode to show +/- counts in the failure summary and
/// to emit `baseline_deltas` in JSON output.
#[derive(Debug, Clone)]
pub struct BaselineDeltas {
    /// Net change in total issue count (positive = more issues).
    pub total_delta: i64,
    /// Per-category deltas keyed by category name.
    pub per_category: Vec<(String, CategoryDelta)>,
}

#[cfg(test)]
#[path = "baseline_tests.rs"]
mod tests;
