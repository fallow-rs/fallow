import assert from "node:assert/strict";
import { test } from "node:test";
import {
  NPM_PACKAGES,
  verifyOpenVsxIdentity,
  verifyOtherRegistryIdentities,
} from "./publisher-auth-check.mjs";

const env = {
  GITHUB_ACTIONS: "true",
  GITHUB_REF: "refs/heads/main",
  GITHUB_WORKFLOW_REF: "fallow-rs/fallow/.github/workflows/release.yml@refs/heads/main",
  ACTIONS_ID_TOKEN_REQUEST_URL:
    "https://vstoken.actions.githubusercontent.com/token?api-version=2.0",
  ACTIONS_ID_TOKEN_REQUEST_TOKEN: "synthetic-request",
};

test("Open VSX exchanges the fixed identity without unsupported production permission routes", async () => {
  const calls = [];
  const send = async (url, options) => {
    calls.push({ url: String(url), ...options });
    if (calls.length === 1) return Response.json({ value: "synthetic-identity" });
    if (String(url) === "https://open-vsx.org/api/-/trusted-publishing/token") {
      assert.equal(options.method, "POST");
      assert.deepEqual(JSON.parse(options.body), {
        namespace: "fallow-rs",
        extension: "fallow-vscode",
        token: "synthetic-identity",
      });
      return Response.json({ value: "synthetic-authority" }, { status: 201 });
    }
    return Response.json({ error: "Unsupported production route" }, { status: 404 });
  };
  assert.deepEqual(await verifyOpenVsxIdentity({ ...env, OVSX_PAT: "must-not-be-used" }, send), {
    ok: true,
  });
  assert.equal(calls.length, 2);
  assert.equal(new URL(calls[0].url).searchParams.get("audience"), "https://open-vsx.org");
  assert.equal(calls[0].headers.Authorization, "Bearer synthetic-request");
  for (const call of calls) assert.equal(call.redirect, "error");
});

test("Open VSX refuses identity exchange failures without token fallback or publication", async () => {
  for (const failedRequest of [0, 1]) {
    let calls = 0;
    const send = async () => {
      const index = calls++;
      if (index === failedRequest) return new Response("private provider details", { status: 403 });
      return Response.json({ value: "synthetic-token" }, { status: index === 1 ? 201 : 200 });
    };
    const result = await verifyOpenVsxIdentity({ ...env, OVSX_PAT: "must-not-be-used" }, send);
    assert.equal(result.ok, false);
    assert.equal(calls, failedRequest + 1);
    assert.doesNotMatch(result.error, /synthetic|private provider/u);
  }
  let calls = 0;
  const never = async () => {
    calls++;
    throw new Error("Unexpected network");
  };
  for (const override of [
    { GITHUB_REF: "refs/heads/other" },
    { GITHUB_WORKFLOW_REF: "other/workflow" },
    { ACTIONS_ID_TOKEN_REQUEST_URL: "https://attacker.example/token" },
    {
      ACTIONS_ID_TOKEN_REQUEST_URL:
        "https://vstoken.actions.githubusercontent.com.attacker.example/token",
    },
    { ACTIONS_ID_TOKEN_REQUEST_URL: "http://vstoken.actions.githubusercontent.com/token" },
    { ACTIONS_ID_TOKEN_REQUEST_TOKEN: "" },
  ])
    assert.equal((await verifyOpenVsxIdentity({ ...env, ...override }, never)).ok, false);
  assert.equal(calls, 0);
  assert.deepEqual(
    await verifyOpenVsxIdentity(env, async () =>
      Response.json({ value: "token", maxSize: 1024 }, { status: calls++ === 1 ? 201 : 200 }),
    ),
    { ok: true },
  );
});

test("Open VSX rejects incorrect exchange statuses and malformed authority", async () => {
  for (const [status, authority, expected] of [
    [200, { value: "synthetic-token" }, "Open VSX trusted exchange check failed (HTTP 200)"],
    [403, { error: "private provider token" }, "Open VSX trusted exchange check failed (HTTP 403)"],
    [201, { success: "Valid token" }, "Open VSX trusted exchange check failed"],
    [201, { value: "" }, "Open VSX trusted exchange check failed"],
    [201, { value: "token\r\n::error::private" }, "Open VSX trusted exchange check failed"],
    [
      201,
      { value: "synthetic-token", error: "private provider token" },
      "Open VSX trusted exchange check failed",
    ],
  ]) {
    let calls = 0;
    const send = async () =>
      ++calls === 1
        ? Response.json({ value: "synthetic-identity" })
        : Response.json(authority, { status });
    assert.deepEqual(await verifyOpenVsxIdentity(env, send), { ok: false, error: expected });
    assert.equal(calls, 2);
  }
});

