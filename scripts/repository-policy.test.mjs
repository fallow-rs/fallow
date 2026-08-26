import assert from "node:assert/strict";
import { readFileSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";
import { checkRepositorySigningKeyParity } from "./signing-key-parity.mjs";

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));

const markdownFilesUnder = (root) =>
  readdirSync(root, { withFileTypes: true }).flatMap((entry) => {
    const path = join(root, entry.name);
    if (entry.isDirectory()) {
      return markdownFilesUnder(path);
    }
    return entry.name.endsWith(".md") ? [path] : [];
  });

const dependabotUpdate = (config, ecosystem, directory) => {
  const update = config
    .split(/(?=^  - package-ecosystem: )/mu)
    .find(
      (candidate) =>
        candidate.includes(`package-ecosystem: ${ecosystem}`) &&
        candidate.includes(`directory: ${directory}`),
    );
  assert.ok(update, `missing Dependabot update for ${ecosystem} in ${directory}`);
  return update;
};

const documentedFields = (guide, heading) => {
  const marker = `### ${heading}`;
  const start = guide.indexOf(marker);
  assert.notEqual(start, -1, `missing ${heading} section`);

  const remaining = guide.slice(start + marker.length);
  const nextHeading = remaining.indexOf("\n### ");
  const section = nextHeading === -1 ? remaining : remaining.slice(0, nextHeading);

  return [...section.matchAll(/^\|\s*`([^`]+)`\s*\|/gm)].map((match) => match[1]);
};

const markdownSection = (document, heading) => {
  const marker = `## ${heading}`;
  const start = document.indexOf(marker);
  assert.notEqual(start, -1, `missing ${heading} section`);

  const remaining = document.slice(start + marker.length);
  const nextHeading = remaining.indexOf("\n## ");
  return nextHeading === -1 ? remaining : remaining.slice(0, nextHeading);
};

