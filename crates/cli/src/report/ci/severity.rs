use fallow_config::Severity;

use crate::output_envelope::CodeClimateSeverity;

#[must_use]
pub const fn sarif_level(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warn => "warning",
        Severity::Off => unreachable!(),
    }
}

#[must_use]
pub const fn review_label(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warn => "warn",
        Severity::Off => "off",
    }
}

#[must_use]
pub const fn codeclimate_severity(severity: Severity) -> CodeClimateSeverity {
    match severity {
        Severity::Error => CodeClimateSeverity::Major,
        Severity::Warn => CodeClimateSeverity::Minor,
        Severity::Off => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_error_across_ci_surfaces() {
        assert_eq!(sarif_level(Severity::Error), "error");
        assert_eq!(review_label(Severity::Error), "error");
        assert_eq!(
            codeclimate_severity(Severity::Error),
            CodeClimateSeverity::Major
        );
    }

    #[test]
    fn maps_warn_across_ci_surfaces() {
        assert_eq!(sarif_level(Severity::Warn), "warning");
        assert_eq!(review_label(Severity::Warn), "warn");
        assert_eq!(
            codeclimate_severity(Severity::Warn),
            CodeClimateSeverity::Minor
        );
    }

    #[test]
    #[should_panic(expected = "internal error: entered unreachable code")]
    fn codeclimate_severity_off_is_unreachable() {
        let _ = codeclimate_severity(Severity::Off);
    }
}
