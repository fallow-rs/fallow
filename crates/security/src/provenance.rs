//! Import provenance of catalogue rows.
//!
//! A row with an `import_provenance` matches a callee only when the module
//! imports that source. The security detectors, the policy effect lookup and
//! the mutation plan all use the one predicate in this module, so a callee
//! name such as `get` never counts as `https.get` in a module that does not
//! import `node:https`.

use fallow_types::extract::ModuleInfo;

use crate::Matcher;

/// The module bindings that can prove an import provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProvenanceBindings {
    /// ES `import` declarations only.
    Imports,
    /// ES `import` declarations and CommonJS `require()` calls.
    ///
    /// `ModuleInfo::release_resolution_payload` drains `require_calls`, so a
    /// caller that reads a released module sees ES imports only.
    ImportsAndRequires,
    /// [`Self::ImportsAndRequires`], and also a leading identifier that no ES
    /// import and no export of the module binds.
    ///
    /// A released module keeps no `require()` facts, so such an identifier
    /// can be a CommonJS binding of the source. A caller that must not miss
    /// a sink, such as the side-effect prune of the mutation plan, uses this
    /// mode. An identifier that an ES import of another source or a local
    /// export binds does not satisfy the row.
    ReleasedModule,
}

/// Rows that must bind the provenance import to the leading identifier of the
/// callee path. A module that only imports the source is not sufficient for
/// these rows (the `child_process.fork()` precedent).
const BINDING_TRACE_ROWS: [&str; 9] = [
    "command-injection",
    "permissive-cors",
    "electron-unsafe-webpreferences",
    "insecure-temp-file",
    "jwt-alg-none",
    "jwt-verify-missing-algorithms",
    "tls-validation-disabled",
    "mysql-multiple-statements",
    "world-writable-permission",
];

impl Matcher {
    /// Whether the row requires the provenance import's local binding to be
    /// the leading identifier of the callee path.
    #[must_use]
    pub fn requires_binding_trace(&self) -> bool {
        BINDING_TRACE_ROWS.contains(&self.id.as_str())
            || (self.id == "weak-crypto" && self.is_literal_aware())
    }

    /// Whether the module satisfies the row's import provenance for the
    /// callee path.
    ///
    /// A row without provenance is always satisfied. Otherwise the module
    /// must import a source that matches the spec (see
    /// [`import_source_matches`]). For a binding-trace row
    /// ([`Self::requires_binding_trace`]), the import must also bind the
    /// leading identifier of `callee_path`.
    #[must_use]
    pub fn provenance_satisfied(
        &self,
        module: &ModuleInfo,
        callee_path: &str,
        bindings: ProvenanceBindings,
    ) -> bool {
        let Some(spec) = &self.import_provenance else {
            return true;
        };
        let leading_ident = callee_path.split('.').next().unwrap_or(callee_path);
        let binding_trace = self.requires_binding_trace();
        let by_import = module.imports.iter().any(|import| {
            import_source_matches(&import.source, spec)
                && (!binding_trace || import.local_name == leading_ident)
        });
        if by_import || bindings == ProvenanceBindings::Imports {
            return by_import;
        }
        let by_require = module.require_calls.iter().any(|call| {
            import_source_matches(&call.source, spec)
                && (!binding_trace
                    || call.local_name.as_deref() == Some(leading_ident)
                    || call
                        .destructured_names
                        .iter()
                        .any(|name| name == leading_ident))
        });
        by_require
            || (bindings == ProvenanceBindings::ReleasedModule
                && !binds_locally(module, leading_ident))
    }
}

/// Whether an ES import or an export of the module binds the identifier.
fn binds_locally(module: &ModuleInfo, ident: &str) -> bool {
    module
        .imports
        .iter()
        .any(|import| import.local_name == ident)
        || module.exports.iter().any(|export| {
            export
                .local_name
                .as_deref()
                .map_or_else(|| export.name.matches_str(ident), |local| local == ident)
        })
}

/// Compare an import source with a provenance spec.
///
/// The `node:` prefix is optional on both sides (`node:child_process` matches
/// `child_process`), and a package subpath matches its package
/// (`mysql2/promise` matches `mysql2`).
#[must_use]
pub fn import_source_matches(source: &str, spec: &str) -> bool {
    let source = strip_node_prefix(source);
    let spec = strip_node_prefix(spec);
    source == spec
        || source
            .strip_prefix(spec)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn strip_node_prefix(value: &str) -> &str {
    value.strip_prefix("node:").unwrap_or(value)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "the tests look up catalogue rows that the catalogue must contain"
)]
mod tests {
    use fallow_types::discover::FileId;
    use fallow_types::extract::{
        ExportInfo, ExportName, ImportInfo, ImportedName, RequireCallInfo, SinkShape, VisibilityTag,
    };
    use oxc_span::Span;

    use super::*;

