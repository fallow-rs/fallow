import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import {
  assertChangelogRelease,
  assertNothingUnreleased,
  assertPublishedRelease,
  changelogLinks,
  changelogSections,
  parseArguments,
  previousReleasedVersion,
  verifyReleaseMetadata,
  versionOfTag,
} from "./verify-release-metadata.mjs";

const emDash = String.fromCodePoint(0x2014);

const COMPARE_ROOT = "https://github.com/fallow-rs/fallow/compare";

const compareUrl = `${COMPARE_ROOT}/v3.20.0...v3.21.0`;

/** A changelog in the exact shape a release commit leaves behind. */
const changelog = ({
  unreleased = "",
  heading = "## [3.21.0] - 2026-08-31",
  body = "### Added\n\n- **Something users can see.**\n",
  links = [`[3.21.0]: ${compareUrl}`],
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
  body: `Real notes.\n\n**Full Changelog**: ${compareUrl}\n`,
  isDraft: false,
  isPrerelease: false,
  ...overrides,
});

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

test("a heading inside a fenced block is a code sample, not a section", () => {
  const fenced = changelog({
    body: [
      "### Added",
      "",
      "- **Documents the changelog format.**",
      "",
      "```markdown",
      "## [9.9.9] - 2026-01-01",
      "```",
      "",
    ].join("\n"),
  });

  assert.deepEqual(
    changelogSections(fenced).map((section) => section.version),
    ["Unreleased", "3.21.0", "3.20.0"],
  );
  assert.doesNotThrow(() => assertChangelogRelease(fenced, "v3.21.0"));
});

test("the previous released version skips the Unreleased section", () => {
  const sections = changelogSections(changelog());

  assert.equal(previousReleasedVersion(sections, "3.21.0"), "3.20.0");
  assert.throws(() => previousReleasedVersion(sections, "3.20.0"), /no section before/u);
  assert.throws(() => previousReleasedVersion(sections, "9.9.9"), /no released section/u);
});

test("sections filed out of order are refused rather than compared backwards", () => {
  const swapped = changelog({
    links: [`[3.21.0]: ${COMPARE_ROOT}/v3.22.0...v3.21.0`],
  }).replace("## [3.20.0] - 2026-08-28", "## [3.22.0] - 2026-08-28");

  assert.throws(() => assertChangelogRelease(swapped, "v3.21.0"), /3\.21\.0 sits above 3\.22\.0/u);
});

