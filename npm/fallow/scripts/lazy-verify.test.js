const test = require("node:test");
const assert = require("node:assert/strict");
const crypto = require("node:crypto");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const {
  ensureVerified,
  SENTINEL_SCHEMA_VERSION,
  VERIFY_LOG_ENV,
  _resetWarningState,
} = require("./lazy-verify");
const { SENTINEL_FILENAME } = require("./sentinel-path");
const { _verifyWithKey, SKIP_ENV } = require("./verify-binary");

// ---- shared fixtures ------------------------------------------------------

const DEFAULT_PLATFORM = "linux";
const DEFAULT_PACKAGE_NAME = "@fallow-cli/test-platform";

function makeKeypair() {
  const { privateKey, publicKey } = crypto.generateKeyPairSync("ed25519");
  const spki = publicKey.export({ format: "der", type: "spki" });
  const rawPub = spki.subarray(spki.length - 32);
  return { privateKey, rawPub };
}

function packageNameForPlatform(platform) {
  return platform === "win32" ? "@fallow-cli/win32-x64-msvc" : DEFAULT_PACKAGE_NAME;
}

function binaryNames(platform) {
  // Platform packages ship a single multicall `fallow` binary.
  return [platform === "win32" ? "fallow.exe" : "fallow"];
}

function computeDigestsForDir(dir, platform) {
  const out = {};
  for (const base of binaryNames(platform)) {
    const full = path.join(dir, base);
    out[base] = "sha256:" + crypto.createHash("sha256").update(fs.readFileSync(full)).digest("hex");
  }
  return out;
}

function mkPlatformDir(privateKey, options) {
  const opts = options || {};
  const platform = opts.platform || DEFAULT_PLATFORM;
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "fallow-lazy-test-"));
  for (const base of binaryNames(platform)) {
    const binaryPath = path.join(dir, base);
    const content = Buffer.from(`mock ${base}`);
    fs.writeFileSync(binaryPath, content);
    if (opts.skipSigFor === base) continue;
    const sig = crypto.sign(null, content, privateKey);
    if (opts.corruptSigFor === base) sig[0] ^= 0xff;
    fs.writeFileSync(`${binaryPath}.sig`, sig);
  }
  fs.writeFileSync(
    path.join(dir, "package.json"),
    JSON.stringify({
      name: opts.packageName || packageNameForPlatform(platform),
      version: opts.version || "2.81.0",
      fallowDigests: opts.skipDigests ? undefined : computeDigestsForDir(dir, platform),
    }),
  );
  return dir;
}

function cleanup(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
}

function captureStderr(t) {
  const lines = [];
  const original = process.stderr.write.bind(process.stderr);
  process.stderr.write = (chunk) => {
    lines.push(typeof chunk === "string" ? chunk : chunk.toString("utf8"));
    return true;
  };
  t.after(() => {
    process.stderr.write = original;
  });
  return { lines };
}

function setupCacheRoot(t) {
  const cacheRoot = fs.mkdtempSync(path.join(os.tmpdir(), "fallow-lazy-cache-"));
  t.after(() => cleanup(cacheRoot));
  return cacheRoot;
}

function baseInput(dir, verifyFn, extras) {
  const manifestPath = path.join(dir, "package.json");
  const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  return {
    platformPkgDir: dir,
    packageName: manifest.name,
    manifestPath,
    verifyFn,
    env: {},
    platform: DEFAULT_PLATFORM,
    ...extras,
  };
}

// ---- happy path: cache miss then cache hit --------------------------------

test("ensureVerified verifies on cache miss and writes the sentinel", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey);
  t.after(() => cleanup(dir));

  const result = ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)));
  assert.equal(result.ok, true);
  assert.equal(result.cached, false);
  assert.equal(result.sentinelPath, path.join(dir, SENTINEL_FILENAME));

  // Sentinel file exists and validates
  const sentinel = JSON.parse(fs.readFileSync(result.sentinelPath, "utf8"));
  assert.equal(sentinel.schemaVersion, SENTINEL_SCHEMA_VERSION);
  assert.equal(sentinel.packageVersion, "2.81.0");
  assert.equal(sentinel.packageName, "@fallow-cli/test-platform");
  assert.equal(Object.keys(sentinel.binaries).length, 1);
});

