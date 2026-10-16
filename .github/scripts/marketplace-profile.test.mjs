import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { createServer } from "node:http";
import { test } from "node:test";
import { fileURLToPath } from "node:url";
import {
  lookupMarketplaceProfile,
  resolveMarketplaceProfile,
  validateIdentityConfig,
} from "./marketplace-profile.mjs";

const profileId = "12345678-1234-1234-1234-123456789abc";
const token = "synthetic-short-lived-token";

const serve = async (t, handler) => {
  const server = createServer(handler);
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(async () => {
    server.closeAllConnections();
    await new Promise((resolve) => server.close(resolve));
  });
  const { port } = server.address();
  return `http://127.0.0.1:${port}`;
};

const transport = (url) => (endpoint, options) => {
  assert.equal(
    endpoint,
    "https://app.vssps.visualstudio.com/_apis/profile/profiles/me?api-version=7.1",
  );
  return fetch(url, options);
};

test("Marketplace profile lookup uses the fixed GET and returns only validated profile ID", async (t) => {
  const url = await serve(t, (request, response) => {
    assert.equal(request.method, "GET");
    assert.equal(request.headers.authorization, `Bearer ${token}`);
    response.setHeader("Content-Type", "application/json");
    response.end(JSON.stringify({ id: profileId, displayName: "private fixture", token }));
  });
  assert.deepEqual(await lookupMarketplaceProfile(token, transport(url)), { ok: true, profileId });
});

test("Marketplace rejects invalid profiles, HTTP failures and redirects without forwarding the token", async (t) => {
  let redirectSinkReached = false;
  const url = await serve(t, (request, response) => {
    if (request.url === "/redirect") {
      response.writeHead(302, { Location: "/sink" });
      response.end();
      return;
    }
    if (request.url === "/sink") redirectSinkReached = true;
    if (request.url === "/http-error") {
      response.writeHead(403);
      response.end(`upstream private response ${token}`);
      return;
    }
    response.setHeader("Content-Type", "application/json");
    response.end(
      JSON.stringify({ id: request.url === "/invalid" ? `invalid\n${token}` : profileId }),
    );
  });
  assert.deepEqual(await lookupMarketplaceProfile(token, transport(`${url}/good`)), {
    ok: true,
    profileId,
  });
  for (const [path, error] of [
    ["invalid", "Marketplace profile response has no valid profile ID"],
    ["http-error", "Marketplace profile request returned HTTP 403"],
    ["redirect", "Marketplace profile request failed"],
  ]) {
    assert.deepEqual(await lookupMarketplaceProfile(token, transport(`${url}/${path}`)), {
      ok: false,
      error,
    });
  }
  assert.equal(redirectSinkReached, false);
});

test("Marketplace captures only the configured tenant's short-lived CLI token in memory", async (t) => {
  const config = {
    clientId: "11111111-2222-3333-4444-555555555555",
    tenantId: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
  };
  const url = await serve(t, (request, response) => {
    assert.equal(request.headers.authorization, `Bearer ${token}`);
    response.end(JSON.stringify({ id: profileId, accessToken: token }));
  });
  const runAzure = (command, args, options) => {
    assert.equal(command, "az");
    if (args[1] === "show") {
      assert.deepEqual(args, ["account", "show", "--output", "json"]);
      return JSON.stringify({
        tenantId: config.tenantId,
        user: { type: "servicePrincipal", name: config.clientId },
      });
    }
    assert.deepEqual(args, [
      "account",
      "get-access-token",
      "--resource",
      "499b84ac-1321-427f-aa17-267ca6975798",
      "--tenant",
      config.tenantId,
      "--output",
      "json",
    ]);
    assert.deepEqual(options.stdio, ["ignore", "pipe", "pipe"]);
    return JSON.stringify({ tenant: config.tenantId, accessToken: token });
  };
  const success = await resolveMarketplaceProfile(config, runAzure, transport(url));
  assert.deepEqual(success, { ok: true, profileId });
  const failedAzure = (command, args, options) => {
    if (args[1] === "show") return runAzure(command, args, options);
    throw new Error(`private CLI diagnostic ${token}`);
  };
  assert.deepEqual(await resolveMarketplaceProfile(config, failedAzure, transport(url)), {
    ok: false,
    error: "Marketplace Azure token acquisition failed",
  });
  for (const tenant of ["bbbbbbbb-bbbb-cccc-dddd-eeeeeeeeeeee", null, {}]) {
    const wrongTenant = (command, args, options) =>
      args[1] === "show"
        ? runAzure(command, args, options)
        : JSON.stringify({ tenant, accessToken: token });
    assert.deepEqual(await resolveMarketplaceProfile(config, wrongTenant, transport(url)), {
      ok: false,
      error: "Marketplace Azure token tenant does not match configuration",
    });
  }
  assert.deepEqual(validateIdentityConfig(config), { ok: true });
  assert.deepEqual(validateIdentityConfig({ ...config, clientId: "" }), {
    ok: false,
    error: "Marketplace Azure identity configuration is incomplete",
  });
});

test("Marketplace rejects a personal account or different app before any profile request", async (t) => {
  const config = {
    clientId: "11111111-2222-3333-4444-555555555555",
    tenantId: "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
  };
  let requests = 0;
  const url = await serve(t, (request, response) => {
    requests++;
    response.end(JSON.stringify({ id: profileId }));
  });
  const executeAs = (user) => (command, args) =>
    JSON.stringify(
      args[1] === "show"
        ? { tenantId: config.tenantId, user }
        : { tenant: config.tenantId, accessToken: token },
    );
  const valid = { type: "servicePrincipal", name: config.clientId };
  assert.deepEqual(await resolveMarketplaceProfile(config, executeAs(valid), transport(url)), {
    ok: true,
    profileId,
  });
  assert.equal(requests, 1);
  for (const user of [
    { type: "user", name: "owner@example.invalid" },
    { type: "servicePrincipal", name: "22222222-2222-3333-4444-555555555555" },
    null,
  ]) {
    assert.deepEqual(await resolveMarketplaceProfile(config, executeAs(user), transport(url)), {
      ok: false,
      error: "Marketplace Azure account does not match the configured app identity",
    });
    assert.equal(requests, 1);
  }
});

test("a CI run of marketplace-profile.mjs checks the configuration before its arguments", () => {
  const repoRoot = fileURLToPath(new URL("../..", import.meta.url));
  // Synthetic IDs pass validation. The extra argument stops the run before any Azure call.
  const result = spawnSync(
    process.execPath,
    [".github/scripts/marketplace-profile.mjs", "--validate-config", "--extra"],
    {
      cwd: repoRoot,
      encoding: "utf8",
      env: {
        PATH: process.env.PATH,
        GITHUB_ACTIONS: "true",
        GITHUB_REF: "refs/heads/main",
        AZURE_CLIENT_ID: profileId,
        AZURE_TENANT_ID: profileId,
      },
    },
  );

  assert.equal(result.status, 1);
  assert.equal(result.stdout, "");
  assert.equal(result.stderr, "Unsupported Marketplace identity verification arguments\n");
});
