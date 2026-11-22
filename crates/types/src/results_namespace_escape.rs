//! Namespace bindings that hide exports from the unused-export check.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::serde_path;

/// A namespace binding that the code uses as a whole value, so fallow
/// credits every export of the target module and does not check them.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NamespaceEscape {
    /// The file that declares the binding.
    #[serde(serialize_with = "serde_path::serialize")]
    pub path: PathBuf,
    /// 1-based line of the import that declares the binding.
    pub line: u32,
    /// 0-based byte column of that import.
    pub col: u32,
    /// The local name of the binding.
    pub local_name: String,
    /// The module whose exports get the full credit.
    #[serde(serialize_with = "serde_path::serialize")]
    pub module: PathBuf,
    /// The count of exports in `module` and the modules it re-exports from
    /// that would be unused-export or unused-type findings without this credit.
    pub hidden_exports: u32,
    /// Why the binding gets the full credit.
    pub reason: NamespaceEscapeReason,
}

/// Why a namespace binding gets the full export credit of its target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum NamespaceEscapeReason {
    /// The code uses the binding as a value: a cast, an assignment, a call
    /// argument, a spread, a computed member read, or a reflective call such
    /// as `Object.keys(Ns)`.
    WholeObjectUse,
    /// A module that is not an entry point re-exports the binding with
    /// `export { Ns }`, so fallow cannot see which members its consumers read.
    ReExportedBinding,
    /// The code reads no member of the binding by name and does not use it
    /// as a value, for example only in `typeof Ns`.
    NoMemberRead,
}

impl NamespaceEscapeReason {
    /// The kebab-case name of the reason, as in JSON output.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::WholeObjectUse => "whole-object-use",
            Self::ReExportedBinding => "re-exported-binding",
            Self::NoMemberRead => "no-member-read",
        }
    }
}

impl NamespaceEscape {
    /// The message of the finding, with `module` as the output shows the
    /// target path: `Ns escapes as a value; 2 exports of src/a.ts are not
    /// checked`.
    #[must_use]
    pub fn message(&self, module: &str) -> String {
        let (noun, verb) = if self.hidden_exports == 1 {
            ("export", "is")
        } else {
            ("exports", "are")
        };
        format!(
            "{} escapes as a value; {} {noun} of {module} {verb} not checked",
            self.local_name, self.hidden_exports
        )
    }
}