test("ensureVerified verifies and caches a win32 executable on any host", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey, { platform: "win32" });
  t.after(() => cleanup(dir));
  const input = baseInput(dir, (binaryPath) => _verifyWithKey(binaryPath, rawPub), {
    platform: "win32",
  });

  const verified = ensureVerified(input);
  assert.equal(verified.ok, true);
  assert.equal(verified.cached, false);
  const sentinel = JSON.parse(fs.readFileSync(verified.sentinelPath, "utf8"));
  assert.deepEqual(Object.keys(sentinel.binaries), ["fallow.exe"]);

  const cached = ensureVerified({
    ...input,
    verifyFn: () => assert.fail("signature verification must not run on a cache hit"),
  });
  assert.equal(cached.ok, true);
  assert.equal(cached.cached, true);
});

test("ensureVerified returns cached:true on a valid sentinel", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey);
  t.after(() => cleanup(dir));

  // First call writes sentinel
  ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)));

  // Second call should hit cache (verifyFn must NOT be called)
  let verifyCallCount = 0;
  const result = ensureVerified(
    baseInput(dir, (p) => {
      verifyCallCount += 1;
      return _verifyWithKey(p, rawPub);
    }),
  );
  assert.equal(result.ok, true);
  assert.equal(result.cached, true);
  assert.equal(verifyCallCount, 0, "sig verify should NOT have run on a cache hit");
});

// ---- cache invalidation modes ---------------------------------------------

test("ensureVerified invalidates sentinel on mtime drift", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey);
  t.after(() => cleanup(dir));

  ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)));

  // Bump the mtime of one binary; sentinel should now be stale.
  const newTime = new Date(Date.now() + 10_000);
  fs.utimesSync(path.join(dir, binaryNames(DEFAULT_PLATFORM)[0]), newTime, newTime);

  let verifyCallCount = 0;
  const result = ensureVerified(
    baseInput(dir, (p) => {
      verifyCallCount += 1;
      return _verifyWithKey(p, rawPub);
    }),
  );
  assert.equal(result.ok, true);
  assert.equal(result.cached, false);
  assert.equal(verifyCallCount, 1, "verify should rerun for the single binary");
});

test("ensureVerified invalidates sentinel on packageVersion drift", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey);
  t.after(() => cleanup(dir));

  ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)));

  // Rewrite manifest with a different version.
  const manifest = JSON.parse(fs.readFileSync(path.join(dir, "package.json"), "utf8"));
  manifest.version = "2.81.1";
  fs.writeFileSync(path.join(dir, "package.json"), JSON.stringify(manifest));

  const result = ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)));
  assert.equal(result.cached, false);
});

test("ensureVerified invalidates sentinel on packageName drift", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey, { packageName: "@fallow-cli/x" });
  t.after(() => cleanup(dir));

  ensureVerified({
    ...baseInput(dir, (p) => _verifyWithKey(p, rawPub)),
    packageName: "@fallow-cli/x",
  });

  // Now claim a different package name; sentinel becomes stale.
  const result = ensureVerified({
    ...baseInput(dir, (p) => _verifyWithKey(p, rawPub)),
    packageName: "@fallow-cli/y",
  });
  // Manifest still says @fallow-cli/x, so sentinel validates against manifest
  // but the sentinel was originally written for x. Since we pass packageName=y
  // for the cache lookup but the manifest still says x, the sentinel
  // .packageName=x matches the manifest.name=x. We must rewrite manifest too
  // to truly drift. Skip this case and instead force sentinel rewrite to
  // have a different name:
  fs.writeFileSync(
    result.sentinelPath || path.join(dir, SENTINEL_FILENAME),
    JSON.stringify({
      schemaVersion: SENTINEL_SCHEMA_VERSION,
      verifiedAt: new Date().toISOString(),
      packageVersion: "2.81.0",
      packageName: "@fallow-cli/wrong-name",
      binaries: {
        [binaryNames(DEFAULT_PLATFORM)[0]]: {
          mtimeMs: fs.statSync(path.join(dir, binaryNames(DEFAULT_PLATFORM)[0])).mtimeMs,
        },
      },
    }),
  );

  const result2 = ensureVerified({
    ...baseInput(dir, (p) => _verifyWithKey(p, rawPub)),
    packageName: "@fallow-cli/x",
  });
  assert.equal(result2.cached, false);
});

