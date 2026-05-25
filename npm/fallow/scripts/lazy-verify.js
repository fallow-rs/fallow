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

const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');

const { resolveSentinelPath, SENTINEL_FILENAME } = require('./sentinel-path');
const { verifyInstalledSync, SKIP_ENV } = require('./verify-binary');

const SENTINEL_SCHEMA_VERSION = 1;
const VERIFY_LOG_ENV = 'FALLOW_VERIFY_LOG';

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
  if (typeof v !== 'string') return false;
  const lower = v.trim().toLowerCase();
  return lower === '1' || lower === 'true' || lower === 'yes';
}

function emitVerifyLog(env, payload) {
  if (!isVerifyLogEnabled(env)) return;
  // Stable single-line format. Fields are space-separated key=value pairs.
  // Order matters for log-grep ergonomics; do not reorder casually.
  const parts = [];
  for (const key of ['outcome', 'cache', 'sentinel', 'reason', 'code', 'binary']) {
    if (payload[key] !== undefined && payload[key] !== null) {
      const v = String(payload[key]).replace(/[\s"]/g, '_');
      parts.push(`${key}=${v}`);
    }
  }
  process.stderr.write(`fallow-verify ${parts.join(' ')}\n`);
}

function binaryTargetsForPlatform(platform) {
  const ext = platform === 'win32' ? '.exe' : '';
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
    const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
    if (typeof manifest.version === 'string' && manifest.version.length > 0) {
      return { version: manifest.version, name: manifest.name };
    }
  } catch {}
  return null;
}

// Read and validate the sentinel. Returns true when the sentinel is valid for
// the current platform package state (binary mtimes + version + package name
// all match). Any failure mode (missing file, malformed JSON, schema drift,
// stale mtime, mismatched name/version) returns false, triggering re-verify.
function isSentinelValid(sentinelPath, platformPkgDir, manifest, platform) {
  if (typeof sentinelPath !== 'string' || sentinelPath.length === 0) {
    return false;
  }
  let raw;
  try {
    raw = fs.readFileSync(sentinelPath, 'utf8');
  } catch {
    return false;
  }
  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return false;
  }
  if (!parsed || typeof parsed !== 'object') return false;
  if (parsed.schemaVersion !== SENTINEL_SCHEMA_VERSION) return false;
  if (parsed.packageVersion !== manifest.version) return false;
  if (parsed.packageName !== manifest.name) return false;
  if (!parsed.binaries || typeof parsed.binaries !== 'object') return false;

  const targets = binaryTargetsForPlatform(platform);
  for (const target of targets) {
    const recorded = parsed.binaries[target];
    if (!recorded || typeof recorded.mtimeMs !== 'number') return false;
    const current = statMtimeMs(path.join(platformPkgDir, target));
    if (current === null) return false;
    // Allow a tiny rounding tolerance: some filesystems (NFS, FAT) round mtimes
    // to whole seconds, so within 1ms is treated as equal.
    if (Math.abs(current - recorded.mtimeMs) > 1) return false;
  }
  return true;
}

function buildSentinelPayload(platformPkgDir, manifest, platform) {
  const binaries = {};
  for (const target of binaryTargetsForPlatform(platform)) {
    const mtimeMs = statMtimeMs(path.join(platformPkgDir, target));
    if (mtimeMs === null) {
      return null;
    }
    binaries[target] = { mtimeMs };
  }
  return {
    schemaVersion: SENTINEL_SCHEMA_VERSION,
    verifiedAt: new Date().toISOString(),
    packageVersion: manifest.version,
    packageName: manifest.name,
    binaries,
  };
}

// Write the sentinel via tmp + rename for atomicity. On POSIX the rename is
// atomic; on Windows the worst case is last-writer-wins under concurrency,
// and both writers produce structurally identical payloads (only `verifiedAt`
// differs, which is informational).
function writeSentinel(sentinelPath, payload) {
  const dir = path.dirname(sentinelPath);
  const tmpName = `${path.basename(sentinelPath)}.${process.pid}.${crypto.randomBytes(6).toString('hex')}.tmp`;
  const tmpPath = path.join(dir, tmpName);
  try {
    fs.writeFileSync(tmpPath, JSON.stringify(payload), { flag: 'wx' });
    fs.renameSync(tmpPath, sentinelPath);
    return { ok: true };
  } catch (err) {
    // Best-effort cleanup; ignore unlink errors.
    try { fs.unlinkSync(tmpPath); } catch {}
    return { ok: false, code: err.code || 'unknown', message: err.message };
  }
}

