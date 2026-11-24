//! Clone-region attribution of the audit.
//!
//! The detector keeps each maximal repeat. So one paste can give a long
//! group with fewer copies and a short group with more copies, and the two
//! groups cover the same lines. The audit collapses the groups whose
//! instances overlap into one region, maps each instance through the diff to
//! the base, and grades the region:
//!
//! - `introduced`: a member group has a copy that the base does not have, or
//!   the change made two old pieces of code equal.
//! - `inherited`: each member group is a base group, or the change wrote no
//!   line of it.
//! - `inherited-edited`: each member group lies inside a base group, and the
//!   change edited one or more of its copies. Each copy grows by fewer lines
//!   than `duplicates.minLines`. The rule reads the line count only, so a
//!   rewrite that keeps the size of every copy is an edit, also when it
//!   writes many new identical lines.
//!
//! The `introduced-clone-group` gate counts regions, and only an
//! `introduced` region fails it.
//!
//! An `ignoredClones` entry follows an accepted clone through the audit
//! match. The engine key of the entry does not change.

use std::path::Path;

use fallow_engine::changed_files::RenamedFile;
use fallow_engine::duplicates::{CloneFingerprintSet, IgnoredCloneGroup};
use fallow_engine::module_graph::relative_key_path;
use fallow_output::DiffIndex;
use fallow_types::duplicates::{CloneGroup, CloneInstance};
use rustc_hash::{FxHashMap, FxHashSet};

use crate::audit_keys::{ChangedPaths, dupe_group_key};
use crate::audit_moved::moved_dupe_group_keys;
use crate::diff_risk::clones::{AcceptedCloneGroup, BaseCloneGroup, CloneSpan};
use crate::{
    CloneDemotionReason, CloneRegionFinding, CloneRegionGrade, CloneRegionSpan, DupesReportPayload,
};

/// The head side of the region match.
pub struct CloneRegionHead<'a> {
    /// Head clone groups in output order.
    pub groups: &'a [&'a CloneGroup],
    /// Head clone groups that `duplicates.ignoredClones` hid.
    pub ignored: &'a [IgnoredCloneGroup],
    /// Root that key paths are relative to.
    pub root: &'a Path,
    /// The `duplicates.minLines` of the run: the fewest lines of a clone.
    pub min_lines: usize,
}

/// The base side of the region match.
pub struct CloneRegionBase<'a> {
    /// Attribution keys of the base clone groups, at head paths.
    pub keys: &'a FxHashSet<String>,
    /// Instance locations of the base clone groups, at base paths.
    pub groups: &'a [BaseCloneGroup],
    /// Base clone groups that `duplicates.ignoredClones` hid, at base paths.
    pub accepted: &'a [AcceptedCloneGroup],
    /// The renamed files of the change.
    pub renames: &'a [RenamedFile],
    /// The changed files of the change, as key paths.
    pub changed: &'a ChangedPaths,
}

/// The graded regions of one audit run.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CloneRegions {
    regions: Vec<CloneRegion>,
    /// Region index of each head group, in output order.
    group_region: Vec<usize>,
    /// Whether each head group adds duplication that no `ignoredClones`
    /// entry accepts, in output order.
    group_introduced: Vec<bool>,
    /// Whether the change wrote no line of each head group with a new key,
    /// in output order (issue #2164).
    group_demoted: Vec<bool>,
}

/// One region: the head clone groups whose instances overlap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneRegion {
    /// Grade of the region.
    pub grade: CloneRegionGrade,
    /// Whether the region fails the `introduced-clone-group` gate under
    /// `new-only`: the grade is `introduced`, and no `ignoredClones` entry
    /// accepts the new copies.
    pub gates: bool,
    /// Indices of the member groups, in output order.
    pub groups: Vec<usize>,
    /// The union of the member instance lines in each file, at head paths.
    pub spans: Vec<CloneSpan>,
    /// The `ignoredClones` entries that accept the region.
    pub accepted_by: Vec<String>,
    /// The `ignoredClones` keys of the member groups, when the region follows
    /// an accepted base clone that the change edited. The config needs these
    /// keys so that `fallow dupes` hides the edited clone too.
    pub ignore_hint: Vec<String>,
    /// Member groups that the change wrote no line of, although their key is
    /// not in the base (issue #2164).
    pub demoted: Vec<usize>,
    /// The member groups of an `inherited-edited` region that lie inside a
    /// base clone that an `ignoredClones` entry accepted.
    edited_accepted: Vec<usize>,
}

