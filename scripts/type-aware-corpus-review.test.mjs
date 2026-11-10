import assert from "node:assert/strict";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import test from "node:test";

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
  candidate: { parent_name: "Owner", member_name: key, kind: "class_method" },
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

const approve = (key, buckets = []) => ({
  key,
  verdict: "approve",
  reason: "ok",
  feature_buckets: buckets,
});

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

const record = (reviewsDir) =>
  recordReviews({
    ledger,
    decisions,
    manifest,
    reviewsDir,
    digests,
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
    first: [approve("tac_a", ["generics", "unlisted"]), approve("tac_b"), approve("tac_c")],
    second: [approve("tac_a", ["generics"]), approve("tac_b", ["generics"]), approve("tac_c")],
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

test("record-reviews fails on a rejection without a recorded overrule", (t) => {
  const rejected = { key: "tac_b", verdict: "reject", reason: "no caller", feature_buckets: [] };
  const records = [approve("tac_a"), rejected, approve("tac_c")];
  assert.throws(
    () =>
      record(
        reviewsFixture(t, {
          first: records,
          second: [approve("tac_a"), approve("tac_b"), approve("tac_c")],
        }),
      ),
    /tac_b is rejected \(first: no caller\)/,
  );
  const overruled = record(
    reviewsFixture(
      t,
      { first: records, second: records },
      { tac_b: "Overrides behavior of its class." },
    ),
  );
  assert.deepEqual(overruled.overruled_rejections, {
    tac_b: { reason: "Overrides behavior of its class.", rejected_by: ["first", "second"] },
  });
});

test("record-reviews requires two complete reviewer files for each project", (t) => {
  const complete = [approve("tac_a"), approve("tac_b"), approve("tac_c")];
  assert.throws(
    () => record(reviewsFixture(t, { only: complete })),
    /needs two or more reviewer files/,
  );
  assert.throws(
    () => record(reviewsFixture(t, { first: complete, second: complete.slice(1) })),
    /second must review exactly the packet records \(missing: tac_a/,
  );
  assert.throws(
    () => record(reviewsFixture(t, { first: complete, second: [...complete, approve("tac_x")] })),
    /unknown: tac_x/,
  );
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
