#!/usr/bin/env node

import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const DEFAULT_POSTHOG_HOST = "https://eu.posthog.com";
const DEFAULT_WINDOW_DAYS = 14;
const DEFAULT_MIN_TARGET_EVENTS = 500;
const DEFAULT_MIN_BASELINE_EVENTS = 1000;
const DEFAULT_MAX_DELTA_POINTS = 5;
const DEFAULT_MAX_MULTIPLIER = 1.5;

const REQUIRED_ENV = ["POSTHOG_PERSONAL_API_KEY", "POSTHOG_PROJECT_ID"];

const parseNumber = (value, fallback) => {
  if (value === undefined || value === "") {
    return fallback;
  }
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    throw new Error(`expected a number, got ${value}`);
  }
  return parsed;
};

const parseInteger = (value, fallback) => {
  const parsed = parseNumber(value, fallback);
  if (!Number.isInteger(parsed) || parsed < 0) {
    throw new Error(`expected a non-negative integer, got ${value}`);
  }
  return parsed;
};

const parseArgs = (argv) => {
  const args = {
    targetVersion: process.env.FALLOW_TELEMETRY_TARGET_VERSION ?? null,
    fixturePath: null,
    windowDays: parseInteger(process.env.FALLOW_TELEMETRY_WINDOW_DAYS, DEFAULT_WINDOW_DAYS),
    minTargetEvents: parseInteger(
      process.env.FALLOW_TELEMETRY_MIN_TARGET_EVENTS,
      DEFAULT_MIN_TARGET_EVENTS,
    ),
    minBaselineEvents: parseInteger(
      process.env.FALLOW_TELEMETRY_MIN_BASELINE_EVENTS,
      DEFAULT_MIN_BASELINE_EVENTS,
    ),
    maxDeltaPoints: parseNumber(
      process.env.FALLOW_TELEMETRY_MAX_DELTA_POINTS,
      DEFAULT_MAX_DELTA_POINTS,
    ),
    maxMultiplier: parseNumber(process.env.FALLOW_TELEMETRY_MAX_MULTIPLIER, DEFAULT_MAX_MULTIPLIER),
  };

  for (let index = 2; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = argv[index + 1];
    if (arg === "--target") {
      args.targetVersion = next;
      index += 1;
      continue;
    }
    if (arg === "--fixture") {
      args.fixturePath = next;
      index += 1;
      continue;
    }
    if (arg === "--window-days") {
      args.windowDays = parseInteger(next, args.windowDays);
      index += 1;
      continue;
    }
    if (arg === "--min-target-events") {
      args.minTargetEvents = parseInteger(next, args.minTargetEvents);
      index += 1;
      continue;
    }
    if (arg === "--min-baseline-events") {
      args.minBaselineEvents = parseInteger(next, args.minBaselineEvents);
      index += 1;
      continue;
    }
    if (arg === "--max-delta-points") {
      args.maxDeltaPoints = parseNumber(next, args.maxDeltaPoints);
      index += 1;
      continue;
    }
    if (arg === "--max-multiplier") {
      args.maxMultiplier = parseNumber(next, args.maxMultiplier);
      index += 1;
      continue;
    }
    throw new Error(`unknown argument: ${arg}`);
  }

  if (typeof args.targetVersion !== "string" || args.targetVersion.trim() === "") {
    throw new Error("missing target version, pass --target or FALLOW_TELEMETRY_TARGET_VERSION");
  }
  return { ...args, targetVersion: normalizeVersion(args.targetVersion) };
};

const normalizeVersion = (version) => version.trim().replace(/^v/, "");

const failRate = (row) => (row.events === 0 ? 0 : row.failed / row.events);

const formatPercent = (value) => `${(value * 100).toFixed(2)}%`;

const compareVersions = (left, right) => {
  const a = normalizeVersion(left)
    .split(".")
    .map((part) => Number.parseInt(part, 10));
  const b = normalizeVersion(right)
    .split(".")
    .map((part) => Number.parseInt(part, 10));
  for (let index = 0; index < Math.max(a.length, b.length); index += 1) {
    const diff = (a[index] || 0) - (b[index] || 0);
    if (diff !== 0) {
      return diff;
    }
  }
  return 0;
};

export const parseRows = (rawRows) => {
  if (!Array.isArray(rawRows)) {
    throw new Error("PostHog response did not include an array result");
  }
  return rawRows
    .map((row) => {
      const version = Array.isArray(row) ? row[0] : row.version;
      const events = Number(Array.isArray(row) ? row[1] : row.events);
      const failed = Number(Array.isArray(row) ? row[2] : row.failed);
      if (typeof version !== "string" || version.trim() === "") {
        return null;
      }
      if (!Number.isFinite(events) || events < 0 || !Number.isFinite(failed) || failed < 0) {
        throw new Error(`invalid telemetry row for ${version}`);
      }
      if (failed > events) {
        throw new Error(`failed events exceed total events for ${version}`);
      }
      return {
        version: normalizeVersion(version),
        events,
        failed,
      };
    })
    .filter((row) => row !== null);
};

