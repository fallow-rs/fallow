import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { basename, resolve } from "node:path";

import { resolveEvidencePath } from "./locations.mjs";

const REVIEWED_STATUSES = new Set(["confirmed-used", "contract-preserved"]);
const OVERRULE_FILE = "overrule.json";
const CONTEXT_LINES = 3;
const USE_CONTEXT_LINES = 2;

const REVIEW_METHOD =
  "Two or more reviewers checked each record against the pinned source. Confirmed uses were checked for a concrete source use of the exact member, with accessor reads and writes credited to the matching half. Contract-preserved records were checked for the declaration, the heritage edge, the signature, and a required interface member, an abstract member, or an override that changes the behavior of the class.";

const REVIEW_CRITERIA = `Each record is a class member that syntactic analysis reported as unused.
The TypeScript checker then confirmed a use (\`confirmed-used\`), or found an
inherited contract that keeps the member (\`contract-preserved\`). A
\`known-unused claim\` is an earlier claim that the member has no use.

- Approve \`confirmed-used\` only when a listed use is a concrete source use of
  this exact member of this class, an instance of it, or a subclass that
  inherits it. A read uses a getter, and a write uses a setter.
- Approve \`contract-preserved\` only when the inherited declaration is real,
  the class or an ancestor has the \`implements\` or \`extends\` edge, and the
  member is a required interface member, implements an abstract member, or
  overrides behavior.
- Approve a \`known-unused claim\` only when a search of the fixture finds no
  use other than the declaration.
- Otherwise reject, with the reason.

Write \`<reviews dir>/<project>/<reviewer>.json\`:
\`{ "records": [{ "key": "tac_...", "verdict": "approve" | "reject", "reason": "...", "feature_buckets": [] }] }\`
with one entry for each record. \`feature_buckets\` lists, for a
\`confirmed-used\` record, each allowed bucket that the use depends on.`;

const knownUnusedClaims = (decisions) => Object(decisions.known_unused);

/** The ledger entries that a source review must cover, sorted by key. */
export const reviewableEntries = (ledger, decisions) =>
  ledger.candidates
    .filter(
      (entry) =>
        REVIEWED_STATUSES.has(entry.semantic_status) ||
        Object.hasOwn(knownUnusedClaims(decisions), entry.key),
    )
    .toSorted((left, right) => left.key.localeCompare(right.key));

const sourceContext = (readLines, path, line, radius) => {
  const lines = readLines(path);
  if (lines === null) return "    (source file is missing)";
  const first = Math.max(1, line - radius);
  const last = Math.min(lines.length, line + radius);
  return Array.from({ length: last - first + 1 }, (_, offset) => {
    const number = first + offset;
    const marker = number === line ? ">" : " ";
    return `    ${marker}${String(number).padStart(6)} | ${lines[number - 1]}`;
  }).join("\n");
};

const fenced = (label, location, context) =>
  `${label} ${location.path}:${location.line}:${location.col}\n\`\`\`\n${context}\n\`\`\``;

const renderRecord = (entry, claims, readLines) => {
  const { candidate, source_evidence: evidence, contract } = entry;
  const claim = claims[entry.key];
  const status = claim ? "known-unused claim" : entry.semantic_status;
  const parts = [
    `### ${entry.key} [${status}] ${candidate.parent_name}.${candidate.member_name} (${candidate.kind})`,
    fenced(
      "Declaration",
      evidence.declaration,
      sourceContext(readLines, evidence.declaration.path, evidence.declaration.line, CONTEXT_LINES),
    ),
  ];
  if (claim) parts.push(`Claim to verify: ${claim}`);
  for (const use of evidence.uses) {
    parts.push(fenced("Use", use, sourceContext(readLines, use.path, use.line, USE_CONTEXT_LINES)));
  }
  if (contract) {
    const declaration = contract.declaration;
    parts.push(
      fenced(
        `Contract (${contract.relation}, optional=${contract.optional})`,
        declaration,
        sourceContext(readLines, declaration.path, declaration.line, CONTEXT_LINES),
      ),
    );
  }
  return parts.join("\n");
};