const exportedNodeFunctions = (declarations) =>
  [...declarations.matchAll(/^export function ([A-Za-z_$][\w$]*)\(/gmu)].map((match) => match[1]);

const missingDocumentedNodeFunctions = (declarations, readme) => {
  const section = markdownSection(readme, "Editors and integrations");
  return exportedNodeFunctions(declarations).filter(
    (functionName) => !section.includes(`\`${functionName}\``),
  );
};

test("committed binary-signing public keys remain in parity", () => {
  assert.equal(checkRepositorySigningKeyParity().length, 32);
});

test("fuzz Dependabot updates stay scoped to its registry dependency", () => {
  const config = readFileSync(".github/dependabot.yml", "utf8");
  const update = dependabotUpdate(config, "cargo", "/fuzz");
  const allowedDependencies = [...update.matchAll(/^\s+- dependency-name: ([^\s]+)$/gmu)].map(
    (match) => match[1],
  );

  assert.deepEqual(allowedDependencies, ["libfuzzer-sys"]);
});

test("review Electron holds majors that exceed its wrapper and runtime", () => {
  const config = readFileSync(".github/dependabot.yml", "utf8");
  const update = dependabotUpdate(config, "npm", "/apps/review-electron");

  assert.match(
    update,
    /- dependency-name: vite\s+update-types: \["version-update:semver-major"\]/u,
  );
  assert.match(
    update,
    /- dependency-name: "@vitejs\/plugin-react"\s+update-types: \["version-update:semver-major"\]/u,
  );
  assert.match(
    update,
    /- dependency-name: "@types\/node"\s+update-types: \["version-update:semver-major"\]/u,
  );
});

test("root Node API overview follows the published declarations", () => {
  const declarations = readFileSync("crates/napi/types/index.d.ts", "utf8");
  const readme = readFileSync("README.md", "utf8");
  const missing = missingDocumentedNodeFunctions(declarations, readme);

  assert.deepEqual(missing, [], `root Node API overview is missing: ${missing.join(", ")}`);
  assert.match(
    markdownSection(readme, "Editors and integrations"),
    /\[package API reference\]\(crates\/napi\/README\.md\)/u,
  );

  const firstFunction = exportedNodeFunctions(declarations)[0];
  const neuteredReadme = readme.replace(`\`${firstFunction}\``, "`removedFunction`");
  assert.deepEqual(missingDocumentedNodeFunctions(declarations, neuteredReadme), [firstFunction]);
});

test("canonical Fallow skill follows the published Node API", () => {
  const declarations = readFileSync("crates/napi/types/index.d.ts", "utf8");
  const skill = readFileSync(".agents/skills/fallow/SKILL.md", "utf8");
  const section = markdownSection(skill, "Node.js Bindings");
  const missing = exportedNodeFunctions(declarations).filter(
    (functionName) => !section.includes(`\`${functionName}\``),
  );

  assert.deepEqual(missing, [], `canonical Fallow skill is missing: ${missing.join(", ")}`);
});

test("Fallow skills preserve exit status and avoid volatile plugin counts", () => {
  const skillEntrypoints = [".agents/skills/fallow/SKILL.md", "npm/fallow/skills/fallow/SKILL.md"];

  for (const path of skillEntrypoints) {
    const skill = readFileSync(path, "utf8");
    assert.doesNotMatch(
      skill,
      /always append `\|\| true`|append `\|\| true` to every/iu,
      `${path} must preserve the Fallow exit status`,
    );
    assert.match(
      skill,
      /do not force a successful status/iu,
      `${path} must reject blanket suppression`,
    );
    assert.doesNotMatch(skill, /2>\/dev\/null/u, `${path} must preserve stderr diagnostics`);
  }

  const skillPaths = [".agents/skills/fallow", "npm/fallow/skills/fallow"].flatMap(
    markdownFilesUnder,
  );
  for (const path of skillPaths) {
    const skill = readFileSync(path, "utf8");
    assert.doesNotMatch(
      skill,
      /\b\d+\s+(?:(?:auto-detecting|built-in)\s+)?(?:framework\s+)?plugins\b/iu,
      `${path} must resolve the current plugin count from fallow schema`,
    );
    assert.doesNotMatch(skill, /\|\| true/u, `${path} must preserve the Fallow exit status`);
    assert.doesNotMatch(skill, /2>\/dev\/null/u, `${path} must preserve stderr diagnostics`);
  }
});

test("Fallow skills preserve similar-code agent safety rules", () => {
  const skillEntrypoints = [".agents/skills/fallow/SKILL.md", "npm/fallow/skills/fallow/SKILL.md"];

  for (const path of skillEntrypoints) {
    const skill = readFileSync(path, "utf8");
    for (const required of [
      "discovery only",
      "Agents must not authorize setup",
      "Inspect a candidate before judging it",
      "candidate_worthy",
      "behaviorally_equivalent",
      "refactor_safe",
      "--candidates similar-code.json",
      "typed `snapshot`",
      "fails closed on stale source",
      "abstain when evidence is incomplete",
    ]) {
      assert.match(skill, new RegExp(required, "u"), `${path} is missing: ${required}`);
    }
  }

  const releasedSkill = readFileSync("npm/fallow/skills/fallow/SKILL.md", "utf8");
  for (const required of [
    "compare semantically similar functions",
    "find_similar_code",
    "inspect_similar_code",
    "paths:",
    "fallow similar-code review",
    'completion.status: "complete"',
    "needs-human-review",
  ]) {
    assert.match(
      releasedSkill,
      new RegExp(required, "u"),
      `released skill is missing: ${required}`,
    );
  }

  const mcpGuides = [
    ".agents/skills/fallow/SKILL.md",
    ".agents/rules/mcp-server.md",
    "npm/fallow/skills/fallow/SKILL.md",
  ];
  for (const path of mcpGuides) {
    const line = readFileSync(path, "utf8")
      .split("\n")
      .find(
        (candidate) =>
          candidate.includes("find_similar_code") &&
          (candidate.includes("Scope") ||
            candidate.includes("Supports") ||
            candidate.includes("paths:")),
      );
    assert.ok(line, `${path} must document find_similar_code`);
    assert.match(line, /(?:`paths`|paths:)/u, `${path} must use the live MCP paths parameter`);
    assert.doesNotMatch(
      line,
      /find_similar_code[^.\n]*(?:`files`|files:)/u,
      `${path} must not advertise the unknown MCP files parameter`,
    );
  }
});

test("released Node reference follows the published declarations", () => {
  const declarations = readFileSync("crates/napi/types/index.d.ts", "utf8");
  const reference = readFileSync("npm/fallow/skills/fallow/references/node-bindings.md", "utf8");
  const missing = exportedNodeFunctions(declarations).filter(
    (functionName) => !reference.includes(`\`${functionName}\``),
  );

  assert.deepEqual(missing, [], `released Node reference is missing: ${missing.join(", ")}`);
  assert.match(reference, /precisely typed `SimilarCodeReport`/u);
});

test("NAPI similar-code declarations and compile fixture stay precise", () => {
  const declarations = readFileSync("crates/napi/types/index.d.ts", "utf8");
  const fixture = readFileSync("crates/napi/tests/types/similar-code.compile.ts", "utf8");
  const similarCodeSection = declarations.slice(
    declarations.indexOf("export type SimilarCodeProvider"),
    declarations.indexOf("export interface HealthFinding"),
  );

  assert.doesNotMatch(similarCodeSection, /Record<string, unknown>/u);
  assert.match(similarCodeSection, /embedding_semantics_version: number/u);
  assert.match(similarCodeSection, /completion: SimilarCodeCompletion/u);
  assert.match(fixture, /generation\.embedding_semantics_version/u);
});

test("similar-code sidecar docs follow the canonical protocol versions", () => {
  const manifest = readJson("crates/api/similar-code-protocol.json");
  const readme = readFileSync("tools/similar-code-sidecar/README.md", "utf8");

  assert.match(readme, new RegExp(`Wire protocol v${manifest.wire_protocol_version}\\b`, "u"));
  assert.match(
    readme,
    new RegExp(`embedding\\s+semantics v${manifest.embedding_semantics_version}\\b`, "u"),
  );
  assert.match(readme, new RegExp(`"protocol_version": ${manifest.wire_protocol_version}\\b`, "u"));
  assert.match(
    readme,
    new RegExp(`"embedding_semantics_version": ${manifest.embedding_semantics_version}\\b`, "u"),
  );
});

test("NAPI declarations have one canonical public source", () => {
  const packageJson = readJson("crates/napi/package.json");
  const entry = readFileSync("crates/napi/index.d.ts", "utf8");
  const declarations = readFileSync("crates/napi/types/index.d.ts", "utf8");

  assert.equal(packageJson.types, "index.d.ts");
  assert.ok(packageJson.files.includes("types"));
  assert.equal(entry, 'export * from "./types/index";\n');
  assert.match(declarations, /export interface AnalysisOptions/u);
  assert.match(declarations, /export function detectDeadCode/u);
});

test("published Node packages and Action smoke tests use Node 22", () => {
  const packagePaths = ["npm/fallow/package.json", "crates/napi/package.json"];

  for (const path of packagePaths) {
    assert.equal(readJson(path).engines.node, ">=22", path);
  }

  const napiLock = readJson("crates/napi/package-lock.json");
  assert.equal(napiLock.packages[""].engines.node, ">=22", "NAPI lock root metadata");

  const actionWorkflow = readFileSync(".github/workflows/test-action.yml", "utf8");
  const versions = [...actionWorkflow.matchAll(/^\s+node-version:\s*['"]?(\d+)['"]?\s*$/gm)].map(
    (match) => match[1],
  );
  assert.ok(versions.length > 0, "Action workflow must select a Node runtime");
  assert.deepEqual([...new Set(versions)], ["22"]);
});

test("root repository tooling declares its exact Node floor", () => {
  const rootPackage = readJson("package.json");
  const rootLock = readJson("package-lock.json");
  const contributing = readFileSync("CONTRIBUTING.md", "utf8");

  assert.equal(rootPackage.engines.node, ">=22.12.0");
  assert.equal(rootLock.packages[""].engines.node, ">=22.12.0");
  assert.match(contributing, /Repository tooling requires Node\.js 22\.12\.0 or later\./);
});

test("type-aware manifest keeps every runtime and package surface in parity", () => {
  const cargo = readFileSync("Cargo.toml", "utf8");
  const workspaceVersion = cargo.match(/^\[workspace\.package\]\nversion = "([^"]+)"/mu)?.[1];
  assert.ok(workspaceVersion, "Cargo.toml must declare workspace.package.version");

  const manifest = readJson("crates/api/type-aware-protocol.json");
  const sidecarPackage = readJson("tools/type-aware-sidecar/package.json");
  const sidecarLock = readJson("tools/type-aware-sidecar/package-lock.json");
  const generated = readFileSync("tools/type-aware-sidecar/src/generated-protocol.mjs", "utf8");
  const vscodePackager = readFileSync("editors/vscode/scripts/package-type-aware.mjs", "utf8");
  const vscodePackageVerifier = readFileSync(
    "editors/vscode/scripts/verify-packaged-type-aware.mjs",
    "utf8",
  );
  const vscodeTargets = readFileSync("editors/vscode/scripts/vsix-targets.mjs", "utf8");
  const vscodeWorkspace = readFileSync("editors/vscode/pnpm-workspace.yaml", "utf8");
  const vscodePackage = readJson("editors/vscode/package.json");
  const windowsCandidateSmoke = readFileSync(
    "scripts/type-aware-windows-candidate-smoke.mjs",
    "utf8",
  );
  const bundledBackends = [
    "@typescript/typescript-darwin-arm64",
    "@typescript/typescript-darwin-x64",
    "@typescript/typescript-linux-arm64",
    "@typescript/typescript-linux-x64",
    "@typescript/typescript-win32-arm64",
    "@typescript/typescript-win32-x64",
  ];

  assert.equal(manifest.schema_version, 1);
  assert.equal(manifest.wire_protocol_version, 7);
  assert.equal(manifest.semantic_schema_version, 3);
  assert.equal(manifest.analysis_operation, "semantic-queries");
  assert.equal(manifest.status_operation, "status");
  assert.deepEqual(manifest.query_operations, [
    "symbol-use",
    "symbol-trace",
    "api-surface",
    "symbol-impact",
    "type-coupling",
  ]);
  assert.deepEqual(manifest.session_envelope_types, ["analyze", "shutdown"]);
  assert.equal(manifest.sidecar.package, sidecarPackage.name);
  assert.equal(manifest.sidecar.version_source, "workspace-package");
  assert.equal(sidecarPackage.version, workspaceVersion);
  assert.equal(sidecarLock.version, workspaceVersion);
  assert.equal(sidecarLock.packages[""].version, workspaceVersion);
  assert.equal(sidecarPackage.dependencies.typescript, manifest.backend.version);
  assert.equal(vscodePackage.devDependencies.typescript, manifest.backend.version);
  assert.match(vscodePackage.scripts.prepackage, /scripts\/copy-license\.mjs/u);
  assert.match(vscodePackage.scripts["verify:vsix"], /verify-packaged-type-aware\.mjs/u);
  assert.equal(sidecarLock.packages[""].dependencies.typescript, manifest.backend.version);
  assert.equal(sidecarLock.packages["node_modules/typescript"].version, manifest.backend.version);
  assert.deepEqual(sidecarPackage.bin, {
    "fallow-type-aware": "fallow-type-aware.mjs",
  });
  assert.deepEqual(sidecarLock.packages[""].bin, sidecarPackage.bin);
  assert.ok(sidecarPackage.files.includes("fallow-type-aware.mjs"));
  assert.ok(sidecarPackage.files.includes("src"));
  assert.match(generated, /Generated from crates\/api\/type-aware-protocol\.json/u);
  assert.match(vscodePackager, /cpSync\(join\(sourceRoot, "src"\)/u);
  for (const packageName of bundledBackends) {
    assert.match(vscodeTargets, new RegExp(packageName.replace("/", "\\/"), "u"));
  }
  assert.match(vscodePackageVerifier, /getVsixVariant/u);
  assert.match(vscodeTargets, /tsc(?:\\\\\.exe)?/u);
  assert.match(vscodeTargets, /maxBytes/u);
  assert.match(vscodeWorkspace, /supportedArchitectures:/u);
  assert.match(vscodeWorkspace, /- win32/u);
  assert.match(vscodeWorkspace, /- darwin/u);
  assert.match(vscodeWorkspace, /- linux/u);
  assert.match(windowsCandidateSmoke, /platformPackage\.replace\("@fallow-cli\/", ""\)/u);
  assert.match(windowsCandidateSmoke, /join\(temporaryRoot, platformPack\)/u);
});

test("VS Code public release verification stays exact and credential-free", () => {
  const vscodePackage = readJson("editors/vscode/package.json");
  const verifier = readFileSync("scripts/vscode-public-verify.mjs", "utf8");
  const verifierTests = readFileSync("scripts/vscode-public-verify.test.mjs", "utf8");
  const releaseSecurity = readFileSync("docs/development/release-security.md", "utf8");
  const releaseProcedure = readFileSync("docs/development/release-procedure.md", "utf8");
  assert.equal(vscodePackage.devDependencies["@vscode/vsce"], "3.9.2");
  assert.equal(vscodePackage.devDependencies.ovsx, "1.1.1");
  assert.match(verifier, /from "\.\.\/editors\/vscode\/scripts\/vsix-targets\.mjs"/u);
  assert.match(verifier, /VSIX_VARIANTS\.map\(\(\{ target \}\) => target\)/u);
  assert.match(verifier, /marketplace\.visualstudio\.com\/.*extensionquery/u);
  assert.match(verifier, /open-vsx\.org\/api/u);
  assert.match(verifier, /\$\{target\}\/\$\{version\}/u);
  assert.match(verifier, /payload.*fileCount.*sha256/su);
  assert.match(verifier, /DEFAULT_METADATA_ATTEMPTS = 120/u);
  assert.match(verifier, /DEFAULT_PAYLOAD_ATTEMPTS = 24/u);
  assert.doesNotMatch(verifier, /VSCE_PAT|OVSX_PAT|secrets\./u);
  assert.match(verifierTests, /missing[\s\S]*duplicate[\s\S]*stale/u);
  assert.match(verifierTests, /universal fallback/u);
  assert.match(verifierTests, /changed content/u);
  assert.match(releaseSecurity, /vscode-public-verify/u);
  assert.match(releaseProcedure, /Verify public VS Code registry\s+targets/u);
  assert.match(releaseSecurity, /release-verified.*similar-code-conformance/u);
  assert.match(releaseProcedure, /similar-code-conformance.*release-verified/su);
});

test("type-aware public surfaces expose only the stable protocol", () => {
  const protocol = readFileSync("tools/type-aware-sidecar/src/protocol.mjs", "utf8");
  const cli = readFileSync("tools/type-aware-sidecar/src/cli.mjs", "utf8");
  const guide = readFileSync("docs/type-aware-analysis.md", "utf8");
  const readme = readFileSync("tools/type-aware-sidecar/README.md", "utf8");

  assert.doesNotMatch(protocol, /class-member-uses|operation === "batch"/u);
  assert.doesNotMatch(cli, /class-member-uses|operation === "batch"/u);
  assert.match(guide, /first stable semantic wire contract is version 6/u);
  assert.match(readme, /Protocol\nv6 accepts/u);

  for (const surface of [guide, readme, readFileSync("README.md", "utf8")]) {
    assert.doesNotMatch(surface, /proof[- ]of[- ]concept|\bpoc\b/iu);
  }
});

test("CONTRIBUTING uses the root contract generation commands", () => {
  const contributing = readFileSync("CONTRIBUTING.md", "utf8");

  assert.match(contributing, /^npm run generate:contracts$/m);
  assert.match(contributing, /^npm run generate:contracts:check$/m);
});

test("plugin authoring guide documents every top-level schema field", () => {
  const guide = readFileSync("docs/plugin-authoring.md", "utf8");
  const schema = readJson("plugin-schema.json");
  const documented = [
    ...documentedFields(guide, "Required"),
    ...documentedFields(guide, "Optional"),
  ].toSorted();
  const schemaFields = Object.keys(schema.properties).toSorted();

  assert.deepEqual(documented, schemaFields);
});

test("FALLOW_FORMAT docs include every GitHub-native format", () => {
  const formatSource = readFileSync("crates/cli/src/cli_format.rs", "utf8");
  const githubFormats = [...formatSource.matchAll(/#\[value\(name = "(github-[^"]+)"\)\]/g)].map(
    (match) => match[1],
  );
  assert.ok(githubFormats.length > 0, "Rust format catalog must include GitHub-native formats");

  const docs = readFileSync("docs/environment-variables.md", "utf8");
  const row = docs.match(/^\| `FALLOW_FORMAT` \| ([^|]+)\|/m);
  assert.ok(row, "environment variable docs must contain a FALLOW_FORMAT row");
  const documentedFormats = new Set([...row[1].matchAll(/`([^`]+)`/g)].map((match) => match[1]));

  for (const format of githubFormats) {
    assert.ok(documentedFormats.has(format), `FALLOW_FORMAT docs are missing ${format}`);
  }
});

test("narrator comment guard runs for commits, Claude, and CI", () => {
  const preCommit = readFileSync(".githooks/pre-commit", "utf8");
  const claudeSettings = readFileSync(".claude/settings.json", "utf8");
  const ci = readFileSync(".github/workflows/ci.yml", "utf8");

  assert.match(preCommit, /check-comment-quality\.mjs --staged/u);
  assert.match(claudeSettings, /check-comment-quality\.mjs.*--working-tree.*--claude-hook/u);
  assert.match(ci, /node scripts\/check-comment-quality\.mjs --all/u);
});
