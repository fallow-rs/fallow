import { strict as assert } from "node:assert";
import { spawn } from "node:child_process";
import { copyFile, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";

const TEST_DIR = dirname(fileURLToPath(import.meta.url));
const ACTION_DIR = resolve(TEST_DIR, "..");
const INSTALL_SCRIPT = process.env.INSTALL_SCRIPT_UNDER_TEST
  ? resolve(process.cwd(), process.env.INSTALL_SCRIPT_UNDER_TEST)
  : join(ACTION_DIR, "scripts", "install.sh");
const VERIFY_RUNNER = join(ACTION_DIR, "scripts", "verify-installed.mjs");
const HANG_VERIFY_TIMEOUT_MS = 50;
const NORMAL_VERIFY_TIMEOUT_MS = 5_000;
const HARD_WATCHDOG_MS = 8_000;
const MOCK_VERSION = "9.9.9";

const writeExecutable = async (path, contents) => {
  await writeFile(path, contents, { mode: 0o755 });
};

const createHarness = async (verifyModule) => {
  const root = await mkdtemp(join(tmpdir(), "fallow-action-verify-"));
  const actionRoot = join(root, "action-root");
  const binDir = join(root, "bin");
  const globalRoot = join(root, "global", "node_modules");
  const projectRoot = join(root, "project");
  const verifyScript = join(actionRoot, "npm", "fallow", "scripts", "verify-binary.js");
  const verifyRunner = join(actionRoot, "action", "scripts", "verify-installed.mjs");

  await Promise.all([
    mkdir(dirname(verifyScript), { recursive: true }),
    mkdir(dirname(verifyRunner), { recursive: true }),
    mkdir(join(globalRoot, "fallow"), { recursive: true }),
    mkdir(projectRoot, { recursive: true }),
    mkdir(binDir, { recursive: true }),
  ]);
  await Promise.all([
    writeFile(verifyScript, verifyModule),
    writeFile(
      join(globalRoot, "fallow", "package.json"),
      `${JSON.stringify({ name: "fallow", version: MOCK_VERSION })}\n`,
    ),
    writeExecutable(
      join(binDir, "npm"),
      `#!/bin/sh
if [ "$1" = "root" ] && [ "$2" = "-g" ]; then
  printf '%s\\n' "$MOCK_GLOBAL_ROOT"
  exit 0
fi
if [ "$1" = "install" ]; then
  exit 0
fi
printf 'unexpected npm invocation: %s\\n' "$*" >&2
exit 1
`,
    ),
    writeExecutable(
      join(binDir, "fallow"),
      `#!/bin/sh
if [ "$1" = "--version" ]; then
  printf 'fallow ${MOCK_VERSION}\\n'
  exit 0
fi
printf 'unexpected fallow invocation: %s\\n' "$*" >&2
exit 1
`,
    ),
  ]);

  // origin/main has no runner yet and ignores this path. The fixed install
  // script requires the production runner, so copy it when it exists.
  try {
    await copyFile(VERIFY_RUNNER, verifyRunner);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }

  return { actionRoot, binDir, globalRoot, projectRoot, root };
};

const killProcessGroup = (child) => {
  if (child.pid === undefined) return;
  try {
    process.kill(-child.pid, "SIGKILL");
  } catch {
    child.kill("SIGKILL");
  }
};

const runInstall = (harness, verifyTimeoutMs = NORMAL_VERIFY_TIMEOUT_MS) =>
  new Promise((resolveRun, rejectRun) => {
    const child = spawn("bash", [INSTALL_SCRIPT], {
      detached: true,
      env: {
        ...process.env,
        FALLOW_ACTION_VERIFY_TIMEOUT_MS: String(verifyTimeoutMs),
        FALLOW_VERSION: MOCK_VERSION,
        GITHUB_ACTION_PATH: harness.actionRoot,
        INPUT_ROOT: harness.projectRoot,
        INPUT_TYPE_AWARE: "false",
        MOCK_GLOBAL_ROOT: harness.globalRoot,
        NODE_ENV: "test",
        PATH: `${harness.binDir}:${process.env.PATH ?? ""}`,
      },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let stdout = "";
    let stderr = "";
    let watchdogFired = false;

    child.stdout.setEncoding("utf8");
    child.stderr.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
    });

    const watchdog = setTimeout(() => {
      watchdogFired = true;
      killProcessGroup(child);
    }, HARD_WATCHDOG_MS);

    child.once("error", (error) => {
      clearTimeout(watchdog);
      rejectRun(error);
    });
    child.once("close", (code, signal) => {
      clearTimeout(watchdog);
      resolveRun({ code, signal, stderr, stdout, watchdogFired });
    });
  });

const assertWatchdogDidNotFire = (result) => {
  assert.equal(
    result.watchdogFired,
    false,
    `install.sh exceeded its ${HARD_WATCHDOG_MS}ms test watchdog; the verifier may be unsupervised\n${result.stdout}\n${result.stderr}`,
  );
};

test("install.sh bounds a verifier that never settles", async (context) => {
  const harness = await createHarness(`
const SKIP_ENV = "FALLOW_SKIP_BINARY_VERIFY";
process.on("SIGTERM", () => {});
const verifyInstalled = () => new Promise(() => {
  setInterval(() => {}, 1_000);
});
module.exports = { SKIP_ENV, verifyInstalled };
`);
  context.after(() => rm(harness.root, { force: true, recursive: true }));

  const result = await runInstall(harness, HANG_VERIFY_TIMEOUT_MS);
  assertWatchdogDidNotFire(result);
  assert.equal(result.code, 124, `${result.stdout}\n${result.stderr}`);
  assert.match(
    `${result.stdout}\n${result.stderr}`,
    /::error::fallow binary verification timed out after 50ms; the verifier process was terminated/,
  );
});

test("install.sh keeps the successful verification path at status zero", async (context) => {
  const harness = await createHarness(`
const SKIP_ENV = "FALLOW_SKIP_BINARY_VERIFY";
const verifyInstalled = async () => ({
  ok: true,
  package: "fallow-test-binary",
  skipped: false,
  version: "${MOCK_VERSION}",
});
module.exports = { SKIP_ENV, verifyInstalled };
`);
  context.after(() => rm(harness.root, { force: true, recursive: true }));

  const result = await runInstall(harness);
  assertWatchdogDidNotFire(result);
  assert.equal(result.code, 0, `${result.stdout}\n${result.stderr}`);
  assert.match(result.stdout, /Verified Ed25519 signatures and SHA-256 digests on fallow binaries/);
  assert.match(result.stdout, /Installed fallow fallow 9\.9\.9/);
});

test("install.sh keeps the skipped verification path at status zero", async (context) => {
  const harness = await createHarness(`
const SKIP_ENV = "FALLOW_SKIP_BINARY_VERIFY";
const verifyInstalled = async () => ({ ok: true, skipped: true });
module.exports = { SKIP_ENV, verifyInstalled };
`);
  context.after(() => rm(harness.root, { force: true, recursive: true }));

  const result = await runInstall(harness);
  assertWatchdogDidNotFire(result);
  assert.equal(result.code, 0, `${result.stdout}\n${result.stderr}`);
  assert.match(result.stdout, /::warning::Binary verification skipped because/);
});

test("install.sh preserves a verifier failure", async (context) => {
  const harness = await createHarness(`
const SKIP_ENV = "FALLOW_SKIP_BINARY_VERIFY";
const verifyInstalled = async () => ({
  binary: "/tmp/fallow",
  code: "sig-invalid",
  message: "test signature mismatch",
  ok: false,
});
module.exports = { SKIP_ENV, verifyInstalled };
`);
  context.after(() => rm(harness.root, { force: true, recursive: true }));

  const result = await runInstall(harness);
  assertWatchdogDidNotFire(result);
  assert.equal(result.code, 1, `${result.stdout}\n${result.stderr}`);
  assert.match(
    `${result.stdout}\n${result.stderr}`,
    /::error::fallow binary verification failed \/tmp\/fallow \(sig-invalid\): test signature mismatch/,
  );
  assert.match(result.stdout, /Verification ran against fallow 9\.9\.9/);
});
