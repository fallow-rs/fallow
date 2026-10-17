import { fileURLToPath } from "node:url";

const REGISTRY = "https://open-vsx.org";
const REQUEST_TIMEOUT_MS = 10_000;
const WORKFLOW_REF = "fallow-rs/fallow/.github/workflows/release.yml@refs/heads/main";

const requestJson = async (url, options, send, expectedStatus = 200) => {
  const response = await send(url, {
    ...options,
    redirect: "error",
    signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
  });
  if (response.status !== expectedStatus)
    throw Object.assign(new Error("Identity request was refused"), { httpStatus: response.status });
  const value = await response.json();
  if (!value || typeof value !== "object" || value.error) {
    throw new Error("Identity response was invalid");
  }
  return value;
};

const validToken = (value) =>
  typeof value === "string" && value.length > 0 && !/[\r\n]/u.test(value);

const getIdentityUrl = (env, audience) => {
  if (
    env.GITHUB_ACTIONS !== "true" ||
    env.GITHUB_REF !== "refs/heads/main" ||
    env.GITHUB_WORKFLOW_REF !== WORKFLOW_REF ||
    !validToken(env.ACTIONS_ID_TOKEN_REQUEST_TOKEN)
  )
    throw new Error("Identity check requires the protected release workflow");
  const url = new URL(env.ACTIONS_ID_TOKEN_REQUEST_URL);
  if (
    url.protocol !== "https:" ||
    !url.hostname.endsWith(".actions.githubusercontent.com") ||
    url.username ||
    url.password ||
    url.port
  )
    throw new Error("GitHub identity endpoint is invalid");
  url.searchParams.set("audience", audience);
  return url;
};

/** Fixed npm publication inventory, checked against the release manifest contract. */
export const NPM_PACKAGES = Object.freeze([
  "@fallow-cli/darwin-arm64",
  "@fallow-cli/darwin-x64",
  "@fallow-cli/linux-x64-gnu",
  "@fallow-cli/linux-arm64-gnu",
  "@fallow-cli/linux-x64-musl",
  "@fallow-cli/linux-arm64-musl",
  "@fallow-cli/win32-x64-msvc",
  "@fallow-cli/win32-arm64-msvc",
  "@fallow-cli/fallow-similar-code-darwin-arm64",
  "@fallow-cli/fallow-similar-code-darwin-x64",
  "@fallow-cli/fallow-similar-code-linux-x64-gnu",
  "@fallow-cli/fallow-similar-code-linux-arm64-gnu",
  "@fallow-cli/fallow-similar-code-linux-x64-musl",
  "@fallow-cli/fallow-similar-code-linux-arm64-musl",
  "@fallow-cli/fallow-similar-code-win32-x64-msvc",
  "@fallow-cli/fallow-similar-code-win32-arm64-msvc",
  "fallow-type-aware",
  "fallow-similar-code",
  "fallow",
  "@fallow-cli/fallow-node-darwin-arm64",
  "@fallow-cli/fallow-node-darwin-x64",
  "@fallow-cli/fallow-node-linux-x64-gnu",
  "@fallow-cli/fallow-node-linux-arm64-gnu",
  "@fallow-cli/fallow-node-linux-x64-musl",
  "@fallow-cli/fallow-node-linux-arm64-musl",
  "@fallow-cli/fallow-node-win32-x64-msvc",
  "@fallow-cli/fallow-node-win32-arm64-msvc",
  "@fallow-cli/fallow-node",
]);
const NPM_REGISTRY = "https://registry.npmjs.org";
const NPM_AUDIENCE = "npm:registry.npmjs.org";
const CRATES_AUDIENCE = "crates.io";
const MAX_IDENTITY_LIFETIME_MS = 30 * 60 * 1000;

const getExpiringIdentity = async (env, audience, send, now, setStage) => {
  setStage("GitHub identity request");
  const identity = await requestJson(
    getIdentityUrl(env, audience),
    {
      method: "GET",
      headers: {
        Authorization: `Bearer ${env.ACTIONS_ID_TOKEN_REQUEST_TOKEN}`,
        Accept: "application/json",
      },
    },
    send,
  );
  setStage("identity claims");
  if (!validToken(identity.value)) throw new Error("Missing identity");
  const claims = JSON.parse(
    Buffer.from(identity.value.split(".")[1], "base64url").toString("utf8"),
  );
  setStage("identity audience");
  if (claims.aud !== audience) throw new Error("Identity audience is invalid");
  setStage("identity expiration shape");
  if (!Number.isSafeInteger(claims.exp)) throw new Error("Identity expiration is invalid");
  const expiry = claims.exp * 1000;
  setStage("identity lifetime");
  if (expiry <= now || expiry > now + MAX_IDENTITY_LIFETIME_MS) {
    throw new Error("Identity audience or expiry is invalid");
  }
  return identity.value;
};

