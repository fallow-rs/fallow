const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const { spawnSync } = require("node:child_process");
const path = require("node:path");

const RUN_BINARY = path.join(__dirname, "run-binary.js");
const BIN_DIR = path.join(__dirname, "..", "bin");

function currentPlatformPackage() {
  const { getPlatformPackage } = require("./platform-package");
  if (process.platform !== "linux") {
    return getPlatformPackage(process.platform, process.arch);
  }
  let libcFamily;
  try {
    libcFamily = require("detect-libc").familySync();
  } catch {
    libcFamily = undefined;
  }
  return getPlatformPackage(process.platform, process.arch, libcFamily);
}

// Install a fake platform package whose `fallow` binary is the given Node
// script. Returns the scratch directory and the environment for the launcher.
function installFakeBinary(t, script, pkg = currentPlatformPackage()) {
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "fallow-launcher-"));
  t.after(() => fs.rmSync(work, { recursive: true, force: true }));

  assert.ok(pkg, "the test host must map to a supported platform package");
  const pkgDir = path.join(work, "node_modules", ...pkg.split("/"));
  fs.mkdirSync(pkgDir, { recursive: true });
  fs.writeFileSync(
    path.join(pkgDir, "package.json"),
    JSON.stringify({ name: pkg, version: "3.5.0" }),
  );

  const binary = path.join(pkgDir, "fallow");
  fs.writeFileSync(binary, `#!/usr/bin/env node\n${script}`);
  fs.chmodSync(binary, 0o755);

  const env = {
    ...process.env,
    NODE_PATH: path.join(work, "node_modules"),
    FALLOW_SKIP_BINARY_VERIFY: "1",
  };
  return { work, env };
}

function runLauncher(t, launcher, args) {
  const { work, env } = installFakeBinary(
    t,
    'require("node:fs").writeFileSync(process.env.FALLOW_TEST_ARGS, JSON.stringify(process.argv.slice(2)));\n',
  );
  const argsFile = path.join(work, "args.json");
  const result = spawnSync(process.execPath, [path.join(BIN_DIR, launcher), ...args], {
    encoding: "utf8",
    env: { ...env, FALLOW_TEST_ARGS: argsFile },
  });
  assert.equal(result.status, 0, result.stderr);
  return JSON.parse(fs.readFileSync(argsFile, "utf8"));
}

// CI does not install the wrapper dependencies, so the launcher finds
// `detect-libc` only through NODE_PATH. A stub there pins the host to glibc.
// An installed real `detect-libc` takes precedence, so skip on a musl host.
function glibcTestPackage() {
  if (process.platform !== "linux") return null;
  try {
    if (require("detect-libc").familySync() !== "glibc") return null;
  } catch {
    // Not installed: the NODE_PATH stub decides.
  }
  const { getPlatformPackage } = require("./platform-package");
  return getPlatformPackage("linux", process.arch, "glibc");
}

function writeGlibcStub(nodeModules) {
  const dir = path.join(nodeModules, "detect-libc");
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, "index.js"), 'module.exports = { familySync: () => "glibc" };\n');
}

const GLIBC_PACKAGE = glibcTestPackage();

test(
  "launcher on a glibc host runs the musl package when the gnu package is missing",
  { skip: GLIBC_PACKAGE === null && "needs a Linux glibc host" },
  (t) => {
    const musl = GLIBC_PACKAGE.replace(/-gnu$/, "-musl");
    const { work, env } = installFakeBinary(t, "process.exit(0);\n", musl);
    writeGlibcStub(path.join(work, "node_modules"));
    const result = spawnSync(process.execPath, [path.join(BIN_DIR, "fallow"), "check"], {
      encoding: "utf8",
      env,
    });
    assert.equal(result.status, 0, result.stderr);
  },
);

