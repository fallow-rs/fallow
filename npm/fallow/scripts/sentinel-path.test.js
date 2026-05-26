const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const {
  SENTINEL_FILENAME,
  packageIdToFilename,
  resolveSentinelPath,
  _isWritable,
} = require("./sentinel-path");

function mkTmp() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "fallow-sentinel-path-"));
}

function cleanup(dir) {
  fs.rmSync(dir, { recursive: true, force: true });
}

test("packageIdToFilename normalises scoped package names", () => {
  assert.equal(packageIdToFilename("@fallow-cli/darwin-arm64"), "fallow-cli__darwin-arm64.json");
  assert.equal(packageIdToFilename("@fallow-cli/linux-x64-gnu"), "fallow-cli__linux-x64-gnu.json");
  assert.equal(packageIdToFilename("fallow"), "fallow.json");
  assert.equal(packageIdToFilename(""), "unknown.json");
  assert.equal(packageIdToFilename(undefined), "unknown.json");
});

test("packageIdToFilename strips backslashes too (Windows scoped paths)", () => {
  assert.equal(packageIdToFilename("@scope\\name"), "scope__name.json");
});

test("isWritable returns true for a writable tmpdir", () => {
  const dir = mkTmp();
  try {
    assert.equal(_isWritable(dir), true);
  } finally {
    cleanup(dir);
  }
});

test("isWritable returns false for a non-existent path", () => {
  assert.equal(_isWritable("/this/path/does/not/exist/probably"), false);
  assert.equal(_isWritable(""), false);
  assert.equal(_isWritable(undefined), false);
});

test("isWritable returns false when the target is a file, not a dir", () => {
  const dir = mkTmp();
  try {
    const filePath = path.join(dir, "a-file");
    fs.writeFileSync(filePath, "x");
    assert.equal(_isWritable(filePath), false);
  } finally {
    cleanup(dir);
  }
});

test("resolveSentinelPath prefers the platform pkg dir when writable", () => {
  const platformPkgDir = mkTmp();
  try {
    const result = resolveSentinelPath({
      platformPkgDir,
      packageName: "@fallow-cli/darwin-arm64",
      env: {},
    });
    assert.equal(result.location, "platform-pkg");
    assert.equal(result.writable, true);
    assert.equal(result.path, path.join(platformPkgDir, SENTINEL_FILENAME));
  } finally {
    cleanup(platformPkgDir);
  }
});

test("resolveSentinelPath falls back to FALLOW_VERIFY_CACHE_DIR when platform pkg dir is non-writable", () => {
  const cacheDir = mkTmp();
  try {
    const result = resolveSentinelPath({
      platformPkgDir: "/dev/null/not-a-dir",
      packageName: "@fallow-cli/darwin-arm64",
      env: { FALLOW_VERIFY_CACHE_DIR: cacheDir },
    });
    assert.equal(result.location, "cache-dir-env");
    assert.equal(result.writable, true);
    assert.equal(result.path, path.join(cacheDir, "fallow-cli__darwin-arm64.json"));
  } finally {
    cleanup(cacheDir);
  }
});

test("resolveSentinelPath honors FALLOW_VERIFY_CACHE_DIR even when platform pkg dir IS writable", () => {
  // Per the cascade documented in the source, the platform pkg dir wins when
  // writable. The cache-dir env is the FALLBACK for when the platform dir is
  // read-only. We pass a non-existent platform dir to force the fallback.
  const cacheDir = mkTmp();
  try {
    const result = resolveSentinelPath({
      platformPkgDir: undefined,
      packageName: "@fallow-cli/linux-x64-gnu",
      env: { FALLOW_VERIFY_CACHE_DIR: cacheDir },
    });
    assert.equal(result.location, "cache-dir-env");
    assert.equal(result.path, path.join(cacheDir, "fallow-cli__linux-x64-gnu.json"));
  } finally {
    cleanup(cacheDir);
  }
});

