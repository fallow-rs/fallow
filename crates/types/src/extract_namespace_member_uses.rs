//! Keys of namespace member uses that have no local binding.

use crate::extract::NamespaceMemberUseFact;

impl NamespaceMemberUseFact {
    /// The `object` of a use through the inline import type
    /// `import("./x").N.a`. An identifier cannot contain `(`, so the key
    /// never matches a local binding.
    #[must_use]
    pub fn import_type_object(source: &str) -> String {
        format!("import({source})")
    }
}
