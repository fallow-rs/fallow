import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import {
  mkdtempSync,
  mkdirSync,
  writeFileSync,
  readFileSync,
  symlinkSync,
  unlinkSync,
  realpathSync,
  rmSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import {
  trialEligible,
  createArtifact,
  installArtifact,
  requireTrialResults,
  checkCompilerEnvironment,
} from "./ci-build-artifact.mjs";

const identity = { checkout_sha: "a".repeat(40), run_id: "123", run_attempt: "1" };
const event = {
  pull_request: {
    head: {
      repo: { full_name: "fallow-rs/fallow" },
      ref: "perf/ci-efficiency",
      sha: "b".repeat(40),
    },
  },
};
test("trial requires an exact trusted PR head opt-in", () => {
  const input = {
    event,
    eventName: "pull_request",
    repository: "fallow-rs/fallow",
    optIn: "b".repeat(40),
  };
  assert.equal(trialEligible(input), true);
  for (const change of [
    { optIn: "" },
    { optIn: "disabled" },
    { optIn: "c".repeat(40) },
    { eventName: "push" },
    { repository: "fork/fallow" },
    { event: { pull_request: { head: { ...event.pull_request.head, ref: "main" } } } },
    {
      event: {
        pull_request: { head: { ...event.pull_request.head, repo: { full_name: "fork/fallow" } } },
      },
    },
  ]) {
    assert.equal(trialEligible({ ...input, ...change }), false);
  }
});
test("required trial results reject skipped and failed dependencies", () => {
  const results = {
    check: "success",
    "napi-trial": "success",
    "debug-cli-trial": "success",
    "action-trial": "success",
    "self-analyze-trial": "success",
  };
  assert.doesNotThrow(() => requireTrialResults(results, true, true));
  for (const job of Object.keys(results))
    for (const status of ["skipped", "failure", "cancelled", undefined]) {
      assert.throws(() => requireTrialResults({ ...results, [job]: status }, true, true));
    }
});
const fixture = (context) => {
  const root = mkdtempSync(join(realpathSync(tmpdir()), "fallow-artifact-"));
  context.after(() => rmSync(root, { recursive: true, force: true }));
  const source = join(root, "source");
  writeFileSync(source, "#!/bin/sh\nprintf verified\n", { mode: 0o755 });
  const artifact = join(root, "artifact");
  createArtifact(source, artifact, identity);
  return { root, artifact, output: join(root, "installed") };
};
test("verified artifact copies executable checked bytes", (context) => {
  const { artifact, output } = fixture(context);
  installArtifact(artifact, output, identity);
  assert.equal(readFileSync(output, "utf8"), "#!/bin/sh\nprintf verified\n");
  assert.equal(execFileSync(output, [], { encoding: "utf8" }), "verified");
});
test("artifact rejects mismatched identity, compiler contract and digest", (context) => {
  for (const [key, value] of Object.entries({
    version: 2,
    checkout_sha: "c".repeat(40),
    run_id: "124",
    run_attempt: "2",
    toolchain: "1.96.0",
    host: "aarch64-unknown-linux-gnu",
    profile: "release",
    features: ["schema-emit"],
    command: "cargo build --workspace",
    rustflags: "-C opt-level=3",
    encoded_rustflags: "x",
    build_target: "x",
    sha256: "0".repeat(64),
  })) {
    const { artifact, output } = fixture(context);
    const manifest = join(artifact, "manifest.json");
    writeFileSync(
      manifest,
      JSON.stringify({ ...JSON.parse(readFileSync(manifest, "utf8")), [key]: value }),
    );
    assert.throws(() => installArtifact(artifact, output, identity), undefined, key);
  }
  const { artifact, output } = fixture(context);
  writeFileSync(join(artifact, "fallow"), "changed");
  assert.throws(() => installArtifact(artifact, output, identity));
});
test("artifact rejects symlink directory and destination escapes", (context) => {
  const { root, artifact, output } = fixture(context);
  const link = join(root, "link");
  symlinkSync(artifact, link);
  assert.throws(() => installArtifact(link, output, identity));
  mkdirSync(join(root, "destination"));
  symlinkSync(join(root, "destination"), join(root, "escape"));
  assert.throws(() => installArtifact(artifact, join(root, "escape", "fallow"), identity));
});

test("artifact rejects missing files and symlink binary or manifest", (context) => {
  for (const file of ["fallow", "manifest.json"]) {
    const { root, artifact, output } = fixture(context);
    const replacement = join(root, "replacement");
    writeFileSync(replacement, readFileSync(join(artifact, file)));
    unlinkSync(join(artifact, file));
    assert.throws(() => installArtifact(artifact, output, identity));
    symlinkSync(replacement, join(artifact, file));
    assert.throws(() => installArtifact(artifact, output, identity));
  }
});

test("compiler boundary requires absent target rather than empty target", () => {
  assert.doesNotThrow(() =>
    checkCompilerEnvironment({
      RUSTFLAGS: "",
      CARGO_ENCODED_RUSTFLAGS: "",
      CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS: "",
    }),
  );
  for (const target of ["", "aarch64-unknown-linux-gnu"])
    assert.throws(
      () => checkCompilerEnvironment({ CARGO_BUILD_TARGET: target }),
      /target must be unset/,
    );
  for (const key of [
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS",
  ])
    assert.throws(() => checkCompilerEnvironment({ [key]: "override" }), /Compiler override/);
});
