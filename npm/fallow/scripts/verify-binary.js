// Ed25519 + SHA-256 binary verification for the fallow npm wrapper.
//
// Verifies each platform binary against a .sig file shipped alongside it in
// the @fallow-cli/<platform> package, then cross-checks the binary bytes
// against an expected SHA-256 digest. The .sig is produced at release time
// by `.github/scripts/sign-binary.mjs` using the workflow's
// ED25519_BINARY_SIGNING_PRIVATE_KEY secret. The matching public key (32 raw
// bytes) is embedded below and is identical to the value already trusted by
// the VS Code extension at editors/vscode/src/download.ts:19-22.
//
// SHA-256 digest source (in order of preference, refs #465 and #597):
//   1. `fallowDigests` field in the platform package's package.json, written
//      at release time by the `npm-prep` job. This is the steady-state path:
//      no network traffic, immune to GitHub API rate limits.
//   2. Fallback: GitHub Release asset digest via the public REST API. Kept
//      for backwards compatibility with platform packages published before
//      #597 that lack the embedded field. Shared-IP CI runners (Buildkite,
//      pooled GHA runners) can exceed the 60 req/hr unauthenticated limit;
//      that failure mode is what motivated #597.
//
// Triggered from scripts/postinstall.js and from the GitHub Action installer
// at action/scripts/install.sh. The escape hatch FALLOW_SKIP_BINARY_VERIFY=1
// is documented in SECURITY.md.
//
// No external dependencies: uses node:crypto and node:fs only.

const crypto = require('node:crypto');
const fs = require('node:fs');
const https = require('node:https');
const path = require('node:path');
const { getPlatformPackage } = require('./platform-package');

const GITHUB_REPO = 'fallow-rs/fallow';
const DIGEST_TIMEOUT_MS = 10000;

// 32-byte Ed25519 public key, identical to BINARY_SIGNING_PUBLIC_KEY in
// editors/vscode/src/download.ts:19-22 and to the ED25519_BINARY_SIGNING_PUBLIC_KEY
// repo variable on fallow-rs/fallow. Embedded rather than fetched so verification
// works offline and cannot be silently downgraded by tampering with the network
// path.
const EMBEDDED_PUBLIC_KEY = Buffer.from([
  131, 78, 111, 215, 115, 51, 230, 238, 223, 119, 147, 71, 199, 16, 172, 180, 3, 210, 216, 35,
  77, 85, 159, 94, 215, 200, 126, 85, 42, 222, 11, 209,
]);

// SPKI DER header for Ed25519 (RFC 8410). 12 bytes prepended to a 32-byte raw
// public key produces a complete SPKI structure that node:crypto.createPublicKey
// accepts directly.
const ED25519_SPKI_HEADER = Buffer.from([
  0x30, 0x2a, 0x30, 0x05, 0x06, 0x03, 0x2b, 0x65, 0x70, 0x03, 0x21, 0x00,
]);

const SKIP_ENV = 'FALLOW_SKIP_BINARY_VERIFY';

function buildPublicKey(rawPubKey) {
  if (!Buffer.isBuffer(rawPubKey) || rawPubKey.length !== 32) {
    throw new Error('expected 32-byte raw Ed25519 public key');
  }
  const spki = Buffer.concat([ED25519_SPKI_HEADER, rawPubKey]);
  return crypto.createPublicKey({ key: spki, format: 'der', type: 'spki' });
}