function isSkipRequested(env) {
  const v = (env || process.env)[SKIP_ENV];
  return v === '1' || v === 'true' || v === 'yes';
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
function ensureVerified(input) {
  const {
    platformPkgDir,
    packageName,
    manifestPath,
    verifyFn,
    digestProvider,
    env = process.env,
    platform = process.platform,
  } = input || {};

  if (isSkipRequested(env)) {
    const reason = `${SKIP_ENV} is set`;
    emitVerifyLog(env, { outcome: 'skipped', reason });
    return { ok: true, skipped: true, reason };
  }

  if (typeof platformPkgDir !== 'string' || platformPkgDir.length === 0) {
    return { ok: false, code: 'platform-package-missing', message: 'platformPkgDir is required' };
  }

  const manifest = manifestPath ? readManifestVersion(manifestPath) : null;
  if (!manifest) {
    return {
      ok: false,
      code: 'manifest-invalid',
      message: `cannot read platform package manifest at ${manifestPath}`,
    };
  }

  const sentinel = resolveSentinelPath({
    platformPkgDir,
    packageName,
    env,
    platform,
  });

  // Cache hit: sentinel exists, schema matches, mtimes match, version matches.
  if (sentinel.path && isSentinelValid(sentinel.path, platformPkgDir, manifest, platform)) {
    emitVerifyLog(env, { outcome: 'ok', cache: 'hit', sentinel: sentinel.path });
    return { ok: true, cached: true, sentinelPath: sentinel.path };
  }

  // Cache miss: run full sig + digest verification.
  const verifyOptions = {
    dirOverride: platformPkgDir,
    version: manifest.version,
    platformId: (packageName || '').replace(/^@fallow-cli\//, '') || 'unknown',
  };
  if (typeof verifyFn === 'function') verifyOptions.verifyFn = verifyFn;
  if (typeof digestProvider === 'function') verifyOptions.digestProvider = digestProvider;

  const result = verifyInstalledSync(verifyOptions);

  if (!result.ok) {
    emitVerifyLog(env, {
      outcome: 'fail',
      cache: 'miss',
      code: result.code,
      binary: result.binary,
    });
    return { ...result, package: packageName };
  }

  // Verify passed. Persist the sentinel if we have a writable location.
  if (sentinel.path) {
    const payload = buildSentinelPayload(platformPkgDir, manifest, platform);
    if (payload) {
      const write = writeSentinel(sentinel.path, payload);
      if (!write.ok) {
        warnOnce(
          'sentinel-write-failed',
          `could not persist verify sentinel at ${sentinel.path} (${write.code}): ` +
          `verification will re-run on next invocation. Set FALLOW_VERIFY_CACHE_DIR ` +
          `to a writable location to enable caching.`,
        );
      }
    }
  } else {
    warnOnce(
      'sentinel-no-writable-location',
      `no writable cache location for verify sentinel (platform pkg dir read-only, ` +
      `$FALLOW_VERIFY_CACHE_DIR unset, $XDG_CACHE_HOME / %LOCALAPPDATA% unavailable). ` +
      `Binary verification will re-run on every invocation. Set ${SKIP_ENV}=1 to ` +
      `bypass verification entirely.`,
    );
  }

  emitVerifyLog(env, {
    outcome: 'ok',
    cache: 'miss',
    sentinel: sentinel.path || '<none>',
  });
  return { ok: true, cached: false, sentinelPath: sentinel.path };
}

// Reset the warn-once memo. Test-only.
function _resetWarningState() {
  _warningEmitted.clear();
}

module.exports = {
  ensureVerified,
  SENTINEL_SCHEMA_VERSION,
  SENTINEL_FILENAME,
  VERIFY_LOG_ENV,
  _resetWarningState,
};
