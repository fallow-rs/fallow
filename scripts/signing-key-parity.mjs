import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const KEY_LENGTH = 32;
const HEX_LENGTH = KEY_LENGTH * 2;
const BASE64_PATTERN = /^[A-Za-z0-9+/]{43}=$/u;

const escapeRegExp = (value) => value.replaceAll(/[.*+?^${}()|[\]\\]/g, "\\$&");

export const parseEmbeddedPublicKey = (source, constantName, surface) => {
  const pattern = new RegExp(
    `\\bconst\\s+${escapeRegExp(constantName)}\\s*=\\s*Buffer\\.from\\(\\s*\\[([\\s\\S]*?)\\]\\s*\\)`,
    "u",
  );
  const match = source.match(pattern);
  assert.ok(match, `${surface}: missing ${constantName} Buffer.from array`);

  const tokens = match[1]
    .split(",")
    .map((token) => token.trim())
    .filter(Boolean);
  assert.equal(tokens.length, KEY_LENGTH, `${surface}: expected exactly ${KEY_LENGTH} key bytes`);

  const bytes = tokens.map((token, index) => {
    assert.match(token, /^(?:0|[1-9]\d{0,2})$/u, `${surface}: invalid byte ${index + 1}`);
    const value = Number(token);
    assert.ok(value <= 255, `${surface}: byte ${index + 1} is outside 0..255`);
    return value;
  });

  return Buffer.from(bytes);
};

export const parseDocumentedPublicKey = (securityPolicy) => {
  const fingerprint = securityPolicy.match(
    /\*\*Public key fingerprint \(raw 32-byte Ed25519, hex\):\*\*\s*```[^\n]*\n([0-9a-fA-F]+)\n```/u,
  )?.[1];
  assert.ok(fingerprint, "SECURITY.md: missing public key fingerprint");
  assert.equal(
    fingerprint.length,
    HEX_LENGTH,
    `SECURITY.md: expected exactly ${HEX_LENGTH} fingerprint characters`,
  );

  const base64 = securityPolicy.match(/base64 form of the public key above \(`([^`]+)`\)/u)?.[1];
  assert.ok(base64, "SECURITY.md: missing base64 public key");
  assert.match(base64, BASE64_PATTERN, "SECURITY.md: public key is not canonical base64");

  return {
    fingerprint: Buffer.from(fingerprint, "hex"),
    base64: Buffer.from(base64, "base64"),
  };
};

export const decodeReleasePublicKey = (value) => {
  assert.ok(value, "release environment: ED25519_BINARY_SIGNING_PUBLIC_KEY is missing");
  assert.match(value, BASE64_PATTERN, "release environment: public key is not canonical base64");
  const key = Buffer.from(value, "base64");
  assert.equal(
    key.length,
    KEY_LENGTH,
    `release environment: expected exactly ${KEY_LENGTH} public key bytes`,
  );
  return key;
};

export const assertSigningKeyParity = ({
  vscodeSource,
  npmVerifierSource,
  securityPolicy,
  releasePublicKey,
}) => {
  const vscodeKey = parseEmbeddedPublicKey(
    vscodeSource,
    "BINARY_SIGNING_PUBLIC_KEY",
    "VS Code extension",
  );
  const npmKey = parseEmbeddedPublicKey(npmVerifierSource, "EMBEDDED_PUBLIC_KEY", "npm verifier");
  const documented = parseDocumentedPublicKey(securityPolicy);

  assert.deepEqual(npmKey, vscodeKey, "npm verifier: embedded public key differs from VS Code");
  assert.deepEqual(
    documented.fingerprint,
    vscodeKey,
    "SECURITY.md: hex fingerprint differs from embedded public key",
  );
  assert.deepEqual(
    documented.base64,
    vscodeKey,
    "SECURITY.md: base64 public key differs from embedded public key",
  );

  if (releasePublicKey !== undefined) {
    assert.deepEqual(
      decodeReleasePublicKey(releasePublicKey),
      vscodeKey,
      "release environment: public key differs from committed public key",
    );
  }

  return vscodeKey;
};

export const checkRepositorySigningKeyParity = ({
  root = resolve(dirname(fileURLToPath(import.meta.url)), ".."),
  releasePublicKey,
} = {}) =>
  assertSigningKeyParity({
    vscodeSource: readFileSync(resolve(root, "editors/vscode/src/download.ts"), "utf8"),
    npmVerifierSource: readFileSync(resolve(root, "npm/fallow/scripts/verify-binary.js"), "utf8"),
    securityPolicy: readFileSync(resolve(root, "SECURITY.md"), "utf8"),
    releasePublicKey,
  });

const isDirectInvocation =
  process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (isDirectInvocation) {
  const args = process.argv.slice(2);
  assert.ok(
    args.length === 0 || (args.length === 1 && args[0] === "--release-env"),
    "usage: node scripts/signing-key-parity.mjs [--release-env]",
  );
  checkRepositorySigningKeyParity({
    releasePublicKey:
      args[0] === "--release-env" ? process.env.ED25519_BINARY_SIGNING_PUBLIC_KEY : undefined,
  });
  console.log("Signing public-key parity verified.");
}
