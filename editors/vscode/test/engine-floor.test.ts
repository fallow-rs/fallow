/**
 * Hard guard for the VS Code extension's engine floor. Two failure modes
 * we want to catch BEFORE `vsce package` runs in CI:
 *
 * 1. `@types/vscode` drifting above `engines.vscode` (vsce hard-rejects:
 *    "@types/vscode <X> greater than engines.vscode <Y>"). This shipped a
 *    bad VSIX once already at the 1.115 -> 1.116 transition.
 *
 * 2. `engines.vscode` silently bumping above 1.96.0 again. Cursor and
 *    Windsurf are forks tracking older VS Code bases; a higher floor
 *    rejects install on those editors. The dependabot ignore on
 *    `@types/vscode` prevents the upstream-driven drift, but a manual
 *    bump would slip through. This sentinel forces the change to also
 *    touch this test, prompting a deliberate decision.
 *
 * 3. `mocha` crossing into 12.x. The integration suites run in-process
 *    inside the extension host, and the 1.96 host ships Node 20.18.1.
 *    Mocha 12 is ESM-only and its CommonJS implementation file requires
 *    ESM siblings, so it needs Node ^20.19 || >=22.12 and fails to load
 *    there with ERR_REQUIRE_ESM. Without this guard the mismatch only
 *    surfaces in the slow extension-host job.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const pkg = JSON.parse(
  readFileSync(resolve(__dirname, "../package.json"), "utf8"),
) as {
  engines: { vscode: string };
  devDependencies: Record<string, string>;
};

const parse = (version: string): [number, number, number] => {
  const cleaned = version.replace(/^[\^~>=]+/, "");
  const parts = cleaned.split(".").map((n) => Number.parseInt(n, 10) || 0);
  return [parts[0] ?? 0, parts[1] ?? 0, parts[2] ?? 0];
};

const compare = (
  a: [number, number, number],
  b: [number, number, number],
): number => {
  for (let i = 0; i < 3; i += 1) {
    const diff = (a[i] ?? 0) - (b[i] ?? 0);
    if (diff !== 0) return diff;
  }
  return 0;
};

describe("package.json engine floor invariant", () => {
  it("keeps @types/vscode <= engines.vscode so vsce can package", () => {
    const engineFloor = parse(pkg.engines.vscode);
    const typesPin = parse(pkg.devDependencies["@types/vscode"] ?? "0.0.0");
    expect(
      compare(typesPin, engineFloor),
      `engines.vscode=${pkg.engines.vscode} must be >= @types/vscode=${pkg.devDependencies["@types/vscode"]}; vsce rejects packaging otherwise`,
    ).toBeLessThanOrEqual(0);
  });

  it("holds engines.vscode at ^1.96.0 so Cursor/Windsurf forks can install", () => {
    expect(
      pkg.engines.vscode,
      "engines.vscode bumped above ^1.96.0; deliberate bump must also update this sentinel test",
    ).toBe("^1.96.0");
    expect(pkg.devDependencies["@types/vscode"]).toBe("1.96.0");
  });

  it("holds mocha on 11.x while the extension host runs Node 20.18.1", () => {
    const [major] = parse(pkg.devDependencies["mocha"] ?? "0.0.0");
    expect(
      major,
      "mocha 12 needs Node ^20.19 || >=22.12 for its internal require of ESM files; the VS Code 1.96 extension host ships Node 20.18.1, so the integration suites cannot load it in-process",
    ).toBe(11);
  });
});