impl CloneRegion {
    /// Whether the region is `inherited-edited` and follows an accepted base
    /// clone.
    #[must_use]
    pub fn follows_edited_accept(&self) -> bool {
        !self.edited_accepted.is_empty()
    }
}

impl CloneRegions {
    /// The graded regions.
    #[must_use]
    pub fn regions(&self) -> &[CloneRegion] {
        &self.regions
    }

    /// Number of regions that fail the gate.
    #[must_use]
    pub fn introduced_count(&self) -> usize {
        self.regions.iter().filter(|region| region.gates).count()
    }

    /// Number of regions that do not fail the gate.
    #[must_use]
    pub fn inherited_count(&self) -> usize {
        self.regions.len() - self.introduced_count()
    }

    /// Number of regions whose member groups the change did not write.
    #[must_use]
    pub fn demoted_count(&self) -> usize {
        self.regions
            .iter()
            .filter(|region| !region.gates && !region.demoted.is_empty())
            .count()
    }

    /// Region index of each head group, in output order.
    #[must_use]
    pub fn group_regions(&self) -> &[usize] {
        &self.group_region
    }

    /// Whether the head group at `index` adds duplication that no
    /// `ignoredClones` entry accepts. Its region then fails the gate. An old
    /// group that overlaps a new group stays not introduced.
    #[must_use]
    pub fn group_introduced(&self, index: usize) -> bool {
        self.group_introduced.get(index).copied().unwrap_or(false)
    }

    /// Whether the change wrote no line of the head group at `index`,
    /// although its key is new (issue #2164).
    #[must_use]
    pub fn group_demoted(&self, index: usize) -> bool {
        self.group_demoted.get(index).copied().unwrap_or(false)
    }

    /// Set the `region` index of each clone group and the `clone_regions`
    /// list of a typed duplication payload whose groups are the graded head
    /// groups, in the same order.
    pub fn annotate_payload(&self, payload: &mut DupesReportPayload) {
        if self.regions.is_empty() {
            return;
        }
        for (finding, region) in payload.clone_groups.iter_mut().zip(&self.group_region) {
            finding.region = Some(*region);
        }
        payload.clone_regions = self.findings();
    }

    /// Insert the `region` index of each clone group and the `clone_regions`
    /// list into serialized duplication JSON whose `clone_groups` are the
    /// graded head groups, in the same order.
    pub fn annotate_json(&self, json: &mut serde_json::Value) {
        if self.regions.is_empty() {
            return;
        }
        if let Some(items) = json
            .get_mut("clone_groups")
            .and_then(serde_json::Value::as_array_mut)
        {
            for (item, region) in items.iter_mut().zip(&self.group_region) {
                if let serde_json::Value::Object(map) = item {
                    map.insert("region".to_owned(), serde_json::json!(region));
                }
            }
        }
        if let (serde_json::Value::Object(map), Ok(regions)) =
            (json, serde_json::to_value(self.findings()))
        {
            map.insert("clone_regions".to_owned(), regions);
        }
    }

    /// The wire form of the regions.
    #[must_use]
    pub fn findings(&self) -> Vec<CloneRegionFinding> {
        self.regions
            .iter()
            .map(|region| CloneRegionFinding {
                grade: region.grade,
                introduced: region.gates,
                groups: region.groups.clone(),
                instances: region
                    .spans
                    .iter()
                    .map(|span| CloneRegionSpan {
                        file: span.path.clone(),
                        start_line: span.start_line,
                        end_line: span.end_line,
                    })
                    .collect(),
                accepted_by: region.accepted_by.clone(),
                ignore_hint: region.ignore_hint.clone(),
                demotion_reason: (!region.gates && !region.demoted.is_empty())
                    .then_some(CloneDemotionReason::NoAddedLines),
            })
            .collect()
    }
}

/// How the change relates to one head clone group.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Member {
    /// The base has the group, under the same key or moved to other files.
    Unchanged,
    /// The key is new, but the change wrote no line of the group.
    Demoted,
    /// Each instance lies inside a different instance of one base group, and
    /// the change edited one or more instances. `accepted` holds the
    /// `ignoredClones` key of the base group when the base hid it.
    Edited { accepted: Option<String> },
    /// The change made duplication that the base did not have. `accepted`
    /// holds the `ignoredClones` keys of the head groups that hid each new
    /// copy, or is empty.
    New { accepted: Vec<String> },
}

