import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import test from "node:test";

import {
  candidateSetDigest,
  independentReviewDigest,
  independentReviewErrors,
} from "./type-aware-corpus.mjs";
import {
  recordReviews,
  reviewableEntries,
  writeReviewPackets,
} from "./type-aware-corpus/review.mjs";

const location = (path, line) => ({ path, line, col: 2, excerpt: "" });

const entry = (key, status, extra = {}) => ({
  key,
  project_id: "p",
  semantic_status: status,
  candidate: {
    path: "src/owner.ts",
    parent_name: "Owner",
    member_name: key,
    kind: "class_method",
    line: 2,
    col: 2,
  },
  source_evidence: { declaration: location("src/owner.ts", 2), uses: [] },
  contract: null,
  ...extra,
});

const ledger = {
  candidates: [
    entry("tac_d", "retained"),
    entry("tac_a", "confirmed-used", {
      source_evidence: {
        declaration: location("src/owner.ts", 2),
        uses: [location("src/use.ts", 1)],
      },
    }),
    entry("tac_b", "contract-preserved", {
      contract: {
        relation: "interface-implementation",
        optional: false,
        declaration: location("node_modules/@types/x/index.d.ts", 1),
      },
    }),
    entry("tac_c", "retained"),
  ],
};

const manifest = { projects: [{ id: "p", feature_buckets: ["generics"] }] };

const decisions = {
  schema_version: 1,
  known_unused: { tac_c: "Owner.tac_c occurs only at its declaration." },
  feature_overrides: { tac_old: ["generics"] },
  independent_reviews: [],
  overruled_rejections: { tac_old: { reason: "stale", rejected_by: ["x"] } },
};

const digests = {
  candidateSetDigest: (keys) => `set:${keys.join(",")}`,
  independentReviewDigest: (entries) => `evidence:${entries.map(({ key }) => key).join(",")}`,
};

const fail = (message) => {
  throw new Error(message);
};

const approve = (key, buckets = [], reason = "ok") => ({
  key,
  verdict: "approve",
  reason,
  feature_buckets: buckets,
});

const reject = (key, reason) => ({ key, verdict: "reject", reason, feature_buckets: [] });

const review = (reason, overrides = {}) =>
  ["tac_a", "tac_b", "tac_c"].map((key) => overrides[key] ?? approve(key, [], reason));

