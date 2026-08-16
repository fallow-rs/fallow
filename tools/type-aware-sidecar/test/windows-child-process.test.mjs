import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import test from "node:test";

import { installWindowsChildProcessPolicy } from "../src/windows-child-process.mjs";

const fakeChildProcess = () => {
  const calls = [];
  return {
    calls,
    spawn(...args) {
      calls.push(args);
      return { pid: 42 };
    },
  };
};

test("Windows child process policy hides both spawn overloads", () => {
  const childProcess = fakeChildProcess();
  let syncCount = 0;
  installWindowsChildProcessPolicy({
    childProcess,
    platform: "win32",
    syncBuiltinESMExports: () => {
      syncCount += 1;
    },
  });

  const argsOptions = {
    cwd: "C:\\project",
    detached: false,
    env: { FALLOW_TEST: "1" },
    stdio: ["ignore", "pipe", "inherit"],
    windowsHide: false,
  };
  childProcess.spawn("tsc.exe", ["--api"], argsOptions);
  assert.deepEqual(childProcess.calls[0], [
    "tsc.exe",
    ["--api"],
    { ...argsOptions, windowsHide: true },
  ]);
  assert.equal(argsOptions.windowsHide, false);

  const optionsOnly = { cwd: "C:\\project", shell: false };
  childProcess.spawn("tsc.exe", optionsOnly);
  assert.deepEqual(childProcess.calls[1], ["tsc.exe", { ...optionsOnly, windowsHide: true }]);
  assert.equal(syncCount, 1);
});

test("Windows child process policy is idempotent", () => {
  const childProcess = fakeChildProcess();
  let syncCount = 0;
  const options = {
    childProcess,
    platform: "win32",
    syncBuiltinESMExports: () => {
      syncCount += 1;
    },
  };
  installWindowsChildProcessPolicy(options);
  const wrapped = childProcess.spawn;
  installWindowsChildProcessPolicy(options);

  assert.equal(childProcess.spawn, wrapped);
  assert.equal(syncCount, 1);
});

test("child process policy leaves non-Windows spawn unchanged", () => {
  const childProcess = fakeChildProcess();
  const original = childProcess.spawn;
  let synced = false;
  installWindowsChildProcessPolicy({
    childProcess,
    platform: "linux",
    syncBuiltinESMExports: () => {
      synced = true;
    },
  });

  assert.equal(childProcess.spawn, original);
  assert.equal(synced, false);
});

test("Windows child process policy updates the real ESM spawn binding", () => {
  const policyUrl = new URL("../src/windows-child-process.mjs", import.meta.url).href;
  const script = `
    import assert from "node:assert/strict";
    import childProcess, { spawn } from "node:child_process";
    import { installWindowsChildProcessPolicy } from ${JSON.stringify(policyUrl)};
    let call;
    childProcess.spawn = (...args) => { call = args; return { pid: 42 }; };
    installWindowsChildProcessPolicy({ platform: "win32" });
    spawn("tsc.exe", { cwd: "C:\\\\project", windowsHide: false });
    assert.deepEqual(call, ["tsc.exe", { cwd: "C:\\\\project", windowsHide: true }]);
  `;
  const result = spawnSync(process.execPath, ["--input-type=module", "--eval", script], {
    encoding: "utf8",
  });

  assert.equal(result.status, 0, result.stderr);
});