/// A base group that a set of spans fits in.
enum Fit<'a> {
    Visible,
    Accepted(&'a str),
}

/// Maps a head instance to its base path and base lines.
struct BaseMapper<'a> {
    root: &'a Path,
    diff: &'a DiffIndex,
    head_to_base: FxHashMap<String, String>,
}

impl<'a> BaseMapper<'a> {
    fn new(root: &'a Path, diff: &'a DiffIndex, renames: &[RenamedFile]) -> Self {
        let head_to_base = renames
            .iter()
            .map(|rename| {
                (
                    relative_key_path(&rename.to, root),
                    relative_key_path(&rename.from, root),
                )
            })
            .collect();
        Self {
            root,
            diff,
            head_to_base,
        }
    }

    /// The base span of a head span. `None` when the span has no base
    /// lines, for example a copy in a new file.
    fn base_span(&self, head: &CloneSpan) -> Option<CloneSpan> {
        let rel = &head.path;
        let key = self.diff.key_for(&self.root.join(rel), self.root)?;
        let (base_start, base_end) = self.diff.base_line_range(
            &key,
            u64::from(head.start_line),
            u64::from(head.end_line),
        )?;
        let path = self.head_to_base.get(rel).cloned().unwrap_or_else(|| {
            self.diff
                .old_path_for_root_relative(rel)
                .map_or_else(|| rel.clone(), |old| old.into_owned())
        });
        Some(CloneSpan {
            path,
            start_line: u32::try_from(base_start).ok()?,
            end_line: u32::try_from(base_end).ok()?,
        })
    }

    /// Whether the change added a line inside `instance`. An instance whose
    /// path does not map into the diff counts as touched, so its group keeps
    /// its gate.
    fn touched(&self, instance: &CloneInstance) -> bool {
        let Some(key) = self.diff.key_for(&instance.file, self.root) else {
            return true;
        };
        let start = u64::try_from(instance.start_line).unwrap_or(u64::MAX);
        let end = u64::try_from(instance.end_line).unwrap_or(u64::MAX);
        self.diff.range_overlaps_added(&key, start, end)
    }
}

/// Grade the head clone groups as regions.
///
/// With `diff` absent, a group whose key is not in the base and that the
/// change did not move is `introduced`.
#[must_use]
pub fn grade_clone_regions(
    head: &CloneRegionHead<'_>,
    base: &CloneRegionBase<'_>,
    diff: Option<&DiffIndex>,
) -> CloneRegions {
    let keys: Vec<String> = head
        .groups
        .iter()
        .map(|group| dupe_group_key(group, head.root))
        .collect();
    let moved = moved_dupe_group_keys(
        keys.iter()
            .map(String::as_str)
            .filter(|key| !base.keys.contains(*key)),
        &keys.iter().map(String::as_str).collect(),
        base.keys,
        base.changed,
    );
    let mapper = diff.map(|diff| BaseMapper::new(head.root, diff, base.renames));
    let members: Vec<Member> = head
        .groups
        .iter()
        .zip(&keys)
        .map(|(group, key)| {
            if base.keys.contains(key) || moved.contains(key) {
                Member::Unchanged
            } else if let Some(mapper) = mapper.as_ref() {
                member_against_base(group, mapper, head, base)
            } else {
                Member::New {
                    accepted: Vec::new(),
                }
            }
        })
        .collect();
    collect_regions(head, &members)
}

fn member_against_base(
    group: &CloneGroup,
    mapper: &BaseMapper<'_>,
    head: &CloneRegionHead<'_>,
    base: &CloneRegionBase<'_>,
) -> Member {
    let touched: Vec<bool> = group
        .instances
        .iter()
        .map(|instance| mapper.touched(instance))
        .collect();
    if !touched.contains(&true) {
        return Member::Demoted;
    }
    let spans = head_spans(group, head.root);
    let mapped: Vec<Option<CloneSpan>> = spans.iter().map(|span| mapper.base_span(span)).collect();
    match edited_fit(&spans, &mapped, base.groups, base.accepted, head.min_lines) {
        Some(Fit::Visible) => return Member::Edited { accepted: None },
        Some(Fit::Accepted(key)) => {
            return Member::Edited {
                accepted: Some(key.to_owned()),
            };
        }
        None => {}
    }
    Member::New {
        accepted: accepting_ignored_groups(group, &mapped, &touched, head),
    }
}

