import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";
import {
  evidenceFreshnessErrors,
  trackedEvidenceFreshnessErrors,
} from "./type-aware-corpus/freshness.mjs";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ISSUE = "https://github.com/fallow-rs/fallow/issues/3776";
const policy = (waiver) => ({ schema_version: 1, maximum_minor_lag: 6, ...(waiver && { waiver }) });

test("fresh evidence within the minor lag passes", () => {
  assert.deepEqual(
    evidenceFreshnessErrors({
      evidenceVersion: "fallow 3.30.2",
      workspaceVersion: "3.36.0",
      policy: policy(),
    }),
    [],
  );
});

test("evidence older than the minor lag fails and names the remedy", () => {
  const errors = evidenceFreshnessErrors({
    evidenceVersion: "fallow 3.9.1",
    workspaceVersion: "3.35.0",
    policy: policy(),
  });
  assert.equal(errors.length, 1);
  assert.match(errors[0], /fallow 3\.9\.1 is 26 minor releases behind 3\.35\.0/);
  assert.match(errors[0], /npm run type-aware:corpus/);
});

test("a different major version is always stale", () => {
  const errors = evidenceFreshnessErrors({
    evidenceVersion: "fallow 3.40.0",
    workspaceVersion: "4.0.0",
    policy: policy(),
  });
  assert.equal(errors.length, 1);
  assert.match(errors[0], /major/);
});

test("a waiver accepts stale evidence up to its last version", () => {
  const waiver = { issue: ISSUE, until_version: "3.40.0", reason: "Open gate decisions." };
  assert.deepEqual(
    evidenceFreshnessErrors({
      evidenceVersion: "fallow 3.9.1",
      workspaceVersion: "3.40.0",
      policy: policy(waiver),
    }),
    [],
  );
  const expired = evidenceFreshnessErrors({
    evidenceVersion: "fallow 3.9.1",
    workspaceVersion: "3.41.0",
    policy: policy(waiver),
  });
  assert.equal(expired.length, 1);
  assert.match(expired[0], /waiver for .*issues\/3776 ended at 3\.40\.0/);
});

test("a waiver on fresh evidence fails so that it does not linger", () => {
  const errors = evidenceFreshnessErrors({
    evidenceVersion: "fallow 3.35.0",
    workspaceVersion: "3.35.0",
    policy: policy({ issue: ISSUE, until_version: "3.40.0", reason: "Done." }),
  });
  assert.equal(errors.length, 1);
  assert.match(errors[0], /remove the waiver/);
});

test("a malformed policy or version fails", () => {
  assert.match(
    evidenceFreshnessErrors({
      evidenceVersion: "fallow 3.9.1",
      workspaceVersion: "3.35.0",
      policy: { schema_version: 1, maximum_minor_lag: -1 },
    })[0],
    /maximum_minor_lag/,
  );
  assert.match(
    evidenceFreshnessErrors({
      evidenceVersion: "fallow",
      workspaceVersion: "3.35.0",
      policy: policy(),
    })[0],
    /evidence version/,
  );
  assert.match(
    evidenceFreshnessErrors({
      evidenceVersion: "fallow 3.9.1",
      workspaceVersion: "3.35.0",
      policy: policy({ issue: "not a url", until_version: "3.40.0", reason: "x" }),
    })[0],
    /waiver/,
  );
});

test("tracked type-aware corpus evidence is fresh or has an active waiver", () => {
  const errors = trackedEvidenceFreshnessErrors({
    evidence: JSON.parse(
      readFileSync(resolve(REPO_ROOT, "benchmarks/type-aware-corpus-evidence.json"), "utf8"),
    ),
    cargoToml: readFileSync(resolve(REPO_ROOT, "Cargo.toml"), "utf8"),
    policy: JSON.parse(
      readFileSync(resolve(REPO_ROOT, "benchmarks/type-aware-corpus-freshness.json"), "utf8"),
    ),
  });
  assert.deepEqual(errors, []);
});
