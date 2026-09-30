import assert from "node:assert/strict";
import { test } from "node:test";
import { selectMiriRunner } from "./select-miri-runner.mjs";

const now = new Date("2026-09-29T12:00:00Z");
const eligible = {
  BLACKSMITH_MIRI_ALLOCATION: JSON.stringify({ month: "2026-09", firstRunNumber: 100, slots: 2 }),
  GITHUB_REPOSITORY: "fallow-rs/fallow",
  GITHUB_EVENT_NAME: "push",
  GITHUB_REF: "refs/heads/main",
  GITHUB_ACTOR: "maintainer",
  GITHUB_RUN_ATTEMPT: "1",
  GITHUB_RUN_NUMBER: "100",
};

test("an allocated main run uses Blacksmith while an unconfigured run stays on GitHub", () => {
  assert.equal(selectMiriRunner(eligible, now), "blacksmith-4vcpu-ubuntu-2404");
  assert.equal(
    selectMiriRunner({ ...eligible, BLACKSMITH_MIRI_ALLOCATION: "" }, now),
    "ubuntu-latest",
  );
});

test("only the original official main or internal PR attempt may consume a reservation", () => {
  assert.equal(
    selectMiriRunner(
      { ...eligible, GITHUB_EVENT_NAME: "pull_request", MIRI_HEAD_REPOSITORY: "fallow-rs/fallow" },
      now,
    ),
    "blacksmith-4vcpu-ubuntu-2404",
  );
  for (const override of [
    { GITHUB_REPOSITORY: "contributor/fallow" },
    { GITHUB_EVENT_NAME: "pull_request", MIRI_HEAD_REPOSITORY: "contributor/fallow" },
    { GITHUB_EVENT_NAME: "pull_request" },
    { GITHUB_EVENT_NAME: "pull_request_target" },
    { GITHUB_EVENT_NAME: "workflow_dispatch" },
    { GITHUB_REF: "refs/heads/topic" },
    { GITHUB_ACTOR: "dependabot[bot]" },
    { GITHUB_RUN_ATTEMPT: "2" },
    { GITHUB_RUN_ATTEMPT: "01" },
    { GITHUB_RUN_ATTEMPT: "" },
  ]) {
    assert.equal(
      selectMiriRunner({ ...eligible, ...override }, now),
      "ubuntu-latest",
      JSON.stringify(override),
    );
  }
});

test("allocation parsing fails closed and cannot renew itself in a later month", () => {
  assert.equal(selectMiriRunner(eligible, now), "blacksmith-4vcpu-ubuntu-2404");
  for (const allocation of [
    "",
    "{",
    "null",
    "[]",
    "true",
    ...[
      {},
      { month: "2026-09", firstRunNumber: 100, slots: 0 },
      { month: "2026-09", firstRunNumber: 100, slots: 17 },
      { month: "2026-09", firstRunNumber: 100, slots: 1.5 },
      { month: "2026-09", firstRunNumber: 100, slots: "2" },
      { month: "2026-09", firstRunNumber: 0, slots: 2 },
      { month: "2026-09", firstRunNumber: -1, slots: 2 },
      { month: "2026-09", firstRunNumber: 100.5, slots: 2 },
      { month: "2026-09", firstRunNumber: "100", slots: 2 },
      { month: "2026-09", firstRunNumber: 9007199254740992, slots: 2 },
      { month: "2026-09", firstRunNumber: 9007199254740991, slots: 2 },
      { month: "2026-13", firstRunNumber: 100, slots: 2 },
      { month: "2026-9", firstRunNumber: 100, slots: 2 },
      { month: "2026-09", firstRunNumber: 100, slots: 2, renew: true },
    ].map((value) => JSON.stringify(value)),
  ]) {
    assert.equal(
      selectMiriRunner({ ...eligible, BLACKSMITH_MIRI_ALLOCATION: allocation }, now),
      "ubuntu-latest",
      allocation,
    );
  }
  assert.equal(selectMiriRunner(eligible, new Date("2026-10-01T00:00:00Z")), "ubuntu-latest");
  assert.equal(selectMiriRunner(eligible, new Date("2026-08-31T23:59:59Z")), "ubuntu-latest");
  assert.equal(selectMiriRunner(eligible, new Date("invalid")), "ubuntu-latest");
});

test("distinct concurrent CI run numbers consume only their disjoint reserved slots", () => {
  const allocation = JSON.stringify({ month: "2026-09", firstRunNumber: 100, slots: 16 });
  const selected = Array.from({ length: 20 }, (_, offset) => offset + 98).filter(
    (run) =>
      selectMiriRunner(
        { ...eligible, BLACKSMITH_MIRI_ALLOCATION: allocation, GITHUB_RUN_NUMBER: String(run) },
        now,
      ) === "blacksmith-4vcpu-ubuntu-2404",
  );
  assert.deepEqual(
    selected,
    [100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115],
  );
  for (const run of ["", "0", "-1", "100.5", "1e2", "0100", "100\n", "9007199254740992"]) {
    assert.equal(
      selectMiriRunner({ ...eligible, GITHUB_RUN_NUMBER: run }, now),
      "ubuntu-latest",
      run,
    );
  }
});

