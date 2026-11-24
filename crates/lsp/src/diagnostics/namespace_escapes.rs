//! Diagnostics for namespace imports that the code uses as a whole value.

use std::path::Path;

use fallow_api::EditorAnalysisResults as AnalysisResults;
use fallow_types::output_dead_code::{EffectiveSeverity, NamespaceEscapeFinding};
use ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Uri};
use rustc_hash::FxHashMap;

use super::{doc_link_for_code, finding_data};
use crate::position::{PositionMapper, line_range_from_byte_col};

const CODE: &str = "namespace-escape";

/// Push one diagnostic on the import line of each namespace escape. The
/// severity follows the rule: `error` gives an error, `warn` a warning.
pub fn push_namespace_escape_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    root: &Path,
    mapper: &mut PositionMapper,
) {
    for finding in &results.namespace_escapes {
        let Some(uri) = Uri::from_file_path(&finding.escape.path) else {
            continue;
        };
        let diagnostic = namespace_escape_diagnostic(finding, root, mapper);
        map.entry(uri).or_default().push(diagnostic);
    }
}

/// The diagnostic of one namespace escape, on the import specifier that
/// declares the binding.
#[must_use]
pub fn namespace_escape_diagnostic(
    finding: &NamespaceEscapeFinding,
    root: &Path,
    mapper: &mut PositionMapper,
) -> Diagnostic {
    let escape = &finding.escape;
    let range = line_range_from_byte_col(
        mapper,
        &escape.path,
        escape.line.saturating_sub(1),
        escape.col,
    );
    let module = escape
        .module
        .strip_prefix(root)
        .unwrap_or(&escape.module)
        .to_string_lossy()
        .replace('\\', "/");
    Diagnostic {
        range,
        severity: Some(match finding.effective_severity {
            Some(EffectiveSeverity::Error) => DiagnosticSeverity::ERROR,
            _ => DiagnosticSeverity::WARNING,
        }),
        source: Some("fallow".to_string()),
        code: Some(NumberOrString::String(CODE.to_string())),
        code_description: doc_link_for_code(CODE),
        message: escape.message(&module),
        data: finding_data(finding.finding_id.as_deref()),
        ..Default::default()
    }
}
