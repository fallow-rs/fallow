// No CI job runs the type-aware corpus, so its tracked evidence can drift from
// the analyzer without a visible failure. This check ties the evidence to the
// workspace version: a release bump past the allowed lag fails CI until the
// gate is regenerated or a waiver names the tracking issue and a last version.

const SEMVER = /^(\d+)\.(\d+)\.(\d+)$/;
const ISSUE_URL = /^https:\/\/github\.com\/fallow-rs\/fallow\/issues\/\d+$/;
const REGENERATE =
  "Run the full `npm run type-aware:corpus` gate and commit the regenerated evidence";

const parseVersion = (text) => {
  const match = SEMVER.exec(String(text ?? "").trim());
  return match ? match.slice(1, 4).map(Number) : null;
};

const compareVersions = (left, right) =>
  left.map((part, index) => part - right[index]).find((difference) => difference !== 0) ?? 0;

const validWaiver = (waiver) =>
  [
    ISSUE_URL.test(String(waiver.issue ?? "")),
    parseVersion(waiver.until_version) !== null,
    String(waiver.reason ?? "").trim() !== "",
  ].every(Boolean);

const POLICY_RULES = [
  [(policy) => policy.schema_version === 1, "freshness policy schema_version must be 1"],
  [
    (policy) => Number.isInteger(policy.maximum_minor_lag) && policy.maximum_minor_lag >= 0,
    "freshness policy maximum_minor_lag must be a non-negative integer",
  ],
  [
    (policy) => policy.waiver === undefined || validWaiver(Object(policy.waiver)),
    "freshness waiver needs a fallow issue URL, an until_version and a reason",
  ],
];

const policyErrors = (policy) =>
  POLICY_RULES.filter(([valid]) => !valid(Object(policy))).map(([, message]) => message);

const staleness = (evidence, workspace, maximumMinorLag) => {
  if (evidence[0] !== workspace[0]) return "is from another major version than";
  const lag = workspace[1] - evidence[1];
  return lag > maximumMinorLag ? `is ${lag} minor releases behind` : null;
};

const waiverErrors = (waiver, stale, workspace, workspaceText) => {
  if (!stale) {
    return waiver === undefined
      ? []
      : [`type-aware corpus evidence is fresh; remove the waiver for ${waiver.issue}`];
  }
  if (waiver === undefined) return null;
  if (compareVersions(workspace, parseVersion(waiver.until_version)) <= 0) return [];
  return [
    `type-aware corpus freshness waiver for ${waiver.issue} ended at ${waiver.until_version}; the workspace is ${workspaceText}. ${REGENERATE}, or extend the waiver with a reason.`,
  ];
};

/**
 * Compare the analyzer version that produced the tracked corpus evidence with
 * the workspace version, and return one message per policy violation.
 */
export const evidenceFreshnessErrors = ({ evidenceVersion, workspaceVersion, policy }) => {
  const invalidPolicy = policyErrors(policy);
  if (invalidPolicy.length > 0) return invalidPolicy;
  const evidenceText = String(evidenceVersion ?? "").replace(/^fallow\s+/, "");
  const evidence = parseVersion(evidenceText);
  const workspace = parseVersion(workspaceVersion);
  if (evidence === null) return [`cannot read the evidence version "${evidenceVersion}"`];
  if (workspace === null) return [`cannot read the workspace version "${workspaceVersion}"`];
  const stale = staleness(evidence, workspace, policy.maximum_minor_lag);
  const waived = waiverErrors(policy.waiver, stale, workspace, workspaceVersion);
  if (waived !== null) return waived;
  return [
    `type-aware corpus evidence from fallow ${evidenceText} ${stale} ${workspaceVersion} (maximum lag ${policy.maximum_minor_lag}). ${REGENERATE}, or add a waiver with a tracking issue to benchmarks/type-aware-corpus-freshness.json.`,
  ];
};

const workspacePackageVersion = (cargoToml) => {
  const section = /^\[workspace\.package\]\s*$([\s\S]*?)(?=^\[|(?![\s\S]))/m.exec(cargoToml);
  return /^version\s*=\s*"([^"]+)"/m.exec(section?.[1] ?? "")?.[1];
};

/** Apply the freshness policy to the tracked evidence file and the root Cargo manifest. */
export const trackedEvidenceFreshnessErrors = ({ evidence, cargoToml, policy }) =>
  evidenceFreshnessErrors({
    evidenceVersion: evidence?.generated_from?.discovery_provenance?.fallow?.version,
    workspaceVersion: workspacePackageVersion(cargoToml),
    policy,
  });