// The fake binary writes a large payload the way the Rust binary does: plain
// write calls on fd 1, and it stops at the first write error but exits 0.
// A non-blocking inherited pipe makes a write fail with EAGAIN, so the reader
// gets a short payload (issue #3276).
const LARGE_OUTPUT_BYTES = 4 * 1024 * 1024;
const LARGE_OUTPUT_BINARY =
  'const fs = require("node:fs");\n' +
  `const total = ${LARGE_OUTPUT_BYTES};\n` +
  'const chunk = Buffer.alloc(64 * 1024, "x");\n' +
  "let written = 0;\n" +
  "try {\n" +
  "  while (written < total) written += fs.writeSync(1, chunk, 0, Math.min(chunk.length, total - written));\n" +
  "} catch {}\n";

function bunAvailable() {
  const probe = spawnSync("bun", ["--version"], { encoding: "utf8" });
  return probe.status === 0;
}

function runLargeOutput(t, runtime, { mergeStderr = false } = {}) {
  const { env } = installFakeBinary(t, LARGE_OUTPUT_BINARY);
  const launcher = path.join(BIN_DIR, "fallow");
  const [command, args] = mergeStderr
    ? ["/bin/sh", ["-c", 'exec "$0" "$@" 2>&1', runtime, launcher, "check"]]
    : [runtime, [launcher, "check"]];
  return spawnSync(command, args, { env, maxBuffer: 2 * LARGE_OUTPUT_BYTES });
}

// Count only the payload bytes. In the merged case, stdout also holds the
// FALLOW_SKIP_BINARY_VERIFY warning line.
function payloadBytes(stdout) {
  let count = 0;
  for (const byte of stdout) if (byte === 0x78) count += 1;
  return count;
}

for (const runtime of ["node", "bun"]) {
  for (const mergeStderr of [false, true]) {
    const name = mergeStderr ? "stdout and stderr on one pipe" : "stdout on its own pipe";
    test(
      `launcher under ${runtime} passes the complete binary output (${name})`,
      { skip: process.platform === "win32" || (runtime === "bun" && !bunAvailable()) },
      (t) => {
        const result = runLargeOutput(t, runtime === "node" ? process.execPath : "bun", {
          mergeStderr,
        });
        assert.equal(result.status, 0, String(result.stderr));
        assert.equal(payloadBytes(result.stdout), LARGE_OUTPUT_BYTES);
      },
    );
  }
}

// Run a child that installs guardBrokenStdout, then emits a synthetic stdout
// 'error' with the given code. Node delivers a broken-pipe failure as exactly
// this event ("Emitted 'error' event on Socket instance"), so emitting it is a
// faithful reproduction of `fallow --version | head` without needing a live
// pipe or an installed @fallow-cli platform package. Requiring run-binary.js
// has no side effects beyond defining functions, so no binary is resolved.
function runGuardChild(errorCode) {
  const script =
    `const { guardBrokenStdout } = require(${JSON.stringify(RUN_BINARY)});` +
    `guardBrokenStdout();` +
    `process.stdout.emit("error", Object.assign(new Error("write ${errorCode}"), { code: "${errorCode}" }));` +
    // Reached only if the guard neither exited (EPIPE) nor rethrew (other).
    `process.exit(42);`;
  return spawnSync(process.execPath, ["-e", script], { encoding: "utf8" });
}

test("guardBrokenStdout swallows EPIPE on stdout and exits 0", () => {
  const res = runGuardChild("EPIPE");
  assert.equal(res.status, 0, "EPIPE on stdout should exit 0 cleanly, not crash");
  assert.doesNotMatch(res.stderr, /EPIPE/, "no EPIPE stack trace on stderr");
});

test("guardBrokenStdout rethrows non-EPIPE stdout errors (exit 1)", () => {
  const res = runGuardChild("ENOSPC");
  assert.equal(res.status, 1, "a non-EPIPE stdout error must surface, not be swallowed");
  // Match the thrown error's header ("Error: write ENOSPC"), not just the
  // message substring: a missing-guard TypeError would leak the script source
  // (`new Error("write ENOSPC")`) into its code frame and match a looser regex,
  // masking a regression. The colon-space header only appears on a real rethrow.
  assert.match(res.stderr, /Error: write ENOSPC/, "the rethrown error reaches stderr");
  assert.doesNotMatch(res.stderr, /is not a function/, "guard must be present, not absent");
});

