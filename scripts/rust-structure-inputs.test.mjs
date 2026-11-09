import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { test } from "node:test";
import { runInNewContext } from "node:vm";

const workflow = readFileSync(".github/workflows/ci.yml", "utf8");
const job = /\n  cargo-fallow:\n[\s\S]*?(?=\n  [\w-]+:\n)/u.exec(workflow)[0];
const staticStep = /      - name: Static checks\n[\s\S]*?(?=\n      - |\n  [\w-]+:|$)/u.exec(
  job,
)[0];
const scanCondition = staticStep.match(/^        if: (.+)$/mu)?.[1] ?? "true";
const shouldScan = (event_name, scan, outcome = "success") =>
  runInNewContext(scanCondition.replaceAll("structure-inputs", "structureInputs"), {
    github: { event_name },
    steps: { structureInputs: { outcome, outputs: { scan } } },
    always: () => true,
    cancelled: () => false,
  });

test("Rust Structure skips analysis only for successful negative PR input detection", () => {
  assert.equal(shouldScan("pull_request", "false"), false, "unrelated PR still runs analysis");
  for (const scan of ["true", "", undefined])
    assert.equal(shouldScan("pull_request", scan), true, "uncertain PR must scan");
  assert.equal(shouldScan("pull_request", "false", "failure"), true);
  assert.equal(shouldScan("pull_request", "false", "skipped"), true);
  assert.equal(shouldScan("push", "false"), true, "main and release proof must scan");
  assert.doesNotMatch(job.split("steps:")[0], /^\s+if:/mu, "required job must stay present");
  assert.match(staticStep, /baseline: cargo-fallow-baseline\.json\n\s+base: auto/u);
});

const fixture = (t) => {
  const root = mkdtempSync(join(tmpdir(), "rust-structure-inputs-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const git = (...args) => execFileSync("git", args, { cwd: root, encoding: "utf8" }).trim();
  git("init", "-q");
  git("config", "user.name", "Test");
  git("config", "user.email", "test@example.com");
  const write = (path, text = "content\n") => {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), text);
  };
  const commit = () => {
    git("add", ".");
    git("-c", "commit.gpgsign=false", "commit", "--allow-empty", "-qm", "fixture");
    return git("rev-parse", "HEAD");
  };
  write("docs/guide.md");
  write("npm/fallow/src/index.js");
  write("src/lib.rs");
  const base = commit();
  const eventPath = join(root, ".git/event.json");
  const outputPath = join(root, ".git/output");
  const script = resolve("scripts/rust-structure-inputs.mjs");
  const run = (overrides = {}, event = null) => {
    const head = git("rev-parse", "HEAD");
    writeFileSync(
      eventPath,
      event ?? JSON.stringify({ pull_request: { base: { sha: base }, head: { sha: head } } }),
    );
    writeFileSync(outputPath, "");
    const output = execFileSync(process.execPath, [script], {
      cwd: root,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...process.env,
        GITHUB_EVENT_NAME: "pull_request",
        GITHUB_EVENT_PATH: eventPath,
        GITHUB_SHA: head,
        GITHUB_OUTPUT: outputPath,
        ...overrides,
      },
    });
    assert.equal(readFileSync(outputPath, "utf8"), output, "published decision must match stdout");
    return output;
  };
  return { root, git, write, commit, base, run };
};

test("the executable skips complete unrelated PR diffs with a Rust positive control", (t) => {
  const repo = fixture(t);
  repo.write("docs/guide.md", "updated\n");
  repo.write("npm/fallow/src/index.js", "export const x = 1;\n");
  repo.write("changelog.d/docs.md", "docs\n");
  repo.commit();
  assert.equal(repo.run(), "scan=false\n");
  repo.git("rm", "docs/guide.md");
  repo.git("mv", "npm/fallow/src/index.js", "npm/fallow/src/renamed.js");
  repo.commit();
  assert.equal(repo.run(), "scan=false\n", "unrelated deletions and renames stay irrelevant");
  repo.write("src/lib.rs", "fn changed() {}\n");
  repo.commit();
  assert.equal(repo.run(), "scan=true\n", "the full PR includes the earlier Rust change");
  repo.write("docs/last.md");
  repo.commit();
  assert.equal(repo.run(), "scan=true\n", "a docs-only tip cannot hide earlier Rust changes");
});

