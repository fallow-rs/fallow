import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import {
  pgoValidationRequired,
  releaseProfileSections,
  workflowJob,
} from "./pgo-validate-inputs.mjs";

const MANIFEST = `[workspace.package]
version = "1.0.0"

[workspace.dependencies]
serde = "1"

[profile.dev]
opt-level = 0

[profile.release]
lto = "fat"
codegen-units = 1

[profile.release.package.regex-syntax]
opt-level = "s"

[profile.release-debug]
inherits = "release"
`;

const RELEASE = `name: Release
env:
  CARGO_TERM_COLOR: always
jobs:
  release-context:
    runs-on: ubuntu-26.04
  pgo-profile:
    runs-on: ubuntu-26.04
    steps:
      - run: scripts/pgo-train.sh

  build:
    needs: [pgo-profile]
    steps:
      - run: cargo build --release
  npm-publish:
    steps:
      - run: npm publish
`;

const files = (base, head) => (side, path) => (side === "base" ? base : head)[path] ?? null;
const decide = (paths, base, head) => pgoValidationRequired(paths, files(base, head));

test("release profile sections include package overrides and exclude other profiles", () => {
  const sections = releaseProfileSections(MANIFEST);
  assert.match(sections, /\[profile\.release\]\nlto = "fat"/u);
  assert.match(sections, /\[profile\.release\.package\.regex-syntax\]/u);
  assert.doesNotMatch(sections, /release-debug|profile\.dev|workspace/u);
});

test("a workflow job ends at the next job", () => {
  assert.equal(
    workflowJob(RELEASE, "build").split("\n").at(-1),
    "      - run: cargo build --release",
  );
  assert.equal(workflowJob(RELEASE, "missing"), null);
});

test("Cargo.toml needs PGO validation only for a release profile change", () => {
  const paths = ["Cargo.toml"];
  for (const edit of [
    MANIFEST.replace('version = "1.0.0"', 'version = "1.1.0"'),
    MANIFEST.replace('serde = "1"', 'serde = "2"'),
    MANIFEST.replace("opt-level = 0", "opt-level = 1"),
    MANIFEST.replace('inherits = "release"', 'inherits = "release"\ndebug = true'),
  ])
    assert.equal(decide(paths, { "Cargo.toml": MANIFEST }, { "Cargo.toml": edit }), false, edit);
  for (const edit of [
    MANIFEST.replace('lto = "fat"', 'lto = "thin"'),
    MANIFEST.replace('opt-level = "s"', "opt-level = 3"),
    `${MANIFEST}\n[profile.release.package.toml]\nopt-level = "s"\n`,
  ])
    assert.equal(decide(paths, { "Cargo.toml": MANIFEST }, { "Cargo.toml": edit }), true, edit);
});

test("release.yml needs PGO validation only for the profile and build jobs", () => {
  const paths = [".github/workflows/release.yml"];
  const base = { ".github/workflows/release.yml": RELEASE };
  const edit = (text) => ({ ".github/workflows/release.yml": text });
  assert.equal(
    decide(paths, base, edit(RELEASE.replace("npm publish", "npm publish --provenance"))),
    false,
  );
  assert.equal(
    decide(
      paths,
      base,
      edit(RELEASE.replace("runs-on: ubuntu-26.04\n  pgo", "runs-on: ubuntu-24.04\n  pgo")),
    ),
    false,
  );
  assert.equal(
    decide(paths, base, edit(RELEASE.replace("pgo-train.sh", "pgo-train.sh --fast"))),
    true,
  );
  assert.equal(
    decide(
      paths,
      base,
      edit(RELEASE.replace("cargo build --release", "cargo build --release --locked")),
    ),
    true,
  );
  assert.equal(decide(paths, base, edit(RELEASE.replace("  build:", "  build-all:"))), true);
});

test("a release profile in a form that the reader does not follow runs the comparison", () => {
  for (const form of [
    '[profile]\nrelease = { lto = "thin" }\n',
    'profile.release.lto = "thin"\n',
    '  [profile.release]\nlto = "thin"\n',
  ]) {
    assert.equal(releaseProfileSections(`${MANIFEST}${form}`), null, form);
    assert.equal(
      decide(["Cargo.toml"], { "Cargo.toml": MANIFEST }, { "Cargo.toml": `${MANIFEST}${form}` }),
      true,
      form,
    );
  }
});

test("a top-level env change of release.yml runs the comparison", () => {
  const paths = [".github/workflows/release.yml"];
  const base = { ".github/workflows/release.yml": RELEASE };
  const head = { ".github/workflows/release.yml": RELEASE.replace("always", "never") };
  assert.equal(decide(paths, base, head), true);
});

test("other filter paths, unreadable files and unknown shapes run the comparison", () => {
  for (const path of [
    "scripts/pgo-train.sh",
    ".github/actions/setup-rust/action.yml",
    ".cargo/config.toml",
    "rust-toolchain.toml",
    ".github/workflows/pgo-validate.yml",
  ])
    assert.equal(decide([path], {}, {}), true, path);
  assert.equal(decide(["Cargo.toml"], {}, { "Cargo.toml": MANIFEST }), true, "added manifest");
  assert.equal(
    decide([".github/workflows/release.yml"], { ".github/workflows/release.yml": RELEASE }, {}),
    true,
  );
  // Paths outside the filter do not start the workflow, so they never decide.
  assert.equal(decide(["crates/cli/src/lib.rs", "README.md"], {}, {}), false);
});

test("the executable compares the merge base with the checkout", (t) => {
  const root = mkdtempSync(join(tmpdir(), "fallow-pgo-inputs-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
  git("init", "-q", "-b", "main");
  git("config", "user.email", "test@example.com");
  git("config", "user.name", "Test");
  const commit = (subject) => {
    git("add", ".");
    git("-c", "commit.gpgsign=false", "commit", "-qm", subject);
    return git("rev-parse", "HEAD");
  };
  mkdirSync(join(root, ".github/workflows"), { recursive: true });
  writeFileSync(join(root, "Cargo.toml"), MANIFEST);
  writeFileSync(join(root, ".github/workflows/release.yml"), RELEASE);
  const base = commit("base");
  const script = join(process.cwd(), "scripts/pgo-validate-inputs.mjs");
  const eventPath = join(root, "event.json");
  const outputPath = join(root, "output");
  const run = (head, event = { pull_request: { base: { sha: base } } }) => {
    writeFileSync(eventPath, JSON.stringify(event));
    return execFileSync(process.execPath, [script], {
      cwd: root,
      encoding: "utf8",
      env: {
        ...process.env,
        GITHUB_EVENT_NAME: "pull_request",
        GITHUB_SHA: head,
        GITHUB_EVENT_PATH: eventPath,
        GITHUB_OUTPUT: outputPath,
      },
    });
  };
  writeFileSync(join(root, "Cargo.toml"), MANIFEST.replace("1.0.0", "1.1.0"));
  const bump = commit("chore: release v1.1.0");
  assert.match(run(bump), /^run=false$/mu);
  assert.match(readFileSync(outputPath, "utf8"), /run=false/u);
  writeFileSync(join(root, "Cargo.toml"), MANIFEST.replace('lto = "fat"', 'lto = "thin"'));
  const profile = commit("perf: thin LTO");
  assert.match(run(profile), /^run=true$/mu);
  assert.match(run(bump, {}), /^run=true$/mu, "a missing base runs the comparison");
  assert.match(run("f".repeat(40)), /^run=true$/mu, "a checkout mismatch runs the comparison");
});
