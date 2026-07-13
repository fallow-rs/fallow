import { afterEach, describe, expect, it, vi } from "vitest";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import {
  cancelActiveProcesses,
  getActiveProcessCount,
  runProcess,
  terminateProcessTree,
  type ProcessRunOptions,
} from "./processRun";

const fixture = resolve(process.cwd(), "test", "fixtures", "child-process.cjs");
const roots: string[] = [];

const root = (): string => {
  const path = mkdtempSync(join(tmpdir(), "fallow-process-run-"));
  roots.push(path);
  return path;
};

const runFixture = (
  mode: string,
  args: string[] = [],
  options: Partial<ProcessRunOptions> = {},
): ReturnType<typeof runProcess> =>
  runProcess({
    command: process.execPath,
    args: [fixture, mode, ...args],
    cwd: process.cwd(),
    input: "",
    deadlineMs: 2_000,
    stdoutLimitBytes: 1_024,
    stderrLimitBytes: 1_024,
    terminationGraceMs: 250,
    ...options,
  });

afterEach(() => {
  cancelActiveProcesses();
  for (const path of roots.splice(0)) rmSync(path, { recursive: true, force: true });
});

describe("runProcess", () => {
  it("collects bounded output and closes stdin on normal exit", async () => {
    const result = await runFixture("normal", [], { input: "payload" });

    expect(result).toEqual({ stdout: "stdout:payload", stderr: "stderr:normal" });
    expect(getActiveProcessCount()).toBe(0);
  });

  it("preserves bounded stderr on nonzero exit", async () => {
    await expect(runFixture("nonzero")).rejects.toMatchObject({
      kind: "exit",
      code: 7,
      stderr: "child failed\nsecondary detail\n",
      message: "child failed\nsecondary detail",
    });
  });

  it("rejects and terminates a child that exceeds the stdout cap", async () => {
    await expect(runFixture("stdout", ["2048"], { stdoutLimitBytes: 64 })).rejects.toMatchObject({
      kind: "stdout-limit",
      stdout: "x".repeat(64),
    });
  });

  it("rejects and terminates a child that exceeds the stderr cap", async () => {
    await expect(runFixture("stderr", ["2048"], { stderrLimitBytes: 64 })).rejects.toMatchObject({
      kind: "stderr-limit",
      stderr: "y".repeat(64),
    });
  });

  it("terminates the child process group when the deadline expires", async () => {
    const dir = root();
    const descendantPid = join(dir, "descendant.pid");
    const descendantTerminated = join(dir, "descendant.terminated");

    await expect(
      runFixture("tree", [descendantPid, descendantTerminated], { deadlineMs: 200 }),
    ).rejects.toMatchObject({ kind: "timeout" });

    expect(Number(readFileSync(descendantPid, "utf8"))).toBeGreaterThan(0);
    expect(readFileSync(descendantTerminated, "utf8")).toBe("terminated");
    expect(getActiveProcessCount()).toBe(0);
  });

  it("cancels all active children for application shutdown", async () => {
    const ready = join(root(), "ready.pid");
    const pending = runFixture("hang", [ready], { deadlineMs: 10_000 });
    await vi.waitFor(() => expect(existsSync(ready)).toBe(true));

    cancelActiveProcesses();

    await expect(pending).rejects.toMatchObject({ kind: "cancelled" });
    expect(getActiveProcessCount()).toBe(0);
  });
});

describe("terminateProcessTree", () => {
  it("signals a detached process group on POSIX", () => {
    const kill = vi.fn();
    const taskkill = vi.fn();

    terminateProcessTree(42, "SIGTERM", "darwin", { kill, taskkill });

    expect(kill).toHaveBeenCalledWith(-42, "SIGTERM");
    expect(taskkill).not.toHaveBeenCalled();
  });

  it("uses taskkill for the full tree on Windows", () => {
    const kill = vi.fn();
    const taskkill = vi.fn();

    terminateProcessTree(42, "SIGTERM", "win32", { kill, taskkill });

    expect(taskkill).toHaveBeenCalledWith(42);
    expect(kill).not.toHaveBeenCalled();
  });
});
