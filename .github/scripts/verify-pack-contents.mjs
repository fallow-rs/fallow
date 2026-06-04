// Release-time integrity gate: assert that every file a packed npm tarball
// PROMISES via its own `package.json` `files` whitelist is actually present
// inside the tarball.
//
// Why this exists: npm silently drops a `files` entry that has no matching
// file on disk at pack time (a whitelist glob with zero matches is skipped,
// no warning, no error). The 2.76.0 release packed the `@fallow-cli/<platform>`
// packages before the `.sig` staging step in the build job was wired, so each
// platform tarball shipped without its `fallow.sig` / `fallow-lsp.sig` /
// `fallow-mcp.sig` siblings even though the `files` field listed them. The
// GitHub Action installer then hard-failed every install resolving to 2.76.0
// with `sig-missing`. The `pack()` helper in release.yml only checked that a
// tarball was produced, never that its contents satisfied the contract, so the
// broken package published cleanly. Refs #944.
//
// This gate reads the contract from the tarball's OWN package.json (so it
// verifies exactly what is about to publish, not the source tree) and fails
// the release if any declared plain-file entry is absent. Glob and negation
// entries are skipped because npm globs may legitimately match nothing.
//
// It also enforces a second, independent invariant for `@fallow-cli/<platform>`
// packages: every shipped binary must have a `.sig` sibling. The declared-files
// check alone would pass a package that dropped the `.sig` entries from BOTH
// `files` and disk (self-consistent but unsigned), which is the more likely
// FUTURE regression; this invariant guards the load-bearing property directly.
//
// Usage:
//   node verify-pack-contents.mjs <tarball.tgz> [<tarball.tgz> ...]
//
// Exits 0 when every declared file is present in every tarball, 1 otherwise.
// No external dependencies: uses node:child_process to drive the system `tar`.

import { execFileSync } from "node:child_process";
import { pathToFileURL } from "node:url";

// A `files` entry that contains any of these is a glob/negation pattern. npm
// resolves these against the working tree and they may match zero files
// legitimately, so they are out of scope for a strict presence check.
const GLOB_CHARS = /[*?[\]{}!]/;

// Files a `@fallow-cli/<platform>` package ships that are NOT signed binaries,
// so they have no `.sig` sibling. Matched case-insensitively at the top level.
const PLATFORM_METADATA = /^(package\.json|readme|licen[sc]e)/i;

// True for the signed CLI platform packages (`@fallow-cli/linux-x64-gnu`, ...),
// false for the `fallow` wrapper and the NAPI packages (`@fallow-cli/fallow-node*`),
// which ship no signed binaries.
function requiresSignedBinaries(name) {
  if (typeof name !== "string" || !name.startsWith("@fallow-cli/")) {
    return false;
  }
  return !name.slice("@fallow-cli/".length).startsWith("fallow-node");
}

// The declared-files check only verifies "deliver what you declare": if a
// future refactor dropped the `.sig` entries from BOTH `files` and disk, the
// tarball would be self-consistent and pass, silently reproducing #944. This
// independent invariant asserts the load-bearing property directly: every
// shipped binary in a CLI platform package has a `.sig` sibling. It is
// platform-agnostic (`fallow.exe` on win32 requires `fallow.exe.sig`) because
// it derives the requirement from the binaries actually present, not a
// hardcoded list. Returns the names of binaries whose signature is absent.
function missingSignatureSiblings(name, entries) {
  if (!requiresSignedBinaries(name)) {
    return [];
  }
  const gaps = [];
  for (const entry of entries) {
    if (entry.includes("/") || entry.endsWith(".sig") || PLATFORM_METADATA.test(entry)) {
      continue;
    }
    // Skip directory entries: `tar` lists `bin/` which the listing normalizer
    // strips to `bin`, indistinguishable from a top-level file. A directory is
    // recognizable by having a packed child under it.
    if ([...entries].some((candidate) => candidate.startsWith(`${entry}/`))) {
      continue;
    }
    if (!entries.has(`${entry}.sig`)) {
      gaps.push(`${entry}.sig`);
    }
  }
  return gaps;
}