test("ensureVerified invalidates sentinel on malformed JSON", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey);
  t.after(() => cleanup(dir));

  ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)));
  fs.writeFileSync(path.join(dir, SENTINEL_FILENAME), "not-json-at-all");

  const result = ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)));
  assert.equal(result.cached, false);
});

test("ensureVerified invalidates sentinel on schemaVersion drift", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey);
  t.after(() => cleanup(dir));

  ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)));
  const sentinelPath = path.join(dir, SENTINEL_FILENAME);
  const data = JSON.parse(fs.readFileSync(sentinelPath, "utf8"));
  data.schemaVersion = 999;
  fs.writeFileSync(sentinelPath, JSON.stringify(data));

  const result = ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)));
  assert.equal(result.cached, false);
});

// ---- failure modes --------------------------------------------------------

test("ensureVerified returns sig-invalid on a tampered signature", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey, { corruptSigFor: binaryNames(DEFAULT_PLATFORM)[0] });
  t.after(() => cleanup(dir));

  const result = ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)));
  assert.equal(result.ok, false);
  assert.equal(result.code, "sig-invalid");
  assert.match(result.binary, /fallow/);
  // Sentinel must NOT have been written on failure
  assert.equal(fs.existsSync(path.join(dir, SENTINEL_FILENAME)), false);
});

test("ensureVerified returns digest-unavailable on a pre-#597 manifest", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey, { skipDigests: true });
  t.after(() => cleanup(dir));

  const result = ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)));
  assert.equal(result.ok, false);
  assert.equal(result.code, "digest-unavailable");
  assert.match(result.message, /predates fallow 2\.78\.1/);
  assert.match(result.message, new RegExp(SKIP_ENV));
});

// ---- cache-dir cascade ----------------------------------------------------

test("ensureVerified honors FALLOW_VERIFY_CACHE_DIR when platform pkg dir is non-writable", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey);
  const cacheRoot = setupCacheRoot(t);
  t.after(() => cleanup(dir));

  const result = ensureVerified({
    ...baseInput(dir, (p) => _verifyWithKey(p, rawPub)),
    env: { FALLOW_VERIFY_CACHE_DIR: cacheRoot },
    isWritable: (candidate) => candidate !== dir,
  });
  assert.equal(result.ok, true);
  assert.equal(result.cached, false);
  assert.match(result.sentinelPath, new RegExp(cacheRoot.replace(/\\/g, "\\\\")));
});

test("ensureVerified emits a single warning when no sentinel location is writable", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey);
  const stderr = captureStderr(t);
  t.after(() => cleanup(dir));
  const input = {
    ...baseInput(dir, (p) => _verifyWithKey(p, rawPub)),
    homedir: undefined,
    isWritable: () => false,
  };

  const result = ensureVerified(input);
  assert.equal(result.sentinelPath, null);
  const warnings = stderr.lines.filter((line) => line.includes("no writable cache location"));
  assert.equal(warnings.length, 1);

  ensureVerified(input);
  const repeatedWarnings = stderr.lines.filter((line) =>
    line.includes("no writable cache location"),
  );
  assert.equal(repeatedWarnings.length, 1);
});

