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
  if (response.status !== expectedStatus) throw new Error("Identity request was refused");
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
const MAX_NPM_LIFETIME_MS = 2 * 60 * 60 * 1000;
const CLOCK_SKEW_MS = 5 * 60 * 1000;

const getExpiringIdentity = async (env, audience, send, now) => {
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
  if (!validToken(identity.value)) throw new Error("Missing identity");
  const claims = JSON.parse(
    Buffer.from(identity.value.split(".")[1], "base64url").toString("utf8"),
  );
  const expiry = claims.exp * 1000;
  if (
    claims.aud !== audience ||
    !Number.isSafeInteger(claims.exp) ||
    expiry <= now ||
    expiry > now + MAX_IDENTITY_LIFETIME_MS
  ) {
    throw new Error("Identity audience or expiry is invalid");
  }
  return identity.value;
};

/** Exchange fixed npm and crates identities without package bytes, versions or persistent credentials. */
export const verifyOtherRegistryIdentities = async (
  env = process.env,
  send = fetch,
  now = Date.now(),
) => {
  let registry = "npm";
  let packageName = "identity";
  try {
    for (const name of NPM_PACKAGES) {
      packageName = name;
      const identity = await getExpiringIdentity(env, NPM_AUDIENCE, send, now);
      const authority = await requestJson(
        `${NPM_REGISTRY}/-/npm/v1/oidc/token/exchange/package/${encodeURIComponent(name)}`,
        {
          method: "POST",
          headers: { Authorization: `Bearer ${identity}`, Accept: "application/json" },
        },
        send,
        201,
      );
      const created = Date.parse(authority.created);
      const expires = Date.parse(authority.expires);
      if (
        !validToken(authority.token) ||
        authority.token_type !== "oidc" ||
        !Number.isFinite(created) ||
        !Number.isFinite(expires) ||
        created > now + CLOCK_SKEW_MS ||
        expires <= now ||
        expires <= created ||
        expires - created > MAX_NPM_LIFETIME_MS ||
        expires > now + MAX_NPM_LIFETIME_MS
      )
        throw new Error("npm authority expiry is invalid");
    }
    registry = "crates.io";
    packageName = "matching configurations";
    const cratesIdentity = await getExpiringIdentity(env, CRATES_AUDIENCE, send, now);
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
    if (typeof authority.token !== "string" || !/^cio_tp_[A-Za-z0-9]{32}$/u.test(authority.token)) {
      throw new Error("crates authority is invalid");
    }
    return { ok: true };
  } catch {
    return { ok: false, error: `${registry} trusted identity exchange failed for ${packageName}` };
  }
};

/** Check the fixed extension's trusted identity and namespace rights without uploading packages. */
export const verifyOpenVsxIdentity = async (env = process.env, send = fetch) => {
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
    );
    if (!validToken(authority.value)) throw new Error("Missing authority");
    const permission = await requestJson(
      `${REGISTRY}/api/fallow-rs/verify-pat`,
      {
        method: "GET",
        headers: { Authorization: `Bearer ${authority.value}`, Accept: "application/json" },
      },
      send,
    );
    if (typeof permission.success !== "string" || !permission.success)
      throw new Error("Missing permission");
    return { ok: true };
  } catch {
    return { ok: false, error: "Open VSX trusted identity or namespace permission check failed" };
  }
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const result =
    process.argv.length === 2
      ? await verifyOpenVsxIdentity()
      : { ok: false, error: "Unsupported identity check arguments" };
  console.log(
    result.ok ? "Open VSX trusted identity has namespace publishing permission" : result.error,
  );
  const registries = result.ok ? await verifyOtherRegistryIdentities() : result;
  if (result.ok) {
    if (registries.ok) {
      console.log(
        "npm trusted identity exchanges passed; stage-only policy still requires owner verification",
      );
      console.log(
        "crates.io trusted identity exchange passed; matching configurations only, not complete crate scope",
      );
    } else console.log(registries.error);
  }
  process.exitCode = result.ok && registries.ok ? 0 : 1;
}