test("Open VSX check exercises HTTP identity exchange on loopback", async (t) => {
  const { createServer } = await import("node:http");
  let requests = 0;
  const server = createServer(async (request, response) => {
    requests++;
    response.setHeader("Content-Type", "application/json");
    if (request.url.startsWith("/token")) {
      assert.equal(request.headers.authorization, "Bearer synthetic-request");
      response.end(JSON.stringify({ value: "synthetic-identity" }));
      return;
    }
    if (request.url === "/api/-/trusted-publishing/token") {
      assert.equal(request.method, "POST");
      let body = "";
      for await (const chunk of request) body += chunk;
      assert.deepEqual(JSON.parse(body), {
        namespace: "fallow-rs",
        extension: "fallow-vscode",
        token: "synthetic-identity",
      });
      response.statusCode = 201;
      response.end(JSON.stringify({ value: "synthetic-authority" }));
      return;
    }
    response.statusCode = 404;
    response.end(JSON.stringify({ error: "Unsupported production route" }));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(async () => {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  });
  const send = (url, options) => {
    const fixed = new URL(url);
    assert.ok(["vstoken.actions.githubusercontent.com", "open-vsx.org"].includes(fixed.hostname));
    return fetch(
      `http://127.0.0.1:${server.address().port}${fixed.pathname}${fixed.search}`,
      options,
    );
  };
  assert.deepEqual(await verifyOpenVsxIdentity(env, send), { ok: true });
  assert.equal(requests, 2);
});

test("auth-only CLI checks npm and crates without uploading bytes or printing authority", async (t) => {
  const { mkdtempSync, writeFileSync, readFileSync, rmSync } = await import("node:fs");
  const { tmpdir } = await import("node:os");
  const { join } = await import("node:path");
  const { spawnSync } = await import("node:child_process");
  const directory = mkdtempSync(join(tmpdir(), "fallow-registry-auth-"));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const trace = join(directory, "requests.jsonl");
  const hook = join(directory, "transport.mjs");
  writeFileSync(
    hook,
    `import {appendFileSync} from 'node:fs';
globalThis.fetch = async (url, options) => {
  const endpoint = new URL(url);
  appendFileSync(${JSON.stringify(trace)}, JSON.stringify({url:String(url),method:options.method})+'\\n');
  if (endpoint.hostname === 'vstoken.actions.githubusercontent.com') {
    const claims={aud:endpoint.searchParams.get('audience'),exp:Math.floor(Date.now()/1000)+600};
    return Response.json({value:'e30.'+Buffer.from(JSON.stringify(claims)).toString('base64url')+'.synthetic'});
  }
  if (endpoint.href === 'https://open-vsx.org/api/-/trusted-publishing/token' && options.method === 'POST') return process.env.TEST_REFUSE_OPENVSX === 'true' ? new Response('private provider authority', {status:403}) : Response.json({value:'synthetic-authority'},{status:201});
  if (endpoint.hostname === 'registry.npmjs.org' && options.method === 'POST' && endpoint.pathname.startsWith('/-/npm/v1/oidc/token/exchange/package/')) return process.env.TEST_REFUSE_NPM === 'true' ? new Response('private provider authority',{status:401}) : Response.json({token_type:'oidc',token:'synthetic-npm-authority'},{status:201});
  if (endpoint.href === 'https://crates.io/api/v1/trusted_publishing/tokens' && options.method === 'POST') return process.env.TEST_REFUSE_CRATES === 'true' ? new Response('private provider authority',{status:403}) : Response.json({token:'cio_tp_'+'a'.repeat(32)});
  throw new Error('Unexpected provider route');
};`,
  );
  for (const [refuseOpenVsx, refuseNpm, refuseCrates] of [
    [false, false, false],
    [true, false, false],
    [false, true, false],
    [false, false, true],
  ]) {
    writeFileSync(trace, "");
    const result = spawnSync(
      process.execPath,
      ["--import", hook, ".github/scripts/publisher-auth-check.mjs"],
      {
        env: {
          ...process.env,
          ...env,
          TEST_REFUSE_OPENVSX: String(refuseOpenVsx),
          TEST_REFUSE_NPM: String(refuseNpm),
          TEST_REFUSE_CRATES: String(refuseCrates),
        },
        encoding: "utf8",
        timeout: 15_000,
      },
    );
    assert.equal(result.status, refuseOpenVsx || refuseNpm || refuseCrates ? 1 : 0, result.stderr);
    assert.match(
      result.stdout,
      refuseOpenVsx
        ? /Open VSX trusted exchange check failed \(HTTP 403\)/u
        : /Open VSX trusted identity exchange passed/u,
    );
    assert.doesNotMatch(result.stdout, /has fixed extension publishing permission/u);
    assert.match(
      result.stdout,
      refuseNpm
        ? /npm trusted exchange check failed for fallow \(HTTP 401\)/u
        : /npm trusted identity exchange passed for fallow/u,
    );
    assert.match(
      result.stdout,
      refuseCrates
        ? /crates.io trusted exchange check failed for matching configurations \(HTTP 403\)/u
        : /crates.io trusted identity exchange passed/u,
    );
    assert.doesNotMatch(result.stdout + result.stderr, /synthetic|cio_tp_|private provider/u);
    const calls = readFileSync(trace, "utf8").trim().split("\n").map(JSON.parse);
    assert.ok(calls.some((call) => call.url.endsWith("/package/fallow") && call.method === "POST"));
    assert.ok(
      calls.some((call) => call.url === "https://crates.io/api/v1/trusted_publishing/tokens"),
    );
    assert.ok(
      calls.every(
        (call) => !call.url.includes("/publish") || call.url.includes("trusted-publishing/token"),
      ),
    );
  }
  writeFileSync(trace, "");
  const unsupported = spawnSync(
    process.execPath,
    ["--import", hook, ".github/scripts/publisher-auth-check.mjs", "--unsupported"],
    {
      env: { ...process.env, ...env },
      encoding: "utf8",
      timeout: 15_000,
    },
  );
  assert.equal(unsupported.status, 1);
  assert.match(unsupported.stdout, /Unsupported identity check arguments/u);
  assert.equal(readFileSync(trace, "utf8"), "");
});

test("npm and crates exchanges validate identity expiry and opaque authority without timestamp metadata", async () => {
  const now = Date.parse("2026-10-16T12:00:00Z");
  const identity = (audience, expires = now + 600_000) =>
    `e30.${Buffer.from(JSON.stringify({ aud: audience, exp: Math.floor(expires / 1000) })).toString("base64url")}.synthetic`;
  const token = {
    token_type: "oidc",
    token: "synthetic-npm-token",
  };
  const run = async (override = null, identityOverride = null, refuseCrates = false) => {
    const calls = [];
    const send = async (url, options) => {
      const endpoint = new URL(url);
      calls.push({ url: String(url), ...options });
      if (endpoint.hostname === "vstoken.actions.githubusercontent.com") {
        return Response.json({
          value: identityOverride ?? identity(endpoint.searchParams.get("audience")),
        });
      }
      if (endpoint.hostname === "registry.npmjs.org") {
        return Response.json(override ?? token, { status: 201 });
      }
      assert.equal(endpoint.href, "https://crates.io/api/v1/trusted_publishing/tokens");
      assert.deepEqual(JSON.parse(options.body), { jwt: identity("crates.io") });
      return refuseCrates
        ? new Response("secret provider diagnostic", { status: 403 })
        : Response.json({ token: `cio_tp_${"a".repeat(32)}` });
    };
    return { result: await verifyOtherRegistryIdentities(env, send, now), calls };
  };
  const success = await run();
  assert.equal(success.result.ok, true);
  assert.ok(success.result.outcomes.every((outcome) => outcome.ok));
  assert.deepEqual(
    success.calls
      .filter((call) => call.url.startsWith("https://registry.npmjs.org/"))
      .map((call) => decodeURIComponent(call.url.split("/package/")[1])),
    NPM_PACKAGES,
  );
  for (const call of success.calls) {
    assert.equal(call.redirect, "error");
    if (call.url.includes("/package/")) {
      assert.equal(call.method, "POST");
      assert.equal(call.body, undefined);
      assert.equal(call.headers.Authorization, `Bearer ${identity("npm:registry.npmjs.org")}`);
    }
  }
  for (const [invalid, stage] of [
    [{ ...token, token_type: "persistent" }, "authority token type"],
    [{ ...token, token: "" }, "authority token shape"],
    [{ ...token, token: "secret\r\n::error::private" }, "authority token shape"],
  ]) {
    const refused = await run(invalid);
    assert.equal(refused.result.ok, false);
    assert.equal(
      refused.calls.filter((call) => new URL(call.url).hostname === "registry.npmjs.org").length,
      NPM_PACKAGES.length,
    );
    assert.equal(refused.result.outcomes.at(-1).ok, true);
    assert.ok(refused.result.outcomes[0].error.includes(` ${stage} check failed`));
    assert.doesNotMatch(JSON.stringify(refused.result), /synthetic|secret|private/u);
  }
  for (const [invalid, stage] of [
    [identity("wrong-audience"), "identity audience"],
    [identity("npm:registry.npmjs.org", now - 1), "identity lifetime"],
    ["not-a-jwt", "identity claims"],
    [
      `e30.${Buffer.from(JSON.stringify({ aud: "npm:registry.npmjs.org", exp: "private" })).toString("base64url")}.synthetic`,
      "identity expiration shape",
    ],
  ]) {
    const refused = await run(null, invalid);
    assert.equal(refused.result.ok, false);
    assert.ok(
      refused.calls.every(
        (call) => new URL(call.url).hostname === "vstoken.actions.githubusercontent.com",
      ),
    );
    assert.ok(refused.result.outcomes.every((outcome) => !outcome.ok));
    assert.ok(refused.result.outcomes[0].error.includes(` ${stage} check failed`));
  }
  const cratesRefusal = await run(null, null, true);
  assert.equal(cratesRefusal.result.ok, false);
  assert.match(
    cratesRefusal.result.outcomes.at(-1).error,
    /crates.io trusted exchange check failed for matching configurations \(HTTP 403\)/u,
  );
  assert.doesNotMatch(JSON.stringify(cratesRefusal.result), /secret provider/u);
});

test("npm uses official escapedName wire paths and continues after refusal through crates", async () => {
  const now = Date.parse("2026-10-16T12:00:00Z");
  const calls = [];
  const send = async (url, options) => {
    const endpoint = new URL(url);
    calls.push(endpoint.href);
    if (endpoint.hostname === "vstoken.actions.githubusercontent.com") {
      const claims = { aud: endpoint.searchParams.get("audience"), exp: now / 1000 + 600 };
      return Response.json({
        value: `e30.${Buffer.from(JSON.stringify(claims)).toString("base64url")}.synthetic`,
      });
    }
    if (endpoint.hostname === "registry.npmjs.org") {
      assert.equal(options.method, "POST");
      return new Response("private provider authority", { status: 401 });
    }
    assert.equal(endpoint.href, "https://crates.io/api/v1/trusted_publishing/tokens");
    return Response.json({ token: `cio_tp_${"a".repeat(32)}` });
  };
  const result = await verifyOtherRegistryIdentities(env, send, now);
  assert.equal(result.ok, false);
  assert.equal(
    calls.filter((call) => call.startsWith("https://registry.npmjs.org/")).length,
    NPM_PACKAGES.length,
  );
  assert.deepEqual(
    calls.filter((call) => call.startsWith("https://registry.npmjs.org/")),
    NPM_PACKAGES.map(
      (name) =>
        `https://registry.npmjs.org/-/npm/v1/oidc/token/exchange/package/${name.replaceAll("/", "%2f")}`,
    ),
  );
  assert.ok(calls.includes("https://crates.io/api/v1/trusted_publishing/tokens"));
  assert.match(
    result.outcomes[0].error,
    /npm trusted exchange check failed for @fallow-cli\/darwin-arm64 \(HTTP 401\)/u,
  );
  assert.equal(result.outcomes.at(-1).ok, true);
  assert.doesNotMatch(JSON.stringify(result), /private provider|synthetic|cio_tp_/u);
});

test("registry diagnostics identify fixed stages without reflecting provider content", async () => {
  const now = Date.parse("2026-10-16T12:00:00Z");
  for (const [failure, expected] of [
    ["identity-http", "GitHub identity request"],
    ["claims", "identity claims"],
    ["exchange-http", "trusted exchange"],
    ["authority", "authority token shape"],
  ]) {
    const send = async (url) => {
      const endpoint = new URL(url);
      if (endpoint.hostname === "vstoken.actions.githubusercontent.com") {
        if (failure === "identity-http") return new Response("private provider", { status: 403 });
        const claims = { aud: endpoint.searchParams.get("audience"), exp: now / 1000 + 600 };
        return Response.json({
          value:
            failure === "claims"
              ? "private-provider"
              : `e30.${Buffer.from(JSON.stringify(claims)).toString("base64url")}.synthetic`,
        });
      }
      if (failure === "exchange-http") return new Response("private provider", { status: 401 });
      return Response.json(
        { token: "" },
        { status: endpoint.hostname === "registry.npmjs.org" ? 201 : 200 },
      );
    };
    const result = await verifyOtherRegistryIdentities(env, send, now);
    assert.equal(result.ok, false);
    for (const outcome of result.outcomes) {
      assert.equal(outcome.ok, false);
      assert.ok(outcome.error.includes(` ${expected} check failed for `));
      assert.match(outcome.error, failure.endsWith("http") ? /\(HTTP (401|403)\)$/u : /[^)]$/u);
    }
    assert.doesNotMatch(
      JSON.stringify(result),
      /private provider|private-provider|synthetic|cio_tp_|2026/u,
    );
  }
});
