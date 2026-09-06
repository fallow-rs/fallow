"use strict";

const assert = require("node:assert/strict");
const test = require("node:test");
const path = require("node:path");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const { spawn } = require("node:child_process");
const { once } = require("node:events");
const { resolvePlatformPackage } = require("./run-binary");
const ownManifest = require("../package.json");
const {
  SETUP_PROCESS_TIMEOUT_MS,
  SETUP_TIMEOUT_EXIT_CODE,
  SETUP_TIMEOUT_MESSAGE,
  resolveBinaryArtifact,
  setupTimeoutFor,
  spawnNative,
  superviseSetup,
} = require("./run-binary");

const PACKAGE_NAME = "@fallow-cli/fallow-similar-code-darwin-arm64";
const manifest = (overrides = {}) =>
  JSON.stringify({ name: PACKAGE_NAME, version: ownManifest.version, ...overrides });

test("resolves an exact-version regular native binary", () => {
  const manifestPath = path.join("/packages", "native", "package.json");
  const resolved = resolveBinaryArtifact(
    PACKAGE_NAME,
    () => manifestPath,
    () => manifest(),
    () => ({ isFile: () => true, isSymbolicLink: () => false }),
  );
  assert.equal(resolved.binaryPath, path.join("/packages", "native", "fallow-similar-code"));
});

test("rejects a platform package from another release", () => {
  assert.throws(
    () =>
      resolveBinaryArtifact(
        PACKAGE_NAME,
        () => "/native/package.json",
        () => manifest({ version: "0.0.0" }),
        () => ({ isFile: () => true, isSymbolicLink: () => false }),
      ),
    /version mismatch/,
  );
});

test("rejects symlinked binaries", () => {
  assert.throws(
    () =>
      resolveBinaryArtifact(
        PACKAGE_NAME,
        () => "/native/package.json",
        () => manifest(),
        (filePath) => ({
          isFile: () => true,
          isSymbolicLink: () => !filePath.endsWith("package.json"),
        }),
      ),
    /not a regular file/,
  );
});

test("rejects a symlinked platform manifest", () => {
  assert.throws(
    () =>
      resolveBinaryArtifact(
        PACKAGE_NAME,
        () => "/native/package.json",
        () => manifest(),
        () => ({ isFile: () => true, isSymbolicLink: () => true }),
      ),
    /platform manifest is not a regular file/,
  );
});

test("returns the exact owning manifest in the verification artifact", () => {
  const manifestPath = path.join("/packages", "native", "package.json");
  const artifact = resolveBinaryArtifact(
    PACKAGE_NAME,
    () => manifestPath,
    () => manifest(),
    () => ({ isFile: () => true, isSymbolicLink: () => false }),
  );
  assert.deepEqual(artifact, {
    packageName: PACKAGE_NAME,
    packageVersion: ownManifest.version,
    manifestPath,
    binaryName: "fallow-similar-code",
    binaryPath: path.join("/packages", "native", "fallow-similar-code"),
  });
});

test("rejects a manifest that does not belong to the resolved package", () => {
  assert.throws(
    () =>
      resolveBinaryArtifact(
        PACKAGE_NAME,
        () => "/native/package.json",
        () => manifest({ name: "@fallow-cli/other" }),
        () => ({ isFile: () => true, isSymbolicLink: () => false }),
      ),
    /ownership mismatch/,
  );
});

test("uses the Windows executable basename in the artifact", () => {
  const artifact = resolveBinaryArtifact(
    PACKAGE_NAME,
    () => path.join("C:\\packages", "native", "package.json"),
    () => manifest(),
    () => ({ isFile: () => true, isSymbolicLink: () => false }),
    "win32",
  );
  assert.equal(artifact.binaryName, "fallow-similar-code.exe");
  assert.match(artifact.binaryPath, /fallow-similar-code\.exe$/);
});

test("bounds only the direct setup command", () => {
  assert.equal(setupTimeoutFor(["setup", "--local"]), SETUP_PROCESS_TIMEOUT_MS);
  assert.equal(setupTimeoutFor(["status", "--json"]), null);
  assert.equal(setupTimeoutFor(["serve"]), null);
});

