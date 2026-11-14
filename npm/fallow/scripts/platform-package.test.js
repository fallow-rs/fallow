const test = require("node:test");
const assert = require("node:assert/strict");

const {
  getPlatformPackage,
  getPlatformPackageCandidates,
  resolveInstalledPlatformPackage,
} = require("./platform-package");

test("maps Windows x64 and arm64 to MSVC packages", () => {
  assert.equal(getPlatformPackage("win32", "x64"), "@fallow-cli/win32-x64-msvc");
  assert.equal(getPlatformPackage("win32", "arm64"), "@fallow-cli/win32-arm64-msvc");
});

test("maps Linux packages with libc awareness", () => {
  assert.equal(getPlatformPackage("linux", "x64", "glibc"), "@fallow-cli/linux-x64-gnu");
  assert.equal(getPlatformPackage("linux", "x64", "musl"), "@fallow-cli/linux-x64-musl");
  assert.equal(getPlatformPackage("linux", "x64", null), "@fallow-cli/linux-x64-musl");
});

test("maps macOS packages by architecture", () => {
  assert.equal(getPlatformPackage("darwin", "x64"), "@fallow-cli/darwin-x64");
  assert.equal(getPlatformPackage("darwin", "arm64"), "@fallow-cli/darwin-arm64");
});

test("returns null for unsupported targets", () => {
  assert.equal(getPlatformPackage("win32", "ia32"), null);
  assert.equal(getPlatformPackage("linux", "ppc64"), null);
  assert.equal(getPlatformPackage("freebsd", "x64"), null);
});

test("Linux platform manifests declare libc so npm installs only the matching package", () => {
  const path = require("node:path");
  const npmRoot = path.join(__dirname, "..", "..");
  const { optionalDependencies } = require("../package.json");
  const linuxPackages = Object.keys(optionalDependencies).filter((name) =>
    name.startsWith("@fallow-cli/linux-"),
  );
  assert.ok(linuxPackages.length > 0);
  for (const name of linuxPackages) {
    const dir = name.slice("@fallow-cli/".length);
    const manifest = require(path.join(npmRoot, dir, "package.json"));
    const expected = dir.endsWith("-gnu") ? ["glibc"] : ["musl"];
    assert.deepEqual(manifest.libc, expected, `${name} must declare libc ${expected}`);
  }
});

test("a glibc host falls back to the static musl package, never the reverse", () => {
  assert.deepEqual(getPlatformPackageCandidates("linux", "x64", "glibc"), [
    "@fallow-cli/linux-x64-gnu",
    "@fallow-cli/linux-x64-musl",
  ]);
  assert.deepEqual(getPlatformPackageCandidates("linux", "arm64", "musl"), [
    "@fallow-cli/linux-arm64-musl",
  ]);
  assert.deepEqual(getPlatformPackageCandidates("darwin", "arm64"), ["@fallow-cli/darwin-arm64"]);
  assert.deepEqual(getPlatformPackageCandidates("freebsd", "x64"), []);
});

test("resolveInstalledPlatformPackage returns the first installed candidate", () => {
  const installed = new Set(["@fallow-cli/linux-x64-musl/package.json"]);
  const resolve = (request) => {
    if (!installed.has(request)) throw new Error(`Cannot find module '${request}'`);
    return `/nm/${request}`;
  };
  const candidates = ["@fallow-cli/linux-x64-gnu", "@fallow-cli/linux-x64-musl"];
  assert.deepEqual(resolveInstalledPlatformPackage(candidates, resolve), {
    pkg: "@fallow-cli/linux-x64-musl",
    manifestPath: "/nm/@fallow-cli/linux-x64-musl/package.json",
  });
  installed.clear();
  assert.equal(resolveInstalledPlatformPackage(candidates, resolve), null);
});

test("main platform packages ship only the fallow binary and its signature", () => {
  const path = require("node:path");
  const npmRoot = path.join(__dirname, "..", "..");
  const { optionalDependencies } = require("../package.json");
  const platformPackages = Object.keys(optionalDependencies).filter((name) =>
    name.startsWith("@fallow-cli/"),
  );
  assert.ok(platformPackages.length > 0);
  for (const name of platformPackages) {
    const dir = name.slice("@fallow-cli/".length);
    const manifest = require(path.join(npmRoot, dir, "package.json"));
    const ext = dir.startsWith("win32-") ? ".exe" : "";
    assert.deepEqual(manifest.files, [`fallow${ext}`, `fallow${ext}.sig`], name);
  }
});
