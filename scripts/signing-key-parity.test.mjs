import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";
import { test } from "node:test";
import {
  assertSigningKeyParity,
  checkRepositorySigningKeyParity,
  decodeReleasePublicKey,
  parseEmbeddedPublicKey,
} from "./signing-key-parity.mjs";

const bytes = Array.from({ length: 32 }, (_, index) => index);
const key = Buffer.from(bytes);
const embedded = (name, values = bytes) => `const ${name} = Buffer.from([
  ${values.join(", ")},
]);`;
const securityPolicy = (value = key) => `**Public key fingerprint (raw 32-byte Ed25519, hex):**

\`\`\`
${value.toString("hex")}
\`\`\`

The base64 form of the public key above (\`${value.toString("base64")}\`) is canonical.
`;
const surfaces = (overrides = {}) => ({
  vscodeSource: embedded("BINARY_SIGNING_PUBLIC_KEY"),
  npmVerifierSource: embedded("EMBEDDED_PUBLIC_KEY"),
  companionVerifierSource: embedded("PUBLIC_KEY"),
  securityPolicy: securityPolicy(),
  ...overrides,
});

test("current repository signing public keys agree", () => {
  assert.equal(checkRepositorySigningKeyParity().length, 32);
});

test("embedded public key parsing requires a complete byte array", () => {
  assert.throws(
    () => parseEmbeddedPublicKey(embedded("KEY", bytes.slice(0, 31)), "KEY", "fixture"),
    /expected exactly 32 key bytes/u,
  );
  assert.throws(
    () =>
      parseEmbeddedPublicKey(
        embedded("KEY", [...bytes.slice(0, 31), "not-a-byte"]),
        "KEY",
        "fixture",
      ),
    /invalid byte 32/u,
  );
});

test("either embedded consumer fails with its named surface", () => {
  const changed = [...bytes];
  changed[0] = 255;

  assert.throws(
    () =>
      assertSigningKeyParity(
        surfaces({ vscodeSource: embedded("BINARY_SIGNING_PUBLIC_KEY", changed) }),
      ),
    /npm verifier: embedded public key differs from VS Code/u,
  );
  assert.throws(
    () =>
      assertSigningKeyParity(
        surfaces({ npmVerifierSource: embedded("EMBEDDED_PUBLIC_KEY", changed) }),
      ),
    /npm verifier: embedded public key differs from VS Code/u,
  );
  assert.throws(
    () =>
      assertSigningKeyParity(
        surfaces({ companionVerifierSource: embedded("PUBLIC_KEY", changed) }),
      ),
    /similar-code companion verifier: embedded public key differs from VS Code/u,
  );
});

test("documented hex and base64 drift identify SECURITY.md", () => {
  const changed = Buffer.from(key);
  changed[0] = 255;

  assert.throws(
    () =>
      assertSigningKeyParity(
        surfaces({
          securityPolicy: securityPolicy().replace(key.toString("hex"), changed.toString("hex")),
        }),
      ),
    /SECURITY\.md: hex fingerprint differs/u,
  );
  assert.throws(
    () =>
      assertSigningKeyParity(
        surfaces({
          securityPolicy: securityPolicy().replace(
            key.toString("base64"),
            changed.toString("base64"),
          ),
        }),
      ),
    /SECURITY\.md: base64 public key differs/u,
  );
});

test("release public key accepts parity and rejects drift", () => {
  assert.deepEqual(decodeReleasePublicKey(key.toString("base64")), key);
  assert.equal(
    assertSigningKeyParity(surfaces({ releasePublicKey: key.toString("base64") })).length,
    32,
  );

  const changed = Buffer.from(key);
  changed[0] = 255;
  assert.throws(
    () => assertSigningKeyParity(surfaces({ releasePublicKey: changed.toString("base64") })),
    /release environment: public key differs from committed public key/u,
  );
});

test("signing-key-parity.mjs checks the release key when Node starts it", () => {
  // An all-zero key is fake, so the release check must report drift.
  const fakeKey = Buffer.alloc(32).toString("base64");
  const result = spawnSync(process.execPath, ["scripts/signing-key-parity.mjs", "--release-env"], {
    cwd: resolve(import.meta.dirname, ".."),
    encoding: "utf8",
    env: { ED25519_BINARY_SIGNING_PUBLIC_KEY: fakeKey, PATH: process.env.PATH ?? "" },
  });

  assert.equal(result.status, 1, result.stderr);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /release environment: public key differs from committed public key/u);
});

test("signing-key-parity.mjs --release-env fails when the release key is absent", () => {
  const result = spawnSync(process.execPath, ["scripts/signing-key-parity.mjs", "--release-env"], {
    cwd: resolve(import.meta.dirname, ".."),
    encoding: "utf8",
    env: { PATH: process.env.PATH ?? "" },
  });

  assert.equal(result.status, 1, result.stdout);
  assert.equal(result.stdout, "");
  assert.match(result.stderr, /release environment: ED25519_BINARY_SIGNING_PUBLIC_KEY is missing/u);
});
