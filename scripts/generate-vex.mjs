#!/usr/bin/env node
// Build .vex/openvex.json from the `[[IgnoredVulns]]` entries in every
// osv-scanner.toml. Each ignore carries a written reason, so each one becomes
// a `not_affected` statement. Pass --check to fail when the file is stale.
import { execFileSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";

const OUTPUT = ".vex/openvex.json";
const PRODUCT = "pkg:github/fallow-rs/fallow";
const FIXED_TIMESTAMP = "2026-10-19T00:00:00Z";

const tomlFiles = execFileSync(
  "git",
  ["ls-files", "--", "**/osv-scanner.toml", "osv-scanner.toml"],
  {
    encoding: "utf8",
  },
)
  .split("\n")
  .filter(Boolean)
  .toSorted();

const entries = new Map();
for (const file of tomlFiles) {
  const blocks = readFileSync(file, "utf8")
    .split(/^\[\[IgnoredVulns\]\]\s*$/mu)
    .slice(1);
  for (const block of blocks) {
    const id = /^id\s*=\s*"([^"]+)"/mu.exec(block)?.[1];
    const reason = /^reason\s*=\s*"([^"]+)"/mu.exec(block)?.[1];
    if (!id || !reason) throw new Error(`${file}: an IgnoredVulns entry needs an id and a reason`);
    const previous = entries.get(id);
    entries.set(id, {
      id,
      reason: previous?.reason ?? reason,
      files: [...(previous?.files ?? []), file],
    });
  }
}

const document = {
  "@context": "https://openvex.dev/ns/v0.2.0",
  "@id": "https://github.com/fallow-rs/fallow/blob/main/.vex/openvex.json",
  author: "fallow maintainers",
  timestamp: FIXED_TIMESTAMP,
  version: 1,
  statements: [...entries.values()]
    .toSorted((a, b) => a.id.localeCompare(b.id))
    .map(({ id, reason, files }) => ({
      vulnerability: { name: id },
      products: [{ "@id": PRODUCT }],
      status: "not_affected",
      justification: "vulnerable_code_not_in_execute_path",
      impact_statement: `${reason} Source: ${files.join(", ")}`,
    })),
};
const rendered = `${JSON.stringify(document, null, 2)}\n`;

if (process.argv.includes("--check")) {
  let current = "";
  try {
    current = readFileSync(OUTPUT, "utf8");
  } catch {}
  if (current !== rendered) {
    console.error(`${OUTPUT} is stale. Run: node scripts/generate-vex.mjs`);
    process.exit(1);
  }
} else {
  writeFileSync(OUTPUT, rendered);
}
