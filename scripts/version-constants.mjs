// The cache version and schema version constants of the Rust sources. A
// branch that bumps one of them must still change it against the base. When
// the base makes the same bump, git merges the two equal edits with no
// conflict, and two cache formats then share one version.

import { git } from "./ship-git.mjs";

// A cache version or a schema version constant. The value is an unsigned
// integer or a string literal.
const VERSION_LINE =
  /^([+-])\s*(?:pub(?:\([^)]*\))?\s+)?const\s+([A-Z0-9_]*(?:CACHE|SCHEMA)_VERSION)\s*:\s*(?:u8|u16|u32|u64|u128|usize|&(?:'static\s+)?str)\s*=\s*(\d+|"[^"]*")\s*;/u;

const DEV_NULL = "/dev/null";

/** The path of a `--- a/<path>` or `+++ b/<path>` diff header line. */
const headerPath = (line) => line.slice(4).replace(/^[ab]\//u, "");

const keyOf = ({ file, name }) => `${file}\0${name}`;

/**
 * The version constants whose value `to` changes against `from`: the cache
 * versions (`GRAPH_CACHE_VERSION`, the extraction `CACHE_VERSION` and the
 * others) and the schema versions of the public output
 * (`CHECK_SCHEMA_VERSION` and the others). A deleted file is keyed by its
 * old path. Returns `[{ file, name, from, to }]`, sorted by file and name.
 */
export const versionChanges = (cwd, from, to) => {
  const diff = git(cwd, ["diff", "--no-color", "--no-ext-diff", "-U0", from, to, "--", "*.rs"]);
  const values = new Map();
  let oldFile = null;
  let file = null;
  for (const line of diff.split("\n")) {
    if (line.startsWith("--- ")) {
      oldFile = headerPath(line);
      continue;
    }
    if (line.startsWith("+++ ")) {
      file = line.slice(4) === DEV_NULL ? oldFile : headerPath(line);
      continue;
    }
    const match = line.match(VERSION_LINE);
    if (match === null || file === null) {
      continue;
    }
    const [, sign, name, value] = match;
    const key = keyOf({ file, name });
    const entry = values.get(key) ?? { file, name, from: null, to: null };
    entry[sign === "-" ? "from" : "to"] = value;
    values.set(key, entry);
  }
  return [...values.values()]
    .filter((entry) => entry.from !== entry.to)
    .toSorted((left, right) =>
      left.file === right.file
        ? left.name.localeCompare(right.name)
        : left.file.localeCompare(right.file),
    );
};

export const formatVersionChange = ({ file, name, from, to }) =>
  `    ${file}: ${name} ${from ?? "(new)"} -> ${to ?? "(removed)"}`;

/** The changes of `before` that `after` does not contain. */
export const lostVersionChanges = (before, after) => {
  const kept = new Set(after.map(keyOf));
  return before.filter((change) => !kept.has(keyOf(change)));
};

/** The keys (`file\0name`) of a list of version changes. */
export const versionKeys = (changes) => new Set(changes.map(keyOf));
