use fallow_api::{DEFAULT_TYPE_AWARE_BACKEND_FAMILY, TypeAwareStatus};

/// The word before the backend version. The default backend is TypeScript, and
/// each other backend has a version scheme of its own, so it keeps its name.
fn backend_label(family: Option<&str>) -> &str {
    match family {
        None | Some(DEFAULT_TYPE_AWARE_BACKEND_FAMILY) => "TypeScript",
        Some(family) => family,
    }
}

/// The message of the human `type-aware status` line for an available companion.
pub(super) fn available_message(status: &TypeAwareStatus) -> String {
    format!(
        "Type-aware companion: available ({}, protocol {}, {} {})",
        status.package_version.as_deref().unwrap_or("unknown"),
        status.protocol_version,
        backend_label(status.backend_family.as_deref()),
        status.backend_version.as_deref().unwrap_or("unknown"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(family: Option<&str>, version: Option<&str>) -> TypeAwareStatus {
        TypeAwareStatus {
            available: true,
            discovery_source: None,
            companion_path: None,
            package_version: Some("3.38.1".to_owned()),
            protocol_version: 7,
            backend_family: family.map(str::to_owned),
            backend_version: version.map(str::to_owned),
            remediation: None,
        }
    }

    #[test]
    fn default_backend_line_stays_byte_identical() {
        assert_eq!(
            available_message(&status(Some("typescript-go"), Some("7.0.2"))),
            "Type-aware companion: available (3.38.1, protocol 7, TypeScript 7.0.2)"
        );
    }

    #[test]
    fn tsrs_version_carries_the_tsrs_label() {
        assert_eq!(
            available_message(&status(Some("tsrs"), Some("0.20.1-ts7.1.0-beta"))),
            "Type-aware companion: available (3.38.1, protocol 7, tsrs 0.20.1-ts7.1.0-beta)"
        );
    }

    #[test]
    fn missing_family_keeps_the_typescript_label() {
        assert_eq!(
            available_message(&status(None, None)),
            "Type-aware companion: available (3.38.1, protocol 7, TypeScript unknown)"
        );
    }
}
