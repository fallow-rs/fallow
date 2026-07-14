import assert from "node:assert/strict";
import test from "node:test";

import {
  CI_ONLY_GATES,
  commandsForMode,
  helpText,
  parseArgs,
  runVerification,
} from "./verify-repo.mjs";

const commandSignatures = (commands) => commands.map(({ command, args }) => [command, args]);

const FAST_COMMANDS = [
  ["cargo", ["fmt", "--all", "--", "--check"]],
  ["cargo", ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]],
  ["npm", ["run", "lint:js"]],
  ["npm", ["run", "fmt:js:check"]],
  ["npm", ["run", "generate:contracts:check"]],
  ["npm", ["run", "check:crate-boundaries"]],
];

const FULL_ONLY_COMMANDS = [
  ["node", ["--test", "scripts/*.test.mjs"]],
  ["cargo", ["test", "--workspace", "--lib", "--bins", "--tests", "--examples"]],
  ["cargo", ["check", "--workspace", "--benches"]],
  ["cargo", ["doc", "--workspace", "--no-deps", "--document-private-items"]],
  ["npm", ["--prefix", "crates/napi", "run", "build:debug"]],
  ["npm", ["--prefix", "crates/napi", "test"]],
];

test("fast mode runs the canonical checks in order", () => {
  assert.deepEqual(commandSignatures(commandsForMode("fast")), FAST_COMMANDS);
});

test("full mode runs fast checks first, then the full checks", () => {
  assert.deepEqual(commandSignatures(commandsForMode("full")), [
    ...FAST_COMMANDS,
    ...FULL_ONLY_COMMANDS,
  ]);
});

test("commands use executable and argument arrays without a shell", () => {
  for (const command of commandsForMode("full")) {
    assert.equal(typeof command.command, "string");
    assert.ok(Array.isArray(command.args));
    assert.equal(command.shell, undefined);
  }
});

test("verification stops at the first failed command", () => {
  const executed = [];
  const commands = commandsForMode("fast");
  const exitCode = runVerification("fast", {
    runCommand: (command) => {
      executed.push(command.label);
      return executed.length === 2 ? 7 : 0;
    },
    write: () => {},
  });

  assert.equal(exitCode, 7);
  assert.deepEqual(
    executed,
    commands.slice(0, 2).map(({ label }) => label),
  );
});

test("successful verification discloses gates that remain CI-only", () => {
  let output = "";
  const exitCode = runVerification("fast", {
    runCommand: () => 0,
    write: (message) => {
      output += message;
    },
  });

  assert.equal(exitCode, 0);
  assert.match(output, /CI-only gates/i);
  for (const gate of CI_ONLY_GATES) {
    assert.match(output, new RegExp(gate.helpPattern, "i"));
  }
});

test("argument parsing supports the aliases and rejects ambiguity", () => {
  assert.deepEqual(parseArgs([]), { mode: "fast", help: false });
  assert.deepEqual(parseArgs(["--fast"]), { mode: "fast", help: false });
  assert.deepEqual(parseArgs(["--full"]), { mode: "full", help: false });
  assert.deepEqual(parseArgs(["--help"]), { mode: "fast", help: true });
  assert.deepEqual(parseArgs(["-h"]), { mode: "fast", help: true });
  assert.throws(() => parseArgs(["--fast", "--full"]), /cannot be combined/i);
  assert.throws(() => parseArgs(["--unknown"]), /unknown argument/i);
});

test("help documents prerequisites and gates intentionally left to CI", () => {
  const help = helpText();

  assert.match(help, /Node\.js 22/i);
  assert.match(help, /Rust toolchain/i);
  assert.match(help, /editors\/vscode.*pnpm install/is);
  assert.match(help, /crates\/napi.*npm ci/is);
  assert.match(help, /CI-only gates/i);
  for (const gate of CI_ONLY_GATES) {
    assert.match(help, new RegExp(gate.helpPattern, "i"));
  }
});