export const computeGate = ({
  rows,
  targetVersion,
  minTargetEvents,
  minBaselineEvents,
  maxDeltaPoints,
  maxMultiplier,
}) => {
  const normalizedTarget = normalizeVersion(targetVersion);
  const target = rows.find((row) => row.version === normalizedTarget);
  if (!target) {
    return {
      ok: true,
      skipped: true,
      reason: `target ${normalizedTarget} has no telemetry in the window`,
    };
  }
  if (target.events < minTargetEvents) {
    return {
      ok: true,
      skipped: true,
      reason: `target ${normalizedTarget} has fewer than ${minTargetEvents} events`,
      target,
    };
  }

  const baselineRows = rows.filter(
    (row) =>
      row.version !== normalizedTarget &&
      row.events > 0 &&
      compareVersions(row.version, normalizedTarget) < 0,
  );
  const baseline = baselineRows.reduce(
    (acc, row) => ({
      events: acc.events + row.events,
      failed: acc.failed + row.failed,
    }),
    { events: 0, failed: 0 },
  );

  if (baseline.events < minBaselineEvents) {
    return {
      ok: true,
      skipped: true,
      reason: `baseline has fewer than ${minBaselineEvents} events`,
      target,
      baseline,
    };
  }

  const targetRate = failRate(target);
  const baselineRate = failRate(baseline);
  const delta = targetRate - baselineRate;
  const deltaLimit = maxDeltaPoints / 100;
  const multiplierExceeded =
    baselineRate === 0 ? targetRate > 0 : targetRate >= baselineRate * maxMultiplier;
  const failed = delta >= deltaLimit && multiplierExceeded;

  return {
    ok: !failed,
    skipped: false,
    target,
    baseline,
    targetRate,
    baselineRate,
    delta,
    reason: failed
      ? `target fail rate ${formatPercent(targetRate)} exceeds baseline ${formatPercent(baselineRate)}`
      : `target fail rate ${formatPercent(targetRate)} is within baseline ${formatPercent(baselineRate)}`,
  };
};

const telemetryQuery = (windowDays) => `
SELECT
  properties.fallow_version AS version,
  count() AS events,
  sum(if(event = 'workflow_failed', 1, 0)) AS failed
FROM events
WHERE timestamp >= now() - INTERVAL ${windowDays} DAY
  AND event IN ('workflow_completed', 'workflow_failed')
  AND notEmpty(toString(properties.fallow_version))
GROUP BY version
ORDER BY events DESC
LIMIT 200
`;

const posthogRows = async ({ host, projectId, apiKey, windowDays }) => {
  const url = new URL(`/api/projects/${projectId}/query/`, host);
  const response = await fetch(url, {
    method: "POST",
    headers: {
      Authorization: `Bearer ${apiKey}`,
      "Content-Type": "application/json",
    },
    body: JSON.stringify({
      name: "fallow release telemetry fail-rate gate",
      query: {
        kind: "HogQLQuery",
        query: telemetryQuery(windowDays),
      },
    }),
  });
  const body = await response.json().catch(() => null);
  if (!response.ok) {
    const detail = body && typeof body.detail === "string" ? body.detail : response.statusText;
    throw new Error(`PostHog query failed: ${response.status} ${detail}`);
  }
  const result = body?.results ?? body?.result;
  return parseRows(result);
};

const loadRows = async (args) => {
  if (args.fixturePath) {
    return parseRows(JSON.parse(readFileSync(args.fixturePath, "utf8")));
  }
  const missing = REQUIRED_ENV.filter((name) => !process.env[name]);
  if (missing.length > 0) {
    throw new Error(`missing environment: ${missing.join(", ")}`);
  }
  return posthogRows({
    host: process.env.POSTHOG_HOST || DEFAULT_POSTHOG_HOST,
    projectId: process.env.POSTHOG_PROJECT_ID,
    apiKey: process.env.POSTHOG_PERSONAL_API_KEY,
    windowDays: args.windowDays,
  });
};

const main = async (argv) => {
  const args = parseArgs(argv);
  const rows = await loadRows(args);
  const result = computeGate({ rows, ...args });
  if (result.skipped) {
    console.log(`SKIP telemetry fail-rate gate: ${result.reason}`);
    return 0;
  }
  const targetLine = `${result.target.version}: ${formatPercent(result.targetRate)} (${result.target.failed}/${result.target.events})`;
  const baselineLine = `baseline: ${formatPercent(result.baselineRate)} (${result.baseline.failed}/${result.baseline.events})`;
  if (result.ok) {
    console.log(`OK telemetry fail-rate gate: ${targetLine}; ${baselineLine}`);
    return 0;
  }
  console.error(`::error::telemetry fail-rate regression: ${targetLine}; ${baselineLine}`);
  return 1;
};

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  main(process.argv)
    .then((code) => {
      process.exit(code);
    })
    .catch((err) => {
      console.error(`::error::${err.message}`);
      process.exit(1);
    });
}
