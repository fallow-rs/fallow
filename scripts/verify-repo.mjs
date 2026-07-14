#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import { fileURLToPath, pathToFileURL } from "node:url";
import path from "node:path";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const FAST_COMMANDS = [
  {
    label: "Rust formatting",
    command: "cargo",
    args: ["fmt", "--all", "--", "--check"],
  },
  {
    label: "Rust linting",
    command: "cargo",
    args: ["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"],
  },
  {
    label: "JavaScript linting",
    command: "npm",
    args: ["run", "lint:js"],
  },
  {
    label: "JavaScript formatting",
    command: "npm",
    args: ["run", "fmt:js:check"],
  },
  {
    label: "Generated contract drift",
    command: "npm",
    args: ["run", "generate:contracts:check"],
  },
  {
    label: "Crate boundaries",
    command: "npm",
    args: ["run", "check:crate-boundaries"],
  },
];

const FULL_ONLY_COMMANDS = [
  {
    label: "Repository script tests",
    command: "node",
    args: ["--test", "scripts/*.test.mjs"],
  },
  {
    label: "Workspace tests",
    command: "cargo",
    args: ["test", "--workspace", "--lib", "--bins", "--tests", "--examples"],
  },
  {
    label: "Benchmark compilation",
    command: "cargo",
    args: ["check", "--workspace", "--benches"],
  },
  {
    label: "Rust documentation",
    command: "cargo",
    args: ["doc", "--workspace", "--no-deps", "--document-private-items"],
    env: { RUSTDOCFLAGS: "-D warnings" },
  },
  {
    label: "Local NAPI build",
    command: "npm",
    args: ["--prefix", "crates/napi", "run", "build:debug"],
  },
  {
    label: "Local NAPI tests",
    command: "npm",
    args: ["--prefix", "crates/napi", "test"],
  },
];

export const CI_ONLY_GATES = [
  { label: "Miri", helpPattern: "Miri" },
  { label: "MSRV and cross-platform jobs", helpPattern: "cross-platform" },
  {
    label: "feature-specific and editor integration jobs",
    helpPattern: "feature-specific",
  },
  { label: "release and publish jobs", helpPattern: "release and publish" },
  {
    label: "network and real-project smoke tests",
    helpPattern: "real-project",
  },
];

const VALID_ARGS = new Set(["--fast", "--full", "--help", "-h"]);
const HELP_ARGS = ["--help", "-h"];

const ciOnlyGateList = () => CI_ONLY_GATES.map(({ label }) => `  - ${label}`).join("\n");

export const commandsForMode = (mode) => {
  if (mode === "fast") {
    return FAST_COMMANDS;
  }
  if (mode === "full") {
    return [...FAST_COMMANDS, ...FULL_ONLY_COMMANDS];
  }
  throw new Error(`Unknown verification mode: ${mode}`);
};

export const parseArgs = (args) => {
  const requested = new Set(args);
  const unknown = args.find((arg) => !VALID_ARGS.has(arg));
  if (unknown !== undefined) {
    throw new Error(`Unknown argument: ${unknown}`);
  }
  const modeCount = Number(requested.has("--fast")) + Number(requested.has("--full"));
  if (modeCount > 1) {
    throw new Error("--fast and --full cannot be combined");
  }

  return {
    mode: requested.has("--full") ? "full" : "fast",
    help: HELP_ARGS.some((arg) => requested.has(arg)),
  };
};

export const helpText = () => `Usage: node scripts/verify-repo.mjs [--fast | --full]

Canonical local repository verification:
  --fast  Formatting, linting, generated contracts, and crate boundaries (default)
  --full  Fast checks plus repository script tests, workspace tests, benchmark
          compilation, rustdoc, and the local NAPI build and tests

Prerequisites:
  - Node.js 22 or newer and root dependencies installed with npm install
  - A stable Rust toolchain with rustfmt and clippy
  - editors/vscode dependencies installed with pnpm install
  - Full mode also needs crates/napi dependencies installed with npm ci
    and a platform compiler

CI-only gates not simulated by this local command:
${ciOnlyGateList()}
`;

const defaultRunCommand = ({ command, args, env = {} }) => {
  const result = spawnSync(command, args, {
    cwd: ROOT,
    env: { ...process.env, ...env },
    shell: false,
    stdio: "inherit",
  });

  if (result.error !== undefined) {
    process.stderr.write(`${result.error.message}\n`);
    return 1;
  }
  return result.status ?? 1;
};

const formatCommand = ({ command, args }) =>
  [command, ...args].map((part) => JSON.stringify(part)).join(" ");

export const runVerification = (
  mode,
  { runCommand = defaultRunCommand, write = (message) => process.stdout.write(message) } = {},
) => {
  const commands = commandsForMode(mode);
  write(`Running ${mode} repository verification.\n`);

  for (const command of commands) {
    write(`\n[verify] ${command.label}: ${formatCommand(command)}\n`);
    const exitCode = runCommand(command);
    if (exitCode !== 0) {
      write(`[verify] ${command.label} failed with exit code ${exitCode}.\n`);
      return exitCode;
    }
  }

  write(`\nRepository verification passed (${mode}).\n`);
  write(`\nCI-only gates not run by this local command:\n${ciOnlyGateList()}\n`);
  return 0;
};

export const main = (args) => {
  try {
    const options = parseArgs(args);
    if (options.help) {
      process.stdout.write(helpText());
      return 0;
    }
    return runVerification(options.mode);
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    process.stderr.write(`${message}\n\n${helpText()}`);
    return 2;
  }
};

const isDirectInvocation =
  process.argv[1] !== undefined &&
  import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href;

if (isDirectInvocation) {
  process.exitCode = main(process.argv.slice(2));
}
