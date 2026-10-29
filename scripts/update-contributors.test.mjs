import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import { copyFile, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { fileURLToPath, pathToFileURL } from "node:url";
import { collectContributors, generateContributors } from "./update-contributors.mjs";

const user = (login, id, type = "User") => ({
  login,
  id,
  type,
  avatar_url: `https://avatars.githubusercontent.com/u/${id}?v=4`,
  html_url: `https://github.com/${login}`,
});

test("importing from standard input exposes the generator without starting its CLI", () => {
  const result = spawnSync(process.execPath, ["--input-type=module", "-"], {
    input: `import { generateContributors } from ${JSON.stringify(new URL("./update-contributors.mjs", import.meta.url).href)}; console.log(typeof generateContributors);`,
    encoding: "utf8",
  });
  assert.equal(result.status, 0, result.stderr);
  assert.equal(result.stdout.trim(), "function");
});

test("the CLI generates credits when its path contains spaces and percent signs", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "fallow contributors % "));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const script = join(directory, "update-contributors.mjs");
  const preload = join(directory, "network-fixture.mjs");
  const outputDirectory = join(directory, "output");
  await copyFile(fileURLToPath(new URL("./update-contributors.mjs", import.meta.url)), script);
  await writeFile(
    preload,
    `
    globalThis.fetch = async (input) => {
      const url = new URL(input);
      if (url.origin === "https://avatars.githubusercontent.com") {
        return new Response("avatar", { headers: { "content-type": "image/png" } });
      }
      if (url.pathname.endsWith("/contributors")) return Response.json(${JSON.stringify([user("coder", 1)])});
      if (url.pathname.endsWith("/issues")) return Response.json(${JSON.stringify([{ user: user("reporter", 2), state: "closed" }])});
      throw new Error("Unexpected network request");
    };
  `,
  );
  const result = spawnSync(
    process.execPath,
    ["--import", pathToFileURL(preload).href, script, outputDirectory],
    {
      encoding: "utf8",
      env: { ...process.env, GITHUB_REPOSITORY: "owner/repo", GH_TOKEN: "", GITHUB_TOKEN: "" },
    },
  );
  assert.equal(result.status, 0, result.stderr);
  assert.equal(
    existsSync(join(outputDirectory, "CONTRIBUTORS.md")),
    true,
    "the CLI must begin generation",
  );
  const markdown = await readFile(join(outputDirectory, "CONTRIBUTORS.md"), "utf8");
  assert.match(markdown, /\[coder\]\(https:\/\/github\.com\/coder\)/);
  assert.match(markdown, /\[reporter\]\(https:\/\/github\.com\/reporter\)/);
  assert.match(
    await readFile(join(outputDirectory, "contributors.svg"), "utf8"),
    /coder, reporter/,
  );
});

test("credits code contributors and issue/PR authors once, excluding bots and deleted users", () => {
  const people = collectContributors(
    [user("Zoe", 1), user("alice", 2), user("dependabot[bot]", 3, "Bot")],
    [
      { user: user("ALICE", 2), state: "closed" },
      { user: user("bob", 4), state: "open" },
      {
        user: user("carol", 5),
        state: "closed",
        pull_request: { url: "https://api.github.com/repos/o/r/pulls/1" },
      },
      { user: null },
      { user: user("ghost", 6) },
      { user: user("automation[bot]", 7) },
    ],
  );
  assert.deepEqual(
    people.map(({ login }) => login.toLowerCase()),
    ["alice", "bob", "carol", "zoe"],
  );
});

