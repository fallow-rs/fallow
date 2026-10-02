//! Test-only helpers for signing stub sidecars and forging license JWTs.
//!
//! The keys here are derived from deterministic seeds baked into the
//! `test-sidecar-key` cargo feature; see the feature-gated pubkey consts in
//! `crates/cli/src/health/coverage.rs` and `crates/cli/src/license/mod.rs`.
//! The `compile_error!` in `coverage.rs` prevents the feature from shipping in
//! release builds, so exposing these seeds in test source is safe.

#![allow(dead_code, reason = "used only by feature-gated integration tests")]

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signer, SigningKey};
use serde_json::json;

/// Seed baking into the test sidecar binary-signing keypair. Matches the
/// pubkey hardcoded under `cfg(feature = "test-sidecar-key")` in
/// `crates/cli/src/health/coverage.rs`.
pub const TEST_SIDECAR_SEED: [u8; 32] = [0xAA; 32];

/// Seed baking into the test license JWT signing keypair. Matches the pubkey
/// hardcoded under `cfg(feature = "test-sidecar-key")` in
/// `crates/cli/src/license/mod.rs`.
pub const TEST_LICENSE_SEED: [u8; 32] = [0xBB; 32];

#[must_use]
pub fn sidecar_signing_key() -> SigningKey {
    SigningKey::from_bytes(&TEST_SIDECAR_SEED)
}

#[must_use]
pub fn license_signing_key() -> SigningKey {
    SigningKey::from_bytes(&TEST_LICENSE_SEED)
}

/// Append a detached Ed25519 signature for `binary_path` at
/// `<binary_path>.sig`, matching what `verify_sidecar_signature` expects.
pub fn sign_sidecar_binary(binary_path: &Path) {
    let bytes = fs::read(binary_path).expect("read sidecar binary to sign");
    let signature = sidecar_signing_key().sign(&bytes);
    let sig_path = {
        let mut path = binary_path.as_os_str().to_os_string();
        path.push(".sig");
        std::path::PathBuf::from(path)
    };
    fs::write(&sig_path, signature.to_bytes()).expect("write .sig file");
}

/// Mint a license JWT valid for 30 days that grants the
/// `runtime_coverage` feature. Signed with the test license key.
#[must_use]
pub fn mint_runtime_coverage_jwt() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs()
        .cast_signed();
    let exp = now + 30 * 24 * 60 * 60;

    let header = json!({ "alg": "EdDSA", "typ": "JWT" });
    let payload = json!({
        "iss": "https://test.fallow.cloud",
        "sub": "test-org",
        "tid": "test-tenant",
        "seats": 1,
        "tier": "team",
        "features": ["runtime_coverage"],
        "iat": now,
        "exp": exp,
        "jti": "test-jwt-runtime-coverage",
    });
    encode_jwt(&header, &payload)
}

/// Mint an already-expired JWT to exercise the hard-fail path.
#[must_use]
pub fn mint_expired_runtime_coverage_jwt() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before UNIX epoch")
        .as_secs()
        .cast_signed();
    let iat = now - 180 * 24 * 60 * 60;
    let exp = now - 120 * 24 * 60 * 60;

    let header = json!({ "alg": "EdDSA", "typ": "JWT" });
    let payload = json!({
        "iss": "https://test.fallow.cloud",
        "sub": "test-org",
        "tid": "test-tenant",
        "seats": 1,
        "tier": "team",
        "features": ["runtime_coverage"],
        "iat": iat,
        "exp": exp,
        "jti": "test-jwt-expired",
    });
    encode_jwt(&header, &payload)
}

fn encode_jwt(header: &serde_json::Value, payload: &serde_json::Value) -> String {
    let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(header).expect("encode header"));
    let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(payload).expect("encode payload"));
    let signing_input = format!("{header_b64}.{payload_b64}");
    let signature = license_signing_key().sign(signing_input.as_bytes());
    let signature_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());
    format!("{signing_input}.{signature_b64}")
}
