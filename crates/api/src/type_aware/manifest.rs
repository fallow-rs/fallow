//! Generated constants for the Fallow-owned type-aware protocol contract.

include!(concat!(env!("OUT_DIR"), "/type_aware_protocol.rs"));

/// Environment variable that selects a type-checker backend from [`BACKENDS`].
pub(super) const BACKEND_ENV: &str = "FALLOW_TYPE_AWARE_BACKEND";

/// Backend family and exact version that [`BACKEND_ENV`] selects. An unset or
/// empty value selects the default backend.
///
/// # Errors
///
/// Returns a message that names the known families when the value is unknown.
pub(super) fn selected_backend() -> Result<(&'static str, &'static str), String> {
    let requested = std::env::var(BACKEND_ENV).unwrap_or_default();
    backend_for(requested.trim())
}

fn backend_for(requested: &str) -> Result<(&'static str, &'static str), String> {
    if requested.is_empty() {
        return Ok((BACKEND_FAMILY, BACKEND_VERSION));
    }
    BACKENDS
        .iter()
        .find(|(family, _)| *family == requested)
        .copied()
        .ok_or_else(|| {
            let known = BACKENDS
                .iter()
                .map(|(family, _)| *family)
                .collect::<Vec<_>>();
            format!(
                "unknown {BACKEND_ENV} value `{requested}`; expected one of: {}",
                known.join(", ")
            )
        })
}

/// Backend family for metadata when no companion response exists. An invalid
/// selection falls back to the default; the run reports that error elsewhere.
pub(super) fn selected_backend_family() -> &'static str {
    selected_backend().map_or(BACKEND_FAMILY, |(family, _)| family)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_selection_is_the_default_backend() {
        assert_eq!(backend_for(""), Ok((BACKEND_FAMILY, BACKEND_VERSION)));
        assert_eq!(BACKENDS[0], (BACKEND_FAMILY, BACKEND_VERSION));
    }

    #[test]
    fn known_family_selects_its_pinned_version() {
        let (family, version) = BACKENDS[BACKENDS.len() - 1];
        assert_eq!(backend_for(family), Ok((family, version)));
    }

    #[test]
    fn unknown_family_names_the_known_families() {
        let error = backend_for("tsc").expect_err("tsc is not a backend family");
        assert!(error.contains("`tsc`"), "{error}");
        assert!(error.contains(BACKEND_FAMILY), "{error}");
    }
}
