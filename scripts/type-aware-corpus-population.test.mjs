import assert from "node:assert/strict";
import test from "node:test";
import { candidateKey, compareCandidateSets } from "./type-aware-corpus.mjs";
import { comparePopulation, validatePopulationMetadata } from "./type-aware-corpus/population.mjs";
import { typeAwareResolutionCounts, blockedProjects } from "./type-aware-corpus/summary.mjs";

const candidate = (member) => {
  const fields = {
    path: "src/service.ts",
    parent_name: "Service",
    member_name: member,
    kind: "class_method",
    line: member === "used" ? 2 : 3,
    col: 3,
  };
  return { key: candidateKey("fixture", fields), ...fields };
};
const decision = (member, value, status = "complete") => {
  const item = candidate(member);
  return {
    subject: {
      path: item.path,
      owner: item.parent_name,
      local_name: member,
      declaration_kind: item.kind,
      line: item.line,
      col: item.col,
    },
    decision: value,
    status,
    owning_projects: ["tsconfig.json"],
    reason_code: null,
    closed_world_eligible: false,
  };
};

test("semantic-only promotions do not change the baseline confirmation cohort", () => {
  const metadata = {
    candidate_decisions: [
      decision("used", "confirmed-used"),
      decision("unused", "confirmed-no-static-references"),
    ],
  };
  const result = compareCandidateSets(
    "fixture",
    [candidate("used")],
    [candidate("unused")],
    metadata,
  );
  assert.deepEqual(
    result.map(({ member_name, semantic_status }) => ({ member_name, semantic_status })),
    [{ member_name: "used", semantic_status: "confirmed-used" }],
  );
  const population = comparePopulation(
    "fixture",
    [candidate("used")],
    [candidate("unused")],
    new Map([
      [candidate("used").key, metadata.candidate_decisions[0]],
      [candidate("unused").key, metadata.candidate_decisions[1]],
    ]),
  );
  assert.deepEqual(
    population.promotions.map(({ member_name }) => member_name),
    ["unused"],
  );
  assert.throws(
    () =>
      compareCandidateSets("fixture", [candidate("used")], [candidate("unused")], {
        candidate_decisions: [decision("used", "confirmed-used")],
      }),
    /no semantic decision/,
  );
  assert.throws(
    () =>
      compareCandidateSets("fixture", [candidate("used")], [candidate("unused")], {
        candidate_decisions: [
          decision("used", "confirmed-used"),
          decision("unused", "retained-abstained", "partial"),
        ],
      }),
    /promotion/,
  );
});

test("coverage and blocked projects use only baseline decisions", () => {
  const projects = [
    {
      id: "fixture",
      type_aware: {
        abstained_count: 8,
        unresolved_count: 5,
        abstention_reasons: { blocking_diagnostics: 7 },
        projects: [],
      },
      baseline_type_aware: {
        abstained_count: 1,
        unresolved_count: 2,
        abstention_reasons: { blocking_diagnostics: 2 },
      },
    },
  ];
  assert.deepEqual(typeAwareResolutionCounts(projects), {
    abstainedCount: 1,
    unresolvedCount: 2,
    blockingDiagnosticsCount: 2,
  });
  assert.deepEqual(blockedProjects(projects), [
    { project_id: "fixture", blocked_candidate_count: 2, programs: [] },
  ]);
});

test("global metadata includes hidden semantic queries and omitted capacity without inflating baseline counts", () => {
  const used = decision("used", "confirmed-used");
  const blocked = {
    ...decision("unused", "retained-unresolved", "unavailable"),
    reason_code: "blocking-diagnostics",
  };
  const decisions = new Map([
    [candidate("used").key, used],
    [candidate("unused").key, blocked],
  ]);
  const metadata = {
    candidate_count: 3,
    confirmed_used_count: 1,
    contract_preserved_count: 0,
    no_static_references_count: 0,
    fix_eligible_count: 0,
    unresolved_count: 1,
    abstained_count: 1,
    abstention_reasons: {
      no_project: 0,
      ambiguous_project: 0,
      blocking_diagnostics: 1,
      svelte_virtual_module_exports: 0,
      unknown_symbol: 0,
      unsupported_syntax: 0,
      capacity: 1,
    },
  };
  const counts = validatePopulationMetadata("fixture", metadata, decisions, [candidate("used")]);
  assert.equal(counts.confirmed_used_count, 1);
  assert.equal(counts.unresolved_count, 0);
  assert.equal(counts.abstained_count, 0);
  assert.equal(counts.abstention_reasons.blocking_diagnostics, 0);
  for (const change of [
    { candidate_count: 4 },
    { confirmed_used_count: 2 },
    { candidate_count: -1 },
    { abstention_reasons: { ...metadata.abstention_reasons, capacity: 0 } },
    { abstained_count: 0 },
    { candidate_count: undefined },
  ]) {
    assert.throws(
      () =>
        validatePopulationMetadata("fixture", { ...metadata, ...change }, decisions, [
          candidate("used"),
        ]),
      /metadata/,
    );
  }
});

test("semantic-only negative decisions must appear as promotions", () => {
  assert.throws(
    () =>
      compareCandidateSets("fixture", [candidate("used")], [], {
        candidate_decisions: [
          decision("used", "confirmed-used"),
          decision("unused", "confirmed-no-static-references"),
        ],
      }),
    /missing.*promotion/,
  );
});