function _verifyWithKey(binaryPath, rawPubKey) {
  let binaryBytes;
  try {
    binaryBytes = fs.readFileSync(binaryPath);
  } catch (err) {
    if (err && err.code === 'ENOENT') {
      return { ok: false, code: 'binary-missing', message: `binary not found at ${binaryPath}` };
    }
    return { ok: false, code: 'read-error', message: `cannot read binary at ${binaryPath}: ${err.message}` };
  }

  const sigPath = `${binaryPath}.sig`;
  let signature;
  try {
    signature = fs.readFileSync(sigPath);
  } catch (err) {
    if (err && err.code === 'ENOENT') {
      return { ok: false, code: 'sig-missing', message: `signature not found at ${sigPath}` };
    }
    return { ok: false, code: 'read-error', message: `cannot read signature at ${sigPath}: ${err.message}` };
  }

  if (signature.length !== 64) {
    return { ok: false, code: 'sig-invalid', message: `signature at ${sigPath} has unexpected length ${signature.length} (want 64)` };
  }

  let publicKey;
  try {
    publicKey = buildPublicKey(rawPubKey);
  } catch (err) {
    return { ok: false, code: 'key-invalid', message: `cannot construct public key: ${err.message}` };
  }

  let valid;
  try {
    valid = crypto.verify(null, binaryBytes, publicKey, signature);
  } catch (err) {
    return { ok: false, code: 'sig-invalid', message: `crypto.verify threw: ${err.message}` };
  }
  if (!valid) {
    return { ok: false, code: 'sig-invalid', message: `Ed25519 verification failed for ${binaryPath}` };
  }
  return { ok: true };
}

function verifyBinaryAt(binaryPath) {
  return _verifyWithKey(binaryPath, EMBEDDED_PUBLIC_KEY);
}

function normalizeDigest(digest) {
  if (typeof digest !== 'string') {
    return null;
  }
  const lower = digest.trim().toLowerCase();
  const value = lower.startsWith('sha256:') ? lower.slice('sha256:'.length) : lower;
  return /^[0-9a-f]{64}$/.test(value) ? value : null;
}

function sha256Hex(binaryPath) {
  try {
    return { ok: true, digest: crypto.createHash('sha256').update(fs.readFileSync(binaryPath)).digest('hex') };
  } catch (err) {
    if (err && err.code === 'ENOENT') {
      return { ok: false, code: 'binary-missing', message: `binary not found at ${binaryPath}` };
    }
    return { ok: false, code: 'read-error', message: `cannot read binary at ${binaryPath}: ${err.message}` };
  }
}

function verifyDigestAt(binaryPath, expectedDigest) {
  const normalized = normalizeDigest(expectedDigest);
  if (!normalized) {
    return { ok: false, code: 'digest-invalid', message: `invalid SHA-256 digest '${expectedDigest}'` };
  }

  const actual = sha256Hex(binaryPath);
  if (!actual.ok) {
    return actual;
  }
  if (actual.digest !== normalized) {
    return {
      ok: false,
      code: 'digest-mismatch',
      message: `SHA-256 digest mismatch for ${binaryPath}: got ${actual.digest}, want ${normalized}`,
    };
  }
  return { ok: true };
}

function httpsJson(url, redirects = 0) {
  return new Promise((resolve, reject) => {
    const request = https.get(
      url,
      { headers: { 'User-Agent': 'fallow-binary-verifier' }, timeout: DIGEST_TIMEOUT_MS },
      (response) => {
        if (
          response.statusCode &&
          response.statusCode >= 300 &&
          response.statusCode < 400 &&
          response.headers.location &&
          redirects < 5
        ) {
          response.resume();
          httpsJson(response.headers.location, redirects + 1).then(resolve, reject);
          return;
        }

        const chunks = [];
        response.on('data', (chunk) => chunks.push(chunk));
        response.on('end', () => {
          const body = Buffer.concat(chunks).toString('utf8');
          if (!response.statusCode || response.statusCode >= 400) {
            reject(new Error(`GitHub release API returned HTTP ${response.statusCode || 'unknown'}: ${body.slice(0, 200)}`));
            return;
          }
          try {
            resolve(JSON.parse(body));
          } catch (err) {
            reject(new Error(`GitHub release API returned invalid JSON: ${err.message}`));
          }
        });
      },
    );
    request.on('timeout', () => request.destroy(new Error(`timed out after ${DIGEST_TIMEOUT_MS}ms`)));
    request.on('error', reject);
  });
}

