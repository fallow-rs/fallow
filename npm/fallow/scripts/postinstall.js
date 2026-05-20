const { getPlatformPackage } = require('./platform-package');
const { verifyInstalled, SKIP_ENV } = require('./verify-binary');

const pkg = (() => {
  if (process.platform !== 'linux') {
    return getPlatformPackage(process.platform, process.arch);
  }

  try {
    const { familySync } = require('detect-libc');
    return getPlatformPackage(process.platform, process.arch, familySync());
  } catch {
    return getPlatformPackage(process.platform, process.arch);
  }
})();

if (!pkg) {
  console.warn(
    `fallow: No prebuilt binary for ${process.platform}-${process.arch}. ` +
    `You can build from source: https://github.com/fallow-rs/fallow`
  );
  process.exit(0);
}

try {
  require.resolve(`${pkg}/package.json`);
} catch {
  console.warn(
    `fallow: Platform package ${pkg} not installed. ` +
    `This may happen if you used --no-optional. ` +
    `Run 'npm install' to fix.`
  );
  // Without the platform package there is nothing to verify; keep the existing
  // soft-fail behavior for --no-optional installs.
  process.exit(0);
}

async function main() {
  const result = await verifyInstalled();
  if (result.skipped) {
    console.warn(
      `fallow: binary verification skipped (${SKIP_ENV}=${process.env[SKIP_ENV]}). ` +
      `Only set this when you are deliberately replacing the published binary. ` +
      `See https://github.com/fallow-rs/fallow/blob/main/SECURITY.md for details.`
    );
    process.exit(0);
  }
  if (!result.ok) {
    const where = result.binary ? ` ${result.binary}` : '';
    console.error(
      `fallow: binary verification failed${where} (${result.code}): ${result.message}. ` +
      `This usually means the published platform package was tampered with. ` +
      `See https://github.com/fallow-rs/fallow/blob/main/SECURITY.md for details. ` +
      `Set ${SKIP_ENV}=1 only if you are deliberately replacing the binary.`
    );
    process.exit(1);
  }
}

main().catch((err) => {
  console.error(`fallow: binary verification failed (internal-error): ${err.message}`);
  process.exit(1);
});
