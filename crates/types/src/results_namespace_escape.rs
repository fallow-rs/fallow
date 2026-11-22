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
    /// 0-based byte column of the import specifier that declares the
    /// binding: the `*` of `import * as Ns`, or the specifier of a named
    /// import.
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

    /// The clause that tells why the binding gets the full credit, as the
    /// finding message uses it after `namespace {name}`.
    #[must_use]
    pub const fn description(self) -> &'static str {
        match self {
            Self::WholeObjectUse => "is used as a whole value",
            Self::ReExportedBinding => "is re-exported as a whole object",
            Self::NoMemberRead => "has no member read by name",
        }
    }
}

impl NamespaceEscape {
    /// The message of the finding, with `module` as the output shows the
    /// target path. The first clause depends on the reason, for example
    /// `namespace Ns is used as a whole value; 2 exports of src/a.ts are not
    /// checked`.
    #[must_use]
    pub fn message(&self, module: &str) -> String {
        let (noun, verb) = if self.hidden_exports == 1 {
            ("export", "is")
        } else {
            ("exports", "are")
        };
        format!(
            "namespace {} {}; {} {noun} of {module} {verb} not checked",
            self.local_name,
            self.reason.description(),
            self.hidden_exports
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn escape(reason: NamespaceEscapeReason, hidden_exports: u32) -> NamespaceEscape {
        NamespaceEscape {
            path: PathBuf::from("src/app.ts"),
            line: 1,
            col: 7,
            local_name: "Ns".to_string(),
            module: PathBuf::from("src/lib.ts"),
            hidden_exports,
            reason,
        }
    }

    #[test]
    fn message_names_the_reason() {
        assert_eq!(
            escape(NamespaceEscapeReason::WholeObjectUse, 2).message("src/lib.ts"),
            "namespace Ns is used as a whole value; 2 exports of src/lib.ts are not checked"
        );
        assert_eq!(
            escape(NamespaceEscapeReason::ReExportedBinding, 2).message("src/lib.ts"),
            "namespace Ns is re-exported as a whole object; 2 exports of src/lib.ts are not checked"
        );
        assert_eq!(
            escape(NamespaceEscapeReason::NoMemberRead, 2).message("src/lib.ts"),
            "namespace Ns has no member read by name; 2 exports of src/lib.ts are not checked"
        );
    }

    #[test]
    fn message_uses_singular_for_one_export() {
        assert_eq!(
            escape(NamespaceEscapeReason::WholeObjectUse, 1).message("src/lib.ts"),
            "namespace Ns is used as a whole value; 1 export of src/lib.ts is not checked"
        );
    }
}
