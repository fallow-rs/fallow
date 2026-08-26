"use strict";

const fs = require("node:fs");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");
const { once } = require("node:events");
const { getPlatformPackage, isPlatformPackage } = require("./platform-package");
const { verifyBinary } = require("./verify-binary");
const ownManifest = require("../package.json");

const SETUP_PROCESS_TIMEOUT_MS = 55 * 60 * 1000;
const SETUP_TERMINATE_GRACE_MS = 1000;
const SETUP_HARD_EXIT_GRACE_MS = 1000;
const SETUP_TIMEOUT_EXIT_CODE = 124;
const SETUP_TIMEOUT_MESSAGE =
  "local model setup timed out; check DNS, proxy, and network connectivity before retrying";
const SIGNAL_EXIT_CODES = new Map([
  ["SIGINT", 130],
  ["SIGTERM", 143],
]);

const resolvePlatformPackage = () => {
  if (process.platform !== "linux") {
    return getPlatformPackage(process.platform, process.arch);
  }
  try {
    const { familySync } = require("detect-libc");
    return getPlatformPackage(process.platform, process.arch, familySync());
  } catch {
    return getPlatformPackage(process.platform, process.arch, "musl");
  }
};

const resolveBinaryArtifact = (
  packageName,
  resolve = require.resolve,
  readFile = fs.readFileSync,
  stat = fs.lstatSync,
  platform = process.platform,
) => {
  if (!isPlatformPackage(packageName)) {
    throw new Error(`unsupported similar-code platform package: ${String(packageName)}`);
  }
  const manifestPath = resolve(`${packageName}/package.json`);
  const manifestMetadata = stat(manifestPath);
  if (!manifestMetadata.isFile() || manifestMetadata.isSymbolicLink()) {
    throw new Error(`platform manifest is not a regular file: ${manifestPath}`);
  }
  const manifest = JSON.parse(readFile(manifestPath, "utf8"));
  if (manifest.name !== packageName) {
    throw new Error(
      `platform package ownership mismatch, expected ${packageName} but manifest declares ${String(manifest.name)}`,
    );
  }
  if (manifest.version !== ownManifest.version) {
    throw new Error(
      `version mismatch: fallow-similar-code ${ownManifest.version} requires ${packageName} ${ownManifest.version}, found ${manifest.version}`,
    );
  }
  const binaryName = platform === "win32" ? "fallow-similar-code.exe" : "fallow-similar-code";
  const binaryPath = path.join(path.dirname(manifestPath), binaryName);
  const metadata = stat(binaryPath);
  if (!metadata.isFile() || metadata.isSymbolicLink()) {
    throw new Error(`native sidecar is not a regular file: ${binaryPath}`);
  }
  return Object.freeze({
    packageName,
    packageVersion: manifest.version,
    manifestPath,
    binaryName,
    binaryPath,
  });
};

const resolveBinary = (...args) => resolveBinaryArtifact(...args).binaryPath;

const setupTimeoutFor = (args) => (args[0] === "setup" ? SETUP_PROCESS_TIMEOUT_MS : null);

const spawnNative = (binaryPath, args, spawnProcess = spawnSync) => {
  const options = { env: process.env, stdio: "inherit" };
  const result = spawnProcess(binaryPath, args, options);
  if (result.error) {
    throw result.error;
  }
  return result;
};

const superviseSetup = async (
  binaryPath,
  args,
  {
    spawnProcess = spawn,
    timeoutMs = SETUP_PROCESS_TIMEOUT_MS,
    terminateGraceMs = SETUP_TERMINATE_GRACE_MS,
    hardExitGraceMs = SETUP_HARD_EXIT_GRACE_MS,
  } = {},
) => {
  const child = spawnProcess(binaryPath, args, {
    env: process.env,
    stdio: "inherit",
  });
  let forceTimer;
  let hardExitTimer;
  let forwardedSignal;
  let timedOut = false;
  let resolveHardExit;
  const hardExit = new Promise((resolve) => {
    resolveHardExit = resolve;
  });

  const terminate = (signal, status) => {
    if (child.exitCode !== null || child.signalCode !== null) return;
    child.kill(signal);
    if (forceTimer) return;
    forceTimer = setTimeout(() => {
      if (child.exitCode !== null || child.signalCode !== null) return;
      child.kill("SIGKILL");
      hardExitTimer = setTimeout(() => {
        child.unref();
        resolveHardExit([status, null]);
      }, hardExitGraceMs);
    }, terminateGraceMs);
  };

  const signalHandlers = new Map();
  for (const signal of SIGNAL_EXIT_CODES.keys()) {
    const handler = () => {
      if (child.exitCode !== null || child.signalCode !== null) return;
      forwardedSignal ??= signal;
      terminate(signal, SIGNAL_EXIT_CODES.get(forwardedSignal) ?? 1);
    };
    signalHandlers.set(signal, handler);
    process.on(signal, handler);
  }

  const timeout = setTimeout(() => {
    if (child.exitCode !== null || child.signalCode !== null) return;
    timedOut = true;
    terminate("SIGTERM", SETUP_TIMEOUT_EXIT_CODE);
  }, timeoutMs);

  try {
    const [status, signal] = await Promise.race([once(child, "exit"), hardExit]);
    if (timedOut) {
      return { status: SETUP_TIMEOUT_EXIT_CODE, signal: null, timedOut: true };
    }
    if (forwardedSignal) {
      return {
        status: SIGNAL_EXIT_CODES.get(forwardedSignal) ?? 1,
        signal: forwardedSignal,
        timedOut: false,
      };
    }
    return { status: status ?? 1, signal, timedOut: false };
  } finally {
    clearTimeout(timeout);
    if (forceTimer) clearTimeout(forceTimer);
    if (hardExitTimer) clearTimeout(hardExitTimer);
    for (const [signal, handler] of signalHandlers) process.removeListener(signal, handler);
  }
};

const run = async (args) => {
  try {
    const packageName = resolvePlatformPackage();
    const artifact = resolveBinaryArtifact(packageName);
    const verification = verifyBinary(artifact);
    if (!verification.ok) {
      throw new Error(`binary verification failed: ${verification.message}`);
    }
    const result =
      setupTimeoutFor(args) === null
        ? spawnNative(artifact.binaryPath, args)
        : await superviseSetup(artifact.binaryPath, args);
    if (result.timedOut) {
      throw new Error(SETUP_TIMEOUT_MESSAGE);
    }
    if (result.signal) {
      process.stderr.write(`fallow-similar-code terminated by signal ${result.signal}\n`);
      process.exit(1);
    }
    process.exit(result.status ?? 1);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`fallow-similar-code: ${message}\n`);
    process.exit(1);
  }
};

module.exports = {
  SETUP_PROCESS_TIMEOUT_MS,
  SETUP_TIMEOUT_EXIT_CODE,
  SETUP_TIMEOUT_MESSAGE,
  resolveBinary,
  resolveBinaryArtifact,
  resolvePlatformPackage,
  run,
  setupTimeoutFor,
  spawnNative,
  superviseSetup,
};
