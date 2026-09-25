import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  CI_ONLY_GATES,
  commandsForMode,
  findInstallProblems,
  helpText,
  installsForMode,
  parseArgs,
  runVerification,
} from "./verify-repo.mjs";

const commandSignatures = (commands) => commands.map(({ command, args }) => [command, args]);

const FAST_COMMANDS = [
  ["npm", ["run", "check:knowledge-architecture"]],
  ["npm", ["run", "check:agent-adapters"]],
  ["cargo", ["fmt", "--all", "--", "--check"]],
  ["cargo", ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]],
  ["npm", ["run", "lint:js"]],
  ["npm", ["run", "fmt:js:check"]],
  ["npm", ["run", "generate:contracts:check"]],
  ["npm", ["run", "check:crate-boundaries"]],
  ["npm", ["run", "check:miri-cfg"]],
  ["npm", ["run", "check:emitted-versions"]],
  ["npm", ["run", "check:conformance-fixtures"]],
];

const FULL_ONLY_COMMANDS = [
  ["node", ["--test", "scripts/*.test.mjs"]],
  ["node", ["--test", ".github/scripts/*.test.mjs"]],
  ["npm", ["--prefix", "npm/fallow", "test"]],
  ["npm", ["--prefix", "npm/fallow-similar-code", "test"]],
  ["cargo", ["test", "--workspace", "--lib", "--bins", "--tests", "--examples"]],
  ["npm", ["run", "check:coverage-producers"]],
  ["npm", ["run", "check:semantic-clone-conformance"]],
  ["npm", ["run", "check:dupes-accuracy"]],
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

test("default runner preserves literal arguments without a shell", (t) => {
  const directory = mkdtempSync(join(tmpdir(), "fallow runner "));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const fixture = join(directory, "record arguments.mjs");
  const output = join(directory, "arguments.json");
  const args = ["two words", "$(echo expanded)", "literal;value", "*.mjs", ""];
  writeFileSync(
    fixture,
    'import { writeFileSync } from "node:fs";\n' +
      "writeFileSync(process.argv[2], JSON.stringify(process.argv.slice(3)));\n",
  );
  const command = {
    label: "Literal argument probe",
    command: process.execPath,
    args: [fixture, output, ...args],
  };
  const moduleUrl = new URL("./verify-repo.mjs", import.meta.url).href;
  const result = spawnSync(
    process.execPath,
    [
      "--input-type=module",
      "-e",
      `import { commandsForMode, installsForMode, runVerification } from ${JSON.stringify(moduleUrl)};
       installsForMode("fast").splice(0);
       const commands = commandsForMode("fast");
       commands.splice(0, commands.length, ${JSON.stringify(command)});
       process.exitCode = runVerification("fast");`,
    ],
    { encoding: "utf8" },
  );

  assert.equal(result.status, 0, result.stderr || result.stdout);
  assert.deepEqual(JSON.parse(readFileSync(output, "utf8")), args);
});

test("verification stops at the first failed command", () => {
  const executed = [];
  const commands = commandsForMode("fast");
  const exitCode = runVerification("fast", {
    assertInstall: () => {},
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
    assertInstall: () => {},
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

test("preflight names every missing install and runs no gate", () => {
  const executed = [];
  let output = "";
  const exitCode = runVerification("fast", {
    assertInstall: ({ dependency, installCommand }) => {
      if (dependency !== "oxfmt") {
        throw new Error(`${dependency} missing; run \`${installCommand}\``);
      }
    },
    runCommand: (command) => {
      executed.push(command.label);
      return 0;
    },
    write: (message) => {
      output += message;
    },
  });

  assert.equal(exitCode, 1);
  assert.deepEqual(executed, []);
  assert.match(output, /oxlint missing; run `npm ci`/u);
  assert.match(
    output,
    /json-schema-to-typescript missing; run `pnpm --dir editors\/vscode install`/u,
  );
  assert.doesNotMatch(output, /oxfmt/u);
});

test("full mode also checks the NAPI install", () => {
  const fast = installsForMode("fast").map(({ dependency }) => dependency);
  const full = installsForMode("full").map(({ dependency }) => dependency);
  assert.deepEqual(full, [...fast, "@napi-rs/cli"]);
  assert.deepEqual(
    findInstallProblems("full", () => {}),
    [],
  );
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
  assert.match(help, /npm wrapper tests/i);
  assert.match(help, /CI-only gates/i);
  for (const gate of CI_ONLY_GATES) {
    assert.match(help, new RegExp(gate.helpPattern, "i"));
  }
});