/**
 * The review packet of one project: the review criteria, the output format,
 * and each record with the source lines around its declaration, uses and
 * contract.
 */
const renderReviewPacket = ({ project, root, entries, claims, readLines }) =>
  [
    `# Type-aware review packet: ${project.id}`,
    "",
    `Fixture root: ${root}`,
    `Allowed feature buckets: ${project.feature_buckets.join(", ") || "none"}`,
    "",
    REVIEW_CRITERIA,
    "",
    ...entries.map((entry) => `${renderRecord(entry, claims, readLines)}\n`),
  ].join("\n");

const lineReader = (root, dependencyRoot) => (path) => {
  const sourcePath = resolveEvidencePath(root, path, dependencyRoot);
  if (sourcePath === null || !existsSync(sourcePath)) return null;
  return readFileSync(sourcePath, "utf8").split(/\r?\n/u);
};

/** Writes one review packet for each project with reviewable records. */
export const writeReviewPackets = ({
  ledger,
  decisions,
  manifest,
  rootFor,
  dependencyRoot,
  outDir,
}) => {
  const entries = reviewableEntries(ledger, decisions);
  const directory = resolve(outDir, "review");
  mkdirSync(directory, { recursive: true });
  return manifest.projects
    .map((project) => ({ project, records: entries.filter((e) => e.project_id === project.id) }))
    .filter(({ records }) => records.length > 0)
    .map(({ project, records }) => {
      const root = rootFor(project);
      const packet = renderReviewPacket({
        project,
        root,
        entries: records,
        claims: knownUnusedClaims(decisions),
        readLines: lineReader(root, dependencyRoot),
      });
      const path = resolve(directory, `${project.id}.md`);
      writeFileSync(path, packet);
      return path;
    });
};

const readReviewerFiles = (reviewsDir, projectId, fail) => {
  const directory = resolve(reviewsDir, projectId);
  const files = existsSync(directory)
    ? readdirSync(directory)
        .filter((name) => name.endsWith(".json"))
        .toSorted()
    : [];
  if (files.length < 2) fail(`${projectId} needs two or more reviewer files in ${directory}`);
  if (files.includes(OVERRULE_FILE))
    fail(`${OVERRULE_FILE} belongs in ${reviewsDir}, not in ${directory}`);
  const reviewers = files.map((name) => {
    const records = Object(JSON.parse(readFileSync(resolve(directory, name), "utf8"))).records;
    if (!Array.isArray(records)) fail(`${projectId}/${name} must have a records array`);
    const byKey = new Map(records.map((r) => [r.key, r]));
    if (byKey.size !== records.length) fail(`${projectId}/${name} repeats a record key`);
    return { reviewer: basename(name, ".json"), records: byKey, text: JSON.stringify(records) };
  });
  if (new Set(reviewers.map(({ text }) => text)).size !== reviewers.length) {
    fail(`${projectId} has identical reviewer files; each reviewer must review separately`);
  }
  return reviewers;
};

const validBuckets = (buckets) =>
  buckets === undefined ||
  (Array.isArray(buckets) && buckets.every((bucket) => typeof bucket === "string"));

const requireCompleteReview = (projectId, reviewer, keys, fail) => {
  const expected = new Set(keys);
  const missing = keys.filter((key) => !reviewer.records.has(key));
  const unknown = [...reviewer.records.keys()].filter((key) => !expected.has(key));
  if (missing.length > 0 || unknown.length > 0) {
    fail(
      `${projectId}/${reviewer.reviewer} must review exactly the packet records (missing: ${missing.join(", ") || "none"}; unknown: ${unknown.join(", ") || "none"})`,
    );
  }
  for (const [key, record] of reviewer.records) {
    if (!["approve", "reject"].includes(record.verdict)) {
      fail(`${projectId}/${reviewer.reviewer} has an invalid verdict for ${key}`);
    }
    if (!validBuckets(record.feature_buckets)) {
      fail(
        `${projectId}/${reviewer.reviewer} must give feature_buckets of ${key} as an array of strings`,
      );
    }
  }
};