test("isVersionQuery recognizes --version, -V, and -v as the first argument", () => {
  const { isVersionQuery } = require(RUN_BINARY);
  assert.equal(isVersionQuery(["node", "fallow", "--version"]), true);
  assert.equal(isVersionQuery(["node", "fallow", "-V"]), true);
  assert.equal(
    isVersionQuery(["node", "fallow", "-v"]),
    true,
    "-v must append the verified line too",
  );
  assert.equal(isVersionQuery(["node", "fallow"]), false);
  assert.equal(isVersionQuery(["node", "fallow", "dead-code"]), false);
  assert.equal(
    isVersionQuery(["node", "fallow", "dead-code", "-v"]),
    false,
    "-v only counts as the first arg",
  );
});

test("describeVerified annotates the resolved version's signing status", () => {
  const { describeVerified } = require(RUN_BINARY);
  const ok = { ok: true, sentinelPath: "/c/s" };
  // Signed-era version: appended as `signed`.
  assert.match(
    describeVerified(ok, "2.83.0"),
    /verified: yes \(sentinel \/c\/s\); fallow 2\.83\.0 signed/,
  );
  // Pre-signing version: the fleet pre-flight signal, most useful on skip.
  const skipped = { skipped: true, reason: "FALLOW_SKIP_BINARY_VERIFY is set" };
  assert.match(
    describeVerified(skipped, "2.76.0"),
    /verified: skipped \(.*\); fallow 2\.76\.0 unsigned \(predates 2\.77\.0\)/,
  );
  // Unknown / unreadable version: no annotation, version line stays intact.
  assert.equal(describeVerified(ok, undefined), "verified: yes (sentinel /c/s)");
  assert.equal(describeVerified(ok, ""), "verified: yes (sentinel /c/s)");
});

test("exitCodeForChildFailure preserves status codes and maps signal deaths", () => {
  const { exitCodeForChildFailure } = require(RUN_BINARY);
  assert.equal(exitCodeForChildFailure({ status: 3 }), 3);
  assert.equal(exitCodeForChildFailure({ status: 0 }), 0);
  assert.equal(
    exitCodeForChildFailure({ status: null, signal: "SIGSEGV" }),
    128 + os.constants.signals.SIGSEGV,
  );
  assert.equal(
    exitCodeForChildFailure({ status: null, signal: "SIGKILL" }),
    128 + os.constants.signals.SIGKILL,
  );
  assert.equal(exitCodeForChildFailure({ status: null, signal: undefined }), 1);
  assert.equal(exitCodeForChildFailure({ status: null, signal: "NOT_A_SIGNAL" }), 1);
});

test("resolveTypeAwareCompanion accepts only an exact matching package", (t) => {
  const { resolveTypeAwareCompanion } = require(RUN_BINARY);
  const work = fs.mkdtempSync(path.join(os.tmpdir(), "fallow-type-aware-package-"));
  t.after(() => fs.rmSync(work, { recursive: true, force: true }));
  const manifestPath = path.join(work, "package.json");
  const companionPath = path.join(work, "fallow-type-aware.mjs");
  fs.writeFileSync(companionPath, "#!/usr/bin/env node\n");
  const resolvePackage = () => manifestPath;

  fs.writeFileSync(manifestPath, JSON.stringify({ version: "3.8.0" }));
  assert.equal(resolveTypeAwareCompanion("3.8.0", resolvePackage), companionPath);

  fs.writeFileSync(manifestPath, JSON.stringify({ version: "3.7.0" }));
  assert.equal(resolveTypeAwareCompanion("3.8.0", resolvePackage), undefined);
});