const reviewsFixture = (t, reviewers, overrule = null) => {
  const root = mkdtempSync(resolve(tmpdir(), "fallow-corpus-reviews-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(resolve(root, "p"));
  for (const [name, records] of Object.entries(reviewers)) {
    writeFileSync(resolve(root, "p", `${name}.json`), JSON.stringify({ records }));
  }
  if (overrule) writeFileSync(resolve(root, "overrule.json"), JSON.stringify(overrule));
  return root;
};

const record = (reviewsDir, reviewDigests = digests) =>
  recordReviews({
    ledger,
    decisions,
    manifest,
    reviewsDir,
    digests: reviewDigests,
    reviewedOn: "2026-11-10",
    fail,
  });

test("reviewable entries are the confirmed, contract and known-unused records", () => {
  assert.deepEqual(
    reviewableEntries(ledger, decisions).map(({ key }) => key),
    ["tac_a", "tac_b", "tac_c"],
  );
});

test("record-reviews binds the reviews and keeps only the buckets that all reviewers give", (t) => {
  const reviewsDir = reviewsFixture(t, {
    first: review("first", { tac_a: approve("tac_a", ["generics", "unlisted"]) }),
    second: review("second", {
      tac_a: approve("tac_a", ["generics"]),
      tac_b: approve("tac_b", ["generics"]),
    }),
  });
  const result = record(reviewsDir);
  assert.equal(result.reviewed_on, "2026-11-10");
  assert.deepEqual(result.feature_overrides, { tac_a: ["generics"] });
  assert.deepEqual(result.known_unused, decisions.known_unused);
  assert.equal(Object.hasOwn(result, "overruled_rejections"), false);
  assert.deepEqual(
    result.independent_reviews.map(
      ({ project_id, candidate_count, candidate_set_sha256, evidence_sha256 }) => ({
        project_id,
        candidate_count,
        candidate_set_sha256,
        evidence_sha256,
      }),
    ),
    [
      {
        project_id: "p",
        candidate_count: 2,
        candidate_set_sha256: "set:tac_a,tac_b",
        evidence_sha256: "evidence:tac_a,tac_b",
      },
    ],
  );
});

test("recorded reviews pass the review validation of adjudicate", (t) => {
  const result = record(reviewsFixture(t, { first: review("first"), second: review("second") }), {
    candidateSetDigest,
    independentReviewDigest,
  });
  assert.deepEqual(independentReviewErrors(ledger.candidates, result.independent_reviews), []);
});

test("record-reviews fails on a rejection without a recorded overrule", (t) => {
  const rejected = { tac_b: reject("tac_b", "no caller") };
  assert.throws(
    () => record(reviewsFixture(t, { first: review("first", rejected), second: review("second") })),
    /tac_b is rejected \(first: no caller\)/,
  );
  const reviewers = { first: review("first", rejected), second: review("second", rejected) };
  assert.throws(() => record(reviewsFixture(t, reviewers, { tac_b: " " })), /tac_b is rejected/);
  const overruled = record(
    reviewsFixture(t, reviewers, { tac_b: "Overrides behavior of its class." }),
  );
  assert.deepEqual(overruled.overruled_rejections, {
    tac_b: { reason: "Overrides behavior of its class.", rejected_by: ["first", "second"] },
  });
});

test("record-reviews requires two separate and complete reviewer files", (t) => {
  const first = review("first");
  const cases = [
    [{ only: first }, /needs two or more reviewer files/],
    [{ first, second: first }, /identical reviewer files/],
    [
      { first, second: review("second").slice(1) },
      /second must review exactly the packet records \(missing: tac_a/,
    ],
    [{ first, second: [...review("second"), approve("tac_x")] }, /unknown: tac_x/],
    [
      { first, second: [...review("second"), reject("tac_b", "late")] },
      /second\.json repeats a record key/,
    ],
    [
      { first, second: review("second", { tac_a: { ...approve("tac_a"), verdict: "maybe" } }) },
      /invalid verdict for tac_a/,
    ],
    [
      {
        first,
        second: review("second", { tac_a: { ...approve("tac_a"), feature_buckets: "generics" } }),
      },
      /feature_buckets of tac_a as an array of strings/,
    ],
    [{ first, overrule: review("second") }, /overrule\.json belongs in/],
  ];
  for (const [reviewers, message] of cases) {
    assert.throws(() => record(reviewsFixture(t, reviewers)), message);
  }
});

test("review packets show the source around each location, also in the dependency root", (t) => {
  const root = mkdtempSync(resolve(tmpdir(), "fallow-corpus-packets-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const fixture = resolve(root, "fixture");
  const dependencyRoot = resolve(root, "workspace");
  mkdirSync(resolve(fixture, "src"), { recursive: true });
  mkdirSync(resolve(dependencyRoot, "node_modules/@types/x"), { recursive: true });
  writeFileSync(resolve(fixture, "src/owner.ts"), "class Owner {\n  tac_a() {}\n}\n");
  writeFileSync(resolve(fixture, "src/use.ts"), "new Owner().tac_a();\n");
  writeFileSync(
    resolve(dependencyRoot, "node_modules/@types/x/index.d.ts"),
    "interface X { tac_b(): void }\n",
  );
  const [path] = writeReviewPackets({
    ledger,
    decisions,
    manifest,
    rootFor: () => fixture,
    dependencyRoot,
    outDir: resolve(root, "out"),
  });
  const packet = readFileSync(path, "utf8");
  assert.equal(path, resolve(root, "out/review/p.md"));
  assert.match(packet, /Allowed feature buckets: generics/);
  assert.match(packet, /### tac_a \[confirmed-used\] Owner\.tac_a/);
  assert.match(packet, />\s+2 \|\s+tac_a\(\) \{\}/);
  assert.match(packet, />\s+1 \| new Owner\(\)\.tac_a\(\);/);
  assert.match(packet, />\s+1 \| interface X \{ tac_b\(\): void \}/);
  assert.match(packet, /### tac_c \[known-unused claim\][\s\S]*Claim to verify: Owner\.tac_c/);
  assert.doesNotMatch(packet, /tac_d/);
});
