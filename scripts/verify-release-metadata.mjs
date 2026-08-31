// Enforces the curated release metadata that the maintainer release runbook
// checks by hand. Two surfaces, because `release.yml` is tag-last and the two
// pieces of metadata do not exist at the same moment:
//
//   1. At dispatch the GitHub Release does not exist yet, but CHANGELOG.md
//      does, and the release notes are drafted from it. Gating the changelog
//      section blocks publication on an uncurated release.
//   2. `release: published` is the first moment a title and body exist. They
//      stay editable after publication (immutability freezes assets, tag and
//      target), so a failure there is repairable with `gh release edit`.

import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const EM_DASH = "—";

/**
 * Upstream projects that must not be named on the public release surface.
 * Deliberately not applied to CHANGELOG.md: entries there legitimately name a
 * migration source (`knip.json`) or a documented parity gap.
 */
const COMPETITOR_PATTERN = /knip|jscpd|ts-prune|depcheck/iu;

const RELEASE_HEADING = /^## \[(?<version>[^\]]+)\](?: - (?<date>\S+))?\s*$/u;

const SEMVER = /^\d+\.\d+\.\d+$/u;

const ISO_DATE = /^\d{4}-\d{2}-\d{2}$/u;

/** `v3.21.0` to `3.21.0`, rejecting anything the release workflow would refuse. */
export const versionOfTag = (tag) => {
  assert.match(tag ?? "", /^v\d+\.\d+\.\d+$/u, `release tag must match vMAJOR.MINOR.PATCH: ${tag}`);
  return tag.slice(1);
};

/**
 * Every `## [...]` section in document order, each carrying the raw body that
 * follows it up to the next heading.
 */
export const changelogSections = (changelog) => {
  const lines = changelog.split(/\r?\n/u);
  const starts = [];
  lines.forEach((line, index) => {
    const match = line.match(RELEASE_HEADING);
    if (match) {
      starts.push({ index, version: match.groups.version, date: match.groups.date });
    }
  });

  return starts.map((start, position) => ({
    version: start.version,
    date: start.date,
    line: start.index + 1,
    body: lines.slice(start.index + 1, starts[position + 1]?.index ?? lines.length).join("\n"),
  }));
};

const hasContent = (body) => body.split(/\r?\n/u).some((line) => line.trim() !== "");

/**
 * The released section directly below `version`, which is the tag the compare
 * link must span back to.
 */
export const previousReleasedVersion = (sections, version) => {
  const released = sections.filter((section) => SEMVER.test(section.version));
  const position = released.findIndex((section) => section.version === version);
  assert.notEqual(position, -1, `CHANGELOG.md has no released section for ${version}`);
  const previous = released[position + 1];
  assert.ok(previous, `CHANGELOG.md has no section before ${version} to compare against`);
  return previous.version;
};

/** The `[x.y.z]: <url>` reference definitions at the foot of the changelog. */
export const changelogLinks = (changelog) =>
  new Map(
    Array.from(
      changelog.matchAll(/^\[(?<version>\d+\.\d+\.\d+)\]:\s*(?<url>\S+)\s*$/gmu),
      (match) => [match.groups.version, match.groups.url],
    ),
  );

/**
 * The dispatch-time gate. Verifies the curated source the release notes are
 * written from, and returns the compare URL the published body must carry.
 */
