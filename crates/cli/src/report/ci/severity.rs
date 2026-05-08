use fallow_config::Severity;

#[must_use]
pub const fn sarif_level(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warn => "warning",
        Severity::Off => unreachable!(),
    }
}

#[must_use]
pub const fn codeclimate_severity(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "major",
        Severity::Warn => "minor",
        Severity::Off => unreachable!(),
    }
}

#[must_use]
pub const fn github_check_conclusion(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "failure",
        Severity::Warn => "neutral",
        Severity::Off => "success",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_error_across_ci_surfaces() {
        assert_eq!(sarif_level(Severity::Error), "error");
        assert_eq!(codeclimate_severity(Severity::Error), "major");
        assert_eq!(github_check_conclusion(Severity::Error), "failure");
        assert_eq!(review_label(Severity::Error), "error");
    }

    #[test]
    fn maps_warn_across_ci_surfaces() {
        assert_eq!(sarif_level(Severity::Warn), "warning");
        assert_eq!(codeclimate_severity(Severity::Warn), "minor");
        assert_eq!(github_check_conclusion(Severity::Warn), "neutral");
        assert_eq!(review_label(Severity::Warn), "warn");
    }
}