// `fallow` does not depend on `fallow-type-aware`. pnpm links a project
// devDependency only into the project node_modules, where the wrapper package
// cannot resolve it, so the launcher also resolves from the project root. It
// accepts that companion only when the running `fallow` package is in the same
// node_modules tree, so a global install never runs a project's companion.
function writeProjectCompanion(t, version) {
  const project = fs.mkdtempSync(path.join(os.tmpdir(), "fallow-type-aware-project-"));
  t.after(() => fs.rmSync(project, { recursive: true, force: true }));
  const nodeModules = path.join(project, "node_modules");
  const companionDir = path.join(
    nodeModules,
    ".pnpm",
    "fallow-type-aware@x",
    "node_modules",
    "fallow-type-aware",
  );
  fs.mkdirSync(companionDir, { recursive: true });
  fs.symlinkSync(companionDir, path.join(nodeModules, "fallow-type-aware"), "dir");
  fs.writeFileSync(path.join(companionDir, "fallow-type-aware.mjs"), "#!/usr/bin/env node\n");
  fs.writeFileSync(
    path.join(companionDir, "package.json"),
    JSON.stringify({ name: "fallow-type-aware", version }),
  );
  const fallowScripts = path.join(
    nodeModules,
    ".pnpm",
    "fallow@x",
    "node_modules",
    "fallow",
    "scripts",
  );
  fs.mkdirSync(fallowScripts, { recursive: true });
  return { project, companion: path.join(companionDir, "fallow-type-aware.mjs"), fallowScripts };
}

test("resolveTypeAwareCompanion finds an exact-version companion in the project node_modules", (t) => {
  const { resolveTypeAwareCompanion } = require(RUN_BINARY);
  const { project, companion, fallowScripts } = writeProjectCompanion(t, "3.8.0");
  assert.equal(
    fs.realpathSync(
      resolveTypeAwareCompanion("3.8.0", undefined, undefined, project, fallowScripts),
    ),
    fs.realpathSync(companion),
  );
  assert.equal(
    resolveTypeAwareCompanion("3.9.0", undefined, undefined, project, fallowScripts),
    undefined,
    "a companion of another version is rejected",
  );
});

test("resolveTypeAwareCompanion ignores a project companion for a fallow from another install", (t) => {
  const { resolveTypeAwareCompanion } = require(RUN_BINARY);
  const { project } = writeProjectCompanion(t, "3.8.0");
  const globalInstall = fs.mkdtempSync(path.join(os.tmpdir(), "fallow-global-"));
  t.after(() => fs.rmSync(globalInstall, { recursive: true, force: true }));
  const globalScripts = path.join(globalInstall, "lib", "node_modules", "fallow", "scripts");
  fs.mkdirSync(globalScripts, { recursive: true });
  assert.equal(
    resolveTypeAwareCompanion("3.8.0", undefined, undefined, project, globalScripts),
    undefined,
  );
});

test("childEnvironment marks launcher-wired companions as npm-wrapper", (t) => {
  const { childEnvironment } = require(RUN_BINARY);
  const previousBin = process.env.FALLOW_TYPE_AWARE_BIN;
  const previousScript = process.env.FALLOW_TYPE_AWARE_SCRIPT;
  delete process.env.FALLOW_TYPE_AWARE_BIN;
  delete process.env.FALLOW_TYPE_AWARE_SCRIPT;
  t.after(() => {
    if (previousBin === undefined) delete process.env.FALLOW_TYPE_AWARE_BIN;
    else process.env.FALLOW_TYPE_AWARE_BIN = previousBin;
    if (previousScript === undefined) delete process.env.FALLOW_TYPE_AWARE_SCRIPT;
    else process.env.FALLOW_TYPE_AWARE_SCRIPT = previousScript;
  });

  const env = childEnvironment("3.8.0", () => "/tmp/fallow-type-aware.mjs", {
    platform: "linux",
    execPath: "/usr/bin/node",
  });
  assert.equal(env.FALLOW_TYPE_AWARE_BIN, "/tmp/fallow-type-aware.mjs");
  assert.equal(env.FALLOW_TYPE_AWARE_SCRIPT, undefined);
  assert.equal(env.FALLOW_TYPE_AWARE_BIN_SOURCE, "npm-wrapper");

  const windowsEnv = childEnvironment("3.8.0", () => "C:\\pkg\\fallow-type-aware.mjs", {
    platform: "win32",
    execPath: "C:\\Program Files\\nodejs\\node.exe",
  });
  assert.equal(windowsEnv.FALLOW_TYPE_AWARE_BIN, "C:\\Program Files\\nodejs\\node.exe");
  assert.equal(windowsEnv.FALLOW_TYPE_AWARE_SCRIPT, "C:\\pkg\\fallow-type-aware.mjs");
  assert.equal(windowsEnv.FALLOW_TYPE_AWARE_BIN_SOURCE, "npm-wrapper");

  // No resolvable companion: the environment passes through untouched.
  const untouched = childEnvironment("3.8.0", () => undefined);
  assert.equal(untouched, process.env);
  assert.equal(untouched.FALLOW_TYPE_AWARE_BIN_SOURCE, undefined);
});

