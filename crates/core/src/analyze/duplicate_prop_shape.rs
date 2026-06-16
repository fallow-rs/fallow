//! Duplicate-prop-shape detection: a structural-refactor health signal that
//! groups React/Preact components whose statically-harvested, fully-known prop
//! NAME set is byte-for-byte IDENTICAL after stripping a fixed denylist of
//! ubiquitous DOM / render-passthrough prop names. Three or more such
//! components living in two or more files are a missing shared abstraction (a
//! `Props` type to extract, or a base component), never a correctness error and
//! never an auto-fix.
//!
//! Pure static analysis (ADR-001): no type resolution. "Identical" means the
//! sorted `Vec<String>` of declared prop NAMES (the `ComponentProp.name`), never
//! types: fallow cannot resolve types, so two `{ id, label, value, onChange }`
//! shapes group regardless of whether their declared types diverge. This is the
//! conservative, actionable unit: the user can extract ONE shared `Props` type
//! that exactly fits every member.
//!
//! Analyze-layer only: this detector reads the already-cached, already-released
//! `react_props: Vec<ComponentProp>` and `component_functions:
//! Vec<ComponentFunction>` off each reachable `.jsx` / `.tsx` `ModuleInfo` (the
//! same inputs `unused_component_prop.rs` and `prop_drilling.rs` consume). It is
//! a pure grouping pass over existing extraction: NO new IR, NO cache bump.
//!
//! The anti-noise gates are load-bearing, defended as rule-of-three plus a
//! denylist-survivor floor (NOT tuned magic):
//! - [`MIN_SIGNIFICANT_PROPS`] (4): after subtracting the ubiquitous-name
//!   denylist, the remaining "significant" set must have at least four members.
//!   This is what turns `{ label, onClick }` buttons (one significant prop after
//!   the denylist, or even two raw) into NON-findings: two components touching
//!   the same two props is noise; four+ identically-named DOMAIN props recurring
//!   is a real missing abstraction.
//! - [`MIN_GROUP_SIZE`] (3): the rule-of-three. A duplicate twice is tolerable;
//!   the third occurrence is the abstraction trigger. Two components is
//!   intentionally NOT enough.
//! - [`MIN_DISTINCT_FILES`] (2): a single file declaring two same-shaped
//!   variants is local and usually intentional (a render-prop pair, a
//!   `Foo` / `FooImpl` split). Requiring two distinct files is what makes the
//!   group worth a shared type.
//!
//! Exact full-set identity ONLY: a SUPERSET / SUBSET relationship (one component
//! has the four shared props plus a fifth) does NOT group with the four-prop
//! component. Only byte-identical sorted significant sets bucket together, so
//! the finding can always be acted on by extracting ONE shared type that exactly
//! fits every member. This is the price of zero-invalid-groups: five cards that
//! are `{ title, subtitle, href, imageUrl }` and three that add `{ badge }` form
//! TWO groups, not one. That is documented as a known fragmentation tradeoff.
//!
//! Per-component eligibility (zero-FP doctrine): a component participates only
//! when its props are FULLY harvestable
//! (`ComponentFunction.has_unharvestable_props == false`, which already implies
//! zero rest/spread). A partially-known prop set can never be PROVEN identical
//! to another, so it must abstain, never guess (ADR-001).
//!
//! Multi-file group anchor / suppress model copied from
//! [`super::route_collision`]: one finding per member, the sibling members in
//! `sharing_components`, a FILE-level suppress
//! (`// fallow-ignore-file duplicate-prop-shape`) that drops a suppressed member
//! from its OWN finding but keeps it in siblings' `sharing_components` (the group
//! is real regardless of suppression), plus a line-level suppress at the
//! component definition.
//!
//! Dormant by default: the `duplicate-prop-shape` rule defaults to `off`. The
//! detector body does not run when off, so the cost is zero on the audit hot
//! path. Gated on the project declaring `react` / `react-dom` / `next` /
//! `preact` (the same gate as prop-drilling).

use std::path::{Path, PathBuf};

use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::extract::ModuleInfo;

use crate::discover::FileId;
use crate::graph::ModuleGraph;
use crate::results::{DuplicatePropShape, DuplicatePropShapeMember};

use super::{LineOffsetsMap, byte_offset_to_line_col};

/// The minimum number of SIGNIFICANT props (declared names surviving the
/// ubiquitous-name denylist) a component must declare to participate. Below this
/// floor, two components "sharing a shape" is noise (`{ label, onClick }`
/// buttons), not a missing abstraction. Defended as a denylist-survivor floor,
/// not a tuned constant.
const MIN_SIGNIFICANT_PROPS: usize = 4;

