import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { chmod, mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

const PNG = Buffer.from(
  "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR4nGNk+M/wHwAEBgIAe2BNVAAAAABJRU5ErkJggg==",
  "base64",
);

// The double replaces the network API. The actual workflow shell builds every request.
const GH_DOUBLE = `#!/usr/bin/env node
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
const state = JSON.parse(readFileSync(process.env.PUBLISH_STATE, "utf8"));
const args = process.argv.slice(2);
const sha = (base64) => {
  const bytes = Buffer.from(base64, "base64");
  return createHash("sha1").update(Buffer.from("blob " + bytes.length + "\\0")).update(bytes).digest("hex");
};
if (args[1] === "repos/fallow-rs/fallow/git/trees/badges") {
  console.log(JSON.stringify({ tree: Object.entries(state.files).map(([path, content]) => ({ path, sha: sha(content) })) }));
} else {
  assert.deepEqual(args.slice(0, 3), ["api", "--method", "PUT"]);
  const file = args[3].split("/").at(-1);
  assert.ok(["contributors.png", "CONTRIBUTORS.md"].includes(file));
  const body = JSON.parse(readFileSync(args[args.indexOf("--input") + 1], "utf8"));
  assert.equal(body.branch, "badges");
  assert.equal(body.sha, state.files[file] === undefined ? undefined : sha(state.files[file]));
  state.files[file] = Buffer.from(body.content, "base64").toString("base64");
  state.writes.push(file);
  writeFileSync(process.env.PUBLISH_STATE, JSON.stringify(state));
}
`;

test("the workflow publishes exact binary PNG bytes and preserves other badge files without duplicate updates", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "fallow binary publisher "));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const bin = join(directory, "bin");
  await mkdir(bin);
  await mkdir(join(directory, "contributor-credits"));
  await writeFile(join(bin, "gh"), GH_DOUBLE);
  await chmod(join(bin, "gh"), 0o755);
  const names = "# Contributors\n\n- [alice](https://github.com/alice)\n";
  await writeFile(join(directory, "contributor-credits", "contributors.png"), PNG);
  await writeFile(join(directory, "contributor-credits", "CONTRIBUTORS.md"), names);
  const initial = {
    "coverage.json": Buffer.from("coverage data").toString("base64"),
    "contributors.svg": Buffer.from("historical SVG").toString("base64"),
  };
  const statePath = join(directory, "state.json");
  await writeFile(statePath, JSON.stringify({ files: initial, writes: [] }));
  const workflow = await readFile(
    new URL("../.github/workflows/contributors.yml", import.meta.url),
    "utf8",
  );
  const publisher = workflow.split("      - name: Update generated files on badges\n")[1];
  assert.ok(publisher, "the contributor publication step must exist");
  const shell = publisher
    .split("        run: |\n")[1]
    .split("\n")
    .map((line) => line.slice(10))
    .join("\n");
  const run = () =>
    spawnSync("bash", ["-e", "-u", "-o", "pipefail", "-c", shell], {
      cwd: directory,
      encoding: "utf8",
      env: {
        ...process.env,
        PATH: `${bin}:${process.env.PATH}`,
        GH_REPO: "fallow-rs/fallow",
        GH_TOKEN: "test-token",
        PUBLISH_STATE: statePath,
      },
    });
  const first = run();
  assert.equal(first.status, 0, first.stderr);
  const firstState = JSON.parse(await readFile(statePath, "utf8"));
  assert.deepEqual(
    Buffer.from(firstState.files["contributors.png"], "base64"),
    PNG,
    "the Contents API must receive the original PNG bytes",
  );
  assert.equal(Buffer.from(firstState.files["CONTRIBUTORS.md"], "base64").toString("utf8"), names);
  assert.equal(firstState.files["coverage.json"], initial["coverage.json"]);
  assert.equal(firstState.files["contributors.svg"], initial["contributors.svg"]);
  const second = run();
  assert.equal(second.status, 0, second.stderr);
  assert.deepEqual(
    JSON.parse(await readFile(statePath, "utf8")),
    firstState,
    "unchanged gallery files must not produce another commit",
  );
});
