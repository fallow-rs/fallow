import { spawn, spawnSync } from "node:child_process";

export const AGENT_DEADLINE_MS = 5 * 60_000;
export const FALLOW_DEADLINE_MS = 2 * 60_000;
export const STDOUT_LIMIT_BYTES = 64 * 1024 * 1024;
export const STDERR_LIMIT_BYTES = 1024 * 1024;
const TERMINATION_GRACE_MS = 1_000;

export interface ProcessRunOptions {
  command: string;
  args: string[];
  cwd: string;
  input: string;
  deadlineMs: number;
  stdoutLimitBytes: number;
  stderrLimitBytes: number;
  terminationGraceMs?: number;
}

export interface ProcessRunResult {
  stdout: string;
  stderr: string;
}

export type ProcessRunErrorKind =
  | "spawn"
  | "exit"
  | "timeout"
  | "stdout-limit"
  | "stderr-limit"
  | "cancelled";

/** Typed process failure with bounded output retained for user-facing diagnostics. */
export class ProcessRunError extends Error {
  readonly kind: ProcessRunErrorKind;
  readonly code: string | number | null;
  readonly stdout: string;
  readonly stderr: string;

  constructor(
    kind: ProcessRunErrorKind,
    message: string,
    code: string | number | null,
    stdout: string,
    stderr: string,
    cause?: unknown,
  ) {
    super(message, cause === undefined ? undefined : { cause });
    this.name = "ProcessRunError";
    this.kind = kind;
    this.code = code;
    this.stdout = stdout;
    this.stderr = stderr;
  }
}

export interface ProcessTreeOperations {
  kill: (pid: number, signal: NodeJS.Signals) => void;
  taskkill: (pid: number) => void;
}

const defaultProcessTreeOperations: ProcessTreeOperations = {
  kill: (pid, signal) => process.kill(pid, signal),
  taskkill: (pid) => {
    const result = spawnSync("taskkill", ["/pid", String(pid), "/t", "/f"], {
      stdio: "ignore",
      windowsHide: true,
    });
    if (result.error) throw result.error;
  },
};

/** Terminates a detached POSIX process group or a complete Windows process tree. */
export const terminateProcessTree = (
  pid: number,
  signal: NodeJS.Signals,
  platform: NodeJS.Platform = process.platform,
  operations: ProcessTreeOperations = defaultProcessTreeOperations,
): void => {
  if (platform === "win32") {
    operations.taskkill(pid);
    return;
  }
  operations.kill(-pid, signal);
};

interface ActiveProcess {
  cancel: () => void;
}

const activeProcesses = new Set<ActiveProcess>();

/** Cancels every child still owned by the Electron main process. */
export const cancelActiveProcesses = (): void => {
  for (const active of activeProcesses) active.cancel();
};

/** Returns active child count for lifecycle assertions and diagnostics. */
export const getActiveProcessCount = (): number => activeProcesses.size;

interface BoundedChunks {
  chunks: Buffer[];
  bytes: number;
}

const appendBounded = (output: BoundedChunks, chunk: Buffer, limit: number): boolean => {
  const remaining = Math.max(0, limit - output.bytes);
  if (remaining > 0) output.chunks.push(chunk.subarray(0, remaining));
  output.bytes += Math.min(chunk.length, remaining);
  return chunk.length > remaining;
};

const asString = (output: BoundedChunks): string => Buffer.concat(output.chunks).toString("utf8");

/** Runs a child with bounded output, deadline enforcement, and process-tree cleanup. */
export const runProcess = (options: ProcessRunOptions): Promise<ProcessRunResult> =>
  new Promise((resolve, reject) => {
    const child = spawn(options.command, options.args, {
      cwd: options.cwd,
      detached: process.platform !== "win32",
      stdio: "pipe",
      windowsHide: true,
    });
    const stdout: BoundedChunks = { chunks: [], bytes: 0 };
    const stderr: BoundedChunks = { chunks: [], bytes: 0 };
    const graceMs = options.terminationGraceMs ?? TERMINATION_GRACE_MS;
    let settled = false;
    let terminationError: ProcessRunError | null = null;
    let forceTimer: NodeJS.Timeout | undefined;

    const output = (): ProcessRunResult => ({ stdout: asString(stdout), stderr: asString(stderr) });
    const cleanup = (): void => {
      clearTimeout(deadlineTimer);
      if (forceTimer) clearTimeout(forceTimer);
      activeProcesses.delete(active);
    };
    const settle = (action: () => void): void => {
      if (settled) return;
      settled = true;
      cleanup();
      action();
    };
    const signalTree = (signal: NodeJS.Signals): void => {
      if (child.pid === undefined) return;
      try {
        terminateProcessTree(child.pid, signal);
      } catch {
        try {
          child.kill(signal);
        } catch {
          /* the process already exited */
        }
      }
    };
    const beginTermination = (error: ProcessRunError): void => {
      if (settled || terminationError) return;
      terminationError = error;
      child.stdin.end();
      signalTree("SIGTERM");
      forceTimer = setTimeout(() => signalTree("SIGKILL"), graceMs);
      forceTimer.unref();
    };
    const errorFor = (
      kind: ProcessRunErrorKind,
      message: string,
      code: string | number | null = null,
      cause?: unknown,
    ): ProcessRunError => {
      const current = output();
      return new ProcessRunError(kind, message, code, current.stdout, current.stderr, cause);
    };
    const active: ActiveProcess = {
      cancel: () => beginTermination(errorFor("cancelled", "process cancelled")),
    };
    const deadlineTimer = setTimeout(
      () =>
        beginTermination(errorFor("timeout", `process timed out after ${options.deadlineMs}ms`)),
      options.deadlineMs,
    );
    deadlineTimer.unref();
    activeProcesses.add(active);

    child.stdin.on("error", () => {
      /* early child exit can close stdin before the supplied input is flushed */
    });
    child.stdout.on("data", (value: Buffer | string) => {
      if (terminationError) return;
      if (appendBounded(stdout, Buffer.from(value), options.stdoutLimitBytes)) {
        beginTermination(
          errorFor(
            "stdout-limit",
            `process stdout exceeded ${options.stdoutLimitBytes} byte limit`,
          ),
        );
      }
    });
    child.stderr.on("data", (value: Buffer | string) => {
      if (terminationError) return;
      if (appendBounded(stderr, Buffer.from(value), options.stderrLimitBytes)) {
        beginTermination(
          errorFor(
            "stderr-limit",
            `process stderr exceeded ${options.stderrLimitBytes} byte limit`,
          ),
        );
      }
    });
    child.on("error", (error: NodeJS.ErrnoException) => {
      const code = error.code ?? null;
      settle(() => reject(errorFor("spawn", error.message, code, error)));
    });
    child.on("close", (code) => {
      settle(() => {
        if (terminationError) {
          reject(terminationError);
          return;
        }
        const result = output();
        if (code === 0) {
          resolve(result);
          return;
        }
        reject(
          new ProcessRunError(
            "exit",
            result.stderr.trim() || `exit ${code}`,
            code,
            result.stdout,
            result.stderr,
          ),
        );
      });
    });
    child.stdin.end(options.input);
  });
