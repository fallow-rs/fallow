// Shared launcher used by bin/fallow, bin/fallow-lsp, and bin/fallow-mcp.
//
// 1. Resolves the platform package for the current process (platform + arch + libc).
// 2. Runs ensureVerified (Ed25519 + SHA-256 lazy first-run verify).
// 3. Execs the platform binary.
// 4. For `<bin> --version`, appends a `verified: ...` status line to stdout
//    so procurement teams have a single command that surfaces the integrity
//    posture (replaces the install-time confirmation message removed when
//    postinstall verification was retired for RFC 868 readiness).

const { execFileSync } = require('node:child_process');
const path = require('node:path');
const fs = require('node:fs');

const { getPlatformPackage } = require('./platform-package');
const { ensureVerified } = require('./lazy-verify');

function resolvePlatformPackageName() {
  if (process.platform !== 'linux') {
    return getPlatformPackage(process.platform, process.arch);
  }
  try {
    const { familySync } = require('detect-libc');
    return getPlatformPackage(process.platform, process.arch, familySync());
  } catch {
    // musl binaries are statically linked and work on both glibc and musl
    return getPlatformPackage(process.platform, process.arch, 'musl');
  }
}

function isVersionQuery(argv) {
  // clap registers both --version and -V on the root command.
  const tail = argv.slice(2);
  if (tail.length === 0) return false;
  return tail[0] === '--version' || tail[0] === '-V';
}

function describeVerified(result) {
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
    return 'verified: yes (sentinel not persisted)';
  }
  return `verified: no (${result.code})`;
}

function runBinary(binaryBaseName) {
  const pkg = resolvePlatformPackageName();
  if (!pkg) {
    process.stderr.write(`Unsupported platform: ${process.platform}-${process.arch}\n`);
    process.exit(1);
  }

  let manifestPath;
  let platformPkgDir;
  try {
    manifestPath = require.resolve(`${pkg}/package.json`);
    platformPkgDir = path.dirname(manifestPath);
  } catch {
    process.stderr.write(
      `Could not find ${pkg}. Run 'npm install' to install the platform-specific binary.\n`,
    );
    process.exit(1);
  }

  const binaryName = process.platform === 'win32' ? `${binaryBaseName}.exe` : binaryBaseName;
  const binaryPath = path.join(platformPkgDir, binaryName);

  if (!fs.existsSync(binaryPath)) {
    process.stderr.write(`Binary not found at ${binaryPath}\n`);
    process.exit(1);
  }

  // Lazy first-run verify. Errors are user-facing.
  const verifyResult = ensureVerified({
    platformPkgDir,
    packageName: pkg,
    manifestPath,
  });

  if (!verifyResult.ok) {
    const where = verifyResult.binary ? ` ${verifyResult.binary}` : '';
    process.stderr.write(
      `fallow: binary verification failed${where} (${verifyResult.code}): ${verifyResult.message}\n` +
      `See https://github.com/fallow-rs/fallow/blob/main/SECURITY.md for the trust model. ` +
      `Set FALLOW_SKIP_BINARY_VERIFY=1 only when you deliberately replace the published binary.\n`,
    );
    process.exit(1);
  }

  try {
    execFileSync(binaryPath, process.argv.slice(2), { stdio: 'inherit' });
  } catch (e) {
    if (e.status !== undefined) {
      // Append the verified line only on a successful --version exit. The
      // child wrote its version line to stdout via the inherited handle;
      // adding a line here is safe because the child has already exited.
      if (e.status === 0 && isVersionQuery(process.argv)) {
        process.stdout.write(`${describeVerified(verifyResult)}\n`);
      }
      process.exit(e.status);
    }
    throw e;
  }

  // execFileSync only throws on non-zero exit. On success (exit 0) it falls
  // through here.
  if (isVersionQuery(process.argv)) {
    process.stdout.write(`${describeVerified(verifyResult)}\n`);
  }
}

module.exports = {
  runBinary,
  describeVerified, // test-only
  isVersionQuery,   // test-only
};
