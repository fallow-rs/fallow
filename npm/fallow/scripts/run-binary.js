// Shared launcher used by bin/fallow, bin/fallow-lsp, and bin/fallow-mcp.
//
// 1. Resolves the platform package for the current process (platform + arch + libc).
// 2. Runs ensureVerified (Ed25519 + SHA-256 lazy first-run verify).
// 3. Execs the platform binary.
// 4. For `<bin> --version`, appends a `verified: ...` status line to stdout
//    so procurement teams have a single command that surfaces the integrity
//    posture (replaces the install-time confirmation message removed when
//    postinstall verification was retired for RFC 868 readiness).

const { execFileSync } = require("node:child_process");
const path = require("node:path");
const fs = require("node:fs");
const os = require("node:os");

const { getPlatformPackage } = require("./platform-package");
const { ensureVerified } = require("./lazy-verify");
const { isPreSigningVersion } = require("./verify-binary");

function resolvePlatformPackageName() {
  if (process.platform !== "linux") {
    return getPlatformPackage(process.platform, process.arch);
  }
  try {
    const { familySync } = require("detect-libc");
    return getPlatformPackage(process.platform, process.arch, familySync());
  } catch {
    // musl binaries are statically linked and work on both glibc and musl
    return getPlatformPackage(process.platform, process.arch, "musl");
  }
}

function isVersionQuery(argv) {
  // The root command answers all three version flags (--version, -V, and the
  // TS/JS-toolchain-style -v), so the verified-status line must be appended for
  // every one of them, not just --version / -V.
  const tail = argv.slice(2);
  if (tail.length === 0) return false;
  return tail[0] === "--version" || tail[0] === "-V" || tail[0] === "-v";
}

// Signing status of the resolved CLI version, appended to the `verified:` line.
// Most informative on the `skipped` path: a fleet running with
// FALLOW_SKIP_BINARY_VERIFY can see whether the pinned version is even signable
// (pre-signing versions predate the 2.77.0 epoch and have no signature to
// verify). Best-effort: an unknown/unreadable version yields no annotation.
function describeSigning(version) {
  if (typeof version !== "string" || version.length === 0) {
    return "";
  }
  return isPreSigningVersion(version)
    ? `; fallow ${version} unsigned (predates 2.77.0)`
    : `; fallow ${version} signed`;
}

function describeVerified(result, version) {
  const status = describeVerifiedStatus(result);
  return `${status}${describeSigning(version)}`;
}

function describeVerifiedStatus(result) {
  if (result.skipped) {
    return `verified: skipped (${result.reason})`;
  }
  if (result.ok) {
    if (result.cached) {
      return `verified: yes (cache hit at ${result.sentinelPath})`;
    }
    if (result.sentinelPath) {
      return `verified: yes (sentinel ${result.sentinelPath})`;
    }
    return "verified: yes (sentinel not persisted)";
  }
  return `verified: no (${result.code})`;
}

// Resolve the platform package directory + manifest path, or print an
// actionable error and exit. Keeps `runBinary` a flat top-level sequence.
function resolvePlatformPaths() {
  const pkg = resolvePlatformPackageName();
  if (!pkg) {
    process.stderr.write(`Unsupported platform: ${process.platform}-${process.arch}\n`);
    process.exit(1);
  }
  try {
    const manifestPath = require.resolve(`${pkg}/package.json`);
    return { pkg, manifestPath, platformPkgDir: path.dirname(manifestPath) };
  } catch {
    process.stderr.write(
      `Could not find ${pkg}. Run 'npm install' to install the platform-specific binary.\n`,
    );
    process.exit(1);
  }
}

function printVerifyError(verifyResult) {
  const where = verifyResult.binary ? ` ${verifyResult.binary}` : "";
  process.stderr.write(
    `fallow: binary verification failed${where} (${verifyResult.code}): ${verifyResult.message}\n` +
      `See https://github.com/fallow-rs/fallow/blob/main/SECURITY.md for the trust model. ` +
      `Set FALLOW_SKIP_BINARY_VERIFY=1 only when you deliberately replace the published binary.\n`,
  );
}

function writeVerifiedLineIfVersionQuery(verifyResult, version) {
  if (isVersionQuery(process.argv)) {
    process.stdout.write(`${describeVerified(verifyResult, version)}\n`);
  }
}

// Exit code for a child failure caught from execFileSync. A signal death has
// status === null; map it to the shell convention 128 + signal number so CI
// gates see a crash, never a success.
function exitCodeForChildFailure(e) {
  if (e.status !== null) return e.status;
  const signalNumber = os.constants.signals[e.signal];
  return signalNumber ? 128 + signalNumber : 1;
}

// Read the resolved CLI version from the platform package manifest (its version
// is released in lockstep with the CLI). Best-effort: never throws, so a
// missing/garbled manifest just omits the signing annotation on --version.
function readResolvedVersion(manifestPath) {
  try {
    const version = JSON.parse(fs.readFileSync(manifestPath, "utf8")).version;
    return typeof version === "string" ? version : undefined;
  } catch {
    return undefined;
  }
}

// Swallow EPIPE on stdout. When fallow's output is piped into a reader that
// closes early (e.g. `fallow --version | head`), the trailing `verified:`
// status line would otherwise surface as an unhandled EPIPE 'error' event and
// dump a Node stack trace. EPIPE arrives as an async 'error' event on the
// stdout stream, not as a throw, so a try/catch around the write cannot catch
// it. The child binary's primary output is already written via inherited
// stdio; the status line is best-effort, so exit cleanly once the reader is
// gone. Scoped to stdout so a genuine error write to stderr still sets exit 1.
function guardBrokenStdout() {
  process.stdout.on("error", (err) => {
    if (err && err.code === "EPIPE") {
      process.exit(0);
    }
    throw err;
  });
}

function runBinary(binaryBaseName) {
  guardBrokenStdout();
  const { pkg, manifestPath, platformPkgDir } = resolvePlatformPaths();
  const resolvedVersion = readResolvedVersion(manifestPath);

  const binaryName = process.platform === "win32" ? `${binaryBaseName}.exe` : binaryBaseName;
  const binaryPath = path.join(platformPkgDir, binaryName);
  if (!fs.existsSync(binaryPath)) {
    process.stderr.write(`Binary not found at ${binaryPath}\n`);
    process.exit(1);
  }

  // Lazy first-run verify. Errors are user-facing.
  const verifyResult = ensureVerified({ platformPkgDir, packageName: pkg, manifestPath });
  if (!verifyResult.ok) {
    printVerifyError(verifyResult);
    process.exit(1);
  }

  try {
    execFileSync(binaryPath, process.argv.slice(2), { stdio: "inherit" });
  } catch (e) {
    if (e.status === undefined) throw e;
    if (e.status === null) {
      process.stderr.write(`fallow binary terminated by signal ${e.signal ?? "unknown"}\n`);
    }
    // Child has already written its --version line via inherited stdio;
    // append the verified line here only on a clean exit.
    if (e.status === 0) writeVerifiedLineIfVersionQuery(verifyResult, resolvedVersion);
    process.exit(exitCodeForChildFailure(e));
  }

  writeVerifiedLineIfVersionQuery(verifyResult, resolvedVersion);
}

module.exports = {
  runBinary,
  describeVerified, // test-only
  isVersionQuery, // test-only
  guardBrokenStdout, // test-only
  exitCodeForChildFailure, // test-only
};
