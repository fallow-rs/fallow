const test = require("node:test");
const assert = require("node:assert/strict");

const { getPlatformPackage } = require("./platform-package");

test("maps Windows x64 and arm64 to MSVC packages", () => {
  assert.equal(getPlatformPackage("win32", "x64"), "@fallow-cli/win32-x64-msvc");
  assert.equal(getPlatformPackage("win32", "arm64"), "@fallow-cli/win32-arm64-msvc");
});

test("maps Linux packages with libc awareness", () => {
  assert.equal(getPlatformPackage("linux", "x64", "gnu"), "@fallow-cli/linux-x64-gnu");
  assert.equal(getPlatformPackage("linux", "arm64", "musl"), "@fallow-cli/linux-arm64-musl");
  assert.equal(getPlatformPackage("linux", "arm64"), "@fallow-cli/linux-arm64-gnu");
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
