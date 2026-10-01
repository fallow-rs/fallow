//! The corepack `packageManager` field of the root `package.json`, and the
//! root `package.json` override sources that the declared pnpm version reads.
//!
//! The dependency-override and catalog analyzers share these rules so that
//! both read the same sources for the same project.

use fallow_types::workspace::PnpmWorkspaceOverridesIgnoredCause;

/// Package managers the corepack `packageManager` field can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageManagerKind {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

/// The version that `packageManager` declares, for example
/// `10.5.1-rc.0+sha512.abc`. The build part after `+` is not kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclaredVersion {
    pub major: u64,
    /// `None` when the version stops after the major (for example `pnpm@10`).
    pub minor: Option<u64>,
    /// `None` when the version stops after the minor.
    pub patch: Option<u64>,
    /// `true` for a prerelease version such as `10.0.0-rc.3`.
    pub prerelease: bool,
}

impl DeclaredVersion {
    /// Parse the leading `major[.minor[.patch]][-prerelease]` part. Returns
    /// `None` when the version has no numeric major (for example `latest`).
    fn parse(version: &str) -> Option<Self> {
        let leading_number = |text: &str| -> (Option<u64>, usize) {
            let end = text
                .find(|c: char| !c.is_ascii_digit())
                .unwrap_or(text.len());
            (text[..end].parse().ok(), end)
        };
        let (major, major_end) = leading_number(version);
        let major = major?;
        let mut rest = &version[major_end..];
        let next_part = |rest: &mut &str| -> Option<u64> {
            let after_dot = rest.strip_prefix('.')?;
            let (number, end) = leading_number(after_dot);
            *rest = &after_dot[end..];
            number
        };
        let minor = next_part(&mut rest);
        let patch = minor.and_then(|_| next_part(&mut rest));
        let prerelease = patch.is_some() && rest.starts_with('-');
        Some(Self {
            major,
            minor,
            patch,
            prerelease,
        })
    }

    /// Whether this version comes before `major.minor.patch` in semver
    /// order. A prerelease comes before its release. Returns `None` when the
    /// version has no minor or patch that the comparison needs.
    fn is_before(self, major: u64, minor: u64, patch: u64) -> Option<bool> {
        if self.major != major {
            return Some(self.major < major);
        }
        let own_minor = self.minor?;
        if own_minor != minor {
            return Some(own_minor < minor);
        }
        let own_patch = self.patch?;
        if own_patch != patch {
            return Some(own_patch < patch);
        }
        Some(self.prerelease)
    }
}

/// The package manager and version that `packageManager` declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclaredPackageManager {
    pub kind: PackageManagerKind,
    /// The version, or `None` when the version has no numeric major (for
    /// example `pnpm@latest`).
    pub version: Option<DeclaredVersion>,
}

impl DeclaredPackageManager {
    /// Read the `packageManager` field (for example
    /// `"pnpm@10.34.5+sha512.abc"`) from the parsed root `package.json`.
    /// Returns `None` for a missing field or an unknown package manager.
    pub fn from_manifest(manifest: &serde_json::Value) -> Option<Self> {
        let field = manifest.get("packageManager")?.as_str()?.trim();
        let (name, version) = field.split_once('@').unwrap_or((field, ""));
        let kind = match name {
            "npm" => PackageManagerKind::Npm,
            "pnpm" => PackageManagerKind::Pnpm,
            "yarn" => PackageManagerKind::Yarn,
            "bun" => PackageManagerKind::Bun,
            _ => return None,
        };
        Some(Self {
            kind,
            version: DeclaredVersion::parse(version),
        })
    }

    /// The version when the package manager is pnpm.
    pub fn pnpm_version(self) -> Option<DeclaredVersion> {
        match self.kind {
            PackageManagerKind::Pnpm => self.version,
            _ => None,
        }
    }

    /// The major version when the package manager is pnpm.
    pub fn pnpm_major(self) -> Option<u64> {
        self.pnpm_version().map(|version| version.major)
    }
}

/// Major version from a `packageManager` field that names pnpm, read from the
/// root `package.json` source. Returns `None` for another package manager, a
/// missing field, invalid JSON, or a version without a numeric major. The
/// lockfile is no fallback: pnpm 10 and pnpm 11 write the same
/// `lockfileVersion: '9.0'` document.
pub fn declared_pnpm_major(package_json_source: &str) -> Option<u64> {
    let manifest: serde_json::Value = serde_json::from_str(package_json_source).ok()?;
    DeclaredPackageManager::from_manifest(&manifest)?.pnpm_major()
}

/// The last pnpm major version that reads overrides from `package.json`.
const LAST_PNPM_MAJOR_WITH_PACKAGE_JSON_OVERRIDES: u64 = 10;

/// The root `package.json` override sources that the installed pnpm reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackageJsonOverrideReads {
    /// `pnpm.overrides`.
    pub pnpm_overrides: bool,
    /// The top-level `resolutions` object.
    pub resolutions: bool,
}

