//! The user directory that `fallow type-aware setup` installs into.
//!
//! Each install lives in `<store>/<fallow version>/`. The directory holds the
//! unpacked, signature-checked bundle and its `bundle.json` manifest. Discovery
//! checks the store after `FALLOW_TYPE_AWARE_BIN` and the sibling of the
//! active executable. It prefers the install of the running Fallow version.
//! Otherwise it takes the newest install that speaks the same wire protocol, so
//! a Fallow upgrade keeps a working companion until the protocol changes.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::manifest::{SIDECAR_PACKAGE, WIRE_PROTOCOL_VERSION, version_core};
use crate::release_signature::{RELEASE_BINARY_SIGNING_KEY, verify_detached};

/// Discovery source label of a companion from the setup directory.
pub const SETUP_DIRECTORY_SOURCE: &str = "setup-directory";

/// File name of the bundle manifest inside an install directory.
pub const BUNDLE_MANIFEST: &str = "bundle.json";

/// Schema version of [`BundleManifest`].
pub const BUNDLE_MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Manifest that the release packaging script writes into each bundle.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BundleManifest {
    /// Manifest schema version, [`BUNDLE_MANIFEST_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Fallow release that built the bundle.
    pub fallow_version: String,
    /// Wire protocol version that the companion speaks.
    pub wire_protocol_version: u32,
    /// Node `process.platform` that the bundle is for.
    pub platform: String,
    /// Node `process.arch` that the bundle is for.
    pub arch: String,
    /// Version of the bundled `typescript` package.
    pub typescript_version: String,
}

/// A Node platform that has a release bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BundlePlatform {
    /// Node `process.platform`.
    pub platform: &'static str,
    /// Node `process.arch`.
    pub arch: &'static str,
}

impl BundlePlatform {
    /// Release asset name of the bundle for this platform.
    #[must_use]
    pub fn asset_name(self) -> String {
        format!("{SIDECAR_PACKAGE}-{}-{}.tar.gz", self.platform, self.arch)
    }
}

/// Node platforms that the release job builds a bundle for. The TypeScript
/// platform package follows Node, not Fallow: an x64 Node on an arm64 Mac
/// needs the x64 bundle.
pub const BUNDLE_PLATFORMS: &[BundlePlatform] = &[
    BundlePlatform {
        platform: "darwin",
        arch: "arm64",
    },
    BundlePlatform {
        platform: "darwin",
        arch: "x64",
    },
    BundlePlatform {
        platform: "linux",
        arch: "arm64",
    },
    BundlePlatform {
        platform: "linux",
        arch: "x64",
    },
];

/// Select the bundle for the platform and architecture that Node reports.
///
/// # Errors
///
/// Returns a message when no release bundle exists for that Node platform.
pub fn bundle_platform(platform: &str, arch: &str) -> Result<BundlePlatform, String> {
    BUNDLE_PLATFORMS
        .iter()
        .copied()
        .find(|candidate| candidate.platform == platform && candidate.arch == arch)
        .ok_or_else(|| {
            let supported = BUNDLE_PLATFORMS
                .iter()
                .map(|candidate| format!("{}-{}", candidate.platform, candidate.arch))
                .collect::<Vec<_>>()
                .join(", ");
            if platform == "win32" {
                return format!(
                    "Node reports {platform}-{arch}. `fallow type-aware setup` has no bundle for Windows. Install Fallow and the companion from npm: `npm install --global fallow@{version} {SIDECAR_PACKAGE}@{version}`.",
                    version = env!("CARGO_PKG_VERSION")
                );
            }
            format!(
                "Node reports {platform}-{arch}, but release bundles exist only for {supported}."
            )
        })
}

/// The store directory of this user, or `None` when the environment has no
/// home or data directory.
#[must_use]
pub fn store_root() -> Option<PathBuf> {
    store_root_from(&|key| std::env::var_os(key).map(PathBuf::from))
}

