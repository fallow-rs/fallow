import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  assertChangelogRelease,
  assertPublishedRelease,
  changelogLinks,
  changelogSections,
  previousReleasedVersion,
  verifyReleaseMetadata,
  versionOfTag,
} from "./verify-release-metadata.mjs";

const emDash = String.fromCodePoint(0x2014);

const COMPARE_ROOT = "https://github.com/fallow-rs/fallow/compare";

/** A changelog in the exact shape a release commit leaves behind. */
const changelog = ({
  unreleased = "",
  heading = "## [3.21.0] - 2026-08-31",
  body = "### Added\n\n- **Something users can see.**\n",
  links = [`[3.21.0]: ${COMPARE_ROOT}/v3.20.0...v3.21.0`],
} = {}) =>
  [
    "# Changelog",
    "",
    "## [Unreleased]",
    "",
    unreleased,
    heading,
    "",
    body,
    "## [3.20.0] - 2026-08-28",
    "",
    "### Fixed",
    "",
    "- **Something earlier.**",
    "",
    ...links,
    `[3.20.0]: ${COMPARE_ROOT}/v3.19.0...v3.20.0`,
    "",
  ].join("\n");

const publishedRelease = (overrides = {}) => ({
  name: "v3.21.0: coverage geometry the matcher can prove",
  body: `Real notes.\n\n**Full Changelog**: ${COMPARE_ROOT}/v3.20.0...v3.21.0\n`,
  isDraft: false,
  isPrerelease: false,
  ...overrides,
});

const compareUrl = `${COMPARE_ROOT}/v3.20.0...v3.21.0`;

test("a release tag is reduced to its version, and anything else is refused", () => {
  assert.equal(versionOfTag("v3.21.0"), "3.21.0");
  for (const rejected of ["3.21.0", "v3.21", "v3.21.0-rc.1", "", undefined]) {
    assert.throws(() => versionOfTag(rejected), /release tag must match/u);
  }
});

test("sections carry the body that follows each heading", () => {
  const sections = changelogSections(changelog());

  assert.deepEqual(
    sections.map((section) => section.version),
    ["Unreleased", "3.21.0", "3.20.0"],
  );
  assert.equal(sections[1].date, "2026-08-31");
  assert.equal(sections[0].date, undefined);
  assert.match(sections[1].body, /Something users can see/u);
  assert.doesNotMatch(sections[1].body, /Something earlier/u);
});

test("the previous released version skips the Unreleased section", () => {
  const sections = changelogSections(changelog());

  assert.equal(previousReleasedVersion(sections, "3.21.0"), "3.20.0");
  assert.throws(() => previousReleasedVersion(sections, "3.20.0"), /no section before/u);
  assert.throws(() => previousReleasedVersion(sections, "9.9.9"), /no released section/u);
});

test("link definitions are read from the foot of the changelog", () => {
  assert.equal(changelogLinks(changelog()).get("3.21.0"), compareUrl);
  assert.equal(changelogLinks(changelog()).size, 2);
});

test("a curated changelog section passes and reports what the notes must link to", () => {
  assert.deepEqual(assertChangelogRelease(changelog(), "v3.21.0"), {
    version: "3.21.0",
    previousTag: "v3.20.0",
    compareUrl,
  });
});

test("the changelog gate refuses a section that was never written", () => {
  assert.throws(
    () => assertChangelogRelease(changelog({ heading: "## [3.99.0] - 2026-08-31" }), "v3.21.0"),
    /exactly one "## \[3\.21\.0\]" section, found 0/u,
  );
  assert.throws(
    () => assertChangelogRelease(changelog({ body: "\n" }), "v3.21.0"),
    /the 3\.21\.0 section is empty/u,
  );
});

test("the changelog gate refuses a duplicated section", () => {
  const duplicated = changelog().replace(
    "## [3.20.0] - 2026-08-28",
    "## [3.21.0] - 2026-08-30\n\n- **Duplicate.**\n\n## [3.20.0] - 2026-08-28",
  );

  assert.throws(
    () => assertChangelogRelease(duplicated, "v3.21.0"),
    /exactly one "## \[3\.21\.0\]" section, found 2/u,
  );
});

test("the changelog gate requires a well-formed release date", () => {
  assert.throws(
    () => assertChangelogRelease(changelog({ heading: "## [3.21.0]" }), "v3.21.0"),
    /needs a YYYY-MM-DD date, found none/u,
  );
  assert.throws(
    () => assertChangelogRelease(changelog({ heading: "## [3.21.0] - soon" }), "v3.21.0"),
    /needs a YYYY-MM-DD date, found soon/u,
  );
});

test("the changelog gate refuses an em-dash the release notes would inherit", () => {
  assert.throws(
    () => assertChangelogRelease(changelog({ body: `- A ${emDash} B\n` }), "v3.21.0"),
    /section contains an em-dash/u,
  );
});

test("the changelog gate refuses entries still parked under Unreleased", () => {
  assert.throws(
    () => assertChangelogRelease(changelog({ unreleased: "- **Forgotten.**\n" }), "v3.21.0"),
    /Unreleased section still holds entries/u,
  );
});

test("the changelog gate refuses a missing or misdirected compare link", () => {
  assert.throws(
    () => assertChangelogRelease(changelog({ links: [] }), "v3.21.0"),
    /no "\[3\.21\.0\]:" link definition/u,
  );
  assert.throws(
    () =>
      assertChangelogRelease(
        changelog({ links: [`[3.21.0]: ${COMPARE_ROOT}/v3.19.0...v3.21.0`] }),
        "v3.21.0",
      ),
    /must compare v3\.20\.0\.\.\.v3\.21\.0/u,
  );
});