/// The minimum number of distinct components a duplicate-shape group must hold.
/// The rule-of-three: a duplicate twice is tolerable; the third occurrence is
/// the abstraction trigger. Two components is intentionally not enough.
const MIN_GROUP_SIZE: usize = 3;

/// The minimum number of distinct files a group must span. A single file with
/// two same-shaped variants is local and usually intentional; two distinct files
/// is what makes the shared abstraction worth extracting.
const MIN_DISTINCT_FILES: usize = 2;

/// Result of the duplicate-prop-shape scan: the surviving groups (one
/// [`DuplicatePropShape`] per participating member) plus the number of React
/// components inspected (for the observability diagnostic, so a silent dep-gate /
/// silent over-abstain is visible). Mirrors `PropDrillingScan.components_scanned`.
#[derive(Debug, Default)]
pub struct DuplicatePropShapeScan {
    /// One located record per component that participates in a surviving group.
    pub groups: Vec<DuplicatePropShape>,
    /// React components inspected across all reachable JSX modules.
    pub components_scanned: usize,
}

/// One eligible component's contribution to a shape bucket: where it lives and
/// its definition span, so the located record anchors at the component
/// definition.
struct Member {
    file: FileId,
    span_start: u32,
    component_name: String,
    path: PathBuf,
}

/// Find duplicate prop-shape groups. Returns an empty scan unless the project
/// declares `react` / `react-dom` / `next` / `preact`. Emits one
/// [`DuplicatePropShape`] per component that participates in a surviving group;
/// each member's `sharing_components` lists the OTHER members (path-sorted), and
/// `shape` is the sorted significant prop-name set the group shares.
#[must_use]
pub fn find_duplicate_prop_shapes(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    declared_deps: &FxHashSet<String>,
    line_offsets_by_file: &LineOffsetsMap<'_>,
) -> DuplicatePropShapeScan {
    let gated = declared_deps.contains("react")
        || declared_deps.contains("react-dom")
        || declared_deps.contains("next")
        || declared_deps.contains("preact");
    if !gated {
        return DuplicatePropShapeScan::default();
    }

    let modules_by_id: FxHashMap<FileId, &ModuleInfo> =
        modules.iter().map(|m| (m.file_id, m)).collect();

    // Bucket eligible components by their significant prop-name vector. The key
    // is the sorted+deduped `Vec<String>` of declared names surviving the
    // denylist; FxHashMap gives deterministic iteration, and the key vectors are
    // sorted, so emit order is stable after the final member path-sort (ADR-004).
    let mut buckets: FxHashMap<Vec<String>, Vec<Member>> = FxHashMap::default();
    let mut components_scanned = 0usize;

    for node in &graph.modules {
        if !node.is_reachable() || !is_react_file(&node.path) {
            continue;
        }
        let Some(module) = modules_by_id.get(&node.file_id) else {
            continue;
        };
        if module.component_functions.is_empty() {
            continue;
        }
        components_scanned += module.component_functions.len();

        // Index harvested props by the component they were declared on.
        let mut props_by_comp: FxHashMap<&str, Vec<&str>> = FxHashMap::default();
        for prop in &module.react_props {
            props_by_comp
                .entry(prop.component.as_str())
                .or_default()
                .push(prop.name.as_str());
        }

        for func in &module.component_functions {
            // A partially-known prop set can never be PROVEN identical to
            // another: abstain (ADR-001). The flag already implies zero
            // rest/spread.
            if func.has_unharvestable_props {
                continue;
            }
            let Some(names) = props_by_comp.get(func.name.as_str()) else {
                continue;
            };
            let significant = significant_prop_set(names);
            if significant.len() < MIN_SIGNIFICANT_PROPS {
                continue;
            }
            buckets.entry(significant).or_default().push(Member {
                file: node.file_id,
                span_start: func.span_start,
                component_name: func.name.clone(),
                path: node.path.clone(),
            });
        }
    }

    let mut groups = Vec::new();
    for (shape, mut members) in buckets {
        if members.len() < MIN_GROUP_SIZE {
            continue;
        }
        let distinct_files: FxHashSet<FileId> = members.iter().map(|m| m.file).collect();
        if distinct_files.len() < MIN_DISTINCT_FILES {
            continue;
        }
        // Path-sort members for stable `sharing_components` / output (ADR-004).
        members.sort_by(|a, b| {
            a.path
                .cmp(&b.path)
                .then(a.span_start.cmp(&b.span_start))
                .then(a.component_name.cmp(&b.component_name))
        });
        emit_group(&shape, &members, line_offsets_by_file, &mut groups);
    }

    // Sort the emitted records by anchor for deterministic output: the shape
    // first (so a group's members stay adjacent), then file / line / component.
    groups.sort_by(|a, b| {
        a.shape
            .cmp(&b.shape)
            .then(a.file.cmp(&b.file))
            .then(a.line.cmp(&b.line))
            .then(a.component.cmp(&b.component))
    });

    DuplicatePropShapeScan {
        groups,
        components_scanned,
    }
}

