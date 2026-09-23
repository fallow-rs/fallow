#!/usr/bin/env node
// The `lint:js` script in package.json holds the one list of JavaScript scopes.
// The format scripts must repeat it, because an npm script cannot read a file
// on every platform. The pre-commit hook reads the list through this helper, so
// it never holds a copy of its own.

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const SOURCE_SCRIPT = "lint:js";
const MIRROR_SCRIPTS = ["fmt:js", "fmt:js:check"];
const JS_EXTENSIONS = /\.(?:js|ts|mjs|cjs|jsx|tsx)$/u;

const scriptScopes = (packageJson, name) => {
  const script = packageJson.scripts?.[name];
  if (typeof script !== "string") {
    throw new Error(`package.json has no ${name} script`);
  }
  const segments = script.split("&&");
  return segments[segments.length - 1]
    .trim()
    .split(/\s+/u)
    .slice(1)
    .filter((token) => !token.startsWith("-"));
};

/**
 * Return the scope list of the `lint:js` script.
 * @param {{ scripts?: Record<string, string> }} packageJson
 * @returns {string[]}
 */
export const lintScopes = (packageJson) => scriptScopes(packageJson, SOURCE_SCRIPT);

/**
 * Throw when `fmt:js` or `fmt:js:check` holds a scope list that is not the
 * `lint:js` list. The message names each scope that only one side holds.
 * @param {{ scripts?: Record<string, string> }} packageJson
 */
export const assertScopesAgree = (packageJson) => {
  const source = lintScopes(packageJson);
  const problems = MIRROR_SCRIPTS.flatMap((name) => {
    const mirror = scriptScopes(packageJson, name);
    const extra = mirror.filter((scope) => !source.includes(scope));
    const missing = source.filter((scope) => !mirror.includes(scope));
    return [
      ...extra.map((scope) => `${name} holds ${scope}, but ${SOURCE_SCRIPT} does not`),
      ...missing.map((scope) => `${name} does not hold ${scope}, but ${SOURCE_SCRIPT} does`),
    ];
  });
  if (problems.length > 0) {
    throw new Error(`The JavaScript scope lists differ:\n${problems.join("\n")}`);
  }
};

const inScope = (path, scope) =>
  JS_EXTENSIONS.test(scope) ? path === scope : path.startsWith(`${scope}/`);

/**
 * Return the staged paths that are JavaScript or TypeScript files inside a scope.
 * @param {string[]} paths
 * @param {string[]} scopes
 * @returns {string[]}
 */
export const stagedLintTargets = (paths, scopes) =>
  paths.filter((path) => JS_EXTENSIONS.test(path) && scopes.some((scope) => inScope(path, scope)));

/**
 * With `--filter-staged`, read paths from standard input and print the lint
 * targets. With no argument, check that the scope lists agree.
 * @param {string[]} args
 * @returns {number}
 */
export const main = (args = process.argv.slice(2)) => {
  const packageJson = JSON.parse(readFileSync(join(REPO_ROOT, "package.json"), "utf8"));
  try {
    assertScopesAgree(packageJson);
  } catch (error) {
    process.stderr.write(`${error.message}\n`);
    return 1;
  }
  if (args[0] !== "--filter-staged") {
    return 0;
  }
  const paths = readFileSync(0, "utf8").split(/\r?\n/u).filter(Boolean);
  const targets = stagedLintTargets(paths, lintScopes(packageJson));
  process.stdout.write(targets.map((path) => `${path}\n`).join(""));
  return 0;
};

if (process.argv[1] !== undefined && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  process.exitCode = main();
}