/// The head spans of a clone group, root-relative.
fn head_spans(group: &CloneGroup, root: &Path) -> Vec<CloneSpan> {
    group
        .instances
        .iter()
        .map(|instance| CloneSpan {
            path: relative_key_path(&instance.file, root),
            start_line: u32::try_from(instance.start_line).unwrap_or(u32::MAX),
            end_line: u32::try_from(instance.end_line).unwrap_or(u32::MAX),
        })
        .collect()
}

/// The number of lines of a span.
fn span_lines(span: &CloneSpan) -> u32 {
    span.end_line.saturating_sub(span.start_line) + 1
}

/// The base group that a head group edits: each head span maps to base
/// lines, and the mapped spans fit in one base group. A base group that
/// `ignoredClones` did not hide wins.
///
/// The growth of a copy is its head line count minus the line count of its
/// mapped base span. Each copy must grow by fewer than `min_lines` lines. A
/// rewrite that keeps the line count does not grow the copy, so it is an
/// edit, also when it rewrites the whole copy. `mapped` holds the base span
/// of each head span, `None` for a span with no base lines.
fn edited_fit<'a>(
    head: &[CloneSpan],
    mapped: &[Option<CloneSpan>],
    visible: &[BaseCloneGroup],
    accepted: &'a [AcceptedCloneGroup],
    min_lines: usize,
) -> Option<Fit<'a>> {
    let spans: Vec<CloneSpan> = mapped.iter().cloned().collect::<Option<_>>()?;
    let growth = head
        .iter()
        .zip(&spans)
        .map(|(head, base)| span_lines(head).saturating_sub(span_lines(base)))
        .max()
        .unwrap_or(0);
    if usize::try_from(growth).unwrap_or(usize::MAX) >= min_lines {
        return None;
    }
    if visible
        .iter()
        .any(|group| spans_fit_in_group(&spans, &group.instances, growth))
    {
        return Some(Fit::Visible);
    }
    accepted
        .iter()
        .find(|group| spans_fit_in_group(&spans, &group.instances, growth))
        .map(|group| Fit::Accepted(group.key.as_str()))
}

/// The change that a head clone group compares with: the diff of the run and
/// its renamed files.
pub struct CloneChange<'a> {
    /// Root that the head paths are relative to.
    pub root: &'a Path,
    /// The diff of the run.
    pub diff: &'a DiffIndex,
    /// The renamed files of the change.
    pub renames: &'a [RenamedFile],
}

/// Whether the head clone group with the root-relative spans `head` is an
/// edit of one of `base_groups`, by the rule that grades a clone region
/// `inherited-edited`: each copy maps through the diff into a different copy
/// of one base group, at the same relative place, and grows by fewer than
/// `min_lines` lines. The base group may have more copies.
#[must_use]
pub fn edits_base_clone(
    head: &[CloneSpan],
    change: &CloneChange<'_>,
    base_groups: &[BaseCloneGroup],
    min_lines: usize,
) -> bool {
    let mapper = BaseMapper::new(change.root, change.diff, change.renames);
    let mapped: Vec<Option<CloneSpan>> = head.iter().map(|span| mapper.base_span(span)).collect();
    // No accepted groups: an accepted base group changes only which entry
    // accepts the region, not whether the group is an edit.
    edited_fit(head, &mapped, base_groups, &[], min_lines).is_some()
}

