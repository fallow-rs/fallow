const TOTAL_KEY = "total_issues";
const KNIP_ROW_META = new Set(["file", "owners"]);

const safeCount = (value) => (Number.isSafeInteger(value) ? value : null);

/**
 * The `total_issues` of a Fallow dead-code report: the root field, else the
 * field of `summary`, else null. The arrays of the report are not counted.
 */
export const fallowIssueTotal = (report) =>
  safeCount(report?.[TOTAL_KEY]) ?? safeCount(report?.summary?.[TOTAL_KEY]);

/**
 * The category counts of `report.summary` that are above 0, sorted by key.
 * `total_issues` is not a category.
 */
export const fallowIssueCounts = (report) => {
  const summary = report?.summary && typeof report.summary === "object" ? report.summary : {};
  return Object.fromEntries(
    Object.entries(summary)
      .filter(([key, value]) => key !== TOTAL_KEY && typeof value === "number" && value > 0)
      .toSorted(([left], [right]) => left.localeCompare(right)),
  );
};

const issueCount = (value) => {
  if (Array.isArray(value)) return value.length;
  if (!value || typeof value !== "object") return 0;
  return Object.values(value).reduce(
    (sum, members) => sum + (Array.isArray(members) ? members.length : 0),
    0,
  );
};

/**
 * The number of findings in a Knip JSON report: each unused file, plus each
 * issue of each row of `issues`. One row holds all issues of one file.
 */
export const knipIssueTotal = (report) => {
  const files = Array.isArray(report?.files) ? report.files.length : 0;
  const rows = Array.isArray(report?.issues) ? report.issues : [];
  return rows.reduce(
    (sum, row) =>
      sum +
      Object.entries(row)
        .filter(([key]) => !KNIP_ROW_META.has(key))
        .reduce((rowSum, [, value]) => rowSum + issueCount(value), 0),
    files,
  );
};
