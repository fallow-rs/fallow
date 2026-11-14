//! Provider binary that the `fallow` npm launcher passes in.
//!
//! The npm platform packages of `fallow` do not ship the similar-code
//! companion. A project installs the exact-version `fallow-similar-code`
//! package, and the launcher passes the path of its native binary. Anything
//! that starts `fallow` can set the variable, so the path is accepted only
//! when its detached Ed25519 signature verifies against the release key. The
//! check rejects a stale, damaged or unsigned path. It is not a defense
//! against a process that controls the environment or the files of Fallow:
//! such a process can swap the file after the check, or run other code.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use ed25519_dalek::{Signature, VerifyingKey};

use super::protocol::SIDECAR_BINARY;

/// Environment variable through which the `fallow` npm launcher passes the
/// companion binary.
pub(super) const PROVIDER_ENV: &str = "FALLOW_SIMILAR_CODE_PROVIDER";

/// Ed25519 public key of the release binary signatures. It must equal
/// `EMBEDDED_PUBLIC_KEY` in `npm/fallow/scripts/verify-binary.js`;
/// `scripts/signing-key-parity.mjs` checks this.
const RELEASE_BINARY_SIGNING_KEY: [u8; 32] = [
    131, 78, 111, 215, 115, 51, 230, 238, 223, 119, 147, 71, 199, 16, 172, 180, 3, 210, 216, 35,
    77, 85, 159, 94, 215, 200, 126, 85, 42, 222, 11, 209,
];

const SIGNATURE_SUFFIX: &str = ".sig";
const SIGNATURE_BYTES: usize = 64;
const MAX_PROVIDER_BYTES: u64 = 256 * 1024 * 1024;

/// The verified provider from [`PROVIDER_ENV`], or `None` when it is unset.
pub(super) fn provider_from_env() -> Option<Result<PathBuf, String>> {
    let value = std::env::var_os(PROVIDER_ENV)?;
    Some(
        verified_provider(Path::new(&value), &RELEASE_BINARY_SIGNING_KEY)
            .map_err(|error| {
                format!(
                    "{PROVIDER_ENV} was rejected: {error}. Reinstall `fallow-similar-code` at the same version as Fallow, or unset {PROVIDER_ENV}."
                )
            }),
    )
}

#[expect(
    clippy::filetype_is_file,
    reason = "the provider must be a regular file; symlinks and special files are rejected"
)]
pub(super) fn verified_provider(path: &Path, key: &[u8; 32]) -> Result<PathBuf, String> {
    if !path.is_absolute() {
        return Err(format!("{} is not an absolute path", path.display()));
    }
    let name = path.file_name().and_then(|name| name.to_str());
    if !name.is_some_and(is_provider_name) {
        return Err(format!(
            "{} is not a `{SIDECAR_BINARY}` executable",
            path.display()
        ));
    }
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    if !metadata.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if metadata.len() > MAX_PROVIDER_BYTES {
        return Err(format!("{} is larger than expected", path.display()));
    }
    let binary =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let signature = read_signature(&signature_path(path))?;
    let key = VerifyingKey::from_bytes(key)
        .map_err(|error| format!("the compiled-in release key is invalid: {error}"))?;
    key.verify_strict(&binary, &signature).map_err(|_| {
        format!(
            "the signature of {} does not match the Fallow release key",
            path.display()
        )
    })?;
    dunce::canonicalize(path).map_err(|error| format!("cannot resolve {}: {error}", path.display()))
}

// Only the exact executable name of this platform. On Windows, the standard
// library starts `<path>.exe` for a path without an extension, so an
// extensionless name would let Fallow check one file and run another.
fn is_provider_name(name: &str) -> bool {
    name.strip_suffix(std::env::consts::EXE_SUFFIX) == Some(SIDECAR_BINARY)
}

fn signature_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(SIGNATURE_SUFFIX);
    PathBuf::from(name)
}

#[expect(
    clippy::filetype_is_file,
    reason = "the signature must be a regular file; symlinks and special files are rejected"
)]
fn read_signature(path: &Path) -> Result<Signature, String> {
    let metadata = path
        .symlink_metadata()
        .map_err(|error| format!("missing signature {}: {error}", path.display()))?;
    if !metadata.file_type().is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if metadata.len() != SIGNATURE_BYTES as u64 {
        return Err(format!(
            "{} has {} bytes, expected {SIGNATURE_BYTES}",
            path.display(),
            metadata.len()
        ));
    }
    let bytes =
        std::fs::read(path).map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    let bytes: [u8; SIGNATURE_BYTES] = bytes.as_slice().try_into().map_err(|_| {
        format!(
            "{} has {} bytes, expected {SIGNATURE_BYTES}",
            path.display(),
            bytes.len()
        )
    })?;
    Ok(Signature::from_bytes(&bytes))
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "test fixture construction must fail immediately"
)]
mod tests {
    use ed25519_dalek::{Signer, SigningKey};

