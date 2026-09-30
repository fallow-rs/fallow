import assert from "node:assert/strict";
import { test } from "node:test";
import { selectHeavyRunner } from "./select-heavy-runner.mjs";

const now = new Date("2026-09-30T12:00:00Z");
const allocation = {
  month: "2026-09",
  budgetCredits: 2222,
  priorReservedCredits: 2100,
  allocations: [{ workflow: "ci.yml", job: "check", firstRunNumber: 100, slots: 1 }],
};
const miri = {
  month: "2026-09",
  budgetCredits: 2222,
  priorReservedCredits: 450,
  firstRunNumber: 90,
  slots: 11,
};
const eligible = {
  GITHUB_REPOSITORY: "fallow-rs/fallow",
  GITHUB_EVENT_NAME: "push",
  GITHUB_REF: "refs/heads/main",
  GITHUB_WORKFLOW_REF: "fallow-rs/fallow/.github/workflows/ci.yml@refs/heads/main",
  GITHUB_RUN_NUMBER: "100",
  GITHUB_RUN_ATTEMPT: "1",
  GITHUB_ACTOR: "maintainer",
  HEAVY_JOB: "check",
  BLACKSMITH_HEAVY_ALLOCATION: JSON.stringify(allocation),
  BLACKSMITH_MIRI_ALLOCATION: JSON.stringify(miri),
};
const select = (record = allocation, environment = {}, date = now) =>
  selectHeavyRunner(
    { ...eligible, ...environment, BLACKSMITH_HEAVY_ALLOCATION: JSON.stringify(record) },
    date,
  );

test("Check admits only reserved runs after accounting for the complete Miri window", () => {
  assert.equal(select(), "blacksmith-4vcpu-ubuntu-2404");
  assert.equal(select(allocation, { GITHUB_RUN_NUMBER: "99" }), "ubuntu-latest");
  assert.equal(select(allocation, { GITHUB_RUN_NUMBER: "101" }), "ubuntu-latest");
  assert.equal(select({ ...allocation, priorReservedCredits: 2099 }), "ubuntu-latest");
  assert.equal(select({ ...allocation, budgetCredits: 2189 }), "ubuntu-latest");
  assert.equal(select({ ...allocation, budgetCredits: 2190 }), "blacksmith-4vcpu-ubuntu-2404");
});

test("every window is validated before any admission, including nonmatching windows", () => {
  assert.equal(select(), "blacksmith-4vcpu-ubuntu-2404");
  const valid = allocation.allocations[0];
  for (const window of [
    { ...valid, workflow: ["ci.yml"] },
    { ...valid, job: ["check"] },
    { ...valid, workflow: "unknown.yml" },
    { ...valid, job: "miri" },
    { ...valid, slots: "1" },
    { ...valid, firstRunNumber: "100" },
    { ...valid, slots: 0 },
    { ...valid, firstRunNumber: 0 },
    { ...valid, slots: 1.5 },
    { ...valid, firstRunNumber: -1 },
    { ...valid, slots: Number.MAX_SAFE_INTEGER },
    { ...valid, firstRunNumber: Number.MAX_SAFE_INTEGER, slots: 2 },
    { ...valid, slots: undefined },
    { ...valid, runner: "blacksmith-4vcpu-ubuntu-2404" },
    null,
    [],
  ]) {
    assert.equal(
      select({ ...allocation, allocations: [window] }),
      "ubuntu-latest",
      JSON.stringify(window),
    );
    assert.equal(
      select({ ...allocation, allocations: [valid, window] }),
      "ubuntu-latest",
      "a later invalid record must reject the complete allocation",
    );
  }
  assert.equal(
    select({
      ...allocation,
      budgetCredits: 3000,
      allocations: [valid, { ...valid, workflow: ["release-validation.yml"], job: "drift-full" }],
    }),
    "ubuntu-latest",
    "a coerced later identity must reject the entire record",
  );
  assert.equal(
    select({ ...allocation, allocations: [valid, valid] }),
    "ubuntu-latest",
    "duplicate windows are not reusable credits",
  );
});

