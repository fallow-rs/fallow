import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import {
  MIN_NODE,
  bootstrap,
  decideHooksPath,
  findToolProblems,
  hkInstallArgs,
  isAtLeast,
} from "./bootstrap.mjs";

const fakeIo = ({
  nodeVersion = "26.0.0",
  commands = ["pnpm", "hk", "mise"],
  hooksPath = null,
  globalHooksPath = null,
  gitVersion = "2.56.0",
  unsetSucceeds = true,
  failingCommand = null,
} = {}) => {
  const calls = [];
  const logs = [];
  const warnings = [];
  let currentHooksPath = hooksPath;
  const io = {
    nodeVersion,
    gitVersion,
    hasCommand: (name) => commands.includes(name),
    readHooksPath: () => ({
      local: currentHooksPath,
      effective: currentHooksPath ?? globalHooksPath,
    }),
    unsetHooksPath: () => {
      calls.push(["git", "config", "--local", "--unset-all", "core.hooksPath"]);
      if (unsetSucceeds) currentHooksPath = null;
      return unsetSucceeds;
    },
    run: (command, args, cwd) => {
      calls.push([command, ...args, `@${cwd}`]);
      return command === failingCommand ? 2 : 0;
    },
    log: (message) => logs.push(message),
    warn: (message) => warnings.push(message),
  };
  return { io, calls, logs, warnings, hooksPath: () => currentHooksPath };
};

test("the Node.js floor matches the root engines field", () => {
  const rootPackage = JSON.parse(readFileSync("package.json", "utf8"));
  assert.equal(rootPackage.engines.node, `>=${MIN_NODE}`);
});

test("isAtLeast compares major, minor and patch numbers", () => {
  assert.equal(isAtLeast("22.12.0", "22.12.0"), true);
  assert.equal(isAtLeast("v22.12.1", "22.12.0"), true);
  assert.equal(isAtLeast("26.0.0", "22.12.0"), true);
  assert.equal(isAtLeast("22.11.9", "22.12.0"), false);
  assert.equal(isAtLeast("20.19.0", "22.12.0"), false);
  assert.equal(isAtLeast("22.12.0-nightly", "22.12.0"), true);
});

test("tool check names one install hint per missing tool", () => {
  assert.deepEqual(findToolProblems({ nodeVersion: "26.0.0", hasCommand: () => true }), []);

  const problems = findToolProblems({ nodeVersion: "20.19.0", hasCommand: () => false });
  assert.equal(problems.length, 3);
  assert.match(problems[0], /Node\.js 20\.19\.0 is too old.*22\.13\.0/u);
  assert.match(problems[1], /^pnpm is missing\..*mise install/u);
  assert.match(problems[2], /^hk is missing\..*mise install/u);

  const onlyHk = findToolProblems({ nodeVersion: "22.13.0", hasCommand: (name) => name !== "hk" });
  assert.deepEqual(onlyHk.length, 1);
  assert.match(onlyHk[0], /^hk is missing/u);
});

test("hooksPath decision removes only the legacy hooks folder", () => {
  assert.equal(decideHooksPath(null), "keep");
  assert.equal(decideHooksPath(""), "keep");
  assert.equal(decideHooksPath(".githooks"), "unset");
  assert.equal(decideHooksPath("./.githooks/"), "unset");
  assert.equal(decideHooksPath(".husky"), "warn");
  assert.equal(decideHooksPath("/usr/local/share/hooks"), "warn");
});

test("hk uses mise when mise is on PATH", () => {
  assert.deepEqual(hkInstallArgs(true), ["install", "--mise"]);
  assert.deepEqual(hkInstallArgs(false), ["install"]);
});

test("bootstrap stops before any change when a tool is missing", () => {
  const fake = fakeIo({ commands: ["mise"], hooksPath: ".githooks" });
  assert.equal(bootstrap(fake.io), 1);
  assert.deepEqual(fake.calls, []);
  assert.equal(fake.hooksPath(), ".githooks");
  assert.equal(fake.warnings.length, 2);
});

test("bootstrap installs hooks and both dependency trees", () => {
  const fake = fakeIo();
  assert.equal(bootstrap(fake.io), 0);
  assert.deepEqual(fake.calls, [
    ["hk", "install", "--mise", "@."],
    ["pnpm", "install", "--frozen-lockfile", "@."],
    ["pnpm", "install", "--frozen-lockfile", "@editors/vscode"],
  ]);
  assert.deepEqual(fake.warnings, []);
});

test("bootstrap without mise installs plain hk hooks", () => {
  const fake = fakeIo({ commands: ["pnpm", "hk"] });
  assert.equal(bootstrap(fake.io), 0);
  assert.deepEqual(fake.calls[0], ["hk", "install", "@."]);
});

test("bootstrap removes the legacy hooksPath and says so", () => {
  const fake = fakeIo({ hooksPath: ".githooks" });
  assert.equal(bootstrap(fake.io), 0);
  assert.deepEqual(fake.calls[0], ["git", "config", "--local", "--unset-all", "core.hooksPath"]);
  assert.equal(fake.hooksPath(), null);
  assert.ok(fake.logs.some((line) => /Removed core\.hooksPath=\.githooks/u.test(line)));

  // A second run finds nothing to remove and makes the same install calls.
  const callsAfterFirstRun = fake.calls.length;
  assert.equal(bootstrap(fake.io), 0);
  assert.equal(
    fake.calls.slice(callsAfterFirstRun).some(([command]) => command === "git"),
    false,
  );
});

test("bootstrap keeps another hooksPath and warns", () => {
  const fake = fakeIo({ hooksPath: ".husky" });
  assert.equal(bootstrap(fake.io), 0);
  assert.equal(fake.hooksPath(), ".husky");
  assert.equal(
    fake.calls.some(([command]) => command === "git"),
    false,
  );
  assert.equal(fake.warnings.length, 1);
  assert.match(fake.warnings[0], /core\.hooksPath is \.husky\. Bootstrap keeps it\./u);
  assert.match(fake.warnings[0], /in addition to the hk hooks/u);
});

test("on git before 2.54 the warning says that the hk hooks do not run", () => {
  const fake = fakeIo({ hooksPath: ".husky", gitVersion: "2.50.1" });
  assert.equal(bootstrap(fake.io), 0);
  assert.match(fake.warnings[0], /git 2\.50\.1 runs only the hooks in that folder/u);
});

test("bootstrap warns about a legacy hooksPath from another scope and keeps it", () => {
  // A global value can serve other projects, so bootstrap does not remove it.
  const fake = fakeIo({ globalHooksPath: ".githooks" });
  assert.equal(bootstrap(fake.io), 0);
  assert.equal(
    fake.calls.some(([command]) => command === "git"),
    false,
  );
  assert.match(fake.warnings[0], /core\.hooksPath is \.githooks\. Bootstrap keeps it\./u);

  // A second run gives the same result, not a failure.
  assert.equal(bootstrap(fake.io), 0);
});

test("bootstrap fails when it cannot remove the legacy hooksPath", () => {
  const fake = fakeIo({ hooksPath: ".githooks", unsetSucceeds: false });
  assert.equal(bootstrap(fake.io), 1);
  assert.equal(
    fake.calls.some(([command]) => command === "pnpm"),
    false,
  );
  assert.match(fake.warnings[0], /cannot remove it/u);
});

test("bootstrap stops at the first failed step", () => {
  const fake = fakeIo({ failingCommand: "hk" });
  assert.equal(bootstrap(fake.io), 1);
  assert.equal(fake.calls.length, 1);
  assert.match(fake.warnings[0], /Install the git hooks failed with exit code 2/u);
});