/// Emit one [`DuplicatePropShape`] per member of a surviving group. Each
/// member's `sharing_components` is the OTHER members (the sibling list), so the
/// route-collision anchor model holds: a per-member finding plus the group
/// roster minus self.
fn emit_group(
    shape: &[String],
    members: &[Member],
    line_offsets_by_file: &LineOffsetsMap<'_>,
    out: &mut Vec<DuplicatePropShape>,
) {
    let group_size = u32::try_from(members.len()).unwrap_or(u32::MAX);
    for member in members {
        let (line, _col) =
            byte_offset_to_line_col(line_offsets_by_file, member.file, member.span_start);
        let sharing_components: Vec<DuplicatePropShapeMember> = members
            .iter()
            .filter(|other| {
                other.file != member.file || other.component_name != member.component_name
            })
            .map(|other| {
                let (other_line, _) =
                    byte_offset_to_line_col(line_offsets_by_file, other.file, other.span_start);
                DuplicatePropShapeMember {
                    file: other.path.clone(),
                    line: other_line,
                    component: other.component_name.clone(),
                }
            })
            .collect();
        out.push(DuplicatePropShape {
            file: member.path.clone(),
            line,
            component: member.component_name.clone(),
            shape: shape.to_vec(),
            group_size,
            sharing_components,
        });
    }
}

/// Subtract the ubiquitous DOM / render-passthrough prop names from a
/// component's declared name set, returning the SIGNIFICANT names sorted and
/// deduped. Two components sharing only `{ className, children, onClick }` yield
/// an EMPTY significant set and never group; the denylist is the entire
/// false-positive defense, so it is comprehensive (handlers, ARIA / data
/// prefixes, layout / HTML passthrough attributes).
fn significant_prop_set(names: &[&str]) -> Vec<String> {
    let mut significant: Vec<String> = names
        .iter()
        .filter(|name| !is_ubiquitous_prop(name))
        .map(|name| (*name).to_string())
        .collect();
    significant.sort_unstable();
    significant.dedup();
    significant
}

/// Whether a prop name is in the ubiquitous DOM / render-passthrough denylist:
/// a name that conveys NO domain shape because it is a generic HTML attribute,
/// an event handler, or an ARIA / data-* / test-id convention. Subtracted before
/// the size floor and the identity comparison.
///
/// `data-*` and `aria-*` are matched by prefix (so `data-testid`, `data-foo`,
/// and `aria-label` are all stripped). The named set is comprehensive for real
/// React: it includes the common pointer / mouse / focus / keyboard / form
/// handlers and the layout-wrapper passthrough attributes (`draggable`,
/// `contentEditable`, `dir`, `lang`, `slot`, `autoFocus`, ...) so a `Box` and a
/// `Stack` primitive do not group on their shared HTML surface.
fn is_ubiquitous_prop(name: &str) -> bool {
    if name.starts_with("data-") || name.starts_with("aria-") {
        return true;
    }
    UBIQUITOUS_PROP_NAMES.contains(&name)
}

/// The fixed denylist of ubiquitous DOM / render-passthrough prop names. These
/// convey no domain shape: a group must share `MIN_SIGNIFICANT_PROPS` names
/// OUTSIDE this set to qualify. Kept sorted for readability.
const UBIQUITOUS_PROP_NAMES: &[&str] = &[
    "autoFocus",
    "children",
    "class",
    "className",
    "contentEditable",
    "dir",
    "disabled",
    "draggable",
    "hidden",
    "id",
    "key",
    "lang",
    "name",
    "onBlur",
    "onChange",
    "onClick",
    "onFocus",
    "onInput",
    "onKeyDown",
    "onMouseDown",
    "onMouseEnter",
    "onMouseLeave",
    "onMouseUp",
    "onPointerDown",
    "onPointerUp",
    "onScroll",
    "onSubmit",
    "ref",
    "role",
    "slot",
    "style",
    "tabIndex",
    "title",
];

/// Whether the path is a React/Preact JSX module (`.jsx` / `.tsx`).
fn is_react_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("jsx" | "tsx")
    )
}
