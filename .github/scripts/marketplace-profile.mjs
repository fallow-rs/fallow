import { execFileSync } from "node:child_process";
import { appendFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const PROFILE_URL = "https://app.vssps.visualstudio.com/_apis/profile/profiles/me?api-version=7.1";
const MARKETPLACE_RESOURCE = "499b84ac-1321-427f-aa17-267ca6975798";
const ID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/iu;
const REQUEST_TIMEOUT_MS = 10_000;
const TOKEN_TIMEOUT_MS = 30_000;
const TOKEN_OUTPUT_LIMIT = 128 * 1024;

/** Validate the explicit app and tenant used for the protected identity login. */
export const validateIdentityConfig = ({ clientId, tenantId }) =>
  typeof clientId === "string" &&
  ID_PATTERN.test(clientId) &&
  typeof tenantId === "string" &&
  ID_PATTERN.test(tenantId)
    ? { ok: true }
    : { ok: false, error: "Marketplace Azure identity configuration is incomplete" };

/** Fetch the fixed global profile endpoint without returning credentials or profile attributes. */
export const lookupMarketplaceProfile = async (accessToken, fetchProfile = fetch) => {
  if (typeof accessToken !== "string" || !accessToken || /[\r\n]/u.test(accessToken)) {
    return { ok: false, error: "Marketplace access token is unavailable" };
  }
  let response;
  try {
    response = await fetchProfile(PROFILE_URL, {
      method: "GET",
      headers: { Authorization: `Bearer ${accessToken}`, Accept: "application/json" },
      redirect: "error",
      signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
    });
  } catch {
    return { ok: false, error: "Marketplace profile request failed" };
  }
  if (response.status !== 200) {
    return { ok: false, error: `Marketplace profile request returned HTTP ${response.status}` };
  }
  let profile;
  try {
    profile = await response.json();
  } catch {
    return { ok: false, error: "Marketplace profile response is invalid" };
  }
  if (typeof profile?.id !== "string" || !ID_PATTERN.test(profile.id)) {
    return { ok: false, error: "Marketplace profile response has no valid profile ID" };
  }
  return { ok: true, profileId: profile.id };
};

/** Resolve the app's profile through the official CLI's short-lived Marketplace token. */
export const resolveMarketplaceProfile = async (
  config,
  runAzure = execFileSync,
  fetchProfile = fetch,
) => {
  const validation = validateIdentityConfig(config);
  if (!validation.ok) return validation;
  const cliOptions = {
    encoding: "utf8",
    timeout: TOKEN_TIMEOUT_MS,
    maxBuffer: TOKEN_OUTPUT_LIMIT,
    stdio: ["ignore", "pipe", "pipe"],
  };
  let account;
  try {
    account = JSON.parse(runAzure("az", ["account", "show", "--output", "json"], cliOptions));
  } catch {
    return { ok: false, error: "Marketplace Azure account lookup failed" };
  }
  if (
    account?.user?.type !== "servicePrincipal" ||
    typeof account.user.name !== "string" ||
    account.user.name.toLowerCase() !== config.clientId.toLowerCase() ||
    typeof account.tenantId !== "string" ||
    account.tenantId.toLowerCase() !== config.tenantId.toLowerCase()
  ) {
    return {
      ok: false,
      error: "Marketplace Azure account does not match the configured app identity",
    };
  }
  let credential;
  try {
    const output = runAzure(
      "az",
      [
        "account",
        "get-access-token",
        "--resource",
        MARKETPLACE_RESOURCE,
        "--tenant",
        config.tenantId,
        "--output",
        "json",
      ],
      cliOptions,
    );
    credential = JSON.parse(output);
  } catch {
    return { ok: false, error: "Marketplace Azure token acquisition failed" };
  }
  if (
    typeof credential?.tenant !== "string" ||
    credential.tenant.toLowerCase() !== config.tenantId.toLowerCase()
  ) {
    return { ok: false, error: "Marketplace Azure token tenant does not match configuration" };
  }
  const profile = await lookupMarketplaceProfile(credential.accessToken, fetchProfile);
  if (!profile.ok || !config.verifyPublisher) return profile;
  try {
    const response = await fetchProfile(
      "https://marketplace.visualstudio.com/_apis/securityroles/scopes/gallery.publisher/roleassignments/resources/fallow-rs?api-version=3.2-preview.1",
      {
        method: "GET",
        headers: { Authorization: `Bearer ${credential.accessToken}`, Accept: "application/json" },
        redirect: "error",
        signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
      },
    );
    if (response.status !== 200) throw new Error("Publisher access refused");
    const assignments = await response.json();
    const permitted =
      Array.isArray(assignments?.value) &&
      assignments.value.some(
        (assignment) =>
          typeof assignment?.identity?.id === "string" &&
          assignment.identity.id.toLowerCase() === profile.profileId.toLowerCase() &&
          ["Contributor", "Owner", "Creator"].includes(assignment?.role?.name) &&
          assignment.role.denyPermissions === 0,
      );
    if (permitted) return { ...profile, publisherVerified: true };
  } catch {
    return { ok: false, error: "Marketplace publisher permission lookup failed" };
  }
  return { ok: false, error: "Marketplace app has no verified publishing role for fallow-rs" };
};

/** Verify the frozen VSCE Azure credential chain without publishing or exposing CLI output. */
export const verifyMarketplaceAzureCredential = (runNative = execFileSync, env = process.env) => {
  const safeEnv = { ...env };
  for (const name of [
    "VSCE_PAT",
    "AZURE_CLIENT_SECRET",
    "AZURE_CLIENT_CERTIFICATE_PATH",
    "AZURE_CLIENT_CERTIFICATE_PASSWORD",
  ]) {
    delete safeEnv[name];
  }
  try {
    runNative(
      process.execPath,
      [
        fileURLToPath(
          new URL("../publishers/marketplace/node_modules/@vscode/vsce/vsce", import.meta.url),
        ),
        "verify-pat",
        "fallow-rs",
        "--azure-credential",
      ],
      {
        env: safeEnv,
        encoding: "utf8",
        timeout: TOKEN_TIMEOUT_MS,
        maxBuffer: TOKEN_OUTPUT_LIMIT,
        stdio: ["ignore", "pipe", "pipe"],
      },
    );
    return { ok: true };
  } catch {
    return { ok: false, error: "Marketplace native Azure credential verification failed" };
  }
};

const main = async () => {
  const config = {
    clientId: process.env.AZURE_CLIENT_ID,
    tenantId: process.env.AZURE_TENANT_ID,
    verifyPublisher: process.argv[2] === "--verify-publisher",
  };
  if (process.env.GITHUB_ACTIONS !== "true" || process.env.GITHUB_REF !== "refs/heads/main") {
    console.error("Marketplace identity verification requires the protected main workflow");
    return 1;
  }
  const validation = validateIdentityConfig(config);
  if (!validation.ok) {
    console.error(validation.error);
    return 1;
  }
  if (process.argv.length === 3 && process.argv[2] === "--validate-config") return 0;
  if (process.argv.length === 3 && process.argv[2] === "--verify-vsce") {
    const native = verifyMarketplaceAzureCredential();
    console.log(
      native.ok ? "Marketplace native Azure credential verification passed" : native.error,
    );
    return native.ok ? 0 : 1;
  }
  if (process.argv.length !== 2 && !(process.argv.length === 3 && config.verifyPublisher)) {
    console.error("Unsupported Marketplace identity verification arguments");
    return 1;
  }
  const result = await resolveMarketplaceProfile(config);
  if (!result.ok) {
    console.error(result.error);
    return 1;
  }
  if (process.env.GITHUB_OUTPUT) {
    appendFileSync(process.env.GITHUB_OUTPUT, `profile_id=${result.profileId}\n`);
  }
  console.log(`Marketplace profile ID: ${result.profileId}`);
  if (result.publisherVerified) console.log("Marketplace app has a publishing role for fallow-rs");
  return 0;
};

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  try {
    process.exitCode = await main();
  } catch {
    console.error("Marketplace identity verification failed");
    process.exitCode = 1;
  }
}