const releaseDigestCache = new Map();

async function fetchReleaseDigest(version, assetName) {
  const key = version;
  let release = releaseDigestCache.get(key);
  if (!release) {
    const url = `https://api.github.com/repos/${GITHUB_REPO}/releases/tags/v${version}`;
    release = await httpsJson(url);
    releaseDigestCache.set(key, release);
  }
  const asset = Array.isArray(release.assets)
    ? release.assets.find((candidate) => candidate && candidate.name === assetName)
    : null;
  if (!asset) {
    throw new Error(`release v${version} does not contain asset ${assetName}`);
  }
  const digest = normalizeDigest(asset.digest);
  if (!digest) {
    throw new Error(`release asset ${assetName} is missing a valid SHA-256 digest`);
  }
  return digest;
}

function platformPackageDir(pkg, resolveFrom) {
  // require.resolve('<pkg>/package.json') is reliable across npm, pnpm, yarn,
  // bun. It returns the absolute path to the package's package.json; the
  // binaries sit next to it.
  const options = resolveFrom ? { paths: [resolveFrom] } : undefined;
  const manifestPath = require.resolve(`${pkg}/package.json`, options);
  return { dir: path.dirname(manifestPath), manifestPath };
}

// Read the SHA-256 digest for a binary embedded in the platform package's
// package.json (written by the npm-prep job at release time as
// `fallowDigests[<filename>]`). Returns the normalized digest hex or null
// when the manifest does not exist, cannot be parsed, lacks the field, or
// the value is malformed. Refs #597.
function readEmbeddedDigest(manifestPath, binaryFileName) {
  if (typeof manifestPath !== 'string' || manifestPath.length === 0) {
    return null;
  }
  let manifest;
  try {
    manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  } catch {
    return null;
  }
  if (!manifest || typeof manifest !== 'object') {
    return null;
  }
  const digests = manifest.fallowDigests;
  if (!digests || typeof digests !== 'object') {
    return null;
  }
  return normalizeDigest(digests[binaryFileName]);
}

function binaryTargetsForPlatform(platformId) {
  const isWindows = process.platform === 'win32';
  const ext = isWindows ? '.exe' : '';
  return [
    { binary: `fallow${ext}`, asset: `fallow-${platformId}${ext}` },
    { binary: `fallow-lsp${ext}`, asset: `fallow-lsp-${platformId}${ext}` },
    { binary: `fallow-mcp${ext}`, asset: `fallow-mcp-${platformId}${ext}` },
  ];
}

function isSkipRequested() {
  const v = process.env[SKIP_ENV];
  return v === '1' || v === 'true' || v === 'yes';
}