test("childEnvironment leaves a user-set override unmarked", (t) => {
  const { childEnvironment } = require(RUN_BINARY);
  const previousBin = process.env.FALLOW_TYPE_AWARE_BIN;
  const previousScript = process.env.FALLOW_TYPE_AWARE_SCRIPT;
  process.env.FALLOW_TYPE_AWARE_BIN = "/opt/custom-sidecar";
  process.env.FALLOW_TYPE_AWARE_SCRIPT = "/opt/custom-sidecar.mjs";
  t.after(() => {
    if (previousBin === undefined) delete process.env.FALLOW_TYPE_AWARE_BIN;
    else process.env.FALLOW_TYPE_AWARE_BIN = previousBin;
    if (previousScript === undefined) delete process.env.FALLOW_TYPE_AWARE_SCRIPT;
    else process.env.FALLOW_TYPE_AWARE_SCRIPT = previousScript;
  });

  const env = childEnvironment("3.8.0", () => "/tmp/fallow-type-aware.mjs");
  assert.equal(env, process.env);
  assert.equal(env.FALLOW_TYPE_AWARE_SCRIPT, "/opt/custom-sidecar.mjs");
  assert.equal(env.FALLOW_TYPE_AWARE_BIN_SOURCE, undefined);
});

test(
  "fallow-lsp executes the multicall binary with the lsp-server subcommand",
  { skip: process.platform === "win32" },
  (t) => {
    assert.deepEqual(runLauncher(t, "fallow-lsp", ["--stdio"]), ["lsp-server", "--stdio"]);
  },
);

test(
  "fallow-mcp executes the multicall binary with the mcp-server subcommand",
  { skip: process.platform === "win32" },
  (t) => {
    assert.deepEqual(runLauncher(t, "fallow-mcp", ["--transport", "stdio"]), [
      "mcp-server",
      "--transport",
      "stdio",
    ]);
  },
);

test("missingPlatformPackageMessage names the detected libc and the reinstall flag", () => {
  const { missingPlatformPackageMessage } = require(RUN_BINARY);
  const message = missingPlatformPackageMessage("@fallow-cli/linux-x64-musl", "musl");
  assert.match(message, /Could not find @fallow-cli\/linux-x64-musl\./);
  assert.match(message, /This host uses musl\./);
  assert.match(message, /--libc=musl/);
  assert.doesNotMatch(
    missingPlatformPackageMessage("@fallow-cli/darwin-arm64", undefined),
    /--libc/,
  );
});

