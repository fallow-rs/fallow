// Lazy first-run binary verification for the fallow npm wrapper.
//
// Called from bin/fallow, bin/fallow-lsp, and bin/fallow-mcp before each
// invocation execs the platform binary. Replaces the postinstall-time check
// that npm RFC 868 (npm/cli#9360) Phase 2 will silently disable for any
// consumer that has not added fallow to its `allowScripts` field.
//
// On every invocation:
//   1. Resolve a writable sentinel location (see sentinel-path.js).
//   2. If a valid sentinel exists, fast-path return ok (cache hit).
//   3. Otherwise run verifyInstalledSync (Ed25519 + SHA-256), write the
//      sentinel on success, return result.
//
// Verification is fail-closed: any non-ok outcome surfaces to the caller and
// bin/fallow exits non-zero before execing the binary. FALLOW_SKIP_BINARY_VERIFY
// remains the documented escape hatch.
//
// Refs: SECURITY.md "Binary distribution and verification".
//
// No external deps beyond node:fs / node:path / node:crypto.

const fs = require("node:fs");
const path = require("node:path");
const crypto = require("node:crypto");

const { resolveSentinelPath } = require("./sentinel-path");
const { verifyInstalledSync, SKIP_ENV } = require("./verify-binary");

// Bumped to 2 when SHA-256 + platformPkgDir binding landed (closes the
// cross-install reuse gap in the shared $XDG fallback cache). v1 sentinels
// without these fields are invalidated automatically.
const SENTINEL_SCHEMA_VERSION = 2;
const VERIFY_LOG_ENV = "FALLOW_VERIFY_LOG";

// One-shot warning state: each warning class fires once per process,
// keyed by `code` so independent failure modes are still surfaced.
const _warningEmitted = new Set();

function warnOnce(code, message) {
  if (_warningEmitted.has(code)) {
    return;
  }
  _warningEmitted.add(code);
  process.stderr.write(`fallow: ${message}\n`);
}

function isVerifyLogEnabled(env) {
  const v = (env || process.env)[VERIFY_LOG_ENV];
  if (typeof v !== "string") return false;
  const lower = v.trim().toLowerCase();
  return lower === "1" || lower === "true" || lower === "yes";
}

