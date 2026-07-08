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

module.exports = {
  getPlatformPackage,
};