fn store_root_from(env: &dyn Fn(&str) -> Option<PathBuf>) -> Option<PathBuf> {
    let absolute = |key: &str| env(key).filter(|path| path.is_absolute());
    let base = if cfg!(windows) {
        absolute("LOCALAPPDATA")
    } else if cfg!(target_os = "macos") {
        absolute("HOME").map(|home| home.join("Library").join("Application Support"))
    } else {
        absolute("XDG_DATA_HOME")
            .or_else(|| absolute("HOME").map(|home| home.join(".local").join("share")))
    }?;
    Some(base.join("fallow").join(SIDECAR_PACKAGE))
}

/// The install directory of one Fallow version inside the store.
#[must_use]
pub fn version_dir(store: &Path, version: &str) -> PathBuf {
    store.join(version)
}

/// One install in the store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledBundle {
    /// The install directory.
    pub dir: PathBuf,
    /// Its manifest.
    pub manifest: BundleManifest,
}

/// Read a bundle manifest.
///
/// # Errors
///
/// Returns a message when the file is absent, too large, or not a valid
/// manifest.
#[expect(
    clippy::filetype_is_file,
    reason = "the manifest must be a regular file; links and special files are rejected"
)]
pub fn read_manifest(path: &Path) -> Result<BundleManifest, String> {
    const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_MANIFEST_BYTES {
        return Err(format!("{} is not a valid bundle manifest", path.display()));
    }
    let bytes =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let manifest: BundleManifest = serde_json::from_slice(&bytes)
        .map_err(|error| format!("{} is not a valid bundle manifest: {error}", path.display()))?;
    if manifest.schema_version != BUNDLE_MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "{} has manifest schema {}, expected {BUNDLE_MANIFEST_SCHEMA_VERSION}",
            path.display(),
            manifest.schema_version
        ));
    }
    Ok(manifest)
}

/// Every install in the store whose directory name equals the Fallow version
/// in its manifest. Other entries (temporary directories of an interrupted
/// setup, files) are skipped.
#[must_use]
pub fn installed_bundles(store: &Path) -> Vec<InstalledBundle> {
    let Ok(entries) = std::fs::read_dir(store) else {
        return Vec::new();
    };
    let mut bundles = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let dir = entry.path();
            let name = entry.file_name().into_string().ok()?;
            let manifest = read_manifest(&dir.join(BUNDLE_MANIFEST)).ok()?;
            (manifest.fallow_version == name).then_some(InstalledBundle { dir, manifest })
        })
        .collect::<Vec<_>>();
    bundles.sort_by(|left, right| left.dir.cmp(&right.dir));
    bundles
}

/// The install that discovery uses: the one of `current_version` when it
/// speaks `protocol`, otherwise the newest install that speaks `protocol`.
/// Versions compare by semantic version precedence, so the result does not
/// depend on the order of `bundles`.
#[must_use]
pub fn select_bundle<'a>(
    bundles: &'a [InstalledBundle],
    current_version: &str,
    protocol: u32,
) -> Option<&'a InstalledBundle> {
    let compatible = bundles
        .iter()
        .filter(|bundle| bundle.manifest.wire_protocol_version == protocol);
    if let Some(exact) = compatible
        .clone()
        .find(|bundle| bundle.manifest.fallow_version == current_version)
    {
        return Some(exact);
    }
    compatible
        .filter_map(|bundle| Some((Version::parse(&bundle.manifest.fallow_version)?, bundle)))
        .max_by(|(left, _), (right, _)| left.cmp(right))
        .map(|(_, bundle)| bundle)
}

/// A `major.minor.patch[-prerelease][+build]` version, ordered by semantic
/// version precedence. Build metadata does not count.
#[derive(Debug, PartialEq, Eq)]
struct Version<'a> {
    core: (u64, u64, u64),
    prerelease: Option<&'a str>,
}

impl<'a> Version<'a> {
    fn parse(text: &'a str) -> Option<Self> {
        let core = version_core(text)?;
        let without_build = text.split_once('+').map_or(text, |(version, _)| version);
        let prerelease = without_build.split_once('-').map(|(_, pre)| pre);
        Some(Self { core, prerelease })
    }
}