test("generates complete, deterministic files from every API page without exposing the API token to avatars", async (t) => {
  const directory = await mkdtemp(join(tmpdir(), "fallow-contributors-"));
  t.after(() => rm(directory, { recursive: true, force: true }));
  const outputDirectory = join(directory, "output");
  const calls = [];
  const fetchImpl = async (value, options) => {
    const url = new URL(value);
    calls.push(url.href);
    assert.equal(options.redirect, "error");
    assert.ok(options.signal instanceof AbortSignal);
    if (url.origin === "https://avatars.githubusercontent.com") {
      assert.equal(options.headers, undefined, "avatar requests must not receive credentials");
      assert.equal(url.searchParams.get("s"), "64");
      return new Response(Buffer.from("avatar"), { headers: { "content-type": "image/png" } });
    }
    assert.equal(url.origin, "https://api.github.com");
    assert.equal(options.headers.Authorization, "Bearer private-token");
    assert.equal(url.searchParams.get("per_page"), "100");
    const page = Number(url.searchParams.get("page"));
    const contributors = url.pathname.endsWith("/contributors");
    const rows = contributors
      ? page === 1
        ? Array.from({ length: 100 }, () => user("zoe", 1))
        : [user("alice", 2)]
      : url.searchParams.get("state") !== "all"
        ? []
        : page === 1
          ? Array.from({ length: 100 }, () => ({ user: user("bob", 3), state: "closed" }))
          : [
              {
                user: user("carol", 4),
                state: "closed",
                pull_request: { url: "https://api.github.com/repos/o/r/pulls/1" },
              },
            ];
    return Response.json(rows);
  };
  await generateContributors({
    repository: "owner/repo",
    token: "private-token",
    outputDirectory,
    fetchImpl,
  });
  assert.equal(
    existsSync(join(outputDirectory, "contributors.svg")),
    true,
    "generation must write the gallery",
  );
  const svg = await readFile(join(outputDirectory, "contributors.svg"), "utf8");
  const markdown = await readFile(join(outputDirectory, "CONTRIBUTORS.md"), "utf8");
  assert.match(svg, /data:image\/png;base64,YXZhdGFy/);
  assert.match(svg, /alice, bob, carol, zoe/);
  assert.doesNotMatch(svg, /private-token|https:\/\/avatars/);
  assert.match(markdown, /\[alice\]\(https:\/\/github\.com\/alice\)/);
  assert.match(markdown, /\[bob\]\(https:\/\/github\.com\/bob\)/);
  assert.match(markdown, /\[carol\]\(https:\/\/github\.com\/carol\)/);
  assert.ok(markdown.indexOf("[alice]") < markdown.indexOf("[bob]"));
  assert.ok(calls.some((url) => url.includes("/contributors?") && url.endsWith("page=2")));
  assert.ok(calls.some((url) => url.includes("/issues?") && url.endsWith("page=2")));
  await generateContributors({
    repository: "owner/repo",
    token: "private-token",
    outputDirectory,
    fetchImpl,
  });
  assert.equal(await readFile(join(outputDirectory, "contributors.svg"), "utf8"), svg);
  assert.equal(await readFile(join(outputDirectory, "CONTRIBUTORS.md"), "utf8"), markdown);
});

test("rejects untrusted identities and avatar URLs before rendering markup", () => {
  const invalid = [
    { ...user("alice", 1), login: 'alice"><script>' },
    { ...user("alice", 1), id: -1 },
    { ...user("alice", 1), avatar_url: "https://attacker.example/avatar.png" },
    { ...user("alice", 1), avatar_url: "https://secret@avatars.githubusercontent.com/u/1" },
    { ...user("alice", 1), avatar_url: "http://avatars.githubusercontent.com/u/1" },
  ];
  for (const person of invalid) assert.throws(() => collectContributors([person], []));
  assert.equal(collectContributors([user("safe-name", 1)], [])[0].login, "safe-name");
  assert.deepEqual(
    collectContributors([user("alice", 1)], [{ user: user("ALICE", 2) }]).map(({ id }) => id),
    [1],
  );
  assert.deepEqual(
    collectContributors([user("old-name", 1)], [{ user: user("new-name", 1) }]).map(
      ({ login }) => login,
    ),
    ["new-name"],
  );
});

test("failed API or avatar requests preserve the previous complete outputs", async (t) => {
  const outputDirectory = await mkdtemp(join(tmpdir(), "fallow-contributors-fail-"));
  t.after(() => rm(outputDirectory, { recursive: true, force: true }));
  await writeFile(join(outputDirectory, "contributors.svg"), "previous gallery");
  await writeFile(join(outputDirectory, "CONTRIBUTORS.md"), "previous names");
  for (const failure of ["api", "shape", "empty", "avatar", "type"]) {
    const fetchImpl = async (value) => {
      const url = new URL(value);
      if (url.origin === "https://avatars.githubusercontent.com") {
        if (failure === "avatar") return new Response("unavailable", { status: 503 });
        return new Response("<svg/>", { headers: { "content-type": "image/svg+xml" } });
      }
      if (failure === "api") return new Response("unauthorized", { status: 401 });
      if (failure === "shape") return Response.json({ message: "wrong shape" });
      if (url.pathname.endsWith("/issues") || failure === "empty") return Response.json([]);
      return Response.json([user("alice", 1)]);
    };
    await assert.rejects(
      generateContributors({ repository: "owner/repo", outputDirectory, fetchImpl }),
    );
    assert.equal(
      await readFile(join(outputDirectory, "contributors.svg"), "utf8"),
      "previous gallery",
    );
    assert.equal(
      await readFile(join(outputDirectory, "CONTRIBUTORS.md"), "utf8"),
      "previous names",
    );
  }
  await assert.rejects(
    generateContributors({
      repository: "https://attacker.example",
      outputDirectory,
      fetchImpl: () => assert.fail("invalid repository must not make a request"),
    }),
    /owner\/repository/,
  );
});