test("the Actions entrypoint writes its decision and a summary without disclosing allocation data", async () => {
  const { existsSync, mkdtempSync, readFileSync, rmSync } = await import("node:fs");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");
  const { spawnSync } = await import("node:child_process");
  const directory = mkdtempSync(join(tmpdir(), "miri-runner-"));
  try {
    const output = join(directory, "output");
    const summary = join(directory, "summary");
    const result = spawnSync(process.execPath, ["scripts/select-miri-runner.mjs"], {
      encoding: "utf8",
      env: {
        ...process.env,
        ...eligible,
        BLACKSMITH_MIRI_ALLOCATION: "do-not-log-this-invalid-allocation",
        GITHUB_OUTPUT: output,
        GITHUB_STEP_SUMMARY: summary,
      },
    });
    const selectedOutput = join(directory, "selected-output");
    const selected = spawnSync(process.execPath, ["scripts/select-miri-runner.mjs"], {
      encoding: "utf8",
      env: {
        ...process.env,
        ...eligible,
        BLACKSMITH_MIRI_ALLOCATION: JSON.stringify({
          month: new Date().toISOString().slice(0, 7),
          firstRunNumber: 100,
          slots: 1,
        }),
        GITHUB_OUTPUT: selectedOutput,
        GITHUB_STEP_SUMMARY: join(directory, "selected-summary"),
      },
    });
    assert.equal(selected.status, 0, selected.stderr);
    assert.equal(readFileSync(selectedOutput, "utf8"), "runner=blacksmith-4vcpu-ubuntu-2404\n");
    assert.equal(result.status, 0, result.stderr);
    assert.equal(existsSync(output), true, "entrypoint must emit a runner decision");
    const emitted = readFileSync(output, "utf8");
    assert.equal(emitted, "runner=ubuntu-latest\n");
    assert.match(readFileSync(summary, "utf8"), /GitHub.*allocation/);
    assert.doesNotMatch(
      result.stdout + result.stderr + readFileSync(summary, "utf8"),
      /do-not-log-this/,
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("extended allocations reserve only remaining monthly budget", () => {
  const allocation = {
    month: "2026-09",
    firstRunNumber: 100,
    slots: 1,
    budgetCredits: 300,
    priorReservedCredits: 150,
  };
  const select = (override = {}, date = now) =>
    selectMiriRunner(
      { ...eligible, BLACKSMITH_MIRI_ALLOCATION: JSON.stringify({ ...allocation, ...override }) },
      date,
    );
  assert.equal(
    select(),
    "blacksmith-4vcpu-ubuntu-2404",
    "one slot exactly fills the remaining envelope",
  );
  assert.equal(select({ slots: 2 }), "ubuntu-latest", "prior windows cannot be recycled");
  assert.equal(
    select({ slots: 53, budgetCredits: 8000, priorReservedCredits: 50 }),
    "blacksmith-4vcpu-ubuntu-2404",
  );
  assert.equal(
    select({ slots: 54, budgetCredits: 8000, priorReservedCredits: 50 }),
    "ubuntu-latest",
  );
  assert.equal(
    select({ priorReservedCredits: 0, budgetCredits: 150 }),
    "blacksmith-4vcpu-ubuntu-2404",
    "zero prior reservations are valid",
  );
  for (const override of [
    { budgetCredits: 8001 },
    { budgetCredits: 0 },
    { budgetCredits: -1 },
    { budgetCredits: 150.5 },
    { budgetCredits: "300" },
    { budgetCredits: Number.MAX_SAFE_INTEGER + 1 },
    { priorReservedCredits: -1 },
    { priorReservedCredits: 150.5 },
    { priorReservedCredits: "150" },
    { priorReservedCredits: 301 },
    { priorReservedCredits: Number.MAX_SAFE_INTEGER + 1 },
    { slots: Number.MAX_SAFE_INTEGER },
    { slots: 1.5 },
    { firstRunNumber: Number.MAX_SAFE_INTEGER, slots: 2 },
    { budgetCredits: undefined },
    { priorReservedCredits: undefined },
    { runner: "blacksmith-32vcpu-ubuntu-2404" },
  ])
    assert.equal(select(override), "ubuntu-latest", JSON.stringify(override));
  for (const date of [
    new Date("2026-08-31T23:59:59Z"),
    new Date("2026-10-01T00:00:00Z"),
    new Date("invalid"),
  ])
    assert.equal(select({}, date), "ubuntu-latest");
});

test("a failed summary cannot publish a Blacksmith admission", async () => {
  const { existsSync, mkdtempSync, readFileSync, rmSync } = await import("node:fs");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");
  const { spawnSync } = await import("node:child_process");
  const directory = mkdtempSync(join(tmpdir(), "miri-failed-summary-"));
  try {
    const output = join(directory, "output");
    const result = spawnSync(process.execPath, ["scripts/select-miri-runner.mjs"], {
      encoding: "utf8",
      env: {
        ...process.env,
        ...eligible,
        BLACKSMITH_MIRI_ALLOCATION: JSON.stringify({
          month: new Date().toISOString().slice(0, 7),
          firstRunNumber: 100,
          slots: 1,
        }),
        GITHUB_OUTPUT: output,
        GITHUB_STEP_SUMMARY: directory,
      },
    });
    assert.notEqual(result.status, 0, "writing to a directory must fail");
    assert.equal(
      existsSync(output) ? readFileSync(output, "utf8") : "",
      "",
      "failed selection cannot publish an admission",
    );
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});

test("allocation intervals reject overflow while accepting an exact safe endpoint", () => {
  const select = (firstRunNumber) =>
    selectMiriRunner(
      {
        ...eligible,
        GITHUB_RUN_NUMBER: String(Number.MAX_SAFE_INTEGER),
        BLACKSMITH_MIRI_ALLOCATION: JSON.stringify({
          month: "2026-09",
          firstRunNumber,
          slots: 2,
          budgetCredits: 300,
          priorReservedCredits: 0,
        }),
      },
      now,
    );
  assert.equal(select(Number.MAX_SAFE_INTEGER - 1), "blacksmith-4vcpu-ubuntu-2404");
  assert.equal(
    select(Number.MAX_SAFE_INTEGER),
    "ubuntu-latest",
    "overflow must fail even when the budget fits",
  );
});