// Locate the platform package the wrapper would use at runtime and verify
// each of its three binaries. Returns the same result shape as
// verifyBinaryAt, with `binary` populated on failure so callers can produce
// a useful error.
//
// options:
//   allowSkipEnv  - if false, ignore FALLOW_SKIP_BINARY_VERIFY. Default true.
//   dirOverride   - absolute path to a directory containing the binaries.
//                   Skips platform-package resolution entirely. Test-only
//                   knob; production call sites must not pass it.
//   verifyFn      - function (binaryPath) -> result. Replaces verifyBinaryAt
//                   for tests that need to inject a non-production key.
//   digestProvider - function ({ assetName, binaryPath, packageName, version })
//                   -> sha256 digest. Replaces GitHub Release API lookup in tests.
//   resolveFrom    - module resolution base for locating platform packages.
//                   The GitHub Action passes the global npm root so verifier
//                   code from the action checkout does not trust installed code.
async function verifyInstalled(options) {
  const opts = options || {};
  const skipEnvAllowed = opts.allowSkipEnv !== false;
  if (skipEnvAllowed && isSkipRequested()) {
    return { ok: true, skipped: true, reason: `${SKIP_ENV} is set` };
  }

  const verify = typeof opts.verifyFn === 'function' ? opts.verifyFn : verifyBinaryAt;
  const digestProvider = typeof opts.digestProvider === 'function'
    ? opts.digestProvider
    : ({ assetName, version }) => fetchReleaseDigest(version, assetName);

  let dir;
  let manifestPath;
  let pkg;
  let version;
  let platformId;
  if (typeof opts.dirOverride === 'string' && opts.dirOverride.length > 0) {
    dir = opts.dirOverride;
    pkg = '<override>';
    version = opts.version || '0.0.0';
    platformId = opts.platformId || 'test-platform';
    // Lets tests drop a package.json next to the binaries to exercise the
    // embedded-digest path. readEmbeddedDigest returns null when missing.
    manifestPath = path.join(dir, 'package.json');
  } else {
    if (process.platform !== 'linux') {
      pkg = getPlatformPackage(process.platform, process.arch);
    } else {
      let libcFamily;
      try {
        libcFamily = require('detect-libc').familySync();
      } catch {
        // detect-libc is a dependency, but tolerate its absence the same way
        // postinstall.js already does.
        libcFamily = undefined;
      }
      pkg = getPlatformPackage(process.platform, process.arch, libcFamily);
    }

    if (!pkg) {
      return { ok: false, code: 'platform-unsupported', message: `no prebuilt binary for ${process.platform}-${process.arch}` };
    }

    try {
      ({ dir, manifestPath } = platformPackageDir(pkg, opts.resolveFrom));
    } catch (err) {
      return { ok: false, code: 'platform-package-missing', message: `platform package ${pkg} not installed: ${err.message}`, package: pkg };
    }

    platformId = pkg.replace(/^@fallow-cli\//, '');
    try {
      const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
      version = manifest.version;
    } catch (err) {
      return { ok: false, code: 'manifest-invalid', message: `cannot read platform package manifest for ${pkg}: ${err.message}`, package: pkg };
    }
    if (typeof version !== 'string' || !version.trim()) {
      return { ok: false, code: 'manifest-invalid', message: `platform package ${pkg} does not declare a version`, package: pkg };
    }
  }

  for (const target of binaryTargetsForPlatform(platformId)) {
    const binaryPath = path.join(dir, target.binary);
    const result = verify(binaryPath);
    if (!result.ok) {
      return { ...result, binary: binaryPath, package: pkg };
    }
    // Prefer the digest embedded in the platform package's package.json
    // (written at release time by `npm-prep`). Falling back to the GitHub
    // release API only when no embedded digest is present preserves
    // backwards compatibility with platform packages published before #597.
    // The shared-IP CI rate-limit failure mode that motivated #597 only
    // affects the fallback path, so the steady-state install path is now
    // fully offline.
    let expectedDigest = manifestPath
      ? readEmbeddedDigest(manifestPath, target.binary)
      : null;
    if (!expectedDigest) {
      try {
        expectedDigest = await digestProvider({
          assetName: target.asset,
          binaryPath,
          packageName: pkg,
          version,
        });
      } catch (err) {
        return {
          ok: false,
          code: 'digest-unavailable',
          message: `cannot load SHA-256 digest for ${target.asset}: ${err.message}`,
          binary: binaryPath,
          package: pkg,
        };
      }
    }
    const digestResult = verifyDigestAt(binaryPath, expectedDigest);
    if (!digestResult.ok) {
      return { ...digestResult, binary: binaryPath, package: pkg };
    }
  }
  return { ok: true, package: pkg, version };
}

module.exports = {
  verifyBinaryAt,
  verifyDigestAt,
  verifyInstalled,
  _verifyWithKey,
  fetchReleaseDigest,
  normalizeDigest,
  readEmbeddedDigest,
  sha256Hex,
  EMBEDDED_PUBLIC_KEY,
  ED25519_SPKI_HEADER,
  SKIP_ENV,
};