test("workflow run-number namespaces share the whole budget and keep separate windows", () => {
  const record = {
    ...allocation,
    priorReservedCredits: 0,
    budgetCredits: 510,
    allocations: [
      { workflow: "ci.yml", job: "check", firstRunNumber: 100, slots: 1 },
      { workflow: "release-validation.yml", job: "drift-full", firstRunNumber: 200, slots: 1 },
      { workflow: "release.yml", job: "drift-full", firstRunNumber: 300, slots: 1 },
    ],
  };
  for (const [workflow, event, run, admitted] of [
    ["ci.yml", "push", "100", true],
    ["release-validation.yml", "schedule", "200", true],
    ["release-validation.yml", "workflow_dispatch", "200", true],
    ["release.yml", "workflow_dispatch", "300", true],
    ["release.yml", "schedule", "300", false],
    ["release.yml", "workflow_dispatch", "200", false],
    ["release-validation.yml", "workflow_dispatch", "300", false],
  ]) {
    const environment = {
      BLACKSMITH_MIRI_ALLOCATION: "",
      GITHUB_WORKFLOW_REF: `fallow-rs/fallow/.github/workflows/${workflow}@refs/heads/main`,
      HEAVY_JOB: workflow === "ci.yml" ? "check" : "drift-full",
      GITHUB_RUN_NUMBER: run,
      GITHUB_EVENT_NAME: event,
    };
    assert.equal(
      select(record, environment),
      admitted ? "blacksmith-4vcpu-ubuntu-2404" : "ubuntu-latest",
    );
    assert.equal(
      select({ ...record, budgetCredits: 509 }, environment),
      "ubuntu-latest",
      "even an unmatched window consumes its full reservation",
    );
  }
});

test("strict shared schema rejects unsafe, partial and extra fields", () => {
  assert.equal(select(), "blacksmith-4vcpu-ubuntu-2404");
  for (const record of [
    null,
    [],
    {},
    ...[
      { month: "2026-9" },
      { month: "2026-13" },
      { month: "0000-09" },
      { month: undefined },
      { budgetCredits: undefined },
      { priorReservedCredits: undefined },
      { allocations: undefined },
      { allocations: [] },
      { allocations: {} },
      { budgetCredits: 8001 },
      { budgetCredits: 0 },
      { budgetCredits: -1 },
      { budgetCredits: "2222" },
      { budgetCredits: 2222.5 },
      { budgetCredits: Number.MAX_SAFE_INTEGER + 1 },
      { priorReservedCredits: -1 },
      { priorReservedCredits: "2100" },
      { priorReservedCredits: 2100.5 },
      { priorReservedCredits: 2223 },
      { priorReservedCredits: Number.MAX_SAFE_INTEGER + 1 },
      { extra: true },
    ].map((override) => ({ ...allocation, ...override })),
  ])
    assert.equal(select(record), "ubuntu-latest", JSON.stringify(record));
  for (const raw of ["", "{", "null"]) {
    assert.equal(
      selectHeavyRunner({ ...eligible, BLACKSMITH_HEAVY_ALLOCATION: raw }, now),
      "ubuntu-latest",
    );
  }
  const largest = { ...allocation, budgetCredits: 8000, priorReservedCredits: 7910 };
  assert.equal(
    select(largest),
    "blacksmith-4vcpu-ubuntu-2404",
    "the documented manual larger-allowance ceiling remains usable",
  );
  const endpoint = {
    ...allocation,
    allocations: [
      { ...allocation.allocations[0], firstRunNumber: Number.MAX_SAFE_INTEGER - 1, slots: 2 },
    ],
    budgetCredits: 2280,
  };
  assert.equal(
    select(endpoint, { GITHUB_RUN_NUMBER: String(Number.MAX_SAFE_INTEGER) }),
    "blacksmith-4vcpu-ubuntu-2404",
  );
  for (const run of ["", "0", "100.0", "1e2", "0100", "-100", " 100", "9007199254740992"]) {
    assert.equal(select(allocation, { GITHUB_RUN_NUMBER: run }), "ubuntu-latest", run);
  }
});

