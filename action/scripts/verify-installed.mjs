#!/usr/bin/env node

import { spawn } from "node:child_process";
import { once } from "node:events";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const VERIFY_CHILD_ARG = "--verify-child";
const VERIFY_TIMEOUT_MS = 60_000;
const TERMINATE_GRACE_MS = 1_000;
const HARD_EXIT_GRACE_MS = 1_000;
const TIMEOUT_EXIT_CODE = 124;
const SIGNAL_EXIT_CODES = new Map([
  ["SIGINT", 130],
  ["SIGTERM", 143],
]);

const cleanMessage = (value) => String(value).replace(/[\r\n]+/g, " ");

const runVerification = async () => {
  const require = createRequire(import.meta.url);
  const { verifyInstalled, SKIP_ENV } = require(process.env.ACTION_VERIFY_SCRIPT);
  const result = await verifyInstalled({ resolveFrom: process.env.FALLOW_VERIFY_RESOLVE_FROM });

  if (result.skipped) {
    console.log(
      `::warning::Binary verification skipped because ${SKIP_ENV} is set. Only use this when deliberately replacing the published binary.`,
    );
    return 0;
  }
  if (!result.ok) {
    const where = result.binary ? ` ${result.binary}` : "";
    console.error(
      `::error::fallow binary verification failed${where} (${result.code}): ${cleanMessage(result.message)}`,
    );
    return 1;
  }

  console.log(
    `Verified Ed25519 signatures and SHA-256 digests on fallow binaries (package ${result.package}@${result.version})`,
  );
  return 0;
};

const configuredTimeoutMs = () => {
  if (process.env.NODE_ENV !== "test") return VERIFY_TIMEOUT_MS;

  const override = Number(process.env.FALLOW_ACTION_VERIFY_TIMEOUT_MS);
  if (Number.isSafeInteger(override) && override > 0) return Math.min(override, VERIFY_TIMEOUT_MS);
  return VERIFY_TIMEOUT_MS;
};

const superviseVerification = async () => {
  const entryPath = fileURLToPath(import.meta.url);
  const timeoutMs = configuredTimeoutMs();
  const child = spawn(process.execPath, [entryPath, VERIFY_CHILD_ARG], {
    env: process.env,
    stdio: ["ignore", "inherit", "inherit"],
  });
  let forceTimer;
  let hardExitTimer;
  let forwardedSignal;
  let timedOut = false;
  let resolveHardExit;
  const hardExit = new Promise((resolve) => {
    resolveHardExit = resolve;
  });

  const terminate = (signal, exitCode) => {
    if (child.exitCode !== null || child.signalCode !== null) return;
    child.kill(signal);
    if (forceTimer) return;
    forceTimer = setTimeout(() => {
      if (child.exitCode !== null || child.signalCode !== null) return;
      child.kill("SIGKILL");
      hardExitTimer = setTimeout(() => {
        child.unref();
        resolveHardExit([exitCode, null]);
      }, HARD_EXIT_GRACE_MS);
    }, TERMINATE_GRACE_MS);
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
    console.error(
      `::error::fallow binary verification timed out after ${timeoutMs}ms; the verifier process was terminated`,
    );
    terminate("SIGTERM", TIMEOUT_EXIT_CODE);
  }, timeoutMs);

  try {
    const [code, signal] = await Promise.race([once(child, "exit"), hardExit]);
    if (timedOut) return TIMEOUT_EXIT_CODE;
    if (forwardedSignal) return SIGNAL_EXIT_CODES.get(forwardedSignal) ?? 1;
    if (signal) {
      console.error(
        `::error::fallow binary verification stopped unexpectedly (${cleanMessage(signal)})`,
      );
      return 1;
    }
    return code ?? 1;
  } catch (error) {
    console.error(
      `::error::fallow binary verification failed to start (internal-error): ${cleanMessage(error.message)}`,
    );
    return 1;
  } finally {
    clearTimeout(timeout);
    if (forceTimer) clearTimeout(forceTimer);
    if (hardExitTimer) clearTimeout(hardExitTimer);
    for (const [signal, handler] of signalHandlers) process.removeListener(signal, handler);
  }
};

const main = async () => {
  if (process.argv[2] === VERIFY_CHILD_ARG) return runVerification();
  return superviseVerification();
};

try {
  process.exitCode = await main();
} catch (error) {
  console.error(
    `::error::fallow binary verification failed (internal-error): ${cleanMessage(error.message)}`,
  );
  process.exitCode = 1;
}