// ---- FALLOW_SKIP_BINARY_VERIFY -------------------------------------------

test("ensureVerified short-circuits when FALLOW_SKIP_BINARY_VERIFY is set", () => {
  _resetWarningState();
  const result = ensureVerified({
    platformPkgDir: "/this/path/does/not/exist",
    packageName: "@fallow-cli/x",
    manifestPath: "/also/missing",
    env: { [SKIP_ENV]: "1" },
  });
  assert.equal(result.ok, true);
  assert.equal(result.skipped, true);
  assert.match(result.reason, new RegExp(SKIP_ENV));
});

// ---- FALLOW_VERIFY_LOG ----------------------------------------------------

test("ensureVerified emits one stderr line per outcome when FALLOW_VERIFY_LOG=1", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey);
  t.after(() => cleanup(dir));

  const stderr = captureStderr(t);
  // First invocation: cache miss
  ensureVerified({
    ...baseInput(dir, (p) => _verifyWithKey(p, rawPub)),
    env: { FALLOW_VERIFY_LOG: "1" },
  });
  // Second invocation: cache hit
  ensureVerified({
    ...baseInput(dir, (p) => _verifyWithKey(p, rawPub)),
    env: { FALLOW_VERIFY_LOG: "1" },
  });

  const logs = stderr.lines.filter((l) => l.startsWith("fallow-verify "));
  assert.equal(logs.length, 2);
  assert.match(logs[0], /outcome=ok cache=miss/);
  assert.match(logs[1], /outcome=ok cache=hit/);
});

test("ensureVerified does not log when FALLOW_VERIFY_LOG is unset", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey);
  t.after(() => cleanup(dir));

  const stderr = captureStderr(t);
  ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)));
  const logs = stderr.lines.filter((l) => l.startsWith("fallow-verify "));
  assert.equal(logs.length, 0);
});

// ---- concurrency ----------------------------------------------------------

test("ensureVerified is idempotent under concurrent first-runs", async (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey);
  t.after(() => cleanup(dir));

  const calls = await Promise.all([
    Promise.resolve().then(() => ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)))),
    Promise.resolve().then(() => ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)))),
    Promise.resolve().then(() => ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)))),
  ]);

  for (const r of calls) assert.equal(r.ok, true);
  // Sentinel exists and is valid JSON
  const sentinel = JSON.parse(fs.readFileSync(path.join(dir, SENTINEL_FILENAME), "utf8"));
  assert.equal(sentinel.schemaVersion, SENTINEL_SCHEMA_VERSION);
  assert.equal(sentinel.packageName, "@fallow-cli/test-platform");

  // No leftover .tmp files in the dir
  const files = fs.readdirSync(dir);
  const tmps = files.filter((f) => f.includes(".tmp"));
  assert.deepEqual(tmps, [], "no leftover temp files from concurrent writes");
});

// ---- cross-install sentinel reuse (security regression test) ----------