function readManifestFromTarball(tgzPath) {
  // Stream package/package.json straight to stdout without extracting to disk.
  const raw = execFileSync("tar", ["-xzO", "-f", tgzPath, "package/package.json"], {
    encoding: "utf8",
    maxBuffer: 16 * 1024 * 1024,
  });
  return JSON.parse(raw);
}

function listTarballEntries(tgzPath) {
  const listing = execFileSync("tar", ["-tzf", tgzPath], {
    encoding: "utf8",
    maxBuffer: 64 * 1024 * 1024,
  });
  // npm prefixes every path with `package/`. Normalize away a single trailing
  // slash so directory entries compare cleanly against the prefix check below.
  const entries = new Set();
  for (const line of listing.split("\n")) {
    const trimmed = line.replace(/\/$/, "");
    if (trimmed.startsWith("package/")) {
      entries.add(trimmed.slice("package/".length));
    }
  }
  return entries;
}

// Verify a single tarball against its own `files` whitelist and, for CLI
// platform packages, the every-binary-is-signed invariant.
// Returns { ok, name, version, checked, missing, skipped, missingSignatures }.
export function verifyTarball(tgzPath) {
  const manifest = readManifestFromTarball(tgzPath);
  const declared = Array.isArray(manifest.files) ? manifest.files : [];
  const entries = listTarballEntries(tgzPath);
  const name = typeof manifest.name === "string" ? manifest.name : "<unknown>";

  const missing = [];
  const skipped = [];
  const checked = [];
  for (const entry of declared) {
    if (typeof entry !== "string" || entry.length === 0) {
      continue;
    }
    const normalized = entry.replace(/^\.\//, "").replace(/\/$/, "");
    if (GLOB_CHARS.test(normalized)) {
      skipped.push(entry);
      continue;
    }
    checked.push(normalized);
    // Present as an exact file, or as a directory whose contents were packed.
    const present =
      entries.has(normalized) ||
      [...entries].some((candidate) => candidate.startsWith(`${normalized}/`));
    if (!present) {
      missing.push(normalized);
    }
  }

  const missingSignatures = missingSignatureSiblings(name, entries);

  return {
    ok: missing.length === 0 && missingSignatures.length === 0,
    name,
    version: typeof manifest.version === "string" ? manifest.version : "<unknown>",
    checked,
    missing,
    skipped,
    missingSignatures,
  };
}

function main(argv) {
  const tarballs = argv.slice(2);
  if (tarballs.length === 0) {
    console.error("usage: verify-pack-contents.mjs <tarball.tgz> [<tarball.tgz> ...]");
    return 2;
  }

  let failures = 0;
  for (const tgz of tarballs) {
    let result;
    try {
      result = verifyTarball(tgz);
    } catch (err) {
      console.error(`::error::cannot inspect tarball ${tgz}: ${err.message}`);
      failures += 1;
      continue;
    }
    if (result.ok) {
      console.log(
        `OK ${result.name}@${result.version}: ${result.checked.length} declared file(s) present`,
      );
      continue;
    }
    failures += 1;
    if (result.missing.length > 0) {
      console.error(
        `::error::${result.name}@${result.version} (${tgz}) is missing declared files: ${result.missing.join(", ")}`,
      );
    }
    if (result.missingSignatures.length > 0) {
      console.error(
        `::error::${result.name}@${result.version} (${tgz}) ships an unsigned binary: missing ${result.missingSignatures.join(", ")}`,
      );
    }
  }

  if (failures > 0) {
    console.error(`::error::${failures} tarball(s) failed the packaging integrity check`);
    return 1;
  }
  return 0;
}

// Run as a CLI only when invoked directly, so the test file can import
// verifyTarball without triggering the argv loop. pathToFileURL handles
// percent-encoding so a checkout path with spaces or non-ASCII characters does
// not silently turn this into a no-op import.
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  process.exit(main(process.argv));
}
