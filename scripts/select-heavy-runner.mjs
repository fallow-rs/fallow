import { appendFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const GITHUB_RUNNER = "ubuntu-latest";
const BLACKSMITH_RUNNER = "blacksmith-4vcpu-ubuntu-2404";
const REPOSITORY = "fallow-rs/fallow";
const MAX_BUDGET_CREDITS = 8000;
const MIRI_SLOT_CREDITS = 150;
const MIRI_LEGACY_BUDGET = 2400;
// Full job timeouts at 2 credits/minute plus 30 credits for overhead.
const SLOT_CREDITS = new Map([
  ["ci.yml/check", 90],
  ["release-validation.yml/drift-full", 210],
  ["release.yml/drift-full", 210],
]);
const HEAVY_KEYS = "allocations,budgetCredits,month,priorReservedCredits";
const WINDOW_KEYS = "firstRunNumber,job,slots,workflow";
const MIRI_LEGACY_KEYS = "firstRunNumber,month,slots";
const MIRI_EXTENDED_KEYS = "budgetCredits,firstRunNumber,month,priorReservedCredits,slots";
const positiveInteger = (value) => Number.isSafeInteger(value) && value > 0;
const objectKeys = (value) =>
  value !== null && typeof value === "object" && !Array.isArray(value)
    ? Object.keys(value).toSorted().join(",")
    : null;
const validMonth = (month) =>
  typeof month === "string" && /^[1-9]\d{3}-(0[1-9]|1[0-2])$/.test(month);
const validWindow = (window) =>
  positiveInteger(window.firstRunNumber) &&
  positiveInteger(window.slots) &&
  window.slots - 1 <= Number.MAX_SAFE_INTEGER - window.firstRunNumber;
const validBudget = (budget, prior) =>
  positiveInteger(budget) &&
  budget <= MAX_BUDGET_CREDITS &&
  Number.isSafeInteger(prior) &&
  prior >= 0 &&
  prior <= budget;

const miriReservation = (raw, month) => {
  if (raw === undefined || raw === "") return 0;
  const record = JSON.parse(raw);
  const keys = objectKeys(record);
  if (keys !== MIRI_LEGACY_KEYS && keys !== MIRI_EXTENDED_KEYS) return null;
  if (!validMonth(record.month) || record.month > month || !validWindow(record)) return null;
  const budget = keys === MIRI_LEGACY_KEYS ? MIRI_LEGACY_BUDGET : record.budgetCredits;
  const prior = keys === MIRI_LEGACY_KEYS ? 0 : record.priorReservedCredits;
  if (
    !validBudget(budget, prior) ||
    record.slots > Math.floor((budget - prior) / MIRI_SLOT_CREDITS)
  ) {
    return null;
  }
  return record.month === month ? prior + record.slots * MIRI_SLOT_CREDITS : 0;
};

const parseAllocation = (environment, now) => {
  try {
    if (!Number.isFinite(now.getTime())) return null;
    const month = now.toISOString().slice(0, 7);
    const record = JSON.parse(environment.BLACKSMITH_HEAVY_ALLOCATION);
    if (
      objectKeys(record) !== HEAVY_KEYS ||
      !validMonth(record.month) ||
      record.month !== month ||
      !validBudget(record.budgetCredits, record.priorReservedCredits) ||
      !Array.isArray(record.allocations) ||
      record.allocations.length === 0
    )
      return null;
    const miri = miriReservation(environment.BLACKSMITH_MIRI_ALLOCATION, month);
    if (miri === null || record.priorReservedCredits < miri) return null;
    const seen = new Set();
    let reserved = record.priorReservedCredits;
    for (const window of record.allocations) {
      if (
        objectKeys(window) !== WINDOW_KEYS ||
        !validWindow(window) ||
        typeof window.workflow !== "string" ||
        typeof window.job !== "string"
      )
        return null;
      const pair = `${window.workflow}/${window.job}`;
      const cost = SLOT_CREDITS.get(pair);
      if (
        !cost ||
        seen.has(pair) ||
        window.slots > Math.floor((record.budgetCredits - reserved) / cost)
      ) {
        return null;
      }
      seen.add(pair);
      reserved += window.slots * cost;
    }
    return record.allocations;
  } catch {
    return null;
  }
};

const trustedWorkflow = (environment) => {
  if (
    environment.GITHUB_REPOSITORY !== REPOSITORY ||
    environment.GITHUB_ACTOR === "dependabot[bot]" ||
    environment.GITHUB_RUN_ATTEMPT !== "1"
  )
    return null;
  const match = environment.GITHUB_WORKFLOW_REF?.match(
    /^fallow-rs\/fallow\/\.github\/workflows\/(ci\.yml|release-validation\.yml|release\.yml)@(.+)$/,
  );
  if (!match || match[2] !== environment.GITHUB_REF) return null;
  const workflow = match[1];
  const event = environment.GITHUB_EVENT_NAME;
  const main = environment.GITHUB_REF === "refs/heads/main";
  if (workflow === "ci.yml" && environment.HEAVY_JOB === "check") {
    if (
      (event === "push" && main) ||
      (event === "pull_request" &&
        /^refs\/pull\/[1-9]\d*\/merge$/.test(environment.GITHUB_REF) &&
        environment.HEAVY_HEAD_REPOSITORY === REPOSITORY)
    )
      return workflow;
    return null;
  }
  if (
    environment.HEAVY_JOB === "drift-full" &&
    main &&
    ((workflow === "release-validation.yml" && ["schedule", "workflow_dispatch"].includes(event)) ||
      (workflow === "release.yml" && event === "workflow_dispatch"))
  )
    return workflow;
  return null;
};

const decideRunner = (environment, now) => {
  const workflow = trustedWorkflow(environment);
  if (!workflow)
    return {
      runner: GITHUB_RUNNER,
      reason: "GitHub: this workflow, event or attempt is not eligible.",
    };
  const windows = parseAllocation(environment, now);
  if (!windows)
    return {
      runner: GITHUB_RUNNER,
      reason: "GitHub: the shared allocation or Miri accounting is absent, invalid or expired.",
    };
  const runNumber = Number(environment.GITHUB_RUN_NUMBER);
  const window = windows.find(
    (entry) => entry.workflow === workflow && entry.job === environment.HEAVY_JOB,
  );
  if (
    !/^[1-9]\d*$/.test(environment.GITHUB_RUN_NUMBER ?? "") ||
    !positiveInteger(runNumber) ||
    !window ||
    runNumber < window.firstRunNumber ||
    runNumber - window.firstRunNumber >= window.slots
  )
    return {
      runner: GITHUB_RUNNER,
      reason: "GitHub: this workflow run has no reserved heavy-job slot.",
    };
  return {
    runner: BLACKSMITH_RUNNER,
    reason: `Blacksmith: ${workflow}/${environment.HEAVY_JOB} uses one reserved slot (${SLOT_CREDITS.get(`${workflow}/${environment.HEAVY_JOB}`)} credits).`,
  };
};

/** Select the runner for a bounded, manually accounted heavy-job allocation. */
export const selectHeavyRunner = (environment, now = new Date()) =>
  decideRunner(environment, now).runner;

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const decision = decideRunner(process.env, new Date());
  if (process.env.GITHUB_STEP_SUMMARY) {
    appendFileSync(
      process.env.GITHUB_STEP_SUMMARY,
      `### Heavy validation runner\n\n${decision.reason}\n`,
    );
  }
  console.log(decision.reason);
  appendFileSync(process.env.GITHUB_OUTPUT, `runner=${decision.runner}\n`);
}