test("ensureVerified rejects a sentinel written for a different install dir", (t) => {
  _resetWarningState();
  // Two installs of the same package + version. install A is clean and writes
  // a sentinel to a shared cache; install B has a tampered binary at the same
  // package name + version. B must NOT trust A's sentinel via cache hit even
  // when the recorded mtimes happen to match B's binary mtimes.
  const { privateKey, rawPub } = makeKeypair();
  const installA = mkPlatformDir(privateKey);
  const installB = mkPlatformDir(privateKey);
  // Tamper install B's fallow binary AFTER mkPlatformDir wrote a valid sig
  // for the original bytes (mkPlatformDir does not expose a corrupt-binary
  // option, so simulate the attack by overwriting bytes here).
  fs.writeFileSync(
    path.join(installB, binaryNames(DEFAULT_PLATFORM)[0]),
    Buffer.from("tampered bytes"),
  );
  const sharedCache = fs.mkdtempSync(path.join(os.tmpdir(), "fallow-shared-cache-"));
  const isWritable = (candidate) => candidate !== installA && candidate !== installB;

  t.after(() => {
    cleanup(installA);
    cleanup(installB);
    cleanup(sharedCache);
  });

  // Install A: clean verify. Sentinel lands in the shared cache because the
  // platform pkg dir is read-only.
  const resultA = ensureVerified({
    ...baseInput(installA, (p) => _verifyWithKey(p, rawPub)),
    env: { FALLOW_VERIFY_CACHE_DIR: sharedCache },
    isWritable,
  });
  assert.equal(resultA.ok, true);
  assert.match(resultA.sentinelPath, new RegExp(sharedCache.replace(/\\/g, "\\\\")));

  // Attacker on install B copies install A's mtimes onto B's binaries so the
  // mtime pre-filter would have matched. With only mtime + name + version
  // gates this would produce a cache hit and skip verify; the platformPkgDir
  // + SHA-256 binding must prevent that.
  for (const name of binaryNames()) {
    const aStat = fs.statSync(path.join(installA, name));
    fs.utimesSync(path.join(installB, name), aStat.atime, aStat.mtime);
  }

  let verifyCallCount = 0;
  const resultB = ensureVerified({
    ...baseInput(installB, (p) => {
      verifyCallCount += 1;
      return _verifyWithKey(p, rawPub);
    }),
    env: { FALLOW_VERIFY_CACHE_DIR: sharedCache },
    isWritable,
  });
  assert.equal(resultB.ok, false);
  assert.equal(resultB.code, "sig-invalid");
  assert.ok(verifyCallCount > 0, "expected re-verify on cross-install sentinel read");
});

test("ensureVerified rejects a sentinel where bytes drift but mtime stays", (t) => {
  _resetWarningState();
  const { privateKey, rawPub } = makeKeypair();
  const dir = mkPlatformDir(privateKey);
  t.after(() => cleanup(dir));

  // First invocation writes a sentinel with the clean SHA-256.
  ensureVerified(baseInput(dir, (p) => _verifyWithKey(p, rawPub)));

  // Tamper the binary in place AND restore the prior mtime, so the mtime
  // pre-filter matches but the bytes do not.
  const binPath = path.join(dir, binaryNames(DEFAULT_PLATFORM)[0]);
  const before = fs.statSync(binPath);
  fs.writeFileSync(binPath, Buffer.from("tampered"));
  fs.utimesSync(binPath, before.atime, before.mtime);

  let verifyCallCount = 0;
  const result = ensureVerified(
    baseInput(dir, (p) => {
      verifyCallCount += 1;
      return _verifyWithKey(p, rawPub);
    }),
  );
  assert.equal(result.ok, false);
  assert.equal(result.code, "sig-invalid");
  assert.ok(verifyCallCount > 0, "expected re-verify when bytes diverge from sentinel SHA");
});

// ---- FALLOW_SKIP_BINARY_VERIFY warning (regression test for documented contract) ----

test("ensureVerified warns once on stderr when FALLOW_SKIP_BINARY_VERIFY is set", (t) => {
  _resetWarningState();
  const stderr = captureStderr(t);
  const env = { [SKIP_ENV]: "1" };
  ensureVerified({ platformPkgDir: "/x", packageName: "@fallow-cli/y", manifestPath: "/z", env });
  ensureVerified({ platformPkgDir: "/x", packageName: "@fallow-cli/y", manifestPath: "/z", env });
  const warnings = stderr.lines.filter(
    (l) => l.includes(`${SKIP_ENV} is set`) && l.includes("verification is skipped"),
  );
  assert.equal(warnings.length, 1, "warning should fire exactly once per process");
});

// ---- VERIFY_LOG_ENV export ------------------------------------------------

test("VERIFY_LOG_ENV is exported with the documented name", () => {
  assert.equal(VERIFY_LOG_ENV, "FALLOW_VERIFY_LOG");
});
