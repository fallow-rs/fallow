// Post-release automation: rewrite the Dockerfile's `ARG FALLOW_VERSION` line
// and its two per-arch sha256 pins (`fallow-linux-x64-musl`,
// `fallow-linux-arm64-musl`) to match a just-published release. The pins were
// hand-maintained starting at 2.94.0 (commit 1a8e7ac8d), drifted through 13
// releases, and were fixed manually by #1805. This script is the rewrite step
// invoked by the maintainer's release flow (fallow-release skill step 13),
// mirroring the crates/napi lockfile catch-up (step 12). Refs #1817.
//
// Every guard below fails loud (non-zero exit, file left untouched) instead
// of silently no-oping, so a future Dockerfile refactor (renamed ARG,
// reordered case arms, a third architecture) breaks the release job visibly
// instead of quietly leaving the pins stale again.
//
// Usage:
//   node update-dockerfile-pins.mjs <version> <amd64-sha256> <arm64-sha256> [dockerfile]
//
// <version> is the bare release version (no leading "v"), matching
// GITHUB_REF_NAME with the tag prefix stripped. Exits 0 and rewrites the file
// in place when the pins actually change; exits 1 (file untouched) otherwise.

import { readFileSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

const DEFAULT_DOCKERFILE = "Dockerfile";

// Keyed by the literal `asset="..."` value inside each Dockerfile case arm, so
// replacement is correct even if the case arms are reordered (arm64 before
// amd64, say). An asset value that appears in the Dockerfile but not here (or
// vice versa) trips the "unexpected asset" / "does not declare asset" guards.
const KNOWN_ASSETS = ["fallow-linux-x64-musl", "fallow-linux-arm64-musl"];

const VERSION_PATTERN = /^\d+\.\d+\.\d+$/;
const SHA256_PATTERN = /^[0-9a-f]{64}$/i;
const ARG_LINE_PATTERN = /^ARG FALLOW_VERSION=.*$/;
const ASSET_LINE_PATTERN = /^(\s*)asset="([^"]+)";(.*)$/;
const SHA_LINE_PATTERN = /^(\s*)sha256="([^"]*)";(.*)$/;

function assertValidVersion(version) {
  if (!VERSION_PATTERN.test(version)) {
    throw new Error(`invalid version "${version}": expected bare X.Y.Z (no leading "v")`);
  }
}

function assertValidSha(label, sha) {
  if (!SHA256_PATTERN.test(sha)) {
    throw new Error(`invalid ${label} sha256 "${sha}": expected 64 hex characters`);
  }
}

// Rewrites `source` to match { version, amd64Sha, arm64Sha }. Pure: throws
// instead of touching disk. Also throws (rather than returning the input
// unchanged) when the rewrite would be a no-op, since an unmatched rewrite is
// the most likely symptom of a Dockerfile structure the guards below did not
// anticipate.
export function computeUpdatedDockerfile(source, { version, amd64Sha, arm64Sha }) {
  assertValidVersion(version);
  assertValidSha("amd64", amd64Sha);
  assertValidSha("arm64", arm64Sha);

  const shaByAsset = {
    "fallow-linux-x64-musl": amd64Sha.toLowerCase(),
    "fallow-linux-arm64-musl": arm64Sha.toLowerCase(),
  };

  const argMatches = source.match(new RegExp(ARG_LINE_PATTERN.source, "gm")) ?? [];
  if (argMatches.length !== 1) {
    throw new Error(`expected exactly 1 "ARG FALLOW_VERSION=" line, found ${argMatches.length}`);
  }

  const seenAssets = new Set();
  const resolvedAssets = new Set();
  let pendingAsset = null;

  const rewritten = source.split("\n").map((line) => {
    if (ARG_LINE_PATTERN.test(line)) {
      return `ARG FALLOW_VERSION=${version}`;
    }

    const assetMatch = line.match(ASSET_LINE_PATTERN);
    if (assetMatch) {
      const [, , assetName] = assetMatch;
      if (!KNOWN_ASSETS.includes(assetName)) {
        throw new Error(`unexpected asset "${assetName}" in Dockerfile case arm`);
      }
      if (seenAssets.has(assetName)) {
        throw new Error(`duplicate asset entry "${assetName}"`);
      }
      seenAssets.add(assetName);
      pendingAsset = assetName;
      return line;
    }

    const shaMatch = line.match(SHA_LINE_PATTERN);
    if (shaMatch) {
      if (pendingAsset === null) {
        throw new Error(`sha256 pin ${JSON.stringify(line.trim())} has no preceding asset= line`);
      }
      const [, indent, , trailer] = shaMatch;
      const replaced = `${indent}sha256="${shaByAsset[pendingAsset]}";${trailer}`;
      resolvedAssets.add(pendingAsset);
      pendingAsset = null;
      return replaced;
    }

    return line;
  });

  for (const assetName of KNOWN_ASSETS) {
    if (!seenAssets.has(assetName)) {
      throw new Error(`Dockerfile does not declare asset "${assetName}"`);
    }
    if (!resolvedAssets.has(assetName)) {
      throw new Error(`asset "${assetName}" has no sha256 pin`);
    }
  }

  const updated = rewritten.join("\n");
  if (updated === source) {
    throw new Error("rewrite produced no changes; refusing a silent no-op");
  }
  return updated;
}

function main(argv) {
  const [version, amd64Sha, arm64Sha, dockerfilePath = DEFAULT_DOCKERFILE] = argv.slice(2);
  if (!version || !amd64Sha || !arm64Sha) {
    console.error(
      "usage: update-dockerfile-pins.mjs <version> <amd64-sha256> <arm64-sha256> [dockerfile]",
    );
    return 2;
  }

  let source;
  try {
    source = readFileSync(dockerfilePath, "utf8");
  } catch (err) {
    console.error(`::error::cannot read ${dockerfilePath}: ${err.message}`);
    return 1;
  }

  let updated;
  try {
    updated = computeUpdatedDockerfile(source, { version, amd64Sha, arm64Sha });
  } catch (err) {
    console.error(`::error::${err.message}`);
    return 1;
  }

  writeFileSync(dockerfilePath, updated);
  console.log(`OK: ${dockerfilePath} pinned to FALLOW_VERSION=${version}`);
  return 0;
}

// Run as a CLI only when invoked directly, so the test file can import
// computeUpdatedDockerfile without triggering the argv loop. pathToFileURL
// handles percent-encoding so a checkout path with spaces or non-ASCII
// characters does not silently turn this into a no-op import.
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exit(main(process.argv));
}