/// Whether each span lies inside a different instance of `base`, at the
/// same place in each instance.
///
/// The copies of one clone hold equal text, so a part of the clone starts at
/// the same relative place in each copy. Two base copies can differ in line
/// count when their formatting differs, so the check scales the offset of
/// the first span to the length of each other copy. The scaled offset may
/// differ by the growth of the copies plus one line for a rewritten first
/// line. Without
/// this check, one new block that replaces lines at two different places of
/// a base clone would map inside the base clone.
fn spans_fit_in_group(spans: &[CloneSpan], base: &[CloneSpan], growth: u32) -> bool {
    let mut used = vec![false; base.len()];
    let mut offsets: Vec<(u32, u32)> = Vec::with_capacity(spans.len());
    let all_inside = spans.iter().all(|span| {
        let slot = base.iter().enumerate().position(|(index, outer)| {
            !used[index]
                && outer.path == span.path
                && outer.start_line <= span.start_line
                && span.end_line <= outer.end_line
        });
        slot.is_some_and(|index| {
            used[index] = true;
            let outer = &base[index];
            offsets.push((
                span.start_line - outer.start_line,
                outer.end_line - outer.start_line + 1,
            ));
            true
        })
    });
    if !all_inside {
        return false;
    }
    let Some(&(first_offset, first_length)) = offsets.first() else {
        return true;
    };
    let tolerance = u64::from(growth).saturating_add(1);
    offsets.iter().all(|&(offset, length)| {
        // `offset / length` against `first_offset / first_length`, in lines
        // of the first copy.
        let scaled = u64::from(offset) * u64::from(first_length);
        let expected = u64::from(first_offset) * u64::from(length);
        scaled.abs_diff(expected) <= tolerance.saturating_mul(u64::from(first_length))
    })
}

/// The `ignoredClones` keys of the hidden head groups that accept the new
/// copies of `group`, or an empty list.
///
/// The new copies are the instances with no base lines. Each one must lie
/// inside an instance of a hidden head group. The other instances must be
/// old duplication: they hold no added line. Equal text that the change did
/// not edit was equal at the base too, so the base had that clone. The base
/// snapshot cannot show it, because the base pass reports only the groups in
/// the changed files. So when a paste gives a long group and a short group
/// inside it, and the config accepts the long group, the short group only
/// adds a copy that the base already had (issue #4967).
fn accepting_ignored_groups(
    group: &CloneGroup,
    mapped: &[Option<CloneSpan>],
    touched: &[bool],
    head: &CloneRegionHead<'_>,
) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for (instance, span) in group.instances.iter().zip(mapped) {
        if span.is_some() {
            continue;
        }
        let Some(ignored) = head.ignored.iter().find(|ignored| {
            ignored.group.instances.iter().any(|outer| {
                outer.file == instance.file
                    && outer.start_line <= instance.start_line
                    && instance.end_line <= outer.end_line
            })
        }) else {
            return Vec::new();
        };
        if !keys.contains(&ignored.key) {
            keys.push(ignored.key.clone());
        }
    }
    let edited_old_copy = mapped
        .iter()
        .zip(touched)
        .any(|(span, touched)| span.is_some() && *touched);
    if edited_old_copy {
        return Vec::new();
    }
    keys
}

/// Collapse the groups whose instances overlap, and grade each region.
fn collect_regions(head: &CloneRegionHead<'_>, members: &[Member]) -> CloneRegions {
    let component = overlap_components(head.groups);
    let mut region_of_component: FxHashMap<usize, usize> = FxHashMap::default();
    let mut grouped: Vec<Vec<usize>> = Vec::new();
    let mut group_region = Vec::with_capacity(head.groups.len());
    for (index, root) in component.iter().enumerate() {
        let region = *region_of_component.entry(*root).or_insert_with(|| {
            grouped.push(Vec::new());
            grouped.len() - 1
        });
        grouped[region].push(index);
        group_region.push(region);
    }
    let mut regions: Vec<CloneRegion> = grouped
        .into_iter()
        .map(|groups| grade_region(head, members, groups))
        .collect();
    if regions.iter().any(CloneRegion::follows_edited_accept) {
        let fingerprints = CloneFingerprintSet::from_group_refs(head.groups);
        for region in &mut regions {
            region.ignore_hint = region
                .edited_accepted
                .iter()
                .map(|group| fingerprints.ignored_clone_key_for_group(head.groups[*group]))
                .collect();
        }
    }
    CloneRegions {
        regions,
        group_region,
        group_introduced: members
            .iter()
            .map(|member| matches!(member, Member::New { accepted } if accepted.is_empty()))
            .collect(),
        group_demoted: members
            .iter()
            .map(|member| matches!(member, Member::Demoted))
            .collect(),
    }
}