    fn row(id: &str, shape: SinkShape, callee: &str) -> &'static Matcher {
        crate::catalogue()
            .matchers()
            .iter()
            .find(|matcher| {
                matcher.id == id
                    && matcher.sink_shape == shape
                    && matcher.first_matching_pattern(callee).is_some()
            })
            .expect("catalogue row exists")
    }

    fn import(source: &str, local: &str) -> ImportInfo {
        ImportInfo {
            source: source.to_owned(),
            imported_name: ImportedName::Named(local.to_owned()),
            local_name: local.to_owned(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: Span::default(),
            source_span: Span::default(),
        }
    }

    fn require(source: &str, local: Option<&str>, destructured: &[&str]) -> RequireCallInfo {
        RequireCallInfo {
            source: source.to_owned(),
            span: Span::default(),
            source_span: Span::default(),
            destructured_names: destructured.iter().map(|&name| name.to_owned()).collect(),
            local_name: local.map(str::to_owned),
            is_type_only: false,
        }
    }

    fn export(name: &str) -> ExportInfo {
        ExportInfo {
            name: ExportName::Named(name.to_owned()),
            local_name: None,
            is_type_only: false,
            is_side_effect_used: false,
            visibility: VisibilityTag::None,
            expected_unused_reason: None,
            deprecated: false,
            deprecated_reason: None,
            span: Span::default(),
            members: Vec::new(),
            super_class: None,
        }
    }

    fn module(imports: Vec<ImportInfo>, requires: Vec<RequireCallInfo>) -> ModuleInfo {
        ModuleInfo {
            imports,
            require_calls: requires,
            ..ModuleInfo::empty(FileId(0))
        }
    }

    #[test]
    fn a_local_get_does_not_satisfy_the_https_row() {
        let tls = row("tls-validation-disabled", SinkShape::Call, "get");
        let local = module(vec![import("./get", "get")], Vec::new());
        assert!(!tls.provenance_satisfied(&local, "get", ProvenanceBindings::ImportsAndRequires));
        let unbound = module(Vec::new(), Vec::new());
        assert!(!tls.provenance_satisfied(&unbound, "get", ProvenanceBindings::Imports));
    }

    #[test]
    fn the_https_import_must_bind_the_leading_identifier() {
        let tls = row("tls-validation-disabled", SinkShape::Call, "get");
        let named = module(vec![import("node:https", "get")], Vec::new());
        assert!(tls.provenance_satisfied(&named, "get", ProvenanceBindings::Imports));
        let other = module(vec![import("https", "request")], Vec::new());
        assert!(!tls.provenance_satisfied(&other, "get", ProvenanceBindings::Imports));
    }

    #[test]
    fn a_row_without_binding_trace_accepts_any_import_of_the_source() {
        let fs = row("path-traversal", SinkShape::MemberCall, "fs.readFile");
        assert!(!fs.requires_binding_trace());
        let imported = module(vec![import("fs", "files")], Vec::new());
        assert!(fs.provenance_satisfied(&imported, "fs.readFile", ProvenanceBindings::Imports));
    }

    #[test]
    fn a_require_call_counts_only_when_the_caller_admits_requires() {
        let tls = row(
            "tls-validation-disabled",
            SinkShape::MemberCall,
            "https.get",
        );
        let namespace = module(Vec::new(), vec![require("https", Some("https"), &[])]);
        assert!(!tls.provenance_satisfied(&namespace, "https.get", ProvenanceBindings::Imports));
        assert!(tls.provenance_satisfied(
            &namespace,
            "https.get",
            ProvenanceBindings::ImportsAndRequires
        ));
        let destructured = module(Vec::new(), vec![require("node:https", None, &["get"])]);
        assert!(tls.provenance_satisfied(
            &destructured,
            "get",
            ProvenanceBindings::ImportsAndRequires
        ));
    }

    #[test]
    fn a_released_module_treats_an_unbound_identifier_as_a_hidden_require() {
        let tls = row(
            "tls-validation-disabled",
            SinkShape::MemberCall,
            "https.get",
        );
        let mode = ProvenanceBindings::ReleasedModule;
        let unbound = module(Vec::new(), Vec::new());
        assert!(tls.provenance_satisfied(&unbound, "https.get", mode));
        let elsewhere = module(vec![import("./client", "https")], Vec::new());
        assert!(!tls.provenance_satisfied(&elsewhere, "https.get", mode));
        let get = row("tls-validation-disabled", SinkShape::Call, "get");
        let exported = ModuleInfo {
            exports: vec![export("get")].into(),
            ..module(Vec::new(), Vec::new())
        };
        assert!(!get.provenance_satisfied(&exported, "get", mode));
        let imported = module(vec![import("node:https", "get")], Vec::new());
        assert!(get.provenance_satisfied(&imported, "get", mode));
    }

    #[test]
    fn a_row_without_provenance_is_always_satisfied() {
        let row = crate::catalogue()
            .matchers()
            .iter()
            .find(|matcher| matcher.import_provenance.is_none())
            .expect("a row without provenance exists");
        let empty = module(Vec::new(), Vec::new());
        assert!(row.provenance_satisfied(&empty, "anything", ProvenanceBindings::Imports));
    }

    #[test]
    fn the_source_match_admits_the_node_prefix_and_package_subpaths() {
        assert!(import_source_matches("node:child_process", "child_process"));
        assert!(import_source_matches("child_process", "node:child_process"));
        assert!(import_source_matches("mysql2/promise", "mysql2"));
        assert!(!import_source_matches("mysql2-extra", "mysql2"));
    }
}
