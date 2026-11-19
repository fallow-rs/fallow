//! SARIF results for boundary violations.

use super::*;

pub(super) fn sarif_boundary_violation_fields(
    violation: &BoundaryViolation,
    root: &Path,
    level: &'static str,
) -> SarifFields {
    let from_uri = relative_uri(&violation.from_path, root);
    let to_uri = relative_uri(&violation.to_path, root);
    let via = violation.via_path.as_ref().map_or_else(String::new, |via| {
        format!(", via {}", relative_uri(via, root))
    });
    SarifFields {
        rule_id: "fallow/boundary-violation",
        level,
        message: violation
            .internal_file_message(&|path| relative_uri(path, root))
            .map_or_else(
                || {
                    format!(
                        "Import from zone '{}' to zone '{}' is not allowed ({}{})",
                        violation.from_zone, violation.to_zone, to_uri, via,
                    )
                },
                |message| format!("{message}{via}"),
            ),
        uri: from_uri,
        region: if violation.line > 0 {
            Some((violation.line, violation.col + 1))
        } else {
            None
        },
        source_path: (violation.line > 0).then(|| violation.from_path.clone()),
        properties: None,
    }
}
