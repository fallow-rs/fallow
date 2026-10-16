import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

import { RUNNERS } from "../src/generated-protocol.mjs";
import { VITEST_MAJORS } from "./support/vitest-fixture.mjs";

// A declared runner range is only true when the suite runs its floor. Vitest
// 4.0 was in the range while the suite ran 4.1 and 5.0, and every mutant on
// 4.0 came back as killed. This file keeps the range and the test pins equal.

const PACKAGE_JSON = new URL("../package.json", import.meta.url);
const VITEST_PACKAGE = "vitest";
const VITEST_ALIAS = /^npm:vitest@(.+)$/u;
const pinHint = (alias, version) =>
  `add an exact devDependency to tools/mutation-runner/package.json ` +
  `(for example "${alias}": "npm:vitest@${version}") ` +
  `and add the same entry to VITEST_MAJORS in test/support/vitest-fixture.mjs`;

const parseVersion = (version) => {
  const match = /^(\d+)\.(\d+)\.(\d+)$/u.exec(version);
  if (match === null) return null;
  return { major: Number(match[1]), minor: Number(match[2]), patch: Number(match[3]) };
};

/** Gives the vitest version of a devDependency, or null when it is not a vitest pin. */
const vitestPinVersion = (name, spec) => {
  const alias = VITEST_ALIAS.exec(spec);
  if (alias !== null) return alias[1];
  return name === VITEST_PACKAGE ? spec : null;
};

/** Gives the highest major below an exclusive upper version. */
const lastMajorBelow = ({ major, minor, patch }) => (minor + patch === 0 ? major - 1 : major);

const devDependencies = JSON.parse(readFileSync(PACKAGE_JSON, "utf8")).devDependencies ?? {};
const vitestRange = RUNNERS.find((runner) => runner.package === VITEST_PACKAGE);

/** The vitest versions that the suite runs, with the package.json pin behind each. */
const testedPins = VITEST_MAJORS.map(({ packageName, version }) => {
  const spec = devDependencies[packageName];
  return {
    packageName,
    version,
    pinned: vitestPinVersion(packageName, spec ?? ""),
  };
});

test("every vitest version of the suite is an exact pin in package.json", () => {
  for (const pin of testedPins) {
    assert.equal(
      pin.pinned,
      pin.version,
      `VITEST_MAJORS runs ${pin.packageName} as vitest ${pin.version}, but ` +
        `tools/mutation-runner/package.json does not pin "${pin.packageName}" to vitest ${pin.version}`,
    );
  }
});

test("a test pin covers the floor of the vitest range", () => {
  assert.ok(vitestRange, "the protocol manifest declares no vitest runner range");
  const floor = parseVersion(vitestRange.min_version);
  assert.ok(floor, `the vitest min_version ${vitestRange.min_version} is not x.y.z`);
  const covered = testedPins.some(({ version }) => {
    const parsed = parseVersion(version);
    return parsed !== null && parsed.major === floor.major && parsed.minor === floor.minor;
  });
  assert.ok(
    covered,
    `the vitest range floor ${vitestRange.min_version} is not tested: no test pin has version ` +
      `${floor.major}.${floor.minor}.x (pins: ${testedPins.map((pin) => pin.version).join(", ")}). ` +
      `Pin vitest ${floor.major}.${floor.minor}.x: ` +
      `${pinHint(`vitest${floor.major}${floor.minor}`, `${floor.major}.${floor.minor}.<patch>`)}. ` +
      `Or raise min_version in crates/api/mutation-runner-protocol.json to a tested minor.`,
  );
});

test("a test pin covers each vitest major in the range", () => {
  assert.ok(vitestRange, "the protocol manifest declares no vitest runner range");
  const floor = parseVersion(vitestRange.min_version);
  const ceiling = parseVersion(vitestRange.max_version_exclusive);
  assert.ok(floor && ceiling, "the vitest range versions are not x.y.z");
  const lastMajor = lastMajorBelow(ceiling);
  const testedMajors = new Set(testedPins.map(({ version }) => parseVersion(version)?.major));
  for (let major = floor.major; major <= lastMajor; major += 1) {
    assert.ok(
      testedMajors.has(major),
      `vitest major ${major} is in the range ${vitestRange.min_version} to ` +
        `${vitestRange.max_version_exclusive} (exclusive), but no test pin has major ${major}. ` +
        `Pin vitest ${major}.x: ${pinHint(`vitest${major}`, `${major}.<minor>.<patch>`)}.`,
    );
  }
});