impl Ord for Version<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        self.core
            .cmp(&other.core)
            .then_with(|| match (self.prerelease, other.prerelease) {
                (None, None) => Ordering::Equal,
                (None, Some(_)) => Ordering::Greater,
                (Some(_), None) => Ordering::Less,
                (Some(left), Some(right)) => compare_prerelease(left, right),
            })
    }
}

impl PartialOrd for Version<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// Numeric identifiers compare as numbers and sort before alphanumeric ones;
/// a shorter list sorts first when every shared identifier is equal.
fn compare_prerelease(left: &str, right: &str) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let mut left_parts = left.split('.');
    let mut right_parts = right.split('.');
    loop {
        match (left_parts.next(), right_parts.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(a), Some(b)) => {
                let order = match (a.parse::<u64>(), b.parse::<u64>()) {
                    (Ok(a), Ok(b)) => a.cmp(&b),
                    (Ok(_), Err(_)) => Ordering::Less,
                    (Err(_), Ok(_)) => Ordering::Greater,
                    (Err(_), Err(_)) => a.cmp(b),
                };
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

/// The companion executable of the selected install, when one exists.
#[must_use]
pub fn find_setup_companion(store: &Path) -> Option<PathBuf> {
    let bundles = installed_bundles(store);
    let selected = select_bundle(&bundles, env!("CARGO_PKG_VERSION"), WIRE_PROTOCOL_VERSION)?;
    super::transport::find_sidecar_in(&selected.dir)
}

/// The companion of this user's setup directory, when one exists.
#[must_use]
pub fn find_user_setup_companion() -> Option<PathBuf> {
    find_setup_companion(&store_root()?)
}

/// What `fallow type-aware cleanup` keeps and removes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CleanupPlan {
    /// The install that discovery uses, which cleanup keeps.
    pub keep: Option<PathBuf>,
    /// Every other install.
    pub remove: Vec<PathBuf>,
}

/// Plan the removal of every install except the one that discovery uses.
#[must_use]
pub fn cleanup_plan(store: &Path) -> CleanupPlan {
    let bundles = installed_bundles(store);
    let keep = select_bundle(&bundles, env!("CARGO_PKG_VERSION"), WIRE_PROTOCOL_VERSION)
        .map(|bundle| bundle.dir.clone());
    let remove = bundles
        .into_iter()
        .map(|bundle| bundle.dir)
        .filter(|dir| Some(dir) != keep.as_ref())
        .collect();
    CleanupPlan { keep, remove }
}

/// Public key of the release signatures that this binary holds.
#[must_use]
pub const fn release_signing_key() -> [u8; 32] {
    RELEASE_BINARY_SIGNING_KEY
}

/// Wire protocol version that this Fallow build speaks.
#[must_use]
pub const fn wire_protocol_version() -> u32 {
    WIRE_PROTOCOL_VERSION
}

/// Check the detached Ed25519 signature of a bundle archive against the
/// release key that this binary holds.
///
/// # Errors
///
/// Returns a message when the signature has the wrong length or does not
/// match the archive.
pub fn verify_bundle_signature(archive: &[u8], signature: &[u8]) -> Result<(), String> {
    verify_bundle_signature_with_key(archive, signature, &RELEASE_BINARY_SIGNING_KEY)
}

/// [`verify_bundle_signature`] with an explicit public key. Tests and debug
/// builds use it with a test key; release builds use the release key only.
///
/// # Errors
///
/// Returns a message when the key is invalid, the signature has the wrong
/// length, or the signature does not match the archive.
pub fn verify_bundle_signature_with_key(
    archive: &[u8],
    signature: &[u8],
    key: &[u8; 32],
) -> Result<(), String> {
    verify_detached(archive, signature, key)
        .map_err(|error| format!("the type-aware bundle was rejected: {error}"))
}

#[cfg(test)]
#[path = "setup_store_tests.rs"]
pub(crate) mod tests;
