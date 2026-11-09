//! Opt-in syntactic shared-state inventory, independent of counted findings.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Which optional facts a parser collects. Normal analysis collects none of them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ExtractionMode {
    /// Ordinary extraction.
    #[default]
    Standard,
    /// Include the shared-state inventory facts.
    SharedState,
}

/// A direct property operation. Calls are not classified as writes.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Serialize,
    Deserialize,
    bitcode::Encode,
    bitcode::Decode,
)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum StateOperation {
    /// Value read, including a call receiver.
    Read,
    /// Simple assignment.
    Assign,
    /// Compound or logical assignment, which may read the previous value.
    ReadModifyWrite,
    /// Increment or decrement.
    Update,
    /// Property deletion.
    Delete,
}

/// Binding provenance established by the semantic pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StateRoot {
    /// Index in the containing module's runtime ESM import list.
    Import {
        /// Import index, rebased when embedded scripts are merged.
        import_index: u32,
    },
    /// Exact unshadowed global identifier spelling.
    Global {
        /// One of window, globalThis, self or global.
        name: String,
    },
}

/// Compact source fact with exact static property segments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, bitcode::Encode, bitcode::Decode)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct StateAccessFact {
    /// Lexically established root.
    pub root: StateRoot,
    /// Property segments, preserving a literal dot inside one key.
    pub path: Vec<String>,
    /// Direct operation on the full path.
    pub operation: StateOperation,
    /// Byte offset in the original source.
    pub span_start: u32,
}

/// A retained source location.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct StateSite {
    /// Project-relative source path.
    pub path: PathBuf,
    /// One-based source line.
    pub line: usize,
    /// One-based byte column.
    pub column: usize,
    /// Original byte offset.
    pub offset: u32,
}

/// Manual review guidance. Inventory observations cannot be automatically fixed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct StateAction {
    /// Human-readable next step.
    pub description: String,
    /// Always false.
    pub auto_fixable: bool,
}

/// A write through an imported value binding with one declaration origin.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct ImportedStateWrite {
    /// Stable observation identity.
    pub id: String,
    /// Writer location.
    pub writer: StateSite,
    /// Canonical value declaration, not inferred runtime object ownership.
    pub declaration: StateSite,
    /// Canonical exported name.
    pub binding: String,
    /// Exact property segments relative to the imported value.
    pub property: Vec<String>,
    /// Direct write operation.
    pub operation: StateOperation,
    /// Manual guidance.
    pub actions: Vec<StateAction>,
}

/// Runtime relationships that static observations cannot establish.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum UnknownStateRelation {
    /// No runtime or ordering claim.
    Unknown,
}

/// A source access within a global group.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GlobalStateAccess {
    /// Source location.
    pub site: StateSite,
    /// Full path, including the direct slot.
    pub property: Vec<String>,
    /// Direct operation.
    pub operation: StateOperation,
}

/// One exact root and custom slot, with retained reads and writes across files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct GlobalStateGroup {
    /// Stable observation identity.
    pub id: String,
    /// Exact root spelling. Roots are never normalized across runtimes.
    pub root: String,
    /// Direct custom slot.
    pub slot: String,
    /// Sorted read sites.
    pub reads: Vec<GlobalStateAccess>,
    /// Sorted write sites.
    pub writes: Vec<GlobalStateAccess>,
    /// Whether the files execute in the same runtime is unknown.
    pub same_runtime: UnknownStateRelation,
    /// Execution order is unknown.
    pub execution_order: UnknownStateRelation,
    /// Manual guidance.
    pub actions: Vec<StateAction>,
}

/// Versioned advisory inventory. Its observations are never counted as findings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SharedStateInventory {
    /// Inventory contract version.
    #[cfg_attr(feature = "schema", schemars(range(min = 1, max = 1)))]
    pub version: u32,
    /// False if source parsing or retention limits omitted supported evidence.
    pub complete: bool,
    /// Whether bounded fact retention omitted sites.
    pub truncated: bool,
    /// Imported value property writes.
    pub imported_writes: Vec<ImportedStateWrite>,
    /// Exact global root/slot groups.
    pub globals: Vec<GlobalStateGroup>,
}

/// Combine optional inventories without converting observations into findings.
pub(crate) fn merge_inventory(
    target: &mut Option<SharedStateInventory>,
    other: Option<SharedStateInventory>,
) {
    let Some(other) = other else { return };
    let Some(target) = target else {
        *target = Some(other);
        return;
    };
    target.complete &= other.complete;
    target.truncated |= other.truncated;
    target.imported_writes.extend(other.imported_writes);
    target
        .imported_writes
        .sort_by(|left, right| left.id.cmp(&right.id));
    target
        .imported_writes
        .dedup_by(|left, right| left.id == right.id);
    // Different project roots do not establish a shared global runtime.
    target.globals.extend(other.globals);
    target.globals.sort_by(|left, right| left.id.cmp(&right.id));
    target.globals.dedup();
}

impl SharedStateInventory {
    /// Narrow observations by source site, retaining complete global evidence groups.
    pub fn retain_sites(&mut self, keep: impl Fn(&StateSite) -> bool) {
        self.imported_writes.retain(|write| keep(&write.writer));
        self.globals.retain(|group| {
            group
                .reads
                .iter()
                .chain(&group.writes)
                .any(|access| keep(&access.site))
        });
    }
}

impl StateOperation {
    /// Stable operation spelling used by inventory explanations and identities.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Read => "read",
            Self::Assign => "assign",
            Self::ReadModifyWrite => "read_modify_write",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}
