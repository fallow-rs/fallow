//! Detached Ed25519 signatures of release artifacts.
//!
//! The release workflow signs each binary and each type-aware bundle with one
//! key. Fallow holds the public half and checks a companion with it before the
//! companion is trusted.

use ed25519_dalek::{Signature, VerifyingKey};

/// Ed25519 public key of the release signatures. It must equal
/// `EMBEDDED_PUBLIC_KEY` in `npm/fallow/scripts/verify-binary.js`;
/// `scripts/signing-key-parity.mjs` checks this.
pub const RELEASE_BINARY_SIGNING_KEY: [u8; 32] = [
    131, 78, 111, 215, 115, 51, 230, 238, 223, 119, 147, 71, 199, 16, 172, 180, 3, 210, 216, 35,
    77, 85, 159, 94, 215, 200, 126, 85, 42, 222, 11, 209,
];

/// Length of a detached Ed25519 signature.
pub const SIGNATURE_BYTES: usize = 64;

/// Check a detached signature of `data` against `key`.
///
/// # Errors
///
/// Returns a message when the key is invalid, the signature has the wrong
/// length, or the signature does not match `data`.
pub fn verify_detached(data: &[u8], signature: &[u8], key: &[u8; 32]) -> Result<(), String> {
    let signature: [u8; SIGNATURE_BYTES] = signature.try_into().map_err(|_| {
        format!(
            "the signature has {} bytes, expected {SIGNATURE_BYTES}",
            signature.len()
        )
    })?;
    let key = VerifyingKey::from_bytes(key)
        .map_err(|error| format!("the compiled-in release key is invalid: {error}"))?;
    key.verify_strict(data, &Signature::from_bytes(&signature))
        .map_err(|_| "the signature does not match the Fallow release key".to_owned())
}