fn grade_region(head: &CloneRegionHead<'_>, members: &[Member], groups: Vec<usize>) -> CloneRegion {
    let mut grade = CloneRegionGrade::Inherited;
    let mut gates = false;
    let mut accepted_by: Vec<String> = Vec::new();
    let mut edited_accepted = Vec::new();
    let mut demoted = Vec::new();
    let add_key = |accepted_by: &mut Vec<String>, key: &str| {
        if !accepted_by.iter().any(|known| known == key) {
            accepted_by.push(key.to_owned());
        }
    };
    for index in &groups {
        match &members[*index] {
            Member::Unchanged => {}
            Member::Demoted => demoted.push(*index),
            Member::Edited { accepted } => {
                if grade == CloneRegionGrade::Inherited {
                    grade = CloneRegionGrade::InheritedEdited;
                }
                if let Some(key) = accepted {
                    edited_accepted.push(*index);
                    add_key(&mut accepted_by, key);
                }
            }
            Member::New { accepted } => {
                grade = CloneRegionGrade::Introduced;
                if accepted.is_empty() {
                    gates = true;
                }
                for key in accepted {
                    add_key(&mut accepted_by, key);
                }
            }
        }
    }
    // An entry does not accept a region that fails the gate, and a new copy
    // makes the entry of an edited member irrelevant.
    if gates {
        accepted_by.clear();
    }
    if grade != CloneRegionGrade::InheritedEdited {
        edited_accepted.clear();
    }
    CloneRegion {
        grade,
        gates,
        spans: region_spans(head, &groups),
        ignore_hint: Vec::new(),
        edited_accepted,
        accepted_by,
        groups,
        demoted,
    }
}

/// The union of the member instance lines in each file, sorted by path and
/// line.
fn region_spans(head: &CloneRegionHead<'_>, groups: &[usize]) -> Vec<CloneSpan> {
    let mut lines: Vec<(String, u32, u32)> = groups
        .iter()
        .flat_map(|index| &head.groups[*index].instances)
        .map(|instance| {
            (
                relative_key_path(&instance.file, head.root),
                u32::try_from(instance.start_line).unwrap_or(u32::MAX),
                u32::try_from(instance.end_line).unwrap_or(u32::MAX),
            )
        })
        .collect();
    lines.sort_unstable();
    let mut spans: Vec<CloneSpan> = Vec::new();
    for (path, start_line, end_line) in lines {
        match spans.last_mut() {
            Some(last) if last.path == path && start_line <= last.end_line => {
                last.end_line = last.end_line.max(end_line);
            }
            _ => spans.push(CloneSpan {
                path,
                start_line,
                end_line,
            }),
        }
    }
    spans
}

/// The component root of each group: two groups share a component when an
/// instance of one overlaps an instance of the other in the same file.
fn overlap_components(groups: &[&CloneGroup]) -> Vec<usize> {
    let mut parent: Vec<usize> = (0..groups.len()).collect();
    let mut by_file: FxHashMap<&Path, Vec<(usize, usize, usize)>> = FxHashMap::default();
    for (index, group) in groups.iter().enumerate() {
        for instance in &group.instances {
            by_file.entry(instance.file.as_path()).or_default().push((
                instance.start_line,
                instance.end_line,
                index,
            ));
        }
    }
    let mut files: Vec<_> = by_file.into_values().collect();
    for instances in &mut files {
        instances.sort_unstable();
        let mut reach: Option<(usize, usize)> = None;
        for &(start, end, index) in instances.iter() {
            match reach {
                Some((reach_end, owner)) if start <= reach_end => {
                    union(&mut parent, owner, index);
                    reach = Some((reach_end.max(end), owner));
                }
                _ => reach = Some((end, index)),
            }
        }
    }
    (0..groups.len())
        .map(|index| find(&mut parent, index))
        .collect()
}

fn find(parent: &mut [usize], mut index: usize) -> usize {
    while parent[index] != index {
        parent[index] = parent[parent[index]];
        index = parent[index];
    }
    index
}

fn union(parent: &mut [usize], left: usize, right: usize) {
    let left = find(parent, left);
    let right = find(parent, right);
    if left != right {
        let (low, high) = if left < right {
            (left, right)
        } else {
            (right, left)
        };
        parent[high] = low;
    }
}

/// Whether a head group has a key that the base does not hold. Without such
/// a group, the grade needs no diff.
#[must_use]
pub fn has_unmatched_group(head: &CloneRegionHead<'_>, base_keys: &FxHashSet<String>) -> bool {
    head.groups
        .iter()
        .any(|group| !base_keys.contains(&dupe_group_key(group, head.root)))
}

#[cfg(test)]
#[path = "clone_regions_tests.rs"]
mod tests;
