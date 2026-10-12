export function releaseChannel(version) {
  if (version.includes("-")) {
    return "next";
  }
  if (version.startsWith("0.")) {
    return "beta";
  }
  return "latest";
}
