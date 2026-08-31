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
//
// Publication runs against the default branch, which keeps moving while a
// release publishes, so only rules that read a frozen released section may run
// on that surface. Rules about pending work belong to dispatch alone.

import assert from "node:assert/strict";
import { readFileSync, realpathSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const EM_DASH = "—";

/**
 * Upstream projects that must not be named on the public release surface.
 * Deliberately not applied to CHANGELOG.md: entries there legitimately name a
 * migration source (`knip.json`) or a documented parity gap.
 */
const COMPETITOR_PATTERN = /knip|jscpd|ts-prune|depcheck/iu;

/**
 * The only host a compare link may point at. Without this a typo in the host
 * passes the changelog gate and is then REQUIRED in the published body, so one
 * mistake propagates through both surfaces instead of being caught by either.
 */
const COMPARE_URL_PREFIX = "https://github.com/fallow-rs/fallow/compare/";

const RELEASE_HEADING = /^## \[(?<version>[^\]]+)\](?: - (?<date>\S+))?\s*$/u;

const CODE_FENCE = /^\s*(?:```|~~~)/u;

const SEMVER = /^(?<major>\d+)\.(?<minor>\d+)\.(?<patch>\d+)$/u;

const ISO_DATE = /^(?<year>\d{4})-(?<month>\d{2})-(?<day>\d{2})$/u;

/** `v3.21.0` to `3.21.0`, rejecting anything the release workflow would refuse. */
export const versionOfTag = (tag) => {
  assert.match(tag ?? "", /^v\d+\.\d+\.\d+$/u, `release tag must match vMAJOR.MINOR.PATCH: ${tag}`);
  return tag.slice(1);
};

/** A real calendar date, so a transposed `2026-31-08` cannot pass as a shape. */
const isCalendarDate = (value) => {
  const parts = value?.match(ISO_DATE)?.groups;
  if (!parts) {
    return false;
  }
  const year = Number(parts.year);
  const month = Number(parts.month);
  const day = Number(parts.day);
  const date = new Date(Date.UTC(year, month - 1, day));
  return (
    date.getUTCFullYear() === year && date.getUTCMonth() === month - 1 && date.getUTCDate() === day
  );
};

const versionOrder = (version) => {
  const { major, minor, patch } = version.match(SEMVER).groups;
  return [Number(major), Number(minor), Number(patch)];
};

const isNewerVersion = (candidate, reference) => {
  const left = versionOrder(candidate);
  const right = versionOrder(reference);
  const differing = left.findIndex((value, index) => value !== right[index]);
  return differing !== -1 && left[differing] > right[differing];
};

/**
 * Every `## [...]` section in document order, each carrying the raw body that
 * follows it up to the next heading. Fenced blocks are skipped, so an entry
 * documenting the changelog format cannot open a section from inside its own
 * code sample.
 */
export const changelogSections = (changelog) => {
  const lines = changelog.split(/\r?\n/u);
  const starts = [];
  let fenced = false;

  lines.forEach((line, index) => {
    if (CODE_FENCE.test(line)) {
      fenced = !fenced;
      return;
    }
    if (fenced) {
      return;
    }
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
 * link must span back to. Sections must descend, or a rebase artifact would
 * produce a backwards compare link that the gate then demands.
 */
export const previousReleasedVersion = (sections, version) => {
  const released = sections.filter((section) => SEMVER.test(section.version));
  const position = released.findIndex((section) => section.version === version);
  assert.notEqual(position, -1, `CHANGELOG.md has no released section for ${version}`);
  const previous = released[position + 1];
  assert.ok(previous, `CHANGELOG.md has no section before ${version} to compare against`);
  assert.ok(
    isNewerVersion(version, previous.version),
    `CHANGELOG.md: ${version} sits above ${previous.version}, so the sections are out of order`,
  );
  return previous.version;
};

/** The `[x.y.z]: <url>` reference definitions, a later repeat winning. */
export const changelogLinks = (changelog) =>
  new Map(
    Array.from(
      changelog.matchAll(/^\[(?<version>\d+\.\d+\.\d+)\]:\s*(?<url>\S+)\s*$/gmu),
      (match) => [match.groups.version, match.groups.url],
    ),
  );

/**
 * Verifies the released section the notes are drafted from, and returns the
 * compare URL the published body must carry. Every rule here reads a section
 * that is frozen once the version ships, so it holds at dispatch and still
 * holds afterwards.
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
    isCalendarDate(section.date),
    `CHANGELOG.md: "## [${version}]" needs a real YYYY-MM-DD date, found ${section.date ?? "none"}`,
  );
  assert.ok(
    hasContent(section.body),
    `CHANGELOG.md: the ${version} section is empty, so the release notes were never written`,
  );
  assert.ok(
    !section.body.includes(EM_DASH),
    `CHANGELOG.md: the ${version} section contains an em-dash, which the release notes inherit`,
  );

  const previous = previousReleasedVersion(sections, version);
  const expected = `${COMPARE_URL_PREFIX}v${previous}...v${version}`;
  const link = changelogLinks(changelog).get(version);
  assert.ok(link, `CHANGELOG.md: no "[${version}]:" link definition`);
  assert.equal(link, expected, `CHANGELOG.md: "[${version}]:" must be ${expected}, found ${link}`);

  return { version, previousTag: `v${previous}`, compareUrl: link };
};

/**
 * Everything still pending must ship in this release or be credited to a later
 * one. Dispatch-time only: `[Unreleased]` fills up again the moment the next
 * change lands on the default branch, so asserting it after publication would
 * fail a correct release whenever anything merged during the publish.
 */
export const assertNothingUnreleased = (changelog) => {
  const unreleased = changelogSections(changelog).filter(
    (section) => section.version === "Unreleased",
  );
  assert.equal(
    unreleased.length,
    1,
    `CHANGELOG.md must hold exactly one "## [Unreleased]" section, found ${unreleased.length}`,
  );
  assert.ok(
    !hasContent(unreleased[0].body),
    "CHANGELOG.md: the Unreleased section still holds entries, so they would ship uncredited",
  );
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

export const parseArguments = (argv) => {
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

  if (releaseJson === undefined) {
    assertNothingUnreleased(changelog);
    surfaces.push("an empty CHANGELOG.md [Unreleased] section");
  } else {
    assertPublishedRelease(JSON.parse(readFileSync(releaseJson, "utf8")), tag, compareUrl);
    surfaces.push("published GitHub Release title and body");
  }

  return surfaces;
};

// `process.argv[1]` keeps symlinks while Node realpaths the ESM entry, so a
// lexical comparison silently skips the gate when invoked through one. It is
// not always a real path either (`node -e` puts the first argument there), so
// an unresolvable value means this module was imported, not invoked.
const realPathOrNull = (value) => {
  try {
    return value === undefined ? null : realpathSync(value);
  } catch {
    return null;
  }
};

const isDirectInvocation =
  realPathOrNull(process.argv[1]) === realpathSync(fileURLToPath(import.meta.url));

if (isDirectInvocation) {
  try {
    for (const surface of verifyReleaseMetadata(parseArguments(process.argv.slice(2)))) {
      console.log(`Verified ${surface}.`);
    }
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    process.exitCode = 1;
  }
}
