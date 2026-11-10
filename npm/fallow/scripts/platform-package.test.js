const test = require("node:test");
const assert = require("node:assert/strict");

const { getPlatformPackage } = require("./platform-package");

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
