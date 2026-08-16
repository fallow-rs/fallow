import assert from "node:assert/strict";
import { accessSync, copyFileSync, mkdtempSync, readFileSync, realpathSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const PREACT_COMMIT = "055cc5b8c62326fbb0fcaccb9816504e82f121b8";
const COMMAND_TIMEOUT_MS = 180_000;
const SUCCESS_OR_FINDINGS_EXIT_CODES = [0, 1];
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const candidateBinary = process.env.FALLOW_CANDIDATE_BIN;
const npmCli = join(dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js");
const backendVersion = JSON.parse(
  readFileSync(join(repoRoot, "crates", "api", "type-aware-protocol.json"), "utf8"),
).backend.version;

if (process.platform !== "win32") throw new Error("Windows candidate smoke requires win32");
if (!candidateBinary) throw new Error("FALLOW_CANDIDATE_BIN is required");
accessSync(npmCli);

const run = (command, args, { acceptedExitCodes = [0], ...options } = {}) => {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    maxBuffer: 20 * 1024 * 1024,
    timeout: COMMAND_TIMEOUT_MS,
    ...options,
  });
  assert.ok(
    acceptedExitCodes.includes(result.status),
    `${command} ${args.join(" ")} failed\n${result.stdout ?? ""}\n${result.stderr ?? ""}`,
  );
  return result.stdout;
};

const runNpm = (args, options = {}) => run(process.execPath, [npmCli, ...args], options);

const temporaryRoot = realpathSync(mkdtempSync(join(tmpdir(), "fallow-windows-candidate-")));
try {
  const wrapperPack = JSON.parse(
    runNpm([
      "pack",
      join(repoRoot, "npm", "fallow"),
      "--pack-destination",
      temporaryRoot,
      "--json",
    ]),
  )[0].filename;
  const sidecarPack = JSON.parse(
    runNpm([
      "pack",
      join(repoRoot, "tools", "type-aware-sidecar"),
      "--pack-destination",
      temporaryRoot,
      "--json",
    ]),
  )[0].filename;
  runNpm([
    "install",
    "--prefix",
    temporaryRoot,
    "--ignore-scripts",
    "--no-audit",
    "--no-fund",
    join(temporaryRoot, wrapperPack),
    join(temporaryRoot, sidecarPack),
  ]);

  const installedFallow = join(temporaryRoot, "node_modules", "fallow");
  const require = createRequire(import.meta.url);
  const { getPlatformPackage } = require(join(installedFallow, "scripts", "platform-package.js"));
  const platformPackage = getPlatformPackage(process.platform, process.arch);
  assert.ok(platformPackage);
  const platformManifest = require.resolve(`${platformPackage}/package.json`, {
    paths: [installedFallow],
  });
  const installedBinary = join(dirname(platformManifest), "fallow.exe");
  copyFileSync(resolve(candidateBinary), installedBinary);
  const wrapper = join(installedFallow, "bin", "fallow");
  const environment = { ...process.env, FALLOW_SKIP_BINARY_VERIFY: "1" };
  const status = JSON.parse(
    run(process.execPath, [wrapper, "type-aware", "status", "--format", "json", "--quiet"], {
      env: environment,
    }),
  );
  assert.equal(status.available, true);
  assert.equal(status.discovery_source, "npm-wrapper");
  assert.equal(status.backend_version, backendVersion);

  const preactRoot = join(temporaryRoot, "preact");
  run("git", [
    "clone",
    "--depth",
    "1",
    "--branch",
    "10.25.4",
    "https://github.com/preactjs/preact.git",
    preactRoot,
  ]);
  assert.equal(run("git", ["-C", preactRoot, "rev-parse", "HEAD"]).trim(), PREACT_COMMIT);

  const result = JSON.parse(
    run(
      process.execPath,
      [
        wrapper,
        "dead-code",
        "--root",
        preactRoot,
        "--unused-exports",
        "--unused-types",
        "--type-aware",
        "--format",
        "json",
        "--quiet",
      ],
      { acceptedExitCodes: SUCCESS_OR_FINDINGS_EXIT_CODES, env: environment },
    ),
  );
  const metadata = result._meta?.type_aware ?? result._meta?.check?.type_aware;
  assert.equal(metadata?.executed, true);
  assert.equal(metadata?.backend_version, backendVersion);
  assert.ok(metadata.candidate_count > 0);
  assert.ok(metadata.projects.length > 0);
  process.stdout.write("Windows npm candidate completed the pinned Preact semantic smoke.\n");
} finally {
  rmSync(temporaryRoot, { recursive: true, force: true });
}