test("a curated published release passes", () => {
  assert.doesNotThrow(() => assertPublishedRelease(publishedRelease(), "v3.21.0", compareUrl));
});

test("the publication gate refuses a draft or a prerelease", () => {
  assert.throws(
    () => assertPublishedRelease(publishedRelease({ isDraft: true }), "v3.21.0", compareUrl),
    /still a draft/u,
  );
  assert.throws(
    () => assertPublishedRelease(publishedRelease({ isPrerelease: true }), "v3.21.0", compareUrl),
    /marked as a prerelease/u,
  );
});

test("the publication gate requires a titled summary behind the tag prefix", () => {
  assert.throws(
    () =>
      assertPublishedRelease(publishedRelease({ name: "3.21.0 release" }), "v3.21.0", compareUrl),
    /must start with "v3\.21\.0: "/u,
  );
  assert.throws(
    () => assertPublishedRelease(publishedRelease({ name: "v3.21.0:  " }), "v3.21.0", compareUrl),
    /carries no summary/u,
  );
});

test("the publication gate requires a body that links the full changelog", () => {
  assert.throws(
    () => assertPublishedRelease(publishedRelease({ body: "  \n" }), "v3.21.0", compareUrl),
    /has an empty body/u,
  );
  assert.throws(
    () => assertPublishedRelease(publishedRelease({ body: "Real notes." }), "v3.21.0", compareUrl),
    /missing the compare link/u,
  );
});

test("the publication gate refuses an em-dash in the title or the body", () => {
  const withEmDash = publishedRelease({ name: `v3.21.0: coverage ${emDash} proven` });
  assert.throws(
    () => assertPublishedRelease(withEmDash, "v3.21.0", compareUrl),
    /title contains an em-dash/u,
  );
  assert.throws(
    () =>
      assertPublishedRelease(
        publishedRelease({ body: `Notes ${emDash} more\n\n${compareUrl}\n` }),
        "v3.21.0",
        compareUrl,
      ),
    /body contains an em-dash/u,
  );
});

test("the publication gate refuses an upstream project name on the public surface", () => {
  assert.throws(
    () =>
      assertPublishedRelease(
        publishedRelease({ name: "v3.21.0: knip parity" }),
        "v3.21.0",
        compareUrl,
      ),
    /title names an upstream project/u,
  );
  assert.throws(
    () =>
      assertPublishedRelease(
        publishedRelease({ body: `Faster than jscpd.\n\n${compareUrl}\n` }),
        "v3.21.0",
        compareUrl,
      ),
    /body names an upstream project/u,
  );
});

test("every released section of the repository's changelog is dated, filled and linked", () => {
  const source = readFileSync("CHANGELOG.md", "utf8");
  const sections = changelogSections(source);
  const links = changelogLinks(source);
  const released = sections.filter((section) => /^\d+\.\d+\.\d+$/u.test(section.version));
  const problems = [];

  released.forEach((section, position) => {
    const previous = released[position + 1];
    const link = links.get(section.version);
    if (!/^\d{4}-\d{2}-\d{2}$/u.test(section.date ?? "")) {
      problems.push(`${section.version} has no release date`);
    }
    if (!section.body.split(/\r?\n/u).some((line) => line.trim() !== "")) {
      problems.push(`${section.version} has an empty body`);
    }
    if (!link) {
      problems.push(`${section.version} has no link definition`);
    } else if (previous && !link.endsWith(`/compare/v${previous.version}...v${section.version}`)) {
      problems.push(`${section.version} compares to ${link} instead of v${previous.version}`);
    }
  });

  const versions = new Set(sections.map((section) => section.version));
  for (const [version] of links) {
    if (!versions.has(version)) {
      problems.push(`${version} is linked but has no section`);
    }
  }

  assert.deepEqual(problems, []);
  assert.ok(released.length > 200, "the changelog should still carry its full history");
});

test("the full gate passes on the repository's changelog in its release-commit shape", () => {
  const source = readFileSync("CHANGELOG.md", "utf8");
  const latest = changelogSections(source).find((section) =>
    /^\d+\.\d+\.\d+$/u.test(section.version),
  );
  // The release commit empties [Unreleased] as it renames it, so drop this
  // cycle's pending entries to reproduce the tree the workflow would see.
  const atRelease = source.replace(/## \[Unreleased\]\n[\s\S]*?(?=\n## \[)/u, "## [Unreleased]\n");

  assert.deepEqual(assertChangelogRelease(atRelease, `v${latest.version}`).version, latest.version);
});

test("the entry point reads both surfaces from disk and names what it verified", () => {
  const root = mkdtempSync(join(tmpdir(), "release-metadata-"));
  const releaseJson = join(root, "release.json");
  writeFileSync(join(root, "CHANGELOG.md"), changelog());
  writeFileSync(releaseJson, JSON.stringify(publishedRelease()));

  assert.deepEqual(verifyReleaseMetadata({ tag: "v3.21.0", releaseJson, root }), [
    "CHANGELOG.md section 3.21.0 (compares back to v3.20.0)",
    "published GitHub Release title and body",
  ]);

  writeFileSync(releaseJson, JSON.stringify(publishedRelease({ body: "No link here.\n" })));
  assert.throws(
    () => verifyReleaseMetadata({ tag: "v3.21.0", releaseJson, root }),
    /missing the compare link/u,
  );

  rmSync(root, { recursive: true, force: true });
});
