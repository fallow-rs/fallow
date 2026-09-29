import { appendFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const GITHUB_RUNNER = "ubuntu-latest";
const BLACKSMITH_RUNNER = "blacksmith-4vcpu-ubuntu-2404";
const REPOSITORY = "fallow-rs/fallow";
// Reserve the 60-minute job timeout at 2 credits/minute plus 30 for overhead.
const SLOT_CREDITS = 150;
const TRIAL_CREDITS = 2400;
const MAX_SLOTS = Math.floor(TRIAL_CREDITS / SLOT_CREDITS);
const isPositiveInteger = (value) => Number.isSafeInteger(value) && value > 0;

const parseAllocation = (raw, now) => {
  try {
    const allocation = JSON.parse(raw);
    if (
      allocation === null ||
      typeof allocation !== "object" ||
      Array.isArray(allocation) ||
      Object.keys(allocation).toSorted().join(",") !== "firstRunNumber,month,slots" ||
      typeof allocation.month !== "string" ||
      !/^[1-9]\d{3}-(0[1-9]|1[0-2])$/.test(allocation.month) ||
      !Number.isFinite(now.getTime()) ||
      allocation.month !== now.toISOString().slice(0, 7) ||
      !isPositiveInteger(allocation.firstRunNumber) ||
      !isPositiveInteger(allocation.slots) ||
      allocation.slots > MAX_SLOTS ||
      !Number.isSafeInteger(allocation.firstRunNumber + allocation.slots - 1)
    ) {
      return null;
    }
    return allocation;
  } catch {
    return null;
  }
};

const decideRunner = (environment, now) => {
  const trustedEvent =
    (environment.GITHUB_EVENT_NAME === "push" && environment.GITHUB_REF === "refs/heads/main") ||
    (environment.GITHUB_EVENT_NAME === "pull_request" &&
      environment.MIRI_HEAD_REPOSITORY === REPOSITORY);
  if (
    environment.GITHUB_REPOSITORY !== REPOSITORY ||
    !trustedEvent ||
    environment.GITHUB_ACTOR === "dependabot[bot]" ||
    environment.GITHUB_RUN_ATTEMPT !== "1"
  ) {
    return {
      runner: GITHUB_RUNNER,
      reason: "GitHub: this event or attempt is not eligible for the trial.",
    };
  }
  const allocation = parseAllocation(environment.BLACKSMITH_MIRI_ALLOCATION, now);
  const runNumber = Number(environment.GITHUB_RUN_NUMBER);
  if (!allocation) {
    return {
      runner: GITHUB_RUNNER,
      reason: "GitHub: allocation is absent, invalid, or outside its UTC month.",
    };
  }
  if (
    !/^[1-9]\d*$/.test(environment.GITHUB_RUN_NUMBER ?? "") ||
    !isPositiveInteger(runNumber) ||
    runNumber < allocation.firstRunNumber ||
    runNumber - allocation.firstRunNumber >= allocation.slots
  ) {
    return { runner: GITHUB_RUNNER, reason: "GitHub: this CI run has no reserved trial slot." };
  }
  return {
    runner: BLACKSMITH_RUNNER,
    reason: `Blacksmith: this CI run uses one reserved trial slot (${SLOT_CREDITS} equivalent minutes).`,
  };
};

/** Select the runner for a bounded, manually allocated Miri trial. */
export const selectMiriRunner = (environment, now = new Date()) =>
  decideRunner(environment, now).runner;

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const decision = decideRunner(process.env, new Date());
  appendFileSync(process.env.GITHUB_OUTPUT, `runner=${decision.runner}\n`);
  if (process.env.GITHUB_STEP_SUMMARY) {
    appendFileSync(process.env.GITHUB_STEP_SUMMARY, `### Miri runner\n\n${decision.reason}\n`);
  }
  console.log(decision.reason);
}