export const assertChangelogRelease = (changelog, tag) => {
  const version = versionOfTag(tag);
  const sections = changelogSections(changelog);

  const matching = sections.filter((section) => section.version === version);
  assert.equal(
    matching.length,
    1,
    `CHANGELOG.md must hold exactly one "## [${version}]" section, found ${matching.length}`,
  );
  const [section] = matching;

  assert.ok(
    section.date !== undefined && ISO_DATE.test(section.date),
    `CHANGELOG.md: "## [${version}]" needs a YYYY-MM-DD date, found ${section.date ?? "none"}`,
  );
  assert.ok(
    hasContent(section.body),
    `CHANGELOG.md: the ${version} section is empty, so the release notes were never written`,
  );
  assert.ok(
    !section.body.includes(EM_DASH),
    `CHANGELOG.md: the ${version} section contains an em-dash, which the release notes inherit`,
  );

  const unreleased = sections.find((entry) => entry.version === "Unreleased");
  assert.ok(unreleased, "CHANGELOG.md: the Unreleased section is missing");
  assert.ok(
    !hasContent(unreleased.body),
    "CHANGELOG.md: the Unreleased section still holds entries, so they would ship uncredited",
  );

  const previous = previousReleasedVersion(sections, version);
  const link = changelogLinks(changelog).get(version);
  assert.ok(link, `CHANGELOG.md: no "[${version}]:" link definition`);
  assert.ok(
    link.endsWith(`/compare/v${previous}...v${version}`),
    `CHANGELOG.md: "[${version}]:" must compare v${previous}...v${version}, found ${link}`,
  );

  return { version, previousTag: `v${previous}`, compareUrl: link };
};

/**
 * The publication-time gate. `release` is a `gh release view --json` payload,
 * `compareUrl` comes from the changelog gate above.
 */
export const assertPublishedRelease = (release, tag, compareUrl) => {
  versionOfTag(tag);

  assert.equal(release.isDraft, false, `${tag} is still a draft`);
  assert.equal(release.isPrerelease, false, `${tag} is marked as a prerelease`);

  const title = release.name ?? "";
  const prefix = `${tag}: `;
  assert.ok(
    title.startsWith(prefix),
    `release title must start with "${prefix}", found "${title}"`,
  );
  assert.ok(
    title.slice(prefix.length).trim() !== "",
    `release title carries no summary after "${prefix}"`,
  );

  const body = release.body ?? "";
  assert.ok(body.trim() !== "", `release ${tag} has an empty body`);
  assert.ok(
    body.includes(compareUrl),
    `release ${tag} body is missing the compare link ${compareUrl}`,
  );

  assert.ok(!title.includes(EM_DASH), "release title contains an em-dash");
  assert.ok(!body.includes(EM_DASH), "release body contains an em-dash");

  assert.doesNotMatch(title, COMPETITOR_PATTERN, "release title names an upstream project");
  assert.doesNotMatch(body, COMPETITOR_PATTERN, "release body names an upstream project");
};

const parseArguments = (argv) => {
  const options = { tag: undefined, releaseJson: undefined };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (flag === "--tag") {
      index += 1;
      options.tag = argv[index];
    } else if (flag === "--release-json") {
      index += 1;
      options.releaseJson = argv[index];
    } else {
      assert.fail(`unknown argument: ${flag}`);
    }
  }
  assert.ok(
    options.tag,
    "usage: node scripts/verify-release-metadata.mjs --tag vX.Y.Z [--release-json <file>]",
  );
  return options;
};

export const verifyReleaseMetadata = ({
  tag,
  releaseJson,
  root = resolve(dirname(fileURLToPath(import.meta.url)), ".."),
}) => {
  const changelog = readFileSync(resolve(root, "CHANGELOG.md"), "utf8");
  const { version, previousTag, compareUrl } = assertChangelogRelease(changelog, tag);
  const surfaces = [`CHANGELOG.md section ${version} (compares back to ${previousTag})`];

  if (releaseJson !== undefined) {
    assertPublishedRelease(JSON.parse(readFileSync(releaseJson, "utf8")), tag, compareUrl);
    surfaces.push("published GitHub Release title and body");
  }

  return surfaces;
};

const isDirectInvocation =
  process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url);

if (isDirectInvocation) {
  const options = parseArguments(process.argv.slice(2));
  for (const surface of verifyReleaseMetadata(options)) {
    console.log(`Verified ${surface}.`);
  }
}