const exchangeFailure = (registry, target, stage, error) => {
  const status = Number.isInteger(error?.httpStatus) ? ` (HTTP ${error.httpStatus})` : "";
  return { ok: false, error: `${registry} ${stage} check failed for ${target}${status}` };
};

const verifyNpmIdentity = async (name, env, send, now) => {
  let stage = "GitHub identity request";
  try {
    const identity = await getExpiringIdentity(env, NPM_AUDIENCE, send, now, (value) => {
      stage = value;
    });
    stage = "trusted exchange";
    const authority = await requestJson(
      `${NPM_REGISTRY}/-/npm/v1/oidc/token/exchange/package/${name.replaceAll("/", "%2f")}`,
      {
        method: "POST",
        headers: { Authorization: `Bearer ${identity}`, Accept: "application/json" },
      },
      send,
      201,
    );
    stage = "authority token shape";
    if (!validToken(authority.token)) throw new Error("Missing authority");
    stage = "authority token type";
    if (authority.token_type !== "oidc") throw new Error("Invalid authority type");
    // Match the publisher's opaque-token exchange proof; returned lifetime metadata is not required.
    return { ok: true, message: `npm trusted identity exchange passed for ${name}` };
  } catch (error) {
    return exchangeFailure("npm", name, stage, error);
  }
};

const verifyCratesIdentity = async (env, send, now) => {
  let stage = "GitHub identity request";
  try {
    const cratesIdentity = await getExpiringIdentity(env, CRATES_AUDIENCE, send, now, (value) => {
      stage = value;
    });
    stage = "trusted exchange";
    const authority = await requestJson(
      "https://crates.io/api/v1/trusted_publishing/tokens",
      {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          Accept: "application/json",
          "User-Agent": "fallow-release-auth-check (https://github.com/fallow-rs/fallow)",
        },
        body: JSON.stringify({ jwt: cratesIdentity }),
      },
      send,
    );
    stage = "authority token shape";
    if (typeof authority.token !== "string" || !/^cio_tp_[A-Za-z0-9]{32}$/u.test(authority.token)) {
      throw new Error("crates authority is invalid");
    }

    return {
      ok: true,
      message:
        "crates.io trusted identity exchange passed; matching configurations only, not complete crate scope",
    };
  } catch (error) {
    return exchangeFailure("crates.io", "matching configurations", stage, error);
  }
};

/** Check every fixed npm package and crates independently without uploading package bytes. */
export const verifyOtherRegistryIdentities = async (
  env = process.env,
  send = fetch,
  now = Date.now(),
) => {
  const outcomes = [];
  for (const name of NPM_PACKAGES) outcomes.push(await verifyNpmIdentity(name, env, send, now));
  outcomes.push(await verifyCratesIdentity(env, send, now));
  return { ok: outcomes.every((outcome) => outcome.ok), outcomes };
};

/** Exchange the fixed extension's trusted identity without uploading packages. */
export const verifyOpenVsxIdentity = async (env = process.env, send = fetch) => {
  let stage = "GitHub identity";
  try {
    const identityUrl = getIdentityUrl(env, REGISTRY);
    const identity = await requestJson(
      identityUrl,
      {
        method: "GET",
        headers: {
          Authorization: `Bearer ${env.ACTIONS_ID_TOKEN_REQUEST_TOKEN}`,
          Accept: "application/json",
        },
      },
      send,
    );
    if (!validToken(identity.value)) throw new Error("Missing identity");
    stage = "trusted exchange";
    const authority = await requestJson(
      `${REGISTRY}/api/-/trusted-publishing/token`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json", Accept: "application/json" },
        body: JSON.stringify({
          namespace: "fallow-rs",
          extension: "fallow-vscode",
          token: identity.value,
        }),
      },
      send,
      201,
    );
    if (!validToken(authority.value)) throw new Error("Missing authority");
    return { ok: true };
  } catch (error) {
    const status = Number.isInteger(error?.httpStatus) ? ` (HTTP ${error.httpStatus})` : "";
    return { ok: false, error: `Open VSX ${stage} check failed${status}` };
  }
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const result =
    process.argv.length === 2
      ? await verifyOpenVsxIdentity()
      : { ok: false, error: "Unsupported identity check arguments" };
  console.log(
    result.ok
      ? "Open VSX trusted identity exchange passed for fallow-rs.fallow-vscode; publisher permission requires owner verification"
      : result.error,
  );
  const registries = process.argv.length === 2 ? await verifyOtherRegistryIdentities() : result;
  if (process.argv.length === 2) {
    for (const outcome of registries.outcomes)
      console.log(outcome.ok ? outcome.message : outcome.error);
    console.log("npm stage-only policy still requires owner verification");
  }
  process.exitCode = result.ok && registries.ok ? 0 : 1;
}
