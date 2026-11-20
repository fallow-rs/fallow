import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

type Scripts = Readonly<Record<string, string>>;

const scripts = (
  JSON.parse(readFileSync(resolve(__dirname, "../package.json"), "utf8")) as {
    readonly scripts: Scripts;
  }
).scripts;

const SCRIPT_CALL = /^pnpm run ([\w:-]+)$/;
const PACKAGE_MANAGER = /\b(?:pnpm|npm|npx|yarn|corepack)\b/;

/**
 * The scripts that one `pnpm run <name>` call starts, with its `pre` hook.
 * A package manager call in any other shape throws, so the walk cannot miss
 * a script that a body starts.
 */
const reachableScripts = (all: Scripts, name: string, seen = new Set<string>()): Set<string> => {
  for (const hook of [`pre${name}`, name]) {
    const body = all[hook];
    if (body === undefined || seen.has(hook)) {
      continue;
    }
    seen.add(hook);
    for (const command of body.split("&&").map((part) => part.trim())) {
      const call = SCRIPT_CALL.exec(command);
      if (call?.[1] !== undefined && call[1] in all) {
        reachableScripts(all, call[1], seen);
      } else if (PACKAGE_MANAGER.test(command)) {
        throw new Error(`"${hook}" starts "${command}"; use "pnpm run <script>"`);
      }
    }
  }
  return seen;
};

const script = (name: string): string => {
  const body = scripts[name];
  if (body === undefined) {
    throw new Error(`package.json has no "${name}" script`);
  }
  return body;
};

describe("package scripts", () => {
  it("runs the contract codegen in the pre hook of build", () => {
    expect(script("prebuild")).toBe("pnpm run codegen:contracts");
    expect(script("build")).toBe("pnpm run bundle");
  });

  it("runs the integration tests without the contract codegen", () => {
    const reached = reachableScripts(scripts, "test:integration:no-codegen");
    expect(reached).toContain("bundle");
    expect(reached).not.toContain("codegen:contracts");
    expect(reached).not.toContain("build");
    for (const name of reached) {
      expect(script(name)).not.toMatch(/\bcargo\b|codegen-contracts/);
    }
  });

  it("runs the same integration suite with and without the codegen", () => {
    const tail = (name: string): string => script(name).replace(/^pnpm run [\w:-]+ && /, "");
    expect(tail("test:integration:no-codegen")).toBe(tail("test:integration"));
  });

  it("rejects a script call that the walk cannot follow", () => {
    for (const call of ["npm run build", "pnpm --silent run build", "npx rolldown -c"]) {
      expect(() => reachableScripts({ "test:x": `${call} && node run.js` }, "test:x")).toThrow(
        call,
      );
    }
  });
});