// A pnpm-style project with a stand-in `fallow-similar-code` package. Its
// scripts follow the exports of npm/fallow-similar-code/scripts and report the
// given verification result.
function writeSimilarCodeCompanion(t, { version, verified }) {
  const project = fs.mkdtempSync(path.join(os.tmpdir(), "fallow-similar-code-project-"));
  t.after(() => fs.rmSync(project, { recursive: true, force: true }));
  const nodeModules = path.join(project, "node_modules");
  const packageDir = path.join(
    nodeModules,
    ".pnpm",
    "fsc@x",
    "node_modules",
    "fallow-similar-code",
  );
  fs.mkdirSync(path.join(packageDir, "scripts"), { recursive: true });
  fs.symlinkSync(packageDir, path.join(nodeModules, "fallow-similar-code"), "dir");
  fs.writeFileSync(
    path.join(packageDir, "package.json"),
    JSON.stringify({ name: "fallow-similar-code", version }),
  );
  const binaryPath = path.join(packageDir, "native", "fallow-similar-code");
  fs.writeFileSync(
    path.join(packageDir, "scripts", "run-binary.js"),
    `module.exports = {
  resolvePlatformPackage: () => "@fallow-cli/fallow-similar-code-test",
  resolveBinaryArtifact: () => ({ binaryPath: ${JSON.stringify(binaryPath)} }),
};\n`,
  );
  fs.writeFileSync(
    path.join(packageDir, "scripts", "verify-binary.js"),
    `module.exports = { verifyBinary: () => ({ ok: ${verified}, message: "stand-in" }) };\n`,
  );
  const fallowScripts = path.join(
    nodeModules,
    ".pnpm",
    "fallow@x",
    "node_modules",
    "fallow",
    "scripts",
  );
  fs.mkdirSync(fallowScripts, { recursive: true });
  return { project, binaryPath, fallowScripts };
}

test("resolveSimilarCodeProvider returns the verified binary of an exact-version companion", (t) => {
  const { resolveSimilarCodeProvider } = require(RUN_BINARY);
  const good = writeSimilarCodeCompanion(t, { version: "3.8.0", verified: true });
  const options = (fixture) => ({
    projectRoot: fixture.project,
    launcherDir: fixture.fallowScripts,
  });
  assert.equal(resolveSimilarCodeProvider("3.8.0", options(good)), good.binaryPath);
  assert.equal(resolveSimilarCodeProvider("3.9.0", options(good)), undefined, "another version");

  const unverified = writeSimilarCodeCompanion(t, { version: "3.8.0", verified: false });
  assert.equal(resolveSimilarCodeProvider("3.8.0", options(unverified)), undefined);
});

test("resolveSimilarCodeProvider ignores a project companion for a fallow from another install", (t) => {
  const { resolveSimilarCodeProvider } = require(RUN_BINARY);
  const { project } = writeSimilarCodeCompanion(t, { version: "3.8.0", verified: true });
  const globalInstall = fs.mkdtempSync(path.join(os.tmpdir(), "fallow-global-"));
  t.after(() => fs.rmSync(globalInstall, { recursive: true, force: true }));
  const launcherDir = path.join(globalInstall, "lib", "node_modules", "fallow", "scripts");
  fs.mkdirSync(launcherDir, { recursive: true });
  assert.equal(
    resolveSimilarCodeProvider("3.8.0", { projectRoot: project, launcherDir }),
    undefined,
  );
});

test("childEnvironment passes the similar-code provider only to commands that use it", (t) => {
  const { childEnvironment } = require(RUN_BINARY);
  const previous = process.env.FALLOW_SIMILAR_CODE_PROVIDER;
  delete process.env.FALLOW_SIMILAR_CODE_PROVIDER;
  t.after(() => {
    if (previous === undefined) delete process.env.FALLOW_SIMILAR_CODE_PROVIDER;
    else process.env.FALLOW_SIMILAR_CODE_PROVIDER = previous;
  });
  const noTypeAware = () => undefined;
  const resolveProvider = () => "/verified/fallow-similar-code";
  const env = (args) =>
    childEnvironment("3.8.0", noTypeAware, undefined, { args, resolveProvider })
      .FALLOW_SIMILAR_CODE_PROVIDER;

  assert.equal(env(["similar-code", "status"]), "/verified/fallow-similar-code");
  assert.equal(env(["mcp-server"]), "/verified/fallow-similar-code");
  assert.equal(env(["dead-code"]), undefined);

  process.env.FALLOW_SIMILAR_CODE_PROVIDER = "/user/choice";
  assert.equal(env(["similar-code"]), "/user/choice");
});