test("link definitions are read out of the changelog", () => {
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

test("the changelog gate requires a real release date", () => {
  for (const [heading, expected] of [
    ["## [3.21.0]", /found none/u],
    ["## [3.21.0] - soon", /found soon/u],
    ["## [3.21.0] - 2026-31-08", /found 2026-31-08/u],
    ["## [3.21.0] - 2026-02-30", /found 2026-02-30/u],
  ]) {
    assert.throws(() => assertChangelogRelease(changelog({ heading }), "v3.21.0"), expected);
  }
});

test("the changelog gate refuses an em-dash the release notes would inherit", () => {
  assert.throws(
    () => assertChangelogRelease(changelog({ body: `- A ${emDash} B\n` }), "v3.21.0"),
    /section contains an em-dash/u,
  );
});

test("the changelog gate refuses a missing, misdirected or off-host compare link", () => {
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
    /must be https:\/\/github\.com\/fallow-rs\/fallow\/compare\/v3\.20\.0\.\.\.v3\.21\.0/u,
  );
  assert.throws(
    () =>
      assertChangelogRelease(
        changelog({ links: ["[3.21.0]: https://evil.example/compare/v3.20.0...v3.21.0"] }),
        "v3.21.0",
      ),
    /must be https:\/\/github\.com\/fallow-rs\/fallow\/compare\//u,
  );
});

test("pending entries are refused at dispatch and ignored after publication", () => {
  const pending = changelog({ unreleased: "### Added\n\n- **Landed after the release.**\n" });

  assert.throws(() => assertNothingUnreleased(pending), /Unreleased section still holds entries/u);
  // The release commit empties [Unreleased], but the default branch fills it
  // again while the release publishes, so the released section must still pass.
  assert.doesNotThrow(() => assertChangelogRelease(pending, "v3.21.0"));
});

test("a second Unreleased heading from a bad merge is refused", () => {
  const duplicated = changelog().replace(
    "## [Unreleased]",
    "## [Unreleased]\n\n## [Unreleased]\n\n- **Hidden under the second heading.**",
  );

  assert.throws(
    () => assertNothingUnreleased(duplicated),
    /exactly one "## \[Unreleased\]" section, found 2/u,
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
  assert.throws(
    () =>
      assertPublishedRelease(
        publishedRelease({ name: `v3.21.0: coverage ${emDash} proven` }),
        "v3.21.0",
        compareUrl,
      ),
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

test("arguments are read in pairs and anything unrecognized is refused", () => {
  assert.deepEqual(parseArguments(["--tag", "v3.21.0"]), {
    tag: "v3.21.0",
    releaseJson: undefined,
  });
  assert.deepEqual(parseArguments(["--tag", "v3.21.0", "--release-json", "r.json"]), {
    tag: "v3.21.0",
    releaseJson: "r.json",
  });
  assert.throws(() => parseArguments(["--verbose"]), /unknown argument: --verbose/u);
  assert.throws(() => parseArguments([]), /usage: node scripts/u);
  assert.throws(() => parseArguments(["--tag"]), /usage: node scripts/u);
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
    if (section.body.includes(emDash)) {
      problems.push(`${section.version} contains an em-dash`);
    }
    if (!link) {
      problems.push(`${section.version} has no link definition`);
    } else if (previous && link !== `${COMPARE_ROOT}/v${previous.version}...v${section.version}`) {
      problems.push(`${section.version} links ${link} instead of comparing v${previous.version}`);
    }
  });

  const versions = new Set(sections.map((section) => section.version));
  for (const [version] of links) {
    if (!versions.has(version)) {
      problems.push(`${version} is linked but has no section`);
    }
  }

  assert.deepEqual(problems, []);
});

test("pending changelog entries carry no em-dash, so a release cannot inherit one", () => {
  const unreleased = changelogSections(readFileSync("CHANGELOG.md", "utf8")).find(
    (section) => section.version === "Unreleased",
  );

  assert.ok(unreleased, "CHANGELOG.md must keep an Unreleased section");
  assert.ok(!unreleased.body.includes(emDash), "an Unreleased entry contains an em-dash");
});

test("the full gate passes on the repository's changelog in its release-commit shape", () => {
  const source = readFileSync("CHANGELOG.md", "utf8");
  const latest = changelogSections(source).find((section) =>
    /^\d+\.\d+\.\d+$/u.test(section.version),
  );
  // The release commit empties [Unreleased] as it renames it, so drop this
  // cycle's pending entries to reproduce the tree the workflow would see.
  const atRelease = source.replace(
    /## \[Unreleased\][^\n]*\n[\s\S]*?(?=\n## \[)/u,
    "## [Unreleased]\n",
  );

  assert.doesNotThrow(() => assertNothingUnreleased(atRelease));
  assert.equal(assertChangelogRelease(atRelease, `v${latest.version}`).version, latest.version);
});

test("the entry point reads both surfaces from disk and names what it verified", () => {
  const root = mkdtempSync(join(tmpdir(), "release-metadata-"));
  const releaseJson = join(root, "release.json");
  writeFileSync(join(root, "CHANGELOG.md"), changelog());
  writeFileSync(releaseJson, JSON.stringify(publishedRelease()));

  assert.deepEqual(verifyReleaseMetadata({ tag: "v3.21.0", root }), [
    "CHANGELOG.md section 3.21.0 (compares back to v3.20.0)",
    "an empty CHANGELOG.md [Unreleased] section",
  ]);
  assert.deepEqual(verifyReleaseMetadata({ tag: "v3.21.0", releaseJson, root }), [
    "CHANGELOG.md section 3.21.0 (compares back to v3.20.0)",
    "published GitHub Release title and body",
  ]);

  // The publish-time surface must survive a default branch that moved on.
  writeFileSync(
    join(root, "CHANGELOG.md"),
    changelog({ unreleased: "### Fixed\n\n- **Merged during the publish.**\n" }),
  );
  assert.doesNotThrow(() => verifyReleaseMetadata({ tag: "v3.21.0", releaseJson, root }));
  assert.throws(
    () => verifyReleaseMetadata({ tag: "v3.21.0", root }),
    /Unreleased section still holds entries/u,
  );

  writeFileSync(releaseJson, JSON.stringify(publishedRelease({ body: "No link here.\n" })));
  assert.throws(
    () => verifyReleaseMetadata({ tag: "v3.21.0", releaseJson, root }),
    /missing the compare link/u,
  );

  rmSync(root, { recursive: true, force: true });
});
