import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

// The skill reference names every workspace diagnostic kind that the dead-code
// analyze pass records. The exhaustive `is_analysis_stage` match is the source
// of truth, so a new analysis-stage kind fails here until the reference names
// it.

const REPO_ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const WORKSPACE_RS = "crates/types/src/workspace.rs";
const SKILL_REFERENCE = "npm/fallow/skills/fallow/references/cli-reference.md";
const DOC_SENTENCE =
  /The (?:\w+ )?analysis-stage kinds \(([^)]*)\) are recorded by the dead-code analyze pass/u;

const functionBody = (source, signature) => {
  const start = source.indexOf(signature);
  assert.notEqual(start, -1, `missing \`${signature}\` in ${WORKSPACE_RS}`);
  const end = source.indexOf("\n    }\n", start);
  assert.notEqual(end, -1, `unterminated \`${signature}\` in ${WORKSPACE_RS}`);
  return source.slice(start, end);
};

const analysisStageKindIds = (source) => {
  const ids = new Map();
  const idBody = functionBody(source, "pub const fn id(&self) -> &'static str {");
  for (const [, variant, id] of idBody.matchAll(
    /Self::(\w+)(?: \{ \.\. \})? => "([a-z0-9-]+)"/gu,
  )) {
    ids.set(variant, id);
  }

  const stageBody = functionBody(source, "pub const fn is_analysis_stage(&self) -> bool {");
  const trueArm = stageBody.slice(0, stageBody.indexOf("=> true"));
  assert.notEqual(trueArm.length, stageBody.length, "missing `=> true` arm in is_analysis_stage");
  return [...trueArm.matchAll(/Self::(\w+)/gu)].map(([, variant]) => {
    assert.ok(ids.has(variant), `no id() arm for ${variant}`);
    return ids.get(variant);
  });
};

const documentedAnalysisStageKinds = (markdown) => {
  const match = DOC_SENTENCE.exec(markdown);
  assert.ok(match, `missing the analysis-stage sentence in ${SKILL_REFERENCE}`);
  return [...match[1].matchAll(/`([a-z0-9-]+)`/gu)].map(([, id]) => id);
};

test("the skill reference names exactly the analysis-stage kinds", () => {
  const source = readFileSync(join(REPO_ROOT, WORKSPACE_RS), "utf8");
  const markdown = readFileSync(join(REPO_ROOT, SKILL_REFERENCE), "utf8");
  assert.deepEqual(
    documentedAnalysisStageKinds(markdown).toSorted(),
    analysisStageKindIds(source).toSorted(),
  );
});

test("the parser reads the true arm of is_analysis_stage only", () => {
  const source = `
    pub const fn id(&self) -> &'static str {
        match self {
            Self::Alpha => "alpha",
            Self::Beta { .. } => "beta",
            Self::Gamma => "gamma",
        }
    }

    pub const fn is_analysis_stage(&self) -> bool {
        match self {
            Self::Alpha | Self::Beta { .. } => true,
            Self::Gamma => false,
        }
    }
`;
  assert.deepEqual(analysisStageKindIds(source), ["alpha", "beta"]);
});

test("a reference that misses a kind does not match", () => {
  const markdown = "The analysis-stage kinds (`alpha`) are recorded by the dead-code analyze pass.";
  assert.deepEqual(documentedAnalysisStageKinds(markdown), ["alpha"]);
  assert.notDeepEqual(documentedAnalysisStageKinds(markdown), ["alpha", "beta"]);
});
