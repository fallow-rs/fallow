function getPlatformPackage(platform, arch, libcFamily) {
  if (platform === "win32") {
    if (arch === "x64") return "@fallow-cli/win32-x64-msvc";
    if (arch === "arm64") return "@fallow-cli/win32-arm64-msvc";
    return null;
  }

  if (platform === "darwin") {
    if (arch === "x64" || arch === "arm64") {
      return `@fallow-cli/darwin-${arch}`;
    }
    return null;
  }

  if (platform === "linux") {
    const libc = libcFamily === "glibc" ? "gnu" : "musl";
    if (arch === "x64" || arch === "arm64") {
      return `@fallow-cli/linux-${arch}-${libc}`;
    }
    return null;
  }

  return null;
}

// The platform packages declare `libc`, so npm installs only the package for
// the libc that it detects at install time. The musl binaries are statically
// linked and also run on glibc, so a glibc host accepts an installed musl
// package (for example a node_modules tree that was built on Alpine). A glibc
// binary does not run on musl, so there is no fallback in that direction.
function getPlatformPackageCandidates(platform, arch, libcFamily) {
  const primary = getPlatformPackage(platform, arch, libcFamily);
  if (primary === null) return [];
  if (primary.endsWith("-gnu")) return [primary, primary.replace(/-gnu$/, "-musl")];
  return [primary];
}

function resolveInstalledPlatformPackage(candidates, resolve = require.resolve) {
  for (const pkg of candidates) {
    try {
      return { pkg, manifestPath: resolve(`${pkg}/package.json`) };
    } catch {
      // Not installed; try the next candidate.
    }
  }
  return null;
}

module.exports = {
  getPlatformPackage,
  getPlatformPackageCandidates,
  resolveInstalledPlatformPackage,
};