function emitVerifyLog(env, payload) {
  if (!isVerifyLogEnabled(env)) return;
  // Stable single-line format. Fields are space-separated key=value pairs.
  // Order matters for log-grep ergonomics; do not reorder casually.
  const parts = [];
  for (const key of ["outcome", "cache", "sentinel", "reason", "code", "binary"]) {
    if (payload[key] !== undefined && payload[key] !== null) {
      const v = String(payload[key]).replace(/[\s"]/g, "_");
      parts.push(`${key}=${v}`);
    }
  }
  process.stderr.write(`fallow-verify ${parts.join(" ")}\n`);
}

function binaryTargetsForPlatform(platform) {
  const ext = platform === "win32" ? ".exe" : "";
  return [`fallow${ext}`, `fallow-lsp${ext}`, `fallow-mcp${ext}`];
}

function statMtimeMs(absPath) {
  try {
    return fs.statSync(absPath).mtimeMs;
  } catch {
    return null;
  }
}

function readManifestVersion(manifestPath) {
  try {
    const manifest = JSON.parse(fs.readFileSync(manifestPath, "utf8"));
    if (typeof manifest.version === "string" && manifest.version.length > 0) {
      return { version: manifest.version, name: manifest.name };
    }
  } catch {}
  return null;
}

// Parse the sentinel JSON file, returning the parsed object or null on any
// failure (missing file, IO error, malformed JSON, non-object value).
function readSentinelFile(sentinelPath) {
  if (typeof sentinelPath !== "string" || sentinelPath.length === 0) {
    return null;
  }
  let raw;
  try {
    raw = fs.readFileSync(sentinelPath, "utf8");
  } catch {
    return null;
  }
  try {
    const parsed = JSON.parse(raw);
    return parsed && typeof parsed === "object" ? parsed : null;
  } catch {
    return null;
  }
}

// True when the structural fields (schema, version, name, binaries map,
// install identity) all match. Mtime + bytes checks are in separate helpers
// to keep each function flat.
//
// `platformPkgDir` binds the sentinel to a specific install location. This
// matters when the sentinel lives in the shared $XDG fallback cache: two
// installs of the same package + version on the same host would otherwise
// share a sentinel keyed only by name/version, and an attacker who can write
// to one install's binary could ride the other install's sentinel state.
function sentinelStructureMatches(parsed, manifest, platformPkgDir) {
  if (!parsed) return false;
  if (parsed.schemaVersion !== SENTINEL_SCHEMA_VERSION) return false;
  if (parsed.packageVersion !== manifest.version) return false;
  if (parsed.packageName !== manifest.name) return false;
  if (parsed.platformPkgDir !== platformPkgDir) return false;
  return parsed.binaries && typeof parsed.binaries === "object";
}

// Compute a hex SHA-256 of a file's bytes. Used to bind the sentinel to the
// exact binary bytes that passed verification, so a tampered binary with the
// same mtime cannot ride a stale cache entry.
function sha256OfFile(absPath) {
  try {
    return crypto.createHash("sha256").update(fs.readFileSync(absPath)).digest("hex");
  } catch {
    return null;
  }
}

// True when every recorded binary still matches the on-disk state. The mtime
// check is a cheap pre-filter; the SHA-256 check is the load-bearing
// integrity gate that defends against same-mtime cross-install reuse where a
// tampered binary happens to land with the recorded mtime.
function sentinelBinariesMatch(parsed, platformPkgDir, platform) {
  for (const target of binaryTargetsForPlatform(platform)) {
    const recorded = parsed.binaries[target];
    if (!recorded || typeof recorded.mtimeMs !== "number") return false;
    if (typeof recorded.sha256 !== "string" || recorded.sha256.length !== 64) return false;
    const binaryPath = path.join(platformPkgDir, target);
    const current = statMtimeMs(binaryPath);
    if (current === null) return false;
    // 1ms tolerance covers NFS / FAT mtime rounding.
    if (Math.abs(current - recorded.mtimeMs) > 1) return false;
    const sha = sha256OfFile(binaryPath);
    if (sha !== recorded.sha256) return false;
  }
  return true;
}

// Read and validate the sentinel. Returns true when the sentinel is valid for
// the current platform package state (binary mtimes + bytes + version +
// package name + install identity all match). Any failure mode (missing file,
// malformed JSON, schema drift, stale mtime, mismatched bytes / name /
// version, install-dir mismatch) returns false, triggering re-verify.
function isSentinelValid(sentinelPath, platformPkgDir, manifest, platform) {
  const parsed = readSentinelFile(sentinelPath);
  if (!sentinelStructureMatches(parsed, manifest, platformPkgDir)) return false;
  return sentinelBinariesMatch(parsed, platformPkgDir, platform);
}

function buildSentinelPayload(platformPkgDir, manifest, platform) {
  const binaries = {};
  for (const target of binaryTargetsForPlatform(platform)) {
    const binaryPath = path.join(platformPkgDir, target);
    const mtimeMs = statMtimeMs(binaryPath);
    const sha256 = sha256OfFile(binaryPath);
    if (mtimeMs === null || sha256 === null) {
      return null;
    }
    binaries[target] = { mtimeMs, sha256 };
  }
  return {
    schemaVersion: SENTINEL_SCHEMA_VERSION,
    verifiedAt: new Date().toISOString(),
    packageVersion: manifest.version,
    packageName: manifest.name,
    platformPkgDir,
    binaries,
  };
}

// Write the sentinel via tmp + rename for atomicity. On POSIX the rename is
// atomic; on Windows the worst case is last-writer-wins under concurrency,
// and both writers produce structurally identical payloads (only `verifiedAt`
// differs, which is informational).
function writeSentinel(sentinelPath, payload) {
  const dir = path.dirname(sentinelPath);
  const tmpName = `${path.basename(sentinelPath)}.${process.pid}.${crypto.randomBytes(6).toString("hex")}.tmp`;
  const tmpPath = path.join(dir, tmpName);
  try {
    fs.writeFileSync(tmpPath, JSON.stringify(payload), { flag: "wx" });
    fs.renameSync(tmpPath, sentinelPath);
    return { ok: true };
  } catch (err) {
    // Best-effort cleanup; ignore unlink errors.
    try {
      fs.unlinkSync(tmpPath);
    } catch {}
    return { ok: false, code: err.code || "unknown", message: err.message };
  }
}

function isSkipRequested(env) {
  const v = (env || process.env)[SKIP_ENV];
  return v === "1" || v === "true" || v === "yes";
}

// Main entry point. Synchronous by design: bin/fallow runs this before
// execFileSync, so the verify result must be available without awaiting.
//
// Required input:
//   platformPkgDir: absolute path to the @fallow-cli/<platform> directory
//   packageName:    the platform package name (used as sentinel filename in
//                   the cache-dir fallback locations)
//   manifestPath:   absolute path to the platform package's package.json
//                   (read for version + name)
//
// Optional input (all dependency-injected for tests):
//   verifyFn       - replaces verifyBinaryAt (sig check) in verifyInstalledSync
//   digestProvider - sync function returning a sha256 digest string; used when
//                    fallowDigests is missing from the manifest (tests only;
//                    production install path always has fallowDigests)
//   env            - process.env (defaults to process.env)
//   platform       - process.platform (defaults to process.platform)
//   logger         - function (line: string) -> void (defaults to stderr)
//
// Returns one of:
//   { ok: true, cached: true, sentinelPath }
//   { ok: true, cached: false, sentinelPath: string|null }
//   { ok: true, skipped: true, reason }
//   { ok: false, code, message, binary?, package? }
function buildVerifyOptions(input, manifest) {
  const opts = {
    dirOverride: input.platformPkgDir,
    version: manifest.version,
    platformId: (input.packageName || "").replace(/^@fallow-cli\//, "") || "unknown",
  };
  if (typeof input.verifyFn === "function") opts.verifyFn = input.verifyFn;
  if (typeof input.digestProvider === "function") opts.digestProvider = input.digestProvider;
  return opts;
}

// Persist the sentinel on a successful verify. Logs (warn-once) when the
// resolved cache location is read-only or every cascade step failed.
function persistSentinel(sentinel, platformPkgDir, manifest, platform) {
  if (!sentinel.path) {
    warnOnce(
      "sentinel-no-writable-location",
      `no writable cache location for verify sentinel (platform pkg dir read-only, ` +
        `$FALLOW_VERIFY_CACHE_DIR unset, $XDG_CACHE_HOME / %LOCALAPPDATA% unavailable). ` +
        `Binary verification will re-run on every invocation. Set ${SKIP_ENV}=1 to ` +
        `bypass verification entirely.`,
    );
    return;
  }
  const payload = buildSentinelPayload(platformPkgDir, manifest, platform);
  if (!payload) return;
  const write = writeSentinel(sentinel.path, payload);
  if (write.ok) return;
  warnOnce(
    "sentinel-write-failed",
    `could not persist verify sentinel at ${sentinel.path} (${write.code}): ` +
      `verification will re-run on next invocation. Set FALLOW_VERIFY_CACHE_DIR ` +
      `to a writable location to enable caching.`,
  );
}

function ensureVerified(input) {
  const {
    platformPkgDir,
    packageName,
    manifestPath,
    env = process.env,
    platform = process.platform,
  } = input || {};

  if (isSkipRequested(env)) {
    const reason = `${SKIP_ENV} is set`;
    // Warn once per process so the bypass stays visible in CI logs and
    // vendor audits regardless of whether the user runs `--version` or
    // sets FALLOW_VERIFY_LOG. Documented in SECURITY.md.
    warnOnce(
      "skip-binary-verify-set",
      `${SKIP_ENV} is set; binary verification is skipped. ` +
        `Unset the variable to re-enable Ed25519 + SHA-256 verification. ` +
        `See SECURITY.md for the trust model.`,
    );
    emitVerifyLog(env, { outcome: "skipped", reason });
    return { ok: true, skipped: true, reason };
  }

  if (typeof platformPkgDir !== "string" || platformPkgDir.length === 0) {
    return { ok: false, code: "platform-package-missing", message: "platformPkgDir is required" };
  }

  const manifest = manifestPath ? readManifestVersion(manifestPath) : null;
  if (!manifest) {
    return {
      ok: false,
      code: "manifest-invalid",
      message: `cannot read platform package manifest at ${manifestPath}`,
    };
  }

  const sentinel = resolveSentinelPath({ platformPkgDir, packageName, env, platform });

  // Cache hit: sentinel exists, schema matches, mtimes match, version matches.
  if (sentinel.path && isSentinelValid(sentinel.path, platformPkgDir, manifest, platform)) {
    emitVerifyLog(env, { outcome: "ok", cache: "hit", sentinel: sentinel.path });
    return { ok: true, cached: true, sentinelPath: sentinel.path };
  }

  // Cache miss: run full sig + digest verification.
  const result = verifyInstalledSync(buildVerifyOptions(input || {}, manifest));

  if (!result.ok) {
    emitVerifyLog(env, {
      outcome: "fail",
      cache: "miss",
      code: result.code,
      binary: result.binary,
    });
    return { ...result, package: packageName };
  }

  persistSentinel(sentinel, platformPkgDir, manifest, platform);
  emitVerifyLog(env, { outcome: "ok", cache: "miss", sentinel: sentinel.path || "<none>" });
  return { ok: true, cached: false, sentinelPath: sentinel.path };
}

// Reset the warn-once memo. Test-only.
function _resetWarningState() {
  _warningEmitted.clear();
}

module.exports = {
  ensureVerified,
  SENTINEL_SCHEMA_VERSION,
  VERIFY_LOG_ENV,
  _resetWarningState,
};
