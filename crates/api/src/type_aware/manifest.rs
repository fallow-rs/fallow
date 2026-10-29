//! Generated constants for the Fallow-owned type-aware protocol contract.

/// One type-checker backend from the protocol manifest.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct BackendSpec {
    /// Backend family name, the value of [`BACKEND_ENV`].
    pub family: &'static str,
    /// npm package that provides the backend.
    pub package: &'static str,
    /// Version that the repository tests and that install hints name.
    pub version: &'static str,
    /// Lowest supported `major.minor.patch`, inclusive.
    pub minimum_version: &'static str,
    /// First unsupported `major.minor.patch`.
    pub below_version: &'static str,
}

impl BackendSpec {
    /// Whether a resolved backend version is in the supported range. Only
    /// `major.minor.patch` counts, so a tsrs build tag such as
    /// `-ts7.1.0-beta` or a TypeScript prerelease stays in range.
    pub(super) fn supports(&self, version: &str) -> bool {
        let (Some(core), Some(minimum), Some(below)) = (
            version_core(version),
            version_core(self.minimum_version),
            version_core(self.below_version),
        ) else {
            return false;
        };
        core >= minimum && core < below
    }

    /// Check the backend family and version that a companion response reports.
    ///
    /// # Errors
    ///
    /// Returns a message when the family differs or the version is outside
    /// the supported range.
    pub(super) fn check_reported(&self, family: &str, version: &str) -> Result<(), String> {
        if family != self.family {
            return Err(format!(
                "unsupported type-aware backend {family}; expected {}",
                self.family
            ));
        }
        if !self.supports(version) {
            return Err(format!(
                "unsupported type-aware backend version {version}; {} supports {}",
                self.family,
                self.range()
            ));
        }
        Ok(())
    }

    /// Supported range in npm notation, for messages.
    pub(super) fn range(&self) -> String {
        format!(">={} <{}", self.minimum_version, self.below_version)
    }
}

/// Guarded-fix evidence comes only from the default backend at the version
/// that the corpus validates. Another version in the supported range still
/// refines findings, but it never makes a fix eligible.
pub(super) fn supports_guarded_fix(family: &str, version: &str) -> bool {
    family == BACKEND_FAMILY && version == BACKEND_VERSION
}

fn version_core(version: &str) -> Option<(u64, u64, u64)> {
    let end = version.find(['-', '+']).unwrap_or(version.len());
    let mut parts = version[..end].split('.').map(str::parse::<u64>);
    let core = (
        parts.next()?.ok()?,
        parts.next()?.ok()?,
        parts.next()?.ok()?,
    );
    parts.next().is_none().then_some(core)
}

include!(concat!(env!("OUT_DIR"), "/type_aware_protocol.rs"));

/// Environment variable that selects a type-checker backend from [`BACKENDS`].
pub(super) const BACKEND_ENV: &str = "FALLOW_TYPE_AWARE_BACKEND";

/// Backend that [`BACKEND_ENV`] selects. An unset or empty value selects the
/// default backend. The process reads the variable once, so every request and
/// session in one process uses the same backend.
///
/// # Errors
///
/// Returns a message that names the known families when the value is unknown
/// or is not valid Unicode.
pub(super) fn selected_backend() -> Result<BackendSpec, String> {
    static SELECTED: std::sync::OnceLock<Result<BackendSpec, String>> = std::sync::OnceLock::new();
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

fn backend_for(requested: &str) -> Result<BackendSpec, String> {
    let requested = if requested.is_empty() {
        BACKEND_FAMILY
    } else {
        requested
    };
    BACKENDS
        .iter()
        .find(|backend| backend.family == requested)
        .copied()
        .ok_or_else(|| {
            let known = BACKENDS
                .iter()
                .map(|backend| backend.family)
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
        let backend = backend_for("").expect("default backend");
        assert_eq!(backend, BACKENDS[0]);
        assert_eq!(
            (backend.family, backend.version),
            (BACKEND_FAMILY, BACKEND_VERSION)
        );
    }

    #[test]
    fn known_family_selects_its_backend() {
        let last = BACKENDS[BACKENDS.len() - 1];
        assert_eq!(backend_for(last.family), Ok(last));
    }

    #[test]
    fn unknown_family_names_the_known_families() {
        let error = backend_for("tsc").expect_err("tsc is not a backend family");
        assert!(error.contains("`tsc`"), "{error}");
        assert!(error.contains(BACKEND_FAMILY), "{error}");
    }

    #[test]
    fn every_backend_supports_its_tested_version() {
        for backend in BACKENDS {
            assert!(backend.supports(backend.version), "{backend:?}");
        }
    }

    #[test]
    fn tsrs_accepts_newer_builds_in_its_range() {
        let tsrs = backend_for("tsrs").expect("tsrs backend");
        for version in [
            "0.18.0",
            "0.18.0-ts7.1.0-dev.20260929",
            "0.20.1-ts7.1.0-beta",
            "0.20.1+build",
            "0.99.0",
        ] {
            assert!(tsrs.supports(version), "{version}");
        }
        for version in [
            "0.17.0",
            "0.17.99",
            "1.0.0-ts8.0.0",
            "0.20",
            "0.20.1.2",
            "0.20.1foo",
            "garbage",
        ] {
            assert!(!tsrs.supports(version), "{version}");
        }
        assert_eq!(tsrs.range(), ">=0.18.0 <1.0.0");
    }

    #[test]
    fn reported_backend_must_match_family_and_range() {
        let typescript = backend_for("").expect("default backend");
        assert_eq!(
            typescript.check_reported(BACKEND_FAMILY, "7.1.0-beta"),
            Ok(())
        );
        let error = typescript
            .check_reported(BACKEND_FAMILY, "8.0.0")
            .expect_err("8.0.0 is out of range");
        assert!(error.contains(">=7.0.0 <8.0.0"), "{error}");
        assert!(typescript.check_reported("tsrs", BACKEND_VERSION).is_err());
    }

    #[test]
    fn guarded_fixes_need_the_tested_default_backend_version() {
        assert!(supports_guarded_fix(BACKEND_FAMILY, BACKEND_VERSION));
        assert!(!supports_guarded_fix(BACKEND_FAMILY, "7.1.0-beta"));
        assert!(!supports_guarded_fix("tsrs", BACKEND_VERSION));
    }

    #[test]
    fn typescript_accepts_any_7x_release() {
        let typescript = backend_for("").expect("default backend");
        for version in [
            "7.0.0",
            "7.0.2",
            "7.1.0-beta",
            "7.1.0-dev.20261029.1",
            "7.99.99",
        ] {
            assert!(typescript.supports(version), "{version}");
        }
        for version in ["6.0.3", "8.0.0-beta"] {
            assert!(!typescript.supports(version), "{version}");
        }
    }
}
