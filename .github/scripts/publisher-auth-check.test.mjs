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

test("Open VSX checks fixed identity exchange and namespace permission without packages or PAT", async () => {
  const calls = [];
  const send = async (url, options) => {
    calls.push({ url: String(url), ...options });
    const payloads = [
      { value: "synthetic-identity" },
      { value: "synthetic-authority" },
      { success: "Valid token" },
    ];
    return Response.json(payloads[calls.length - 1]);
  };
  assert.deepEqual(await verifyOpenVsxIdentity({ ...env, OVSX_PAT: "must-not-be-used" }, send), {
    ok: true,
  });
  assert.equal(calls.length, 3);
  assert.equal(new URL(calls[0].url).searchParams.get("audience"), "https://open-vsx.org");
  assert.equal(calls[0].headers.Authorization, "Bearer synthetic-request");
  assert.equal(calls[1].url, "https://open-vsx.org/api/-/trusted-publishing/token");
  assert.equal(calls[1].method, "POST");
  assert.deepEqual(JSON.parse(calls[1].body), {
    namespace: "fallow-rs",
    extension: "fallow-vscode",
    token: "synthetic-identity",
  });
  assert.equal(calls[2].url, "https://open-vsx.org/api/fallow-rs/verify-pat");
  assert.equal(calls[2].method, "GET");
  assert.equal(calls[2].headers.Authorization, "Bearer synthetic-authority");
  for (const call of calls) assert.equal(call.redirect, "error");
});

test("Open VSX refuses identity and permission failures without token fallback or publication", async () => {
  for (const failedRequest of [0, 1, 2]) {
    let calls = 0;
    const send = async () => {
      const index = calls++;
      if (index === failedRequest) return new Response("private provider details", { status: 403 });
      return Response.json({ value: "synthetic-token" });
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
      Response.json({ value: "token", success: "Valid token" }),
    ),
    { ok: true },
  );
});

test("Open VSX check exercises HTTP exchange and permission reads on loopback", async (t) => {
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
      response.end(JSON.stringify({ value: "synthetic-authority" }));
      return;
    }
    assert.equal(request.url, "/api/fallow-rs/verify-pat");
    assert.equal(request.method, "GET");
    assert.equal(request.headers.authorization, "Bearer synthetic-authority");
    response.end(JSON.stringify({ success: "Valid token" }));
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
  assert.equal(requests, 3);
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
  if (endpoint.hostname === 'open-vsx.org') return Response.json(endpoint.pathname.endsWith('verify-pat')?{success:'Valid token'}:{value:'synthetic-authority'});
  if (endpoint.hostname === 'registry.npmjs.org' && options.method === 'POST' && endpoint.pathname.startsWith('/-/npm/v1/oidc/token/exchange/package/')) return Response.json({token_type:'oidc',token:'synthetic-npm-authority',created:new Date().toISOString(),expires:new Date(Date.now()+3600000).toISOString()},{status:201});
  if (endpoint.href === 'https://crates.io/api/v1/trusted_publishing/tokens' && options.method === 'POST') return Response.json({token:'cio_tp_'+'a'.repeat(32)});
  throw new Error('Unexpected provider route');
};`,
  );
  const result = spawnSync(
    process.execPath,
    ["--import", hook, ".github/scripts/publisher-auth-check.mjs"],
    {
      env: { ...process.env, ...env },
      encoding: "utf8",
      timeout: 15_000,
    },
  );
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /npm trusted identity exchanges passed/u);
  assert.match(result.stdout, /crates.io trusted identity exchange passed/u);
  assert.doesNotMatch(result.stdout + result.stderr, /synthetic|cio_tp_/u);
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
});

test("npm and crates exchanges validate authority expiration and stop on refusal", async () => {
  const now = Date.parse("2026-10-16T12:00:00Z");
  const identity = (audience, expires = now + 600_000) =>
    `e30.${Buffer.from(JSON.stringify({ aud: audience, exp: Math.floor(expires / 1000) })).toString("base64url")}.synthetic`;
  const token = {
    token_type: "oidc",
    token: "synthetic-npm-token",
    created: new Date(now).toISOString(),
    expires: new Date(now + 3_600_000).toISOString(),
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
  assert.deepEqual(success.result, { ok: true });
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
  for (const invalid of [
    { ...token, expires: new Date(now - 1).toISOString() },
    { ...token, expires: "invalid" },
    { ...token, created: "invalid" },
    { ...token, created: new Date(now + 600_000).toISOString() },
    { ...token, expires: new Date(now + 86_400_000).toISOString() },
    { ...token, token_type: "persistent" },
    { ...token, token: "" },
  ]) {
    const refused = await run(invalid);
    assert.equal(refused.result.ok, false);
    assert.equal(refused.calls.length, 2);
    assert.doesNotMatch(JSON.stringify(refused.result), /synthetic/u);
  }
  for (const invalid of [
    identity("wrong-audience"),
    identity("npm:registry.npmjs.org", now - 1),
    "not-a-jwt",
  ]) {
    const refused = await run(null, invalid);
    assert.equal(refused.result.ok, false);
    assert.equal(refused.calls.length, 1);
  }
  const cratesRefusal = await run(null, null, true);
  assert.equal(cratesRefusal.result.ok, false);
  assert.match(cratesRefusal.result.error, /crates.io trusted identity exchange failed/u);
  assert.doesNotMatch(cratesRefusal.result.error, /secret provider/u);
});