test("Miri accounting validates full reservations, legacy records and UTC expiry", () => {
  assert.equal(select(), "blacksmith-4vcpu-ubuntu-2404");
  const legacy = { month: "2026-09", firstRunNumber: 100, slots: 14 };
  assert.equal(
    select(allocation, { BLACKSMITH_MIRI_ALLOCATION: JSON.stringify(legacy) }),
    "blacksmith-4vcpu-ubuntu-2404",
  );
  for (const override of [
    { slots: 12 },
    { slots: 0 },
    { slots: "11" },
    { slots: 11.5 },
    { month: "2026-10" },
    { month: "2026-9" },
    { budgetCredits: 8001 },
    { budgetCredits: 2099 },
    { budgetCredits: undefined },
    { priorReservedCredits: undefined },
    { priorReservedCredits: "450" },
    { priorReservedCredits: -1 },
    { firstRunNumber: Number.MAX_SAFE_INTEGER, slots: 2 },
    { extra: true },
  ])
    assert.equal(
      select(allocation, { BLACKSMITH_MIRI_ALLOCATION: JSON.stringify({ ...miri, ...override }) }),
      "ubuntu-latest",
      JSON.stringify(override),
    );
  for (const raw of ["{", "null", "[]", "{}", JSON.stringify({ ...legacy, slots: 17 })]) {
    assert.equal(select(allocation, { BLACKSMITH_MIRI_ALLOCATION: raw }), "ubuntu-latest", raw);
  }
  const noPrior = { ...allocation, priorReservedCredits: 0, budgetCredits: 90 };
  for (const raw of ["", undefined, JSON.stringify({ ...miri, month: "2026-08" })]) {
    assert.equal(
      select(noPrior, { BLACKSMITH_MIRI_ALLOCATION: raw }),
      "blacksmith-4vcpu-ubuntu-2404",
    );
  }
  for (const date of [
    new Date("2026-08-31T23:59:59Z"),
    new Date("2026-10-01T00:00:00Z"),
    new Date("invalid"),
  ]) {
    assert.equal(select(allocation, {}, date), "ubuntu-latest");
  }
  assert.equal(
    select(allocation, {}, new Date("2026-09-30T23:59:59Z")),
    "blacksmith-4vcpu-ubuntu-2404",
  );
});