impl PackageJsonOverrideReads {
    /// pnpm 10 and earlier merge `resolutions` and `pnpm.overrides`; pnpm 11
    /// stopped reading the `pnpm` field and `resolutions`. When the version is
    /// unknown, keep the `pnpm.overrides` behavior that predates the version
    /// check and do not add `resolutions`: findings have one severity per
    /// rule, so an unknown version cannot get a softer severity.
    pub const fn for_pnpm_major(major: Option<u64>) -> Self {
        match major {
            Some(major) if major <= LAST_PNPM_MAJOR_WITH_PACKAGE_JSON_OVERRIDES => Self {
                pnpm_overrides: true,
                resolutions: true,
            },
            Some(_) => Self {
                pnpm_overrides: false,
                resolutions: false,
            },
            None => Self {
                pnpm_overrides: true,
                resolutions: false,
            },
        }
    }
}

/// The first pnpm version that reads the `overrides` section of
/// `pnpm-workspace.yaml`. Its release notes say: "Specifying `overrides` in
/// `pnpm-workspace.yaml` should work." pnpm 9 and pnpm 10.0.0 to 10.5.0 do
/// not read the section.
const FIRST_PNPM_WITH_WORKSPACE_OVERRIDES: (u64, u64, u64) = (10, 5, 1);

