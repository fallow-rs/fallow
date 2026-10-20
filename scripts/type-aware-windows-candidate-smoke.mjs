import assert from "node:assert/strict";
import {
  accessSync,
  copyFileSync,
  mkdtempSync,
  readFileSync,
  realpathSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const PREACT_COMMIT = "055cc5b8c62326fbb0fcaccb9816504e82f121b8";
const COMMAND_TIMEOUT_MS = 180_000;
const SUCCESS_OR_FINDINGS_EXIT_CODES = [0, 1];
const repoRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const require = createRequire(import.meta.url);

/** Throws when the host cannot run the Windows candidate smoke. */
export const assertSmokePreconditions = ({ platform, candidateBinary, platformPackage }) => {
  if (platform !== "win32") throw new Error("Windows candidate smoke requires win32");
  if (!candidateBinary) throw new Error("FALLOW_CANDIDATE_BIN is required");
  assert.ok(platformPackage);
};

/** Throws with the command output when the exit status is not accepted. */
export const assertCommandSucceeded = (command, args, result, acceptedExitCodes = [0]) => {
  assert.ok(
    acceptedExitCodes.includes(result.status),
    `${command} ${args.join(" ")} failed\n${result.stdout ?? ""}\n${result.stderr ?? ""}`,
  );
  return result.stdout;
};

/** Returns the tarball file name from `npm pack --json` output. */
export const packedFilename = (stdout) => JSON.parse(stdout)[0].filename;

/** Returns a copy of the platform manifest at the release version. */
export const stageReleaseVersion = (manifest, releaseVersion) => ({
  ...manifest,
  version: releaseVersion,
});

/** Throws when `type-aware status` does not report the npm-wrapper backend. */
export const assertStatusReport = (status, backendVersion) => {
  assert.equal(status.available, true);
  assert.equal(status.discovery_source, "npm-wrapper");
  assert.equal(status.backend_version, backendVersion);
};

/** Returns the type-aware metadata from a combined or a standalone report. */
export const typeAwareMetadata = (result) =>
  result._meta?.type_aware ?? result._meta?.check?.type_aware;

/** Throws when the dead-code report shows no completed semantic pass. */
export const assertSemanticReport = (result, backendVersion) => {
  const metadata = typeAwareMetadata(result);
  assert.equal(metadata?.executed, true);
  assert.equal(metadata?.backend_version, backendVersion);
  assert.ok(metadata.candidate_count > 0);
  assert.ok(metadata.projects.length > 0);
};

const run = (command, args, { acceptedExitCodes = [0], ...options } = {}) => {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    maxBuffer: 20 * 1024 * 1024,
    timeout: COMMAND_TIMEOUT_MS,
    ...options,
  });
  return assertCommandSucceeded(command, args, result, acceptedExitCodes);
};

const main = () => {
  const candidateBinary = process.env.FALLOW_CANDIDATE_BIN;
  const npmCli = join(dirname(process.execPath), "node_modules", "npm", "bin", "npm-cli.js");
  const { getPlatformPackage } = require(
    join(repoRoot, "npm", "fallow", "scripts", "platform-package.js"),
  );
  const platformPackage = getPlatformPackage(process.platform, process.arch);
  const releaseVersion = JSON.parse(
    readFileSync(join(repoRoot, "npm", "fallow", "package.json"), "utf8"),
  ).version;
  const backendVersion = JSON.parse(
    readFileSync(join(repoRoot, "crates", "api", "type-aware-protocol.json"), "utf8"),
  ).backends[0].version;

  assertSmokePreconditions({ platform: process.platform, candidateBinary, platformPackage });
  accessSync(npmCli);

  const runNpm = (args, options = {}) => run(process.execPath, [npmCli, ...args], options);

  const temporaryRoot = realpathSync(mkdtempSync(join(tmpdir(), "fallow-windows-candidate-")));
  try {
    const wrapperPack = packedFilename(
      runNpm([
        "pack",
        join(repoRoot, "npm", "fallow"),
        "--pack-destination",
        temporaryRoot,
        "--json",
      ]),
    );
    const sidecarPack = packedFilename(
      runNpm([
        "pack",
        join(repoRoot, "tools", "type-aware-sidecar"),
        "--pack-destination",
        temporaryRoot,
        "--json",
      ]),
    );
    const platformPack = packedFilename(
      runNpm([
        "pack",
        join(repoRoot, "npm", platformPackage.replace("@fallow-cli/", "")),
        "--pack-destination",
        temporaryRoot,
        "--json",
      ]),
    );
    runNpm([
      "install",
      "--prefix",
      temporaryRoot,
      "--ignore-scripts",
      "--no-audit",
      "--no-fund",
      join(temporaryRoot, wrapperPack),
      join(temporaryRoot, sidecarPack),
      join(temporaryRoot, platformPack),
    ]);

    const installedFallow = join(temporaryRoot, "node_modules", "fallow");
    const platformManifest = require.resolve(`${platformPackage}/package.json`, {
      paths: [installedFallow],
    });
    const installedPlatformPackage = JSON.parse(readFileSync(platformManifest, "utf8"));
    // Committed platform manifests intentionally stay at the last published
    // version until release npm-prep stages the new packages. Mirror that
    // staging here so the wrapper evaluates the candidate at the release version.
    writeFileSync(
      platformManifest,
      `${JSON.stringify(stageReleaseVersion(installedPlatformPackage, releaseVersion), null, 2)}\n`,
    );
    const installedBinary = join(dirname(platformManifest), "fallow.exe");
    copyFileSync(resolve(candidateBinary), installedBinary);
    const wrapper = join(installedFallow, "bin", "fallow");
    const environment = { ...process.env, FALLOW_SKIP_BINARY_VERIFY: "1" };
    const status = JSON.parse(
      run(process.execPath, [wrapper, "type-aware", "status", "--format", "json", "--quiet"], {
        env: environment,
      }),
    );
    assertStatusReport(status, backendVersion);

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
    assertSemanticReport(result, backendVersion);
    process.stdout.write("Windows npm candidate completed the pinned Preact semantic smoke.\n");
  } finally {
    rmSync(temporaryRoot, { recursive: true, force: true });
  }
};

// `process.argv[1]` keeps symlinks while Node realpaths the ESM entry, so
// compare real paths. An unresolvable value means this module was imported.
const realPathOrNull = (value) => {
  try {
    return value === undefined ? null : realpathSync(value);
  } catch {
    return null;
  }
};

if (realPathOrNull(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url))) main();