test("untrusted contexts and all reruns fall back even with a reserved window", () => {
  assert.equal(select(), "blacksmith-4vcpu-ubuntu-2404");
  const internal = {
    GITHUB_EVENT_NAME: "pull_request",
    GITHUB_REF: "refs/pull/42/merge",
    GITHUB_WORKFLOW_REF: "fallow-rs/fallow/.github/workflows/ci.yml@refs/pull/42/merge",
    HEAVY_HEAD_REPOSITORY: "fallow-rs/fallow",
  };
  assert.equal(select(allocation, internal), "blacksmith-4vcpu-ubuntu-2404");
  for (const environment of [
    { ...internal, HEAVY_HEAD_REPOSITORY: "fork/fallow" },
    { ...internal, HEAVY_HEAD_REPOSITORY: undefined },
    { ...internal, GITHUB_EVENT_NAME: "pull_request_target" },
    { GITHUB_REPOSITORY: "fork/fallow" },
    { GITHUB_ACTOR: "dependabot[bot]" },
    { GITHUB_RUN_ATTEMPT: "2" },
    { GITHUB_RUN_ATTEMPT: "01" },
    { GITHUB_RUN_ATTEMPT: undefined },
    { GITHUB_EVENT_NAME: "schedule" },
    { GITHUB_EVENT_NAME: "workflow_dispatch" },
    {
      GITHUB_REF: "refs/heads/topic",
      GITHUB_WORKFLOW_REF: "fallow-rs/fallow/.github/workflows/ci.yml@refs/heads/topic",
    },
    { GITHUB_WORKFLOW_REF: undefined },
    { GITHUB_WORKFLOW_REF: "fork/fallow/.github/workflows/ci.yml@refs/heads/main" },
    { GITHUB_WORKFLOW_REF: "fallow-rs/fallow/.github/workflows/unknown.yml@refs/heads/main" },
    { GITHUB_WORKFLOW_REF: "fallow-rs/fallow/.github/workflows/ci.yml@refs/heads/topic" },
    { HEAVY_JOB: "drift-full" },
  ])
    assert.equal(select(allocation, environment), "ubuntu-latest", JSON.stringify(environment));
  const drift = {
    ...allocation,
    allocations: [
      { workflow: "release-validation.yml", job: "drift-full", firstRunNumber: 100, slots: 1 },
    ],
    budgetCredits: 2310,
  };
  const direct = {
    GITHUB_EVENT_NAME: "workflow_dispatch",
    GITHUB_WORKFLOW_REF:
      "fallow-rs/fallow/.github/workflows/release-validation.yml@refs/heads/main",
    HEAVY_JOB: "drift-full",
  };
  assert.equal(select(drift, direct), "blacksmith-4vcpu-ubuntu-2404");
  for (const environment of [
    { ...direct, GITHUB_EVENT_NAME: "push" },
    { ...direct, GITHUB_EVENT_NAME: "workflow_call" },
    {
      ...direct,
      GITHUB_REF: "refs/heads/topic",
      GITHUB_WORKFLOW_REF:
        "fallow-rs/fallow/.github/workflows/release-validation.yml@refs/heads/topic",
    },
    { ...direct, GITHUB_RUN_ATTEMPT: "2" },
  ])
    assert.equal(select(drift, environment), "ubuntu-latest");
});

test("the executable publishes real routing output only after a successful summary", async () => {
  const { spawnSync } = await import("node:child_process");
  const { existsSync, mkdtempSync, readFileSync, rmSync } = await import("node:fs");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");
  const directory = mkdtempSync(join(tmpdir(), "heavy-runner-"));
  try {
    const run = (suffix, raw, summary = join(directory, `summary-${suffix}`)) => {
      const output = join(directory, `output-${suffix}`);
      const result = spawnSync(process.execPath, ["scripts/select-heavy-runner.mjs"], {
        encoding: "utf8",
        env: {
          ...process.env,
          ...eligible,
          BLACKSMITH_HEAVY_ALLOCATION: raw,
          BLACKSMITH_MIRI_ALLOCATION: "",
          GITHUB_OUTPUT: output,
          GITHUB_STEP_SUMMARY: summary,
        },
      });
      return { result, output: existsSync(output) ? readFileSync(output, "utf8") : "", summary };
    };
    const raw = JSON.stringify({
      ...allocation,
      month: new Date().toISOString().slice(0, 7),
      priorReservedCredits: 0,
      budgetCredits: 90,
    });
    const valid = run("valid", raw);
    assert.equal(valid.result.status, 0, valid.result.stderr);
    assert.equal(valid.output, "runner=blacksmith-4vcpu-ubuntu-2404\n");
    assert.match(readFileSync(valid.summary, "utf8"), /ci.yml\/check.*90 credits/);
    const invalid = run("invalid", "{");
    assert.equal(invalid.result.status, 0, invalid.result.stderr);
    assert.equal(invalid.output, "runner=ubuntu-latest\n");
    const failed = run("failed", raw, directory);
    assert.notEqual(failed.result.status, 0);
    assert.equal(failed.output, "", "failed selection cannot publish a partial admission");
  } finally {
    rmSync(directory, { recursive: true, force: true });
  }
});
