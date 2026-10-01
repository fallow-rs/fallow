//! The corepack `packageManager` field of the root `package.json`, and the
//! root `package.json` override sources that the declared pnpm version reads.
//!
//! The dependency-override and catalog analyzers share these rules so that
//! both read the same sources for the same project.

/// Package managers the corepack `packageManager` field can name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PackageManagerKind {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

/// The package manager and major version that `packageManager` declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclaredPackageManager {
    pub kind: PackageManagerKind,
    /// The numeric major version, or `None` when the version has no numeric
    /// major (for example `pnpm@latest`).
    pub major: Option<u64>,
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
        let major_end = version
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(version.len());
        let major = version[..major_end].parse().ok();
        Some(Self { kind, major })
    }

    /// The major version when the package manager is pnpm.
    pub fn pnpm_major(self) -> Option<u64> {
        match self.kind {
            PackageManagerKind::Pnpm => self.major,
            _ => None,
        }
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

/// Whether pnpm ignores the `overrides` section of `pnpm-workspace.yaml` for
/// this root `package.json`.
///
/// pnpm 10 and earlier merge `resolutions` and `pnpm.overrides` into one map.
/// When that map has at least one key, it replaces the `pnpm-workspace.yaml`
/// overrides as a whole, without a warning. An empty map does not replace
/// them. pnpm 11 and later do not read these `package.json` sources. When the
/// version is unknown, both sources stay active.
pub fn pnpm_ignores_workspace_overrides(manifest: &serde_json::Value) -> bool {
    let major = DeclaredPackageManager::from_manifest(manifest)
        .and_then(DeclaredPackageManager::pnpm_major);
    if !matches!(major, Some(major) if major <= LAST_PNPM_MAJOR_WITH_PACKAGE_JSON_OVERRIDES) {
        return false;
    }
    let has_keys = |value: Option<&serde_json::Value>| {
        value
            .and_then(serde_json::Value::as_object)
            .is_some_and(|map| !map.is_empty())
    };
    has_keys(manifest.get("resolutions"))
        || has_keys(manifest.get("pnpm").and_then(|pnpm| pnpm.get("overrides")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declared(field: &str) -> Option<DeclaredPackageManager> {
        let manifest = serde_json::json!({ "packageManager": field });
        DeclaredPackageManager::from_manifest(&manifest)
    }

    #[test]
    fn reads_kind_and_major() {
        let pm = |kind, major| Some(DeclaredPackageManager { kind, major });
        assert_eq!(declared("bun@1.3.2"), pm(PackageManagerKind::Bun, Some(1)));
        assert_eq!(
            declared("npm@10.9.0"),
            pm(PackageManagerKind::Npm, Some(10))
        );
        assert_eq!(
            declared("yarn@4.5.0"),
            pm(PackageManagerKind::Yarn, Some(4))
        );
        assert_eq!(
            declared("pnpm@11.28.3+sha512.0123abcd"),
            pm(PackageManagerKind::Pnpm, Some(11))
        );
        assert_eq!(declared("pnpm@latest"), pm(PackageManagerKind::Pnpm, None));
        assert_eq!(declared("pnpm"), pm(PackageManagerKind::Pnpm, None));
        assert_eq!(declared("deno@2.0.0"), None);
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
        let ignores = |manifest: serde_json::Value| pnpm_ignores_workspace_overrides(&manifest);
        let pnpm10 = "pnpm@10.34.5";
        assert!(ignores(serde_json::json!({
            "packageManager": pnpm10, "pnpm": { "overrides": { "a": "1.0.0" } }
        })));
        assert!(ignores(serde_json::json!({
            "packageManager": pnpm10, "resolutions": { "a": "1.0.0" }
        })));
        assert!(ignores(serde_json::json!({
            "packageManager": "pnpm@9.15.9", "pnpm": { "overrides": { "a": "1.0.0" } }
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
}