const rejectionsOf = (key, reviewers) =>
  reviewers
    .filter(({ records }) => records.get(key).verdict === "reject")
    .map(({ reviewer, records }) => ({ reviewer, reason: records.get(key).reason ?? "" }));

const agreedBuckets = (key, reviewers, allowed) =>
  allowed
    .filter((bucket) =>
      reviewers.every(({ records }) => (records.get(key).feature_buckets ?? []).includes(bucket)),
    )
    .toSorted();

const readOverrules = (reviewsDir) => {
  const path = resolve(reviewsDir, OVERRULE_FILE);
  return existsSync(path) ? Object(JSON.parse(readFileSync(path, "utf8"))) : {};
};

const projectVerdicts = ({ project, entries, reviewers, overrules, fail }) => {
  const overruled = {};
  const featureOverrides = {};
  for (const entry of entries) {
    const rejections = rejectionsOf(entry.key, reviewers);
    if (rejections.length > 0) {
      const reason = overrules[entry.key];
      if (typeof reason !== "string" || reason.trim() === "") {
        const detail = rejections.map((r) => `${r.reviewer}: ${r.reason}`).join("; ");
        fail(
          `${entry.key} is rejected (${detail}); fix the cause or record a reason in ${OVERRULE_FILE}`,
        );
      }
      overruled[entry.key] = { reason, rejected_by: rejections.map((r) => r.reviewer) };
    }
    if (entry.semantic_status !== "confirmed-used") continue;
    const buckets = agreedBuckets(entry.key, reviewers, project.feature_buckets);
    if (buckets.length > 0) featureOverrides[entry.key] = buckets;
  }
  return { overruled, featureOverrides };
};

/**
 * Returns the adjudication decisions with the independent reviews, feature
 * overrides and overruled rejections that the reviewer files give.
 */
export const recordReviews = ({
  ledger,
  decisions,
  manifest,
  reviewsDir,
  digests,
  reviewedOn,
  fail,
}) => {
  const entries = reviewableEntries(ledger, decisions);
  const overrules = readOverrules(reviewsDir);
  const reviews = [];
  const overruled = {};
  const featureOverrides = {};
  for (const project of manifest.projects) {
    const projectEntries = entries.filter((entry) => entry.project_id === project.id);
    if (projectEntries.length === 0) continue;
    const reviewers = readReviewerFiles(reviewsDir, project.id, fail);
    const keys = projectEntries.map(({ key }) => key);
    reviewers.forEach((reviewer) => requireCompleteReview(project.id, reviewer, keys, fail));
    const verdicts = projectVerdicts({
      project,
      entries: projectEntries,
      reviewers,
      overrules,
      fail,
    });
    Object.assign(overruled, verdicts.overruled);
    Object.assign(featureOverrides, verdicts.featureOverrides);
    const contractEntries = projectEntries.filter(({ semantic_status: s }) =>
      REVIEWED_STATUSES.has(s),
    );
    if (contractEntries.length === 0) continue;
    reviews.push({
      project_id: project.id,
      candidate_count: contractEntries.length,
      candidate_set_sha256: digests.candidateSetDigest(contractEntries.map(({ key }) => key)),
      evidence_sha256: digests.independentReviewDigest(contractEntries),
      reviewer: `independent-source-review-${project.id}`,
      method: REVIEW_METHOD,
      verdict: "approved",
    });
  }
  const { overruled_rejections: _previous, ...rest } = decisions;
  return {
    ...rest,
    reviewed_on: reviewedOn,
    feature_overrides: featureOverrides,
    independent_reviews: reviews,
    ...(Object.keys(overruled).length > 0 ? { overruled_rejections: overruled } : {}),
  };
};