test("setup timeout guidance names the network phases a user can inspect", () => {
  assert.match(SETUP_TIMEOUT_MESSAGE, /timed out/);
  assert.match(SETUP_TIMEOUT_MESSAGE, /DNS/);
  assert.match(SETUP_TIMEOUT_MESSAGE, /proxy/);
  assert.match(SETUP_TIMEOUT_MESSAGE, /network connectivity/);
});

test("force-kills a local setup process that ignores graceful termination", async () => {
  const result = await superviseSetup(
    process.execPath,
    ["-e", "process.on('SIGTERM', () => {}); setInterval(() => {}, 1000)"],
    { timeoutMs: 80, terminateGraceMs: 40, hardExitGraceMs: 100 },
  );

  assert.deepEqual(result, {
    status: SETUP_TIMEOUT_EXIT_CODE,
    signal: null,
    timedOut: true,
  });
});

test("returns a successful local setup process result", async () => {
  const result = await superviseSetup(process.execPath, ["-e", "process.exit(0)"], {
    timeoutMs: 1000,
    terminateGraceMs: 40,
    hardExitGraceMs: 100,
  });

  assert.deepEqual(result, { status: 0, signal: null, timedOut: false });
});

test("spawns non-setup commands without a wrapper timeout", () => {
  let captured = null;
  const result = spawnNative("/fixture/sidecar", ["status", "--json"], (binary, args, options) => {
    captured = { binary, args, options };
    return { status: 0, signal: null };
  });

  assert.equal(result.status, 0);
  assert.equal(captured.binary, "/fixture/sidecar");
  assert.deepEqual(captured.args, ["status", "--json"]);
  assert.deepEqual(captured.options, { env: process.env, stdio: "inherit" });
});

test(
  "the signed setup launcher preserves forwarded SIGTERM as exit 143",
  { skip: process.platform === "win32", timeout: 10_000 },
  async (t) => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "fallow-setup-signal-"));
    t.after(() => fs.rmSync(root, { recursive: true, force: true }));
    const packageName = resolvePlatformPackage();
    const packageDir = path.join(root, "node_modules", packageName);
    fs.mkdirSync(packageDir, { recursive: true });
    const binaryName = "fallow-similar-code";
    const binaryPath = path.join(packageDir, binaryName);
    const binary = Buffer.from(
      "#!/usr/bin/env node\nprocess.on('SIGTERM', () => process.exit(0)); process.stdout.write('READY\\n'); setInterval(() => {}, 1000);\n",
    );
    const { privateKey, publicKey } = crypto.generateKeyPairSync("ed25519");
    fs.writeFileSync(binaryPath, binary, { mode: 0o755 });
    fs.writeFileSync(`${binaryPath}.sig`, crypto.sign(null, binary, privateKey));
    fs.writeFileSync(
      path.join(packageDir, "package.json"),
      JSON.stringify({
        name: packageName,
        version: ownManifest.version,
        fallowDigests: {
          [binaryName]: `sha256:${crypto.createHash("sha256").update(binary).digest("hex")}`,
        },
      }),
    );
    // The real verifier checks a signed fixture using an ephemeral trust key.
    // Package resolution, digest verification, spawning and signal forwarding all run.
    const driver = `
    const crypto = require("node:crypto");
    const verifier = require(${JSON.stringify(require.resolve("./verify-binary"))});
    const verify = verifier.verifyBinary;
    const publicKey = crypto.createPublicKey(${JSON.stringify(publicKey.export({ type: "spki", format: "pem" }))});
    verifier.verifyBinary = artifact => verify(artifact, { createPublicKey: () => publicKey });
    require(${JSON.stringify(require.resolve("./run-binary"))}).run(["setup"]);
  `;
    const child = spawn(process.execPath, ["-e", driver], {
      env: { ...process.env, NODE_PATH: path.join(root, "node_modules") },
      stdio: ["ignore", "pipe", "pipe"],
    });
    t.after(() => child.kill("SIGKILL"));
    let ready = false;
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString();
      if (!ready && stdout.includes("READY")) {
        ready = true;
        child.kill("SIGTERM");
      }
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString();
    });
    const [status, signal] = await once(child, "close");
    assert.equal(ready, true, stderr);
    assert.equal(signal, null);
    assert.equal(status, 143, stderr);
  },
);
