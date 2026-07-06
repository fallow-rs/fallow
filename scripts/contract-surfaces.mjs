/**
 * Contract surfaces generated from Rust and manifest sources of truth.
 *
 * Keep this manifest narrow: it lists committed generated artifacts and the
 * check command that proves they are in sync. Human-authored docs should link
 * to the manifest instead of duplicating this list.
 */

export const contractSurfaces = Object.freeze([
  {
    id: "config-schema",
    owner: "fallow config-schema",
    generatedPaths: ["schema.json", "npm/fallow/schema.json"],
    checkCommand: "npm run generate:contracts:check",
    docs: ["docs/analyzer-authoring.md"],
    publicStability: "stable",
    newIssueKind: false,
  },
  {
    id: "plugin-schema",
    owner: "fallow plugin-schema",
    generatedPaths: ["plugin-schema.json"],
    checkCommand: "npm run generate:contracts:check",
    docs: ["docs/plugin-authoring.md"],
    publicStability: "stable",
    newIssueKind: false,
  },
  {
    id: "rule-pack-schema",
    owner: "fallow rule-pack-schema",
    generatedPaths: ["rule-pack-schema.json"],
    checkCommand: "npm run generate:contracts:check",
    docs: ["docs/fallow-compliance.md"],
    publicStability: "stable",
    newIssueKind: false,
  },
  {
    id: "capability-schema",
    owner: "fallow schema",
    generatedPaths: ["npm/fallow/capabilities.json"],
    checkCommand: "npm run generate:contracts:check",
    docs: ["docs/analyzer-authoring.md"],
    publicStability: "stable",
    newIssueKind: true,
  },
  {
    id: "issue-registry",
    owner: "fallow schema issue_types",
    generatedPaths: ["npm/fallow/issue-registry.json"],
    checkCommand: "npm run generate:contracts:check",
    docs: ["docs/analyzer-authoring.md"],
    publicStability: "stable",
    newIssueKind: true,
  },
  {
    id: "output-schema",
    owner: "fallow-schema-emit",
    generatedPaths: ["docs/output-schema.json"],
    checkCommand: "npm run generate:contracts:check",
    docs: ["docs/backwards-compatibility.md", "docs/analyzer-authoring.md"],
    publicStability: "stable",
    newIssueKind: true,
  },
  {
    id: "typescript-output-contract",
    owner: "editors/vscode/scripts/codegen-contracts.mjs",
    generatedPaths: [
      "editors/vscode/src/generated/output-contract.d.ts",
      "editors/vscode/src/generated/lsp-initialization-options.d.ts",
      "npm/fallow/types/output-contract.d.ts",
    ],
    checkCommand: "npm run generate:contracts:check",
    docs: ["docs/analyzer-authoring.md"],
    publicStability: "stable",
    newIssueKind: true,
  },
  {
    id: "agent-docs",
    owner: "scripts/generate-agent-docs.mjs",
    generatedPaths: ["npm/fallow/skills/fallow/**"],
    checkCommand: "npm run generate:contracts:check",
    docs: ["docs/analyzer-authoring.md"],
    publicStability: "stable",
    newIssueKind: true,
  },
  {
    id: "napi-types",
    owner: "crates/napi/scripts/write-dts.mjs",
    generatedPaths: ["crates/napi/index.d.ts"],
    checkCommand: "npm run generate:contracts:check",
    docs: ["crates/napi/README.md"],
    publicStability: "stable",
    newIssueKind: false,
  },
]);

export const contractSurfacePaths = () =>
  [...new Set(contractSurfaces.flatMap((surface) => surface.generatedPaths))].toSorted();
