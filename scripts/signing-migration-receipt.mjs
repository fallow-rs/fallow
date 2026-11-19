import assert from "node:assert/strict";
import { createPublicKey, verify } from "node:crypto";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const ANCHOR = "g05v13Mz5u7fd5NHxxCstAPS2CNNVZ9e18h+VSreC9E=";

/** Validate an artifact against independently verified run and current environment metadata. */
export const validateReceipt = (receipt, expected, recipient) => {
  const fields = [
    "schema",
    "mode",
    "repository",
    "source_sha",
    "run_id",
    "run_attempt",
    "environment",
    "signing_public_key",
    "key_id",
    "recipient_public_key",
    "encrypted_value",
  ];
  assert.deepEqual(Object.keys(receipt).toSorted(), fields.toSorted(), "Unexpected receipt fields");
  assert.equal(receipt.schema, 1);
  assert.equal(receipt.mode, "export");
  assert.equal(receipt.repository, "fallow-rs/fallow");
  assert.equal(receipt.environment, "release");
  assert.equal(receipt.signing_public_key, ANCHOR);
  assert.match(expected.source_sha, /^[a-f0-9]{40}$/u);
  assert.match(expected.run_id, /^\d+$/u);
  assert.match(expected.run_attempt, /^\d+$/u);
  for (const field of ["repository", "source_sha", "run_id", "run_attempt"]) {
    assert.equal(receipt[field], expected[field], `Unexpected ${field}`);
  }
  assert.equal(receipt.key_id, recipient.key_id, "Environment transport key changed");
  assert.equal(receipt.recipient_public_key, recipient.key, "Environment recipient changed");
  assert.equal(Buffer.from(recipient.key, "base64").length, 32);
  assert.equal(Buffer.from(recipient.key, "base64").toString("base64"), recipient.key);
  const ciphertext = Buffer.from(receipt.encrypted_value, "base64");
  assert.ok(ciphertext.length >= 92 && ciphertext.length <= 4096, "Invalid ciphertext length");
  assert.equal(
    ciphertext.toString("base64"),
    receipt.encrypted_value,
    "Invalid ciphertext encoding",
  );
  return { encrypted_value: receipt.encrypted_value, key_id: receipt.key_id };
};

/** Verify a detached Ed25519 proof with Node's independent crypto implementation. */
export const verifyProofSignature = (signature, challenge, publicKey) => {
  const raw = Buffer.from(publicKey, "base64");
  assert.equal(raw.length, 32);
  const key = createPublicKey({
    key: Buffer.concat([Buffer.from("302a300506032b6570032100", "hex"), raw]),
    format: "der",
    type: "spki",
  });
  const decoded = Buffer.from(signature, "base64");
  assert.equal(decoded.length, 64);
  assert.equal(decoded.toString("base64"), signature);
  assert.ok(
    verify(null, Buffer.from(challenge, "ascii"), key, decoded),
    "Invalid environment signature",
  );
};

/** Validate the standalone environment signature against the unchanged public anchor. */
const validateProof = (receipt, expected) => {
  const fields = [
    "schema",
    "mode",
    "repository",
    "source_sha",
    "run_id",
    "run_attempt",
    "environment",
    "signing_public_key",
    "challenge",
    "signature",
  ];
  assert.deepEqual(Object.keys(receipt).toSorted(), fields.toSorted(), "Unexpected proof fields");
  assert.equal(receipt.schema, 1);
  assert.equal(receipt.mode, "verify");
  assert.equal(receipt.repository, "fallow-rs/fallow");
  assert.equal(receipt.environment, "release");
  assert.equal(receipt.signing_public_key, ANCHOR);
  assert.match(expected.source_sha, /^[a-f0-9]{40}$/u);
  assert.match(expected.run_id, /^\d+$/u);
  assert.match(expected.run_attempt, /^\d+$/u);
  const source = {};
  for (const field of ["repository", "run_attempt", "run_id", "source_sha"]) {
    assert.equal(receipt[field], expected[field], `Unexpected ${field}`);
    source[field] = expected[field];
  }
  const challenge = `fallow-signing-environment-proof-v1:${JSON.stringify(source)}`;
  assert.equal(receipt.challenge, challenge);
  verifyProofSignature(receipt.signature, challenge, ANCHOR);
  return { verified: true, ...source, signing_public_key: ANCHOR };
};

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  try {
    const mode = process.argv[2];
    assert.ok(mode === "export" || mode === "verify", "Use export or verify");
    assert.equal(process.argv.length, mode === "export" ? 6 : 5);
    const [receipt, expected, recipient] = process.argv
      .slice(3)
      .map((path) => JSON.parse(readFileSync(path, "utf8")));
    const result =
      mode === "export"
        ? validateReceipt(receipt, expected, recipient)
        : validateProof(receipt, expected);
    process.stdout.write(`${JSON.stringify(result)}\n`);
  } catch {
    console.error(
      "Signing migration receipt rejected. Check trusted public run and recipient metadata.",
    );
    process.exitCode = 1;
  }
}
