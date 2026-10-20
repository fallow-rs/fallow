//! Generated constants for the Fallow-owned type-aware protocol contract.

include!(concat!(env!("OUT_DIR"), "/type_aware_protocol.rs"));

/// Environment variable that selects a type-checker backend from [`BACKENDS`].
pub(super) const BACKEND_ENV: &str = "FALLOW_TYPE_AWARE_BACKEND";

/// Backend family and exact version that [`BACKEND_ENV`] selects. An unset or
/// empty value selects the default backend. The process reads the variable
/// once, so every request and session in one process uses the same backend.
///
/// # Errors
///
/// Returns a message that names the known families when the value is unknown
/// or is not valid Unicode.
pub(super) fn selected_backend() -> Result<(&'static str, &'static str), String> {
    static SELECTED: std::sync::OnceLock<Result<(&'static str, &'static str), String>> =
        std::sync::OnceLock::new();
    SELECTED
        .get_or_init(|| match std::env::var_os(BACKEND_ENV) {
            None => backend_for(""),
            Some(value) => value.to_str().map_or_else(
                || Err(format!("{BACKEND_ENV} is not valid Unicode")),
                |requested| backend_for(requested.trim()),
            ),
        })
        .clone()
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
