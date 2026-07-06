#!/usr/bin/env node

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { contractSurfaces } from "./contract-surfaces.mjs";

const stripQuotes = (value) => value.replace(/^['"]|['"]$/g, "");

const indentation = (line) => line.length - line.trimStart().length;

const siblingFilterStarts = (line, filterIndent) =>
  indentation(line) <= filterIndent && /^[A-Za-z0-9_-]+:$/.test(line.trim());

const pathFilterPattern = (line) => {
  const match = line.trim().match(/^-\s+(.+)$/);
  return match ? stripQuotes(match[1].trim()) : null;
};

const shouldSkipPathFilterLine = (line) => {
  const trimmed = line.trim();
  return !trimmed || trimmed.startsWith("#");
};

const wildcardPrefixCovers = (pattern, target, suffixLength) => {
  const prefix = pattern.slice(0, suffixLength);
  return target === prefix || target.startsWith(`${prefix}/`);
};

export const pathPatternCovers = (pattern, target) => {
  if (pattern === target) {
    return true;
  }

  if (pattern.endsWith("/**")) {
    return wildcardPrefixCovers(pattern, target, -3);
  }

  if (pattern.endsWith("/**/*")) {
    return wildcardPrefixCovers(pattern, target, -5);
  }

  return false;
};

export const extractPathFilterPatterns = (workflow, filterName) => {
  const lines = workflow.split(/\r?\n/);
  const filterIndex = lines.findIndex((line) => line.trim() === `${filterName}:`);

  if (filterIndex === -1) {
    throw new Error(`Could not find paths-filter entry '${filterName}'`);
  }

  const filterIndent = indentation(lines[filterIndex]);
  const patterns = [];
  const body = lines.slice(filterIndex + 1);

  for (const line of body) {
    if (shouldSkipPathFilterLine(line)) {
      continue;
    }

    if (siblingFilterStarts(line, filterIndent)) {
      break;
    }

    const pattern = pathFilterPattern(line);
    if (pattern) {
      patterns.push(pattern);
    }
  }

  return patterns;
};

export const checkGithubActionsPathFilter = (workflow, surfaces, { filterName = "rust" } = {}) => {
  const patterns = extractPathFilterPatterns(workflow, filterName);
  const missing = [];

  for (const surface of surfaces) {
    for (const generatedPath of surface.generatedPaths) {
      const covered = patterns.some((pattern) => pathPatternCovers(pattern, generatedPath));
      if (!covered) {
        missing.push({ path: generatedPath, surfaceId: surface.id });
      }
    }
  }

  return { filterName, missing, patterns };
};

export const formatMissingSurfaces = (result) =>
  result.missing.map(({ path, surfaceId }) => `  - ${path} (${surfaceId})`).join("\n");

export const checkGithubActionsFile = (path, surfaces = contractSurfaces, options = {}) => {
  const workflow = readFileSync(path, "utf8");
  return checkGithubActionsPathFilter(workflow, surfaces, options);
};

const usage = () => {
  console.error(
    "Usage: node scripts/check-contract-surfaces.mjs --github-actions .github/workflows/ci.yml",
  );
};

const main = () => {
  const githubActionsIndex = process.argv.indexOf("--github-actions");
  if (githubActionsIndex === -1 || !process.argv[githubActionsIndex + 1]) {
    usage();
    process.exitCode = 2;
    return;
  }

  const result = checkGithubActionsFile(process.argv[githubActionsIndex + 1], contractSurfaces, {
    filterName: "rust",
  });

  if (result.missing.length > 0) {
    console.error(
      `Generated contract surfaces are missing from the '${result.filterName}' path filter:\n${formatMissingSurfaces(result)}`,
    );
    process.exitCode = 1;
    return;
  }

  console.log("contract surfaces: CI path filters cover generated artifacts");
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  main();
}
