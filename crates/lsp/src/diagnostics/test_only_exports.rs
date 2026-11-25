//! Diagnostics for exported values that only test-side files use.

use fallow_api::EditorAnalysisResults as AnalysisResults;
use fallow_types::output_dead_code::{EffectiveSeverity, TestOnlyExportFinding};
use ls_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Uri};
use rustc_hash::FxHashMap;

use super::{doc_link_for_code, finding_data};
use crate::position::{PositionMapper, line_range_from_byte_col};

const CODE: &str = "test-only-export";

/// Push one diagnostic on the export line of each test-only export. The
/// severity follows the rule: `error` gives an error, `warn` a warning.
pub fn push_test_only_export_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.test_only_exports {
        let Some(uri) = Uri::from_file_path(&finding.export.path) else {
            continue;
        };
        let diagnostic = test_only_export_diagnostic(finding, mapper);
        map.entry(uri).or_default().push(diagnostic);
    }
}

/// The diagnostic of one test-only export, on its export line.
#[must_use]
pub fn test_only_export_diagnostic(
    finding: &TestOnlyExportFinding,
    mapper: &mut PositionMapper,
) -> Diagnostic {
    let export = &finding.export;
    let range = line_range_from_byte_col(
        mapper,
        &export.path,
        export.line.saturating_sub(1),
        export.col,
    );
    Diagnostic {
        range,
        severity: Some(match finding.effective_severity {
            Some(EffectiveSeverity::Error) => DiagnosticSeverity::ERROR,
            _ => DiagnosticSeverity::WARNING,
        }),
        source: Some("fallow".to_string()),
        code: Some(NumberOrString::String(CODE.to_string())),
        code_description: doc_link_for_code(CODE),
        message: export.message(),
        data: finding_data(finding.finding_id.as_deref()),
        ..Default::default()
    }
}