test("Rust inputs, configuration, unknown paths and non-PR events always scan", (t) => {
  const repo = fixture(t);
  for (const path of [
    "crates/new/src/lib.rs",
    "npm/hidden.rs",
    "docs/example.rs",
    "fuzz/fuzz_targets/parser.rs",
    "Cargo.toml",
    "Cargo.lock",
    "fuzz/Cargo.toml",
    "fuzz/Cargo.lock",
    "crates/new/Cargo.toml",
    ".cargo/config",
    ".cargo/config.toml",
    "fuzz/.cargo/config.toml",
    "rust-toolchain",
    "rust-toolchain.toml",
    "cargo-fallow-baseline.json",
    ".gitignore",
    ".gitattributes",
    ".github/workflows/ci.yml",
    "scripts/rust-structure-inputs.mjs",
    ".github/actions/structure/action.yml",
    "new/input",
    "docs/new.toml",
    "npm/fallow/Cargo.toml",
    "crates/api/data.json",
  ]) {
    repo.git("reset", "--hard", repo.base);
    repo.write(path);
    repo.commit();
    assert.equal(repo.run(), "scan=true\n", path);
  }
  repo.git("reset", "--hard", repo.base);
  repo.write("docs/guide.md", "updated\n");
  repo.commit();
  assert.equal(repo.run(), "scan=false\n", "negative control must be eligible");
  for (const event of ["push", "workflow_dispatch", "merge_group", ""])
    assert.equal(repo.run({ GITHUB_EVENT_NAME: event }), "scan=true\n", event);
});

test("deleting or renaming a Rust file into an unrelated directory still scans", (t) => {
  const repo = fixture(t);
  repo.git("mv", "src/lib.rs", "docs/moved.md");
  repo.commit();
  assert.equal(repo.run(), "scan=true\n", "renames must check both old and new paths");
  repo.git("reset", "--hard", repo.base);
  repo.git("rm", "src/lib.rs");
  repo.commit();
  assert.equal(repo.run(), "scan=true\n", "Rust deletion must scan");
  repo.git("reset", "--hard", repo.base);
  repo.git("mv", "docs/guide.md", "src/new.rs");
  repo.commit();
  assert.equal(repo.run(), "scan=true\n", "rename destinations can add Rust input");
});

test("empty, malformed, missing and mismatched PR evidence fails closed", (t) => {
  const repo = fixture(t);
  assert.equal(repo.run(), "scan=true\n", "empty diff cannot prove inputs are available");
  repo.write("docs/guide.md", "updated\n");
  const head = repo.commit();
  assert.equal(repo.run(), "scan=false\n");
  for (const event of [
    "{",
    "{}",
    "null",
    JSON.stringify({ pull_request: { base: { sha: "f".repeat(40) }, head: { sha: head } } }),
  ])
    assert.equal(repo.run({}, event), "scan=true\n", event);
  for (const sha of ["", "0".repeat(40), "f".repeat(40), repo.base])
    assert.equal(repo.run({ GITHUB_SHA: sha }), "scan=true\n", sha);
  assert.equal(repo.run({ GITHUB_EVENT_PATH: join(repo.root, "absent") }), "scan=true\n");
});

test("an unfamiliar change status on an otherwise eligible path scans", (t) => {
  const repo = fixture(t);
  const blob = repo.git("rev-parse", "HEAD:docs/guide.md");
  repo.git("update-index", "--cacheinfo", `120000,${blob},docs/guide.md`);
  repo.git("-c", "commit.gpgsign=false", "commit", "-qm", "change file type");
  assert.match(repo.git("diff", "--name-status", repo.base, "HEAD"), /^T\s+docs\/guide\.md$/u);
  assert.equal(repo.run(), "scan=true\n");
});

test("a merge checkout includes base-branch Rust inputs and verifies PR ancestry", (t) => {
  const repo = fixture(t);
  repo.git("checkout", "-qb", "pr");
  repo.write("docs/guide.md", "PR change\n");
  const head = repo.commit();
  repo.git("checkout", "-qb", "main-tip", repo.base);
  repo.write("src/lib.rs", "fn base_changed() {}\n");
  const base = repo.commit();
  repo.git("-c", "commit.gpgsign=false", "merge", "--no-ff", "-qm", "merge PR", "pr");
  const event = JSON.stringify({ pull_request: { base: { sha: base }, head: { sha: head } } });
  assert.equal(
    repo.run({}, event),
    "scan=false\n",
    "the current base already contains its Rust changes",
  );
  const staleBase = JSON.stringify({
    pull_request: { base: { sha: repo.base }, head: { sha: head } },
  });
  assert.equal(
    repo.run({}, staleBase),
    "scan=true\n",
    "stale event must include merged Rust changes",
  );
  repo.git("checkout", "-q", "main-tip~1");
  assert.equal(repo.run({}, event), "scan=true\n", "unrelated checkout cannot prove PR inputs");
});
