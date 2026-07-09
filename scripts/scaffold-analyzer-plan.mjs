#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const CODE_PATTERN = /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/u;
const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..");
const ANALYZER_PLAN_DIR = path.join(REPO_ROOT, ".plans", "analyzers");

const usage = () => {
  console.error("Usage: npm run scaffold:analyzer -- <kebab-code> [--force]");
};

const parseArgs = (args) => {
  const force = args.includes("--force");
  const values = args.filter((arg) => arg !== "--force");

  if (values.length !== 1) {
    return { ok: false, error: "expected exactly one analyzer code" };
  }

  const [code] = values;
  if (!CODE_PATTERN.test(code)) {
    return {
      ok: false,
      error: "analyzer code must be lowercase kebab case, for example unused-store-member",
    };
  }

  return { ok: true, data: { code, force } };
};

const assertPlanPath = (targetPath) => {
  const relative = path.relative(ANALYZER_PLAN_DIR, targetPath);
  if (relative.startsWith("..") || path.isAbsolute(relative)) {
    throw new Error("refusing to write outside .plans/analyzers");
  }
};

const assertWritablePlanPath = (targetPath) => {
  const planDirRealPath = fs.realpathSync(ANALYZER_PLAN_DIR);
  const targetParentRealPath = fs.realpathSync(path.dirname(targetPath));
  if (targetParentRealPath !== planDirRealPath) {
    throw new Error("refusing to write outside .plans/analyzers");
  }

  let targetStats = null;
  try {
    targetStats = fs.lstatSync(targetPath);
  } catch (error) {
    if (!error || error.code !== "ENOENT") {
      throw error;
    }
  }

  if (targetStats?.isSymbolicLink()) {
    throw new Error("refusing to overwrite a symlink plan file");
  }
  if (targetStats && !targetStats.isFile()) {
    throw new Error("refusing to overwrite a non-file plan path");
  }
};

const planTemplate = (code) => `# ${code}

Status: draft

## Contract

- [ ] Add or verify the shared metadata row in \`crates/types/src/issue_meta.rs\`.
- [ ] Pick the stable \`rule_id\`, issue \`code\`, rules key, aliases, suppression token, and docs anchor.
- [ ] Decide whether the finding needs an \`IssueKind\` in \`crates/types/src/suppress.rs\`.
- [ ] Add \`actions\` entries only when agents can apply or suppress the finding safely.

## Implementation

- [ ] Keep extraction, graph facts, and reporting in the narrowest crate that owns the stage.
- [ ] Reuse existing framework detection and config resolution helpers before adding new abstractions.
- [ ] Add rule severity defaults, aliases, and unknown-key suggestions in \`crates/config/src/config/rules.rs\`.
- [ ] Add \`fallow explain\` coverage in \`crates/cli/src/explain.rs\`.
- [ ] Update LSP, MCP, schemas, generated types, and CI-facing formats when the finding is user visible.

## Fixture Matrix

| Fixture kind | Path or test | Proof |
| --- | --- | --- |
| Positive minimal | TBD | Finding appears for the smallest real shape. |
| Negative abstain | TBD | Analyzer stays silent when prerequisites are missing. |
| False-positive guard | TBD | Nearby valid pattern does not report. |
| Suppression | TBD | Intended suppression is consumed. |
| Filter and severity | TBD | Rule key, CLI filter, and severity gate select the finding. |
| Output contract | TBD | JSON, SARIF, compact, and human output stay stable. |
| Framework regression | TBD | Distilled real-world framework pattern keeps working. |

## Verification

- [ ] \`cargo test --workspace --lib --bins --tests --examples\`
- [ ] \`cargo check --workspace --benches\`
- [ ] \`npm run generate:contracts:check\`
- [ ] Real project smoke with \`--format json --quiet\`
`;

const main = () => {
  const parsed = parseArgs(process.argv.slice(2));
  if (!parsed.ok) {
    console.error(parsed.error);
    usage();
    process.exitCode = 1;
    return;
  }

  const { code, force } = parsed.data;
  const targetPath = path.join(ANALYZER_PLAN_DIR, `${code}.md`);
  assertPlanPath(targetPath);

  fs.mkdirSync(ANALYZER_PLAN_DIR, { recursive: true });
  assertWritablePlanPath(targetPath);
  fs.writeFileSync(targetPath, planTemplate(code), {
    encoding: "utf8",
    flag: force ? "w" : "wx",
  });
  console.log(path.relative(REPO_ROOT, targetPath));
};

try {
  main();
} catch (error) {
  if (error && error.code === "EEXIST") {
    console.error("plan already exists, pass --force to overwrite it");
  } else {
    console.error(error instanceof Error ? error.message : String(error));
  }
  process.exitCode = 1;
}
