//! Resolution of CommonJS `require()` calls.
//!
//! Converts `require()` calls into the same [`ResolvedImport`] representation used
//! for ES module imports, allowing the graph builder to treat all import kinds uniformly.
//!
//! Destructured requires (`const { a, b } = require('./x')`) become named imports.
//! Non-destructured requires (`const mod = require('./x')`) become namespace imports
//! as a conservative default — the entire module binding may be used in ways that
//! cannot be statically determined.

use std::path::Path;

use fallow_types::extract::{ImportInfo, ImportedName, RequireCallInfo};

use super::ResolvedImport;
use super::specifier::resolve_specifier;
use super::types::ResolveContext;

/// Resolve CommonJS `require()` calls.
/// Destructured requires become Named imports; others become Namespace (conservative).
pub(super) fn resolve_require_imports(
    ctx: &ResolveContext,
    file_path: &Path,
    require_calls: &[RequireCallInfo],
) -> Vec<ResolvedImport> {
    require_calls
        .iter()
        .flat_map(|req| resolve_single_require(ctx, file_path, req))
        .collect()
}

/// Convert a single `require()` call into one or more `ResolvedImport` entries.
pub(super) fn resolve_single_require(
    ctx: &ResolveContext,
    file_path: &Path,
    req: &RequireCallInfo,
) -> Vec<ResolvedImport> {
    let target = resolve_specifier(ctx, file_path, &req.source, false);

    if req.destructured_names.is_empty() {
        return vec![ResolvedImport {
            info: ImportInfo {
                source: req.source.clone(),
                imported_name: ImportedName::Namespace,
                local_name: req.local_name.clone().unwrap_or_default(),
                is_type_only: false,
                from_style: false,
                span: req.span,
                source_span: req.source_span,
            },
            target,
        }];
    }

    req.destructured_names
        .iter()
        .map(|name| ResolvedImport {
            info: ImportInfo {
                source: req.source.clone(),
                imported_name: ImportedName::Named(name.clone()),
                local_name: name.clone(),
                is_type_only: false,
                from_style: false,
                span: req.span,
                source_span: req.source_span,
            },
            target: target.clone(),
        })
        .collect()
}
