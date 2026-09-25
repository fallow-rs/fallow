#!/usr/bin/env node

import { createHash } from "node:crypto";
import { existsSync, readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";
import { cargoFallowBin } from "./cargo-target.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const DEFAULT_MANIFEST = resolve(REPO_ROOT, "tests/semantic-clone-corpus/manifest.json");
const DEFAULT_FALLOW_BIN = cargoFallowBin(REPO_ROOT);
const EXPECTED_SCHEMA = "fallow-semantic-clone-conformance/v1";

const fail = (message) => {
  throw new Error(message);
};

const parseArgs = (argv) => {
  const options = {
    check: false,
    fallowBin: process.env.FALLOW_BIN ?? DEFAULT_FALLOW_BIN,
    manifest: DEFAULT_MANIFEST,
    pretty: false,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const argument = argv[index];
    if (argument === "--check") {
      options.check = true;
    } else if (argument === "--fallow-bin") {
      options.fallowBin = argv[index + 1] ?? fail("--fallow-bin requires a path");
      index += 1;
    } else if (argument === "--manifest") {
      options.manifest = argv[index + 1] ?? fail("--manifest requires a path");
      index += 1;
    } else if (argument === "--pretty") {
      options.pretty = true;
    } else {
      fail(`unknown argument: ${argument}`);
    }
  }

  return options;
};

const sha256 = (contents) => createHash("sha256").update(contents).digest("hex");

const assertBoolean = (value, path) => {
  if (typeof value !== "boolean") {
    fail(`${path} must be a boolean`);
  }
};

const loadCandidateEvidence = (manifest, root, caseIds) =>
  (manifest.candidate_evidence ?? []).map((entry) => {
    const evidencePath = resolve(root, entry.file);
    const contents = readFileSync(evidencePath);
    if (sha256(contents) !== entry.sha256) {
      fail(`${entry.file} evidence digest mismatch`);
    }
    const evidence = JSON.parse(contents.toString("utf8"));
    if (evidence.$schema !== "fallow-semantic-clone-model-evidence/v1") {
      fail(`${entry.file} has an unsupported evidence schema`);
    }
    if (evidence.corpus_revision !== manifest.source.revision) {
      fail(`${entry.file} targets a different corpus revision`);
    }
    if (evidence.provider?.source_left_machine !== false) {
      fail(`${entry.file} must state that source stayed local`);
    }
    if (!/^[0-9a-f]{64}$/.test(evidence.provider?.runtime_lock_sha256)) {
      fail(`${entry.file} must pin the complete model runtime dependency graph`);
    }
    if (evidence.provider?.runtime_lock_file !== "runtime/package-lock.json") {
      fail(`${entry.file} must reference the recorded model runtime lock`);
    }
    const runtimeLock = readFileSync(resolve(root, evidence.provider.runtime_lock_file));
    if (sha256(runtimeLock) !== evidence.provider.runtime_lock_sha256) {
      fail(`${entry.file} model runtime lock digest mismatch`);
    }
    if (!/^[0-9a-f]{40}$/.test(evidence.model?.revision)) {
      fail(`${entry.file} must pin a full model revision`);
    }
    if (!Array.isArray(evidence.profiles) || evidence.profiles.length === 0) {
      fail(`${entry.file} must contain evaluation profiles`);
    }

    for (const profile of evidence.profiles) {
      if (!Number.isInteger(profile.dimensions) || profile.dimensions <= 0) {
        fail(`${entry.file}:${profile.id} has invalid dimensions`);
      }
      if (!Number.isFinite(profile.threshold) || profile.threshold < -1 || profile.threshold > 1) {
        fail(`${entry.file}:${profile.id} has an invalid threshold`);
      }
      const profileIds = new Set(profile.cases?.map((testCase) => testCase.id));
      if (
        profileIds.size !== caseIds.size ||
        [...caseIds].some((caseId) => !profileIds.has(caseId))
      ) {
        fail(`${entry.file}:${profile.id} does not cover the locked corpus`);
      }
      if (
        profile.cases.some(
          (testCase) =>
            !Number.isFinite(testCase.similarity) ||
            testCase.similarity < -1 ||
            testCase.similarity > 1,
        )
      ) {
        fail(`${entry.file}:${profile.id} has an invalid similarity`);
      }
    }
    return evidence;
  });

const loadManifest = (manifestPath, { includeCandidateEvidence = true } = {}) => {
  const absolutePath = resolve(manifestPath);
  const manifest = JSON.parse(readFileSync(absolutePath, "utf8"));
  if (manifest.$schema !== EXPECTED_SCHEMA) {
    fail(`unsupported semantic clone manifest schema: ${manifest.$schema}`);
  }
  if (!manifest.source?.public_fixture) {
    fail("semantic clone fixtures must be public");
  }
  if (!/^[0-9a-f]{40}$/.test(manifest.source.revision)) {
    fail("semantic clone source revision must be a full Git commit");
  }
  if (!Number.isFinite(manifest.deterministic_baseline?.minimum_pair_coverage)) {
    fail("deterministic baseline requires minimum_pair_coverage");
  }
  if (!Array.isArray(manifest.cases) || manifest.cases.length === 0) {
    fail("semantic clone manifest requires cases");
  }

  const root = dirname(absolutePath);
  const ids = new Set();
  for (const [caseIndex, testCase] of manifest.cases.entries()) {
    const casePath = `cases[${caseIndex}]`;
    if (typeof testCase.id !== "string" || testCase.id.length === 0) {
      fail(`${casePath}.id must be a non-empty string`);
    }
    if (ids.has(testCase.id)) {
      fail(`duplicate semantic clone case id: ${testCase.id}`);
    }
    ids.add(testCase.id);
    if (!Array.isArray(testCase.files) || testCase.files.length !== 2) {
      fail(`${casePath}.files must contain exactly two files`);
    }
    assertBoolean(testCase.truth?.candidate_worthy, `${casePath}.truth.candidate_worthy`);
    assertBoolean(
      testCase.truth?.behaviorally_equivalent,
      `${casePath}.truth.behaviorally_equivalent`,
    );
    assertBoolean(testCase.truth?.refactor_safe, `${casePath}.truth.refactor_safe`);
    assertBoolean(
      testCase.expected_deterministic_finding,
      `${casePath}.expected_deterministic_finding`,
    );
    if (testCase.truth?.manually_verified !== true) {
      fail(`${casePath}.truth must be manually verified`);
    }

    for (const file of testCase.files) {
      if (file.license !== manifest.source.license) {
        fail(`${testCase.id}:${file.fixture} license differs from the source license`);
      }
      const fixturePath = resolve(root, file.fixture);
      const contents = readFileSync(fixturePath);
      const digest = sha256(contents);
      if (digest !== file.sha256) {
        fail(`${testCase.id}:${file.fixture} digest mismatch`);
      }
    }
  }

  const candidateEvidence = includeCandidateEvidence
    ? loadCandidateEvidence(manifest, root, ids)
    : [];
  return { candidateEvidence, manifest, root };
};

const lineCount = (contents) => {
  const text = contents.toString("utf8");
  return text.length === 0 ? 0 : text.replace(/\n$/, "").split("\n").length;
};

const instanceLines = (instance) => Math.max(0, instance.end_line - instance.start_line + 1);

const evaluatePairCoverage = (cloneGroups, fileNames, fileLineCounts) => {
  let bestCoverage = 0;
  let matchingGroups = 0;

  for (const group of cloneGroups) {
    const coverageByFile = new Map();
    for (const instance of group.instances ?? []) {
      if (!fileNames.includes(instance.file)) {
        continue;
      }
      const coverage = instanceLines(instance) / fileLineCounts.get(instance.file);
      coverageByFile.set(instance.file, Math.max(coverageByFile.get(instance.file) ?? 0, coverage));
    }
    if (coverageByFile.size !== fileNames.length) {
      continue;
    }
    matchingGroups += 1;
    bestCoverage = Math.max(
      bestCoverage,
      Math.min(...fileNames.map((file) => coverageByFile.get(file))),
    );
  }

  return {
    best_pair_coverage: Number(bestCoverage.toFixed(6)),
    matching_groups: matchingGroups,
  };
};

const classifyFinding = (detected, refactorSafe) => {
  if (detected) {
    return refactorSafe ? "true_positive" : "false_positive";
  }
  return refactorSafe ? "false_negative" : "true_negative";
};

const validateFallowReport = (report, testCase, fileNames, fileLineCounts, baseline) => {
  if (report === null || typeof report !== "object" || Array.isArray(report)) {
    fail(`${testCase.id}: fallow report must be an object`);
  }
  if (report.kind !== "dupes") {
    fail(`${testCase.id}: fallow report kind must be dupes`);
  }
  if (report.schema_version !== baseline.schema_version) {
    fail(
      `${testCase.id}: fallow schema ${report.schema_version} differs from locked schema ${baseline.schema_version}`,
    );
  }
  if (!Array.isArray(report.clone_groups)) {
    fail(`${testCase.id}: fallow report must contain clone_groups`);
  }
  if (report.stats === null || typeof report.stats !== "object" || Array.isArray(report.stats)) {
    fail(`${testCase.id}: fallow report must contain stats`);
  }

  const expectedLines = [...fileLineCounts.values()].reduce((total, lines) => total + lines, 0);
  if (report.stats.total_files !== fileNames.length) {
    fail(`${testCase.id}: fallow analyzed an unexpected number of files`);
  }
  if (report.stats.total_lines !== expectedLines) {
    fail(`${testCase.id}: fallow analyzed an unexpected number of lines`);
  }

  const allowedFiles = new Set(fileNames);
  for (const [groupIndex, group] of report.clone_groups.entries()) {
    if (!Array.isArray(group?.instances) || group.instances.length < 2) {
      fail(`${testCase.id}: clone_groups[${groupIndex}] must contain at least two instances`);
    }
    for (const [instanceIndex, instance] of group.instances.entries()) {
      const instancePath = `clone_groups[${groupIndex}].instances[${instanceIndex}]`;
      if (!allowedFiles.has(instance?.file)) {
        fail(`${testCase.id}: ${instancePath}.file is outside the locked pair`);
      }
      if (
        !Number.isSafeInteger(instance.start_line) ||
        !Number.isSafeInteger(instance.end_line) ||
        instance.start_line < 1 ||
        instance.end_line < instance.start_line ||
        instance.end_line > fileLineCounts.get(instance.file)
      ) {
        fail(`${testCase.id}: ${instancePath} has invalid line bounds`);
      }
    }
  }

  const nearCandidatesSkipped = report.stats.near_candidates_skipped ?? 0;
  if (!Number.isSafeInteger(nearCandidatesSkipped) || nearCandidatesSkipped < 0) {
    fail(`${testCase.id}: stats.near_candidates_skipped must be a non-negative integer`);
  }

  return {
    cloneGroups: report.clone_groups,
    nearCandidatesSkipped,
  };
};

const summarizeCandidateEvidence = (evidenceRecords, cases) =>
  evidenceRecords.map((evidence) => ({
    provider: evidence.provider,
    model: evidence.model,
    resource_observation: evidence.resource_observation,
    profiles: evidence.profiles.map((profile) => {
      const similarities = new Map(
        profile.cases.map((testCase) => [testCase.id, testCase.similarity]),
      );
      const evaluated = cases.map((testCase) => {
        const similarity = similarities.get(testCase.id);
        const selected = similarity >= profile.threshold;
        return {
          id: testCase.id,
          similarity,
          selected,
          classification: classifyFinding(selected, testCase.truth.candidate_worthy),
          added_vs_deterministic:
            selected && testCase.truth.candidate_worthy && !testCase.deterministic_finding.detected,
        };
      });
      const classifications = Object.fromEntries(
        ["true_positive", "false_positive", "false_negative", "true_negative"].map(
          (classification) => [
            classification,
            evaluated.filter((testCase) => testCase.classification === classification).length,
          ],
        ),
      );
      return {
        id: profile.id,
        dimensions: profile.dimensions,
        model_supports_dimensions: profile.model_supports_dimensions,
        threshold: profile.threshold,
        classifications,
        added_vs_deterministic: evaluated.filter((testCase) => testCase.added_vs_deterministic)
          .length,
        cases: evaluated,
      };
    }),
  }));

const invokeFallow = (binary, testCase, corpusRoot, baseline) => {
  const caseRoot = resolve(corpusRoot, dirname(testCase.files[0].fixture));
  const arguments_ = [
    "dupes",
    "--root",
    caseRoot,
    "--mode",
    baseline.mode,
    "--min-tokens",
    String(baseline.min_tokens),
    "--min-lines",
    String(baseline.min_lines),
    "--format",
    "json",
    "--quiet",
    "--no-cache",
  ];
  if (baseline.near) {
    arguments_.push("--near");
  }

  const result = spawnSync(binary, arguments_, {
    cwd: REPO_ROOT,
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
    timeout: 30_000,
  });
  if (result.error) {
    if (result.error.code === "ETIMEDOUT") {
      fail(`${testCase.id}: fallow timed out after 30 seconds`);
    }
    fail(`${testCase.id}: failed to start fallow: ${result.error.message}`);
  }
  if (result.signal) {
    const output = [result.stdout, result.stderr].filter(Boolean).join("\n").trim();
    fail(`${testCase.id}: fallow terminated by signal ${result.signal}: ${output}`);
  }
  if (result.status !== 0) {
    const output = [result.stdout, result.stderr].filter(Boolean).join("\n").trim();
    fail(`${testCase.id}: fallow exited ${result.status}: ${output}`);
  }
  try {
    return JSON.parse(result.stdout);
  } catch (error) {
    fail(`${testCase.id}: invalid fallow JSON: ${error.message}`);
  }
};

const runConformance = (
  { candidateEvidence, manifest, root },
  invoke = invokeFallow,
  fallowBin,
) => {
  const cases = manifest.cases.map((testCase) => {
    const fileNames = testCase.files.map((file) => file.fixture.split("/").at(-1));
    const fileLineCounts = new Map(
      testCase.files.map((file, index) => [
        fileNames[index],
        lineCount(readFileSync(resolve(root, file.fixture))),
      ]),
    );
    const report = invoke(fallowBin, testCase, root, manifest.deterministic_baseline);
    const validated = validateFallowReport(
      report,
      testCase,
      fileNames,
      fileLineCounts,
      manifest.deterministic_baseline,
    );
    const coverage = evaluatePairCoverage(validated.cloneGroups, fileNames, fileLineCounts);
    const detected =
      coverage.best_pair_coverage >= manifest.deterministic_baseline.minimum_pair_coverage;
    const matchesExpected = detected === testCase.expected_deterministic_finding;

    return {
      id: testCase.id,
      category: testCase.category,
      truth: testCase.truth,
      deterministic_finding: {
        detected,
        expected: testCase.expected_deterministic_finding,
        matches_expected: matchesExpected,
        classification: classifyFinding(detected, testCase.truth.refactor_safe),
        ...coverage,
      },
      candidate_gap: testCase.truth.candidate_worthy && !detected,
      near_candidates_skipped: validated.nearCandidatesSkipped,
    };
  });

  const classifications = Object.fromEntries(
    ["true_positive", "false_positive", "false_negative", "true_negative"].map((classification) => [
      classification,
      cases.filter((testCase) => testCase.deterministic_finding.classification === classification)
        .length,
    ]),
  );

  return {
    schema: EXPECTED_SCHEMA,
    source: manifest.source,
    baseline: manifest.deterministic_baseline,
    vector_fixture: manifest.vector_fixture,
    candidate_evidence: summarizeCandidateEvidence(candidateEvidence, cases),
    summary: {
      classifications,
      candidate_gaps: cases.filter((testCase) => testCase.candidate_gap).length,
      baseline_drift: cases.filter((testCase) => !testCase.deterministic_finding.matches_expected)
        .length,
      complete: cases.every((testCase) => testCase.near_candidates_skipped === 0),
    },
    cases,
  };
};

const main = () => {
  const options = parseArgs(process.argv.slice(2));
  if (!existsSync(options.fallowBin)) {
    fail(
      `fallow binary not found at ${options.fallowBin}; run cargo build -p fallow-cli --bin fallow`,
    );
  }
  const loaded = loadManifest(options.manifest);
  const result = runConformance(loaded, invokeFallow, resolve(options.fallowBin));
  process.stdout.write(`${JSON.stringify(result, null, options.pretty ? 2 : 0)}\n`);
  if (options.check && result.summary.baseline_drift > 0) {
    process.stderr.write(`semantic clone baseline drift: ${result.summary.baseline_drift}\n`);
    process.exitCode = 1;
  }
};

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  try {
    main();
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}

export {
  classifyFinding,
  evaluatePairCoverage,
  loadManifest,
  parseArgs,
  runConformance,
  summarizeCandidateEvidence,
  validateFallowReport,
};
