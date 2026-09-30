import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  appendFileSync,
  chmodSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve, parse } from "node:path";
import { fileURLToPath } from "node:url";

const CONTRACT = {
  version: 1,
  toolchain: "1.97.1",
  host: "x86_64-unknown-linux-gnu",
  profile: "dev",
  features: [],
  command: "cargo build --bin fallow",
  rustflags: "",
  encoded_rustflags: "",
  build_target: "",
};
const SHA = /^[0-9a-f]{40}$/u;
const digest = (bytes) => createHash("sha256").update(bytes).digest("hex");

/** Admit only the selected head of the trusted trial branch. */
export const trialEligible = ({ event, eventName, repository, optIn }) => {
  const head = event?.pull_request?.head;
  return (
    eventName === "pull_request" &&
    repository === "fallow-rs/fallow" &&
    head?.repo?.full_name === repository &&
    head?.ref === "perf/ci-efficiency" &&
    SHA.test(optIn ?? "") &&
    optIn === head?.sha
  );
};

/** Fail if a required comparison arm did not execute successfully. */
export const requireTrialResults = (results, napi, cli, action = cli, selfAnalyze = cli) => {
  const required = [
    ...(napi ? ["check", "napi-trial"] : []),
    ...(cli ? ["debug-cli-trial"] : []),
    ...(action ? ["action-trial"] : []),
    ...(selfAnalyze ? ["self-analyze-trial"] : []),
  ];
  for (const job of required)
    if (results[job] !== "success")
      throw new Error(`Required trial ${job}: ${results[job] ?? "missing"}`);
};

const safePath = (path, kind, missing = false) => {
  const absolute = resolve(path);
  const parts = absolute.slice(parse(absolute).root.length).split("/");
  let current = parse(absolute).root;
  for (const [index, part] of parts.entries()) {
    current = join(current, part);
    let stat;
    try {
      stat = lstatSync(current);
    } catch (error) {
      if (missing && error.code === "ENOENT") continue;
      throw error;
    }
    if (stat.isSymbolicLink()) throw new Error(`Symlink rejected: ${current}`);
    if (index < parts.length - 1 && !stat.isDirectory())
      throw new Error(`Not a directory: ${current}`);
    if (index === parts.length - 1 && !(kind === "file" ? stat.isFile() : stat.isDirectory()))
      throw new Error(`Invalid ${kind}: ${current}`);
  }
  return absolute;
};
const expectations = (identity) => {
  if (
    !SHA.test(identity.checkout_sha) ||
    !/^[1-9][0-9]*$/u.test(identity.run_id) ||
    !/^[1-9][0-9]*$/u.test(identity.run_attempt)
  )
    throw new Error("Invalid run identity");
  return { ...CONTRACT, ...identity };
};

/** Package freshly built CLI bytes with the fixed compiler contract. */
export const createArtifact = (source, directory, identity) => {
  const bytes = readFileSync(safePath(source, "file"));
  const expected = expectations(identity);
  safePath(directory, "directory", true);
  mkdirSync(directory, { recursive: true });
  writeFileSync(join(directory, "fallow"), bytes, { flag: "wx", mode: 0o755 });
  writeFileSync(
    join(directory, "manifest.json"),
    JSON.stringify({ ...expected, sha256: digest(bytes) }) + "\n",
    { flag: "wx" },
  );
};

/** Verify same-run identity and digest before installing executable bytes. */
export const installArtifact = (directory, destination, identity) => {
  safePath(directory, "directory");
  const manifest = JSON.parse(
    readFileSync(safePath(join(directory, "manifest.json"), "file"), "utf8"),
  );
  for (const [key, value] of Object.entries(expectations(identity))) {
    if (JSON.stringify(manifest[key]) !== JSON.stringify(value))
      throw new Error(`Artifact mismatch: ${key}`);
  }
  const bytes = readFileSync(safePath(join(directory, "fallow"), "file"));
  if (digest(bytes) !== manifest.sha256) throw new Error("Artifact digest mismatch");
  safePath(destination, "file", true);
  mkdirSync(dirname(destination), { recursive: true });
  writeFileSync(destination, bytes, { flag: "wx", mode: 0o755 });
  if (digest(readFileSync(safePath(destination, "file"))) !== manifest.sha256)
    throw new Error("Installed digest mismatch");
  chmodSync(destination, 0o755);
};

/** Reject target presence and nonempty compiler overrides at the manifest boundary. */
export const checkCompilerEnvironment = (env) => {
  if (env.CARGO_BUILD_TARGET !== undefined) throw new Error("Cargo target must be unset");
  for (const key of [
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
  ])
    if (env[key]) throw new Error(`Compiler override: ${key}`);
};

const run = () => {
  const mode = process.argv[2];
  if (mode === "trial") {
    const event = JSON.parse(readFileSync(process.env.GITHUB_EVENT_PATH, "utf8"));
    const enabled = trialEligible({
      event,
      eventName: process.env.GITHUB_EVENT_NAME,
      repository: process.env.GITHUB_REPOSITORY,
      optIn: process.env.CI_BUILD_TRIAL_HEAD_SHA,
    });
    appendFileSync(process.env.GITHUB_OUTPUT, `enabled=${enabled}\n`);
    return;
  }
  if (process.platform !== "linux" || process.arch !== "x64")
    throw new Error("Artifact requires Linux x64");
  const checkout_sha = execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim();
  const identity = {
    checkout_sha,
    run_id: process.env.GITHUB_RUN_ID,
    run_attempt: process.env.GITHUB_RUN_ATTEMPT,
  };
  const directory = join(process.env.RUNNER_TEMP, "fallow-cli-trial-artifact");
  if (mode === "create") {
    checkCompilerEnvironment(process.env);
    const compiler = execFileSync("rustc", ["-vV"], { encoding: "utf8" });
    if (
      !compiler.includes(`release: ${CONTRACT.toolchain}\n`) ||
      !compiler.includes(`host: ${CONTRACT.host}\n`)
    )
      throw new Error("Compiler identity mismatch");
    createArtifact("target/debug/fallow", directory, identity);
    appendFileSync(process.env.GITHUB_OUTPUT, `checkout-sha=${checkout_sha}\n`);
    return;
  }
  if (mode === "install") {
    installArtifact(directory, "target/debug/fallow", identity);
    return;
  }
  throw new Error("Expected trial, create or install");
};
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) run();
