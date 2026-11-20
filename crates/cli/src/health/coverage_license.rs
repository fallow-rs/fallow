//! License check for local runtime coverage. A single local capture needs no
//! license, so only the other input shapes fail on a license problem.

use std::ffi::OsStr;
use std::fs;
use std::path::Path;

use fallow_license::{
    DEFAULT_HARD_FAIL_DAYS, Feature, LicenseStatus, load_and_verify, load_raw_jwt,
};
use fallow_output::RuntimeCoverageWatermark;

use crate::license::verifying_key;

/// The license JWT to send to the sidecar and the watermark for the report.
pub(super) struct RuntimeLicense {
    pub jwt: String,
    pub watermark: Option<RuntimeCoverageWatermark>,
}

/// Resolve the license for a runtime coverage input at `path`.
///
/// No license, or a license problem with a single local capture, gives an
/// empty JWT. The sidecar still gates the shapes that need a license when it
/// receives no license. A license problem with any other input shape gives
/// `Err` with the message for the user.
pub(super) fn resolve_runtime_license(path: &Path) -> Result<RuntimeLicense, String> {
    match load_license() {
        Ok(Some((jwt, status))) => Ok(RuntimeLicense {
            jwt,
            watermark: status
                .show_watermark()
                .then_some(RuntimeCoverageWatermark::LicenseExpiredGrace),
        }),
        Ok(None) => Ok(RuntimeLicense {
            jwt: String::new(),
            watermark: None,
        }),
        Err(message) if is_single_local_capture(path) => {
            eprintln!(
                "warning: {message}\n  A single local capture needs no license, so the analysis runs without it."
            );
            Ok(RuntimeLicense {
                jwt: String::new(),
                watermark: None,
            })
        }
        Err(message) => Err(message),
    }
}

/// Load and verify the stored license. `Ok(None)` means that no license
/// exists. `Err` holds the message for a license that exists but cannot be
/// used for runtime coverage.
fn load_license() -> Result<Option<(String, LicenseStatus)>, String> {
    let Some(jwt) = load_raw_jwt().map_err(|err| format!("license: {err}"))? else {
        return Ok(None);
    };
    let key = verifying_key()?;
    let status =
        load_and_verify(&key, DEFAULT_HARD_FAIL_DAYS).map_err(|err| format!("license: {err}"))?;
    validate_license_status(&status)?;
    Ok(Some((jwt, status)))
}

fn validate_license_status(status: &LicenseStatus) -> Result<(), String> {
    match status {
        LicenseStatus::Missing => Err(
            "Continuous runtime monitoring requires a valid license or trial. Run: fallow license activate --trial --email you@company.com".to_owned(),
        ),
        LicenseStatus::HardFail {
            days_since_expiry, ..
        } => Err(format!(
            "license expired {days_since_expiry} days ago. Refresh with: fallow license refresh"
        )),
        _ if !status.permits(&Feature::RuntimeCoverage) => Err(
            "License is valid but does not include continuous runtime monitoring. Upgrade at fallow.tools/upgrade.".to_owned(),
        ),
        _ => Ok(()),
    }
}

/// True when the coverage input is one capture: one file, or a directory
/// with exactly one JSON capture. This is the shape that needs no license.
fn is_single_local_capture(path: &Path) -> bool {
    if !path.is_dir() {
        return true;
    }
    let Ok(entries) = fs::read_dir(path) else {
        return false;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|entry| entry.is_file() && entry.extension() == Some(OsStr::new("json")))
        .take(2)
        .count()
        == 1
}

#[cfg(test)]
mod tests {
    use super::is_single_local_capture;

    #[test]
    fn single_capture_shapes() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let file = dir.path().join("capture.json");
        std::fs::write(&file, b"{}").expect("write capture");
        assert!(is_single_local_capture(&file));
        assert!(is_single_local_capture(dir.path()));
        std::fs::write(dir.path().join("second.json"), b"{}").expect("write capture");
        assert!(!is_single_local_capture(dir.path()));
    }
}