    use super::*;

    const TEST_SEED: [u8; 32] = [7; 32];

    fn provider_name() -> String {
        format!("{SIDECAR_BINARY}{}", std::env::consts::EXE_SUFFIX)
    }

    fn signed_provider(dir: &Path, signer: &SigningKey) -> PathBuf {
        let path = dir.join(provider_name());
        let binary = b"companion binary bytes";
        std::fs::write(&path, binary).unwrap();
        std::fs::write(signature_path(&path), signer.sign(binary).to_bytes()).unwrap();
        path
    }

    fn test_key() -> (SigningKey, [u8; 32]) {
        let signer = SigningKey::from_bytes(&TEST_SEED);
        let key = signer.verifying_key().to_bytes();
        (signer, key)
    }

    #[test]
    fn accepts_a_signed_regular_provider() {
        let dir = tempfile::tempdir().unwrap();
        let (signer, key) = test_key();
        let path = signed_provider(dir.path(), &signer);
        assert_eq!(
            verified_provider(&path, &key).unwrap(),
            dunce::canonicalize(&path).unwrap()
        );
    }

    #[test]
    fn rejects_a_provider_signed_by_another_key() {
        let dir = tempfile::tempdir().unwrap();
        let (_, key) = test_key();
        let path = signed_provider(dir.path(), &SigningKey::from_bytes(&[9; 32]));
        let error = verified_provider(&path, &key).unwrap_err();
        assert!(
            error.contains("does not match the Fallow release key"),
            "{error}"
        );
    }

    #[test]
    fn rejects_a_changed_binary() {
        let dir = tempfile::tempdir().unwrap();
        let (signer, key) = test_key();
        let path = signed_provider(dir.path(), &signer);
        std::fs::write(&path, b"changed bytes").unwrap();
        assert!(verified_provider(&path, &key).is_err());
    }

    #[test]
    fn rejects_a_missing_or_short_signature() {
        let dir = tempfile::tempdir().unwrap();
        let (signer, key) = test_key();
        let path = signed_provider(dir.path(), &signer);
        std::fs::write(signature_path(&path), [0; 10]).unwrap();
        assert!(
            verified_provider(&path, &key)
                .unwrap_err()
                .contains("expected 64")
        );
        std::fs::remove_file(signature_path(&path)).unwrap();
        assert!(
            verified_provider(&path, &key)
                .unwrap_err()
                .contains("missing signature")
        );
    }

    #[test]
    fn rejects_a_relative_path_and_another_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let (signer, key) = test_key();
        let path = signed_provider(dir.path(), &signer);
        let name = provider_name();
        let relative = Path::new(&name);
        assert!(
            verified_provider(relative, &key)
                .unwrap_err()
                .contains("absolute")
        );
        let other = dir.path().join("node");
        std::fs::copy(&path, &other).unwrap();
        std::fs::copy(signature_path(&path), signature_path(&other)).unwrap();
        assert!(verified_provider(&other, &key).is_err());
    }

    #[test]
    fn accepts_only_the_executable_name_of_this_platform() {
        let exact = format!("{SIDECAR_BINARY}{}", std::env::consts::EXE_SUFFIX);
        assert!(is_provider_name(&exact));
        let other = if std::env::consts::EXE_SUFFIX.is_empty() {
            format!("{SIDECAR_BINARY}.exe")
        } else {
            SIDECAR_BINARY.to_owned()
        };
        assert!(!is_provider_name(&other), "{other}");
    }

    #[test]
    fn rejects_an_oversized_signature_before_it_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let (signer, key) = test_key();
        let path = signed_provider(dir.path(), &signer);
        let file = std::fs::File::create(signature_path(&path)).unwrap();
        file.set_len(SIGNATURE_BYTES as u64 + 1).unwrap();
        assert!(
            verified_provider(&path, &key)
                .unwrap_err()
                .contains("expected 64")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlinked_signature() {
        let dir = tempfile::tempdir().unwrap();
        let (signer, key) = test_key();
        let path = signed_provider(dir.path(), &signer);
        let real = dir.path().join("real.sig");
        std::fs::rename(signature_path(&path), &real).unwrap();
        std::os::unix::fs::symlink(&real, signature_path(&path)).unwrap();
        assert!(
            verified_provider(&path, &key)
                .unwrap_err()
                .contains("regular file")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_a_symlinked_provider() {
        let dir = tempfile::tempdir().unwrap();
        let (signer, key) = test_key();
        let real = tempfile::tempdir().unwrap();
        let target = signed_provider(real.path(), &signer);
        let link = dir.path().join(provider_name());
        std::os::unix::fs::symlink(&target, &link).unwrap();
        std::fs::copy(signature_path(&target), signature_path(&link)).unwrap();
        assert!(
            verified_provider(&link, &key)
                .unwrap_err()
                .contains("regular file")
        );
    }
}