/// Why pnpm ignores the `overrides` section of `pnpm-workspace.yaml` for this
/// root `package.json`, or `None` when pnpm reads it.
///
/// - pnpm 10 and earlier merge `resolutions` and `pnpm.overrides` into one
///   map. When that map has at least one key, it replaces the
///   `pnpm-workspace.yaml` overrides as a whole, without a warning. An empty
///   map does not replace them. pnpm 11 and later do not read these
///   `package.json` sources.
/// - pnpm before 10.5.1 does not read the section at all.
///
/// The version cause wins when both apply: before 10.5.1, moving the entries
/// into `pnpm-workspace.yaml` does not help, so the version message gives the
/// correct remedy. When the version is unknown, or
/// has no minor or patch for a pnpm 10 version, pnpm reads the section.
pub fn pnpm_workspace_overrides_ignored(
    manifest: &serde_json::Value,
) -> Option<PnpmWorkspaceOverridesIgnoredCause> {
    let version = DeclaredPackageManager::from_manifest(manifest)
        .and_then(DeclaredPackageManager::pnpm_version)?;
    let has_keys = |value: Option<&serde_json::Value>| {
        value
            .and_then(serde_json::Value::as_object)
            .is_some_and(|map| !map.is_empty())
    };
    let (major, minor, patch) = FIRST_PNPM_WITH_WORKSPACE_OVERRIDES;
    if version.is_before(major, minor, patch).unwrap_or(false) {
        return Some(PnpmWorkspaceOverridesIgnoredCause::PnpmVersion);
    }
    (version.major <= LAST_PNPM_MAJOR_WITH_PACKAGE_JSON_OVERRIDES
        && (has_keys(manifest.get("resolutions"))
            || has_keys(manifest.get("pnpm").and_then(|pnpm| pnpm.get("overrides")))))
    .then_some(PnpmWorkspaceOverridesIgnoredCause::PackageJsonOverrides)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(field: &str) -> Option<DeclaredPackageManager> {
        let manifest = serde_json::json!({ "packageManager": field });
        DeclaredPackageManager::from_manifest(&manifest)
    }

    fn version(field: &str) -> Option<DeclaredVersion> {
        declared(field).and_then(|pm| pm.version)
    }

    fn major(field: &str) -> Option<u64> {
        version(field).map(|version| version.major)
    }

    #[test]
    fn reads_kind_and_major() {
        assert_eq!(
            declared("bun@1.3.2").map(|pm| pm.kind),
            Some(PackageManagerKind::Bun)
        );
        assert_eq!(
            declared("npm@10.9.0").map(|pm| pm.kind),
            Some(PackageManagerKind::Npm)
        );
        assert_eq!(
            declared("yarn@4.5.0").map(|pm| pm.kind),
            Some(PackageManagerKind::Yarn)
        );
        assert_eq!(
            declared("pnpm@latest").map(|pm| pm.kind),
            Some(PackageManagerKind::Pnpm)
        );
        assert_eq!(major("bun@1.3.2"), Some(1));
        assert_eq!(major("npm@10.9.0"), Some(10));
        assert_eq!(major("yarn@4.5.0"), Some(4));
        assert_eq!(major("pnpm@11.28.3+sha512.0123abcd"), Some(11));
        assert_eq!(major("pnpm@latest"), None);
        assert_eq!(major("pnpm"), None);
        assert_eq!(declared("deno@2.0.0"), None);
    }

    #[test]
    fn reads_minor_patch_and_prerelease() {
        let full = |major, minor, patch, prerelease| {
            Some(DeclaredVersion {
                major,
                minor: Some(minor),
                patch: Some(patch),
                prerelease,
            })
        };
        assert_eq!(version("pnpm@10.5.1"), full(10, 5, 1, false));
        assert_eq!(
            version("pnpm@10.5.0+sha512.0123abcd"),
            full(10, 5, 0, false)
        );
        assert_eq!(version("pnpm@10.0.0-rc.3"), full(10, 0, 0, true));
        assert_eq!(
            version("pnpm@10.0.0-rc.3+sha512.0123abcd"),
            full(10, 0, 0, true)
        );
        let partial = |minor| {
            Some(DeclaredVersion {
                major: 10,
                minor,
                patch: None,
                prerelease: false,
            })
        };
        assert_eq!(version("pnpm@10"), partial(None));
        assert_eq!(version("pnpm@10.5"), partial(Some(5)));
        assert_eq!(version("pnpm@10.5-rc.1"), partial(Some(5)));
    }

    #[test]
    fn compares_versions_in_semver_order() {
        let before = |field: &str| version(field).and_then(|v| v.is_before(10, 5, 1));
        assert_eq!(before("pnpm@9.15.9"), Some(true));
        assert_eq!(before("pnpm@9"), Some(true));
        assert_eq!(before("pnpm@10.5.0"), Some(true));
        assert_eq!(before("pnpm@10.0.0-rc.3"), Some(true));
        assert_eq!(before("pnpm@10.5.1-rc.0"), Some(true));
        assert_eq!(before("pnpm@10.5.1"), Some(false));
        assert_eq!(before("pnpm@10.6.0-rc.1"), Some(false));
        assert_eq!(before("pnpm@11"), Some(false));
        assert_eq!(before("pnpm@10"), None);
        assert_eq!(before("pnpm@10.5"), None);
    }

    #[test]
    fn pnpm_major_is_none_for_other_package_managers() {
        assert_eq!(
            declared("pnpm@9.15.0").and_then(DeclaredPackageManager::pnpm_major),
            Some(9)
        );
        assert_eq!(
            declared("yarn@4.5.0").and_then(DeclaredPackageManager::pnpm_major),
            None
        );
    }

    #[test]
    fn override_reads_follow_the_pnpm_major() {
        let reads = PackageJsonOverrideReads::for_pnpm_major;
        assert!(reads(Some(10)).pnpm_overrides && reads(Some(10)).resolutions);
        assert!(!reads(Some(11)).pnpm_overrides && !reads(Some(11)).resolutions);
        assert!(reads(None).pnpm_overrides && !reads(None).resolutions);
    }

    #[test]
    fn pnpm_10_ignores_workspace_overrides_only_for_non_empty_package_json_overrides() {
        let ignores = |manifest: serde_json::Value| {
            pnpm_workspace_overrides_ignored(&manifest)
                == Some(PnpmWorkspaceOverridesIgnoredCause::PackageJsonOverrides)
        };
        let pnpm10 = "pnpm@10.34.5";
        assert!(ignores(serde_json::json!({
            "packageManager": pnpm10, "pnpm": { "overrides": { "a": "1.0.0" } }
        })));
        assert!(ignores(serde_json::json!({
            "packageManager": pnpm10, "resolutions": { "a": "1.0.0" }
        })));
        assert!(!ignores(serde_json::json!({
            "packageManager": pnpm10, "pnpm": { "overrides": {} }, "resolutions": {}
        })));
        assert!(!ignores(serde_json::json!({
            "packageManager": pnpm10, "pnpm": { "onlyBuiltDependencies": [] }
        })));
        assert!(!ignores(serde_json::json!({
            "packageManager": "pnpm@11.25.0", "pnpm": { "overrides": { "a": "1.0.0" } }
        })));
        assert!(!ignores(serde_json::json!({
            "pnpm": { "overrides": { "a": "1.0.0" } }
        })));
        assert!(!ignores(serde_json::json!({
            "packageManager": "yarn@4.5.0", "resolutions": { "a": "1.0.0" }
        })));
    }

    #[test]
    fn pnpm_before_10_5_1_ignores_workspace_overrides() {
        let cause = |field: &str| {
            pnpm_workspace_overrides_ignored(&serde_json::json!({ "packageManager": field }))
        };
        let version_cause = Some(PnpmWorkspaceOverridesIgnoredCause::PnpmVersion);
        assert_eq!(cause("pnpm@9.15.9"), version_cause);
        let pnpm9_with_overrides = serde_json::json!({
            "packageManager": "pnpm@9.15.9", "pnpm": { "overrides": { "a": "1.0.0" } }
        });
        assert_eq!(
            pnpm_workspace_overrides_ignored(&pnpm9_with_overrides),
            version_cause
        );
        assert_eq!(cause("pnpm@10.5.0"), version_cause);
        assert_eq!(cause("pnpm@10.0.0-rc.3"), version_cause);
        assert_eq!(cause("pnpm@10.5.0+sha512.0123abcd"), version_cause);
        assert_eq!(cause("pnpm@10.5.1"), None);
        assert_eq!(cause("pnpm@10.6.0"), None);
        assert_eq!(cause("pnpm@11.25.0"), None);
        assert_eq!(cause("pnpm@10"), None);
        assert_eq!(cause("pnpm@latest"), None);
        assert_eq!(cause("yarn@1.22.22"), None);
    }
}