test("resolveSentinelPath falls back to XDG_CACHE_HOME on Linux/macOS", () => {
  const homeDir = mkTmp();
  const xdg = path.join(homeDir, "xdg-cache");
  fs.mkdirSync(xdg);
  try {
    const result = resolveSentinelPath({
      platformPkgDir: undefined,
      packageName: "@fallow-cli/darwin-arm64",
      env: { XDG_CACHE_HOME: xdg },
      homedir: homeDir,
      platform: "darwin",
    });
    assert.equal(result.location, "xdg");
    assert.equal(result.writable, true);
    assert.equal(
      result.path,
      path.join(xdg, "fallow", "sentinels", "fallow-cli__darwin-arm64.json"),
    );
    // The fallow/sentinels/ subdir must have been created.
    assert.equal(fs.existsSync(path.join(xdg, "fallow", "sentinels")), true);
  } finally {
    cleanup(homeDir);
  }
});

test("resolveSentinelPath falls back to ~/.cache on Linux/macOS when XDG_CACHE_HOME is unset", () => {
  const homeDir = mkTmp();
  try {
    const result = resolveSentinelPath({
      platformPkgDir: undefined,
      packageName: "@fallow-cli/linux-x64-gnu",
      env: {},
      homedir: homeDir,
      platform: "linux",
    });
    assert.equal(result.location, "home-cache");
    assert.equal(
      result.path,
      path.join(homeDir, ".cache", "fallow", "sentinels", "fallow-cli__linux-x64-gnu.json"),
    );
  } finally {
    cleanup(homeDir);
  }
});

test("resolveSentinelPath uses LOCALAPPDATA on Windows", () => {
  const localAppData = mkTmp();
  try {
    const result = resolveSentinelPath({
      platformPkgDir: undefined,
      packageName: "@fallow-cli/win32-x64-msvc",
      env: { LOCALAPPDATA: localAppData },
      homedir: "/c/Users/test",
      platform: "win32",
    });
    assert.equal(result.location, "localappdata");
    assert.equal(result.writable, true);
    assert.equal(
      result.path,
      path.join(localAppData, "fallow", "sentinels", "fallow-cli__win32-x64-msvc.json"),
    );
  } finally {
    cleanup(localAppData);
  }
});

test("resolveSentinelPath returns no path when every location is non-writable", () => {
  const result = resolveSentinelPath({
    platformPkgDir: undefined,
    packageName: "@fallow-cli/darwin-arm64",
    env: {},
    homedir: undefined,
    platform: "darwin",
  });
  assert.equal(result.path, null);
  assert.equal(result.location, "none");
  assert.equal(result.writable, false);
});

test("resolveSentinelPath returns no path on Windows without LOCALAPPDATA", () => {
  const result = resolveSentinelPath({
    platformPkgDir: undefined,
    packageName: "@fallow-cli/win32-x64-msvc",
    env: {},
    homedir: undefined,
    platform: "win32",
  });
  assert.equal(result.path, null);
  assert.equal(result.location, "none");
});

test("resolveSentinelPath honors injected isWritable + ensureDir for full test isolation", () => {
  const calls = [];
  const result = resolveSentinelPath({
    platformPkgDir: "/synthetic/pkg-dir",
    packageName: "@fallow-cli/darwin-arm64",
    env: {},
    isWritable: (dir) => {
      calls.push(["isWritable", dir]);
      return dir === "/synthetic/pkg-dir";
    },
    ensureDir: (dir) => {
      calls.push(["ensureDir", dir]);
      return true;
    },
  });
  assert.equal(result.location, "platform-pkg");
  assert.deepEqual(calls, [["isWritable", "/synthetic/pkg-dir"]]);
});

test("resolveSentinelPath skips cache-dir-env when ensureDir fails for it", () => {
  // FALLOW_VERIFY_CACHE_DIR points at a non-creatable path, XDG points at a
  // creatable one. Confirm the resolver moves past the env override.
  const homeDir = mkTmp();
  try {
    const result = resolveSentinelPath({
      platformPkgDir: undefined,
      packageName: "@fallow-cli/darwin-arm64",
      env: { FALLOW_VERIFY_CACHE_DIR: "/dev/null/inside/a/file" },
      homedir: homeDir,
      platform: "darwin",
    });
    assert.equal(result.location, "home-cache");
  } finally {
    cleanup(homeDir);
  }
});
