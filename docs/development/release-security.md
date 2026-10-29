# Release security

Use this reference when editing `.github/workflows/release.yml` or
`release-procedure.md`. The maintainer dispatches the workflow against the signed
release commit while the version tag is still absent. The workflow never
creates, moves, or publishes Git tags or GitHub Releases.

## Job boundaries

| Job | Responsibility | Credentials |
|---|---|---|
| `release-context` | Bind the dispatch to `main`, the release version, and an absent tag | Read only |
| `pgo-profile` | Build an instrumented binary, train the PGO profile on pinned public fixtures, and upload only the profile | Read only |
| `build` | Build unsigned binaries, archives, and NAPI addons | Read only |
| `authenticode` | Add an Authenticode signature to the Windows executables and NAPI addons, then replace their build artifacts | Artifact Signing OIDC in `windows-signing` |
| `sign` | Sign downloaded raw binaries and stage npm binary payloads on fresh runners | Artifact signing only |
| `validate` | Reusable release validation, including the similar-code sidecar dependency audit | Read only |
| `musl-smoke` | Run the exact unsigned musl binaries in a digest-pinned Alpine container on x64 and arm64 | Read only |
| `release-verified` | Join build, signing, validation, `similar-code-conformance`, and `musl-smoke` | None |
| `release-assets` | Flatten and store the complete GitHub asset bundle, including all VSIX targets | Read only |
| `release-ready` | Join publication jobs and prove the tag is still absent | Read only |
| `publish-crates` | Publish prevalidated crates in dependency order | crates.io OIDC |
| `npm-prep` | Install, assemble, and pack npm artifacts | Read only |
| `npm-publish` | Publish downloaded tarballs and stage the `fallow` root | npm OIDC, stage-only for `fallow` |
| `npm-root-approved` | Wait until the maintainer-approved `fallow` root is public with the staged bytes, for both VSIX publishers and `release-ready` | Read only |
| `vscode-prep` | Build seven VSIX targets plus their inventory and checksums | Read only |
| `vscode-host-smoke` | Run the exact prepared x64 target VSIX on Linux, Windows, and macOS with matching release binaries | Read only |
| `vscode-publish-marketplace` | Check the approved `fallow` root again, then publish the closed VSIX set to Visual Studio Marketplace | VSCE PAT or Marketplace OIDC |
| `vscode-publish-open-vsx` | Check the approved `fallow` root again, then publish the closed VSIX set to Open VSX | OVSX PAT or Open VSX OIDC |
| `vscode-public-verify` | Verify exact public target payloads from both registries | Read only |

Preparation jobs may execute dependency code because they have no publication
credentials. Publication jobs must remain small. They must not install
repository dependencies, run package lifecycle scripts, or execute
repository-controlled build steps. VSIX publishers install only the dedicated
reviewed tool graph under `.github/publishers/` with `npm ci --ignore-scripts`.
Its manifests and integrity-pinned lockfiles come from a sparse checkout of the
validated dispatch SHA. Tools never come from build artifacts or caches, and
publication tokens appear only on the final publication step.
The Open VSX graph overrides its VSCE packaging dependency to the same reviewed
version used for Marketplace, avoiding its older vulnerable secret-scanning
graph. Publication uses prepared VSIX files, with no packaging step.

## VSIX publisher authorization

The `release` environment variables `VSCE_AUTH_MODE` and `OVSX_AUTH_MODE`
independently select `pat` or `oidc`; Marketplace also accepts `azure`. An unset
variable preserves `pat`.
Unknown modes, a missing selected PAT, or unavailable GitHub OIDC identity
fail before upload. In `oidc` mode the workflow excludes the legacy PAT from
the step environment and explicitly requests trusted publishing. Registry
authorization failures never switch back to PAT authentication.

Before selecting `oidc`, the publisher or namespace owner must register and
verify the provider policy for `fallow-rs/fallow`, `release.yml`, the existing
publisher `fallow-rs`, and the protected `release` environment. Require that
environment in the provider binding; a repository/workflow-only policy does
not stop an edited workflow on another ref. Do not activate the mode until
the provider's binding fields and rights are confirmed. The extension is
`fallow-rs.fallow-vscode`.

CLI support does not establish that a registry exposes policy configuration
for this account. If Marketplace has no available trusted-publisher policy
control, leave `VSCE_AUTH_MODE` at `pat`; a prepared `--oidc` route is not an
activated publisher identity. Open VSX namespace policy configuration is a
separate owner action.

The frozen `@vscode/vsce@4.0.0` supports `--oidc`, requesting audience
`marketplace.visualstudio.com`. The frozen `ovsx@1.2.0` supports
`--trusted-publishing`, requesting the registry URL audience
`https://open-vsx.org`. Both exchange the identity for short-lived publication
authority. Their graphs and binary signing keys are independent of this mode
selection. Account policies, mode activation, legacy-token revocation and
real publication require separate owner actions.

## Windows Authenticode signing

Windows Smart App Control and SmartScreen block an unsigned executable that
has no reputation. They do not know the Ed25519 signature that `sign` adds. The
`authenticode` job signs every Fallow Windows file with Azure Artifact Signing:
the CLI, multicall, LSP, MCP and similar-code executables, and the NAPI addon.
The certificate subject is `Fallow B.V.`. The job never signs third-party
files, such as the TypeScript binary in the type-aware VSIX.

The job runs in its own `windows-signing` environment, not in `release`. The
publisher jobs in `release` run third-party tools with `id-token: write`. With
a shared subject, a compromised tool could request a signature as Fallow B.V.
The `windows-signing` deployment branch policy admits `main` only, and no
other job may use the environment.

`WINDOWS_SIGNING_MODE` in the `windows-signing` environment selects the mode. `off`, or no value, keeps the build artifacts unchanged and prints a
warning. `authenticode` requires these nonsecret variables in the same
environment:

| Variable | Value |
|---|---|
| `WINDOWS_SIGNING_CLIENT_ID` | Client ID of the Entra app with the Certificate Profile Signer role |
| `WINDOWS_SIGNING_TENANT_ID` | Tenant ID of that app and of the Artifact Signing account |
| `WINDOWS_SIGNING_ENDPOINT` | Regional account endpoint, for example `https://neu.codesigning.azure.net/` |
| `WINDOWS_SIGNING_ACCOUNT` | Artifact Signing account name |
| `WINDOWS_SIGNING_PROFILE` | Public Trust certificate profile name |

The Entra app federates with issuer
`https://token.actions.githubusercontent.com`, subject
`repo:fallow-rs/fallow:environment:windows-signing` and audience
`api://AzureADTokenExchange`. The repository uses the default OIDC subject, so
the subject must not use the immutable ID format. The app holds only the
Artifact Signing Certificate Profile Signer role on the signing account.

The job runs after `build` and before `sign`. It downloads the raw payload,
the NAPI addon and the release archive, and checks that each archive entry has
the same bytes as the raw payload. After signing, it requires a valid,
timestamped signature with `O=Fallow B.V.` on every file. It then rebuilds the
archive from the signed files and uploads all three artifacts again under their
build names. `sign`, `npm-prep`, `vscode-prep` and `release-assets` therefore
read only signed Windows files. Authenticode changes the file bytes, so it must
come before the Ed25519 signature and before every checksum.

## Marketplace Entra federation

`VSCE_AUTH_MODE=azure` selects Microsoft Entra workload federation. Set the
nonsecret `VSCE_AZURE_CLIENT_ID` and `VSCE_AZURE_TENANT_ID` variables in the
protected `release` environment only after the publisher owner has verified the
identity. The workflow validates the IDs and GitHub identity before login, uses
pinned `azure/login` with `allow-no-subscriptions: true`, and requires its
successful outcome before calling the frozen VSCE CLI with `--azure-credential`.
It installs the frozen publisher graph before Azure credentials exist. Azure
and direct OIDC modes never inject `VSCE_PAT`; the Azure route also clears
inherited client-secret and certificate credentials before publication.

The owner must authorize the app service principal's global Marketplace profile
ID as a Contributor of the existing publisher. App/client IDs, application
object IDs and service-principal object IDs are not that profile ID. Configure
the app's federation issuer as `https://token.actions.githubusercontent.com`,
subject as `repo:fallow-rs/fallow:environment:release`, and audience as
`api://AzureADTokenExchange`. The default subject identifies the repository and
environment, not the workflow filename, so keep environment review, main-only
branch policy and disabled administrator bypass. No project, organization
connection, Azure resource role or subscription ID is part of this workflow.

The manual-only `verify-marketplace-identity.yml` workflow provides the profile
lookup prerequisite after owner-approved source integration. It runs only on
`main` in the protected `release` environment, uses the configured app's Entra
identity, and makes the fixed global profile GET with API version 7.1.
It requires Azure CLI's active account to be the configured service principal
and tenant before obtaining a token. It emits a validated profile ID, never a
token or upstream response body, and refuses redirects. The default lookup
installs no dependencies. Neither mode reads signing or PAT secrets or publishes
an extension. Running it still requires separate owner approval, app registration and
federation setup; the prepared workflow is not provider verification. Use the
returned ID for owner-controlled publisher membership and read it back there.
After the owner grants the profile a publishing role, dispatch that workflow
with `verify_publisher=true`. It also reads the fixed publisher role-assignment
endpoint and requires this profile to hold Contributor, Owner or Creator with
no denied permissions. Reader access does not pass. This optional path installs
only the frozen Marketplace CLI with lifecycle scripts disabled, before Azure
login. It also runs `verify-pat fallow-rs --azure-credential` to check the native
credential chain used by publication. CLI output stays captured, and only a
fixed pass or fail message reaches the log. The native command alone accepts
Reader, so the strict app-profile role check remains required.
The real publisher job must never be dispatched as an identity capability probe.

Open VSX binds identity to `release.yml`. Its owner-approved check therefore
uses that workflow with `auth_check=true`, `tag=auth-check` and `pgo=false`.
The deliberately invalid version tag provides a second barrier if the release
context guard regresses. The check runs only on `main` in the protected
`release` environment. `release-context` and all release build, signing and
publication jobs skip. The separate check job installs no dependencies and
reads no package, PAT or signing secret. It requests GitHub identity for
`https://open-vsx.org`, exchanges it for the fixed `fallow-rs.fallow-vscode`
extension, and validates the exchange response.
No registry URL or namespace is supplied by an input. Redirects and refusal
fail closed, with no PAT fallback and no provider response or token in logs.

The exchange issues short-lived authority at the provider without uploading a
package. A successful check proves the fixed extension identity binding at
that time. It does not prove current namespace publishing permission or an
extension upload, or change either active auth mode. The pinned production
server source has no read-only permission probe usable by this extension-scoped
token;
verify publisher membership separately with owner access.

The same fixed diagnostic path checks npm exchange even if Open VSX fails,
while the combined result remains a failure. It checks each package in the
release workflow's exact npm inventory. It uses audience
`npm:registry.npmjs.org`, requires the documented HTTP 201 response, and
checks the returned opaque token and OIDC token type. Like the npm publisher,
it does not require returned timestamp metadata or claim to verify the
opaque authority's lifetime. Every fixed npm
package is checked independently. The check requests `crates.io` identity and
performs one token-only exchange even if npm fails. Failures report only the
fixed package, request stage and HTTP status, without provider content. Both identities
must have the expected audience and a valid, bounded expiration before they
reach a registry. Requests have fixed timeouts and refuse redirects. No
package bytes, versions, upload, staging call or version reservation are sent.
Authority remains in memory and is not written to logs, outputs or artifacts.

An npm exchange does not prove that the `fallow` root's policy is stage-only:
ordinary and staged publication use the same exchange. The crates.io response
returns an opaque token with no crate inventory or expiration field. A valid
response proves some matching configuration, not all release crates. The
server defines its temporary lifetime, which this response cannot verify.
Read back the full crate configuration inventory and npm root permissions
with owner credentials before declaring complete publication readiness.
References: [npm exchange API](https://api-docs.npmjs.com/),
[pinned npm exchange implementation](https://github.com/npm/cli/blob/v11.19.0/lib/utils/oidc.js),
and [crates.io exchange implementation](https://github.com/rust-lang/crates.io/blob/main/src/controllers/trustpub/tokens/exchange/mod.rs).

App registration, federation, publisher membership, mode activation and legacy
PAT retirement are separate owner actions. A prepared source route does not
prove provider authorization. Keep the current PAT mode until the exact identity
and publisher permissions have been verified without uploading an extension.
This route changes publishing administration, not customer authentication or
shipped extension behavior. Do not create paid resources to enable it.

References: [Marketplace service-principal authorization](https://learn.microsoft.com/en-us/azure/devops/extend/publish/command-line),
[Azure login without a subscription](https://github.com/Azure/login#login-without-subscription),
and [federated app credentials](https://learn.microsoft.com/en-us/entra/workload-id/workload-identity-federation-create-trust).

## Invariants

- Run every credential-bearing job in the `release` environment: `sign`
  (binary signing key), `publish-crates`, `npm-publish`, and both VSIX
  publisher jobs. Its deployment branch policy admits `main` only. The ref
  check in `release-context` is part of the workflow file, so it stops a
  mistaken dispatch but not an edited copy of the workflow on another ref; the
  environment is enforced by GitHub outside the file.
- Keep required publication secrets in the protected `release` environment.
  Rotation can move `VSCE_PAT`, `OVSX_PAT`, or
  `ED25519_BINARY_SIGNING_PRIVATE_KEY` when its value is entered again.
  The existing signing key can also move through a reviewed, one-time
  ciphertext-only transfer without changing the key or its consumers.
  Independently verify a signature made with the protected environment copy
  before retiring the repository copy. GitHub's API does not return secret
  values. A repository copy remains readable by a workflow on any ref and is
  not protected by the environment. The maintainer preflight verifies the
  branch policy, names unprotected copies, and rejects duplicates; complete
  transfer verification and retirement before running that preflight.
  The workflow token cannot read environment settings.
- Pin the crates.io trusted publishing configs to the `release` environment.
  crates.io accepts an environment claim when a config sets none, and rejects a
  token without the matching claim once it does, so the pin is what stops an
  edited workflow on another ref from publishing crates over OIDC.
- Run `authenticode` alone in the `windows-signing` environment, with a
  main-only deployment branch policy. The Entra signing app trusts only that
  environment subject.
- Keep `pgo-profile` outside the `release` environment and without
  credentials. It installs the dependencies of public fixtures with
  `--ignore-scripts`. It uploads only the merged `.profdata` file, as the
  `pgo-profile-<target>` artifact. The instrumented binary never leaves the job. The
  artifact name must not start with `fallow-`, because `release-assets`
  downloads that pattern as release assets. The `build` job fails when a
  release binary contains the LLVM profiler runtime.
- Keep `build` outside the `release` environment, with no signing credential.
  Upload raw binary data only as `unsigned-binaries-<platform>`, separately from
  the existing release archives and NAPI addons. Each `sign` matrix runner is
  fresh, downloads its exact unsigned artifact, and rejects unexpected,
  missing, empty, or nonregular entries before the signing key is exposed.
  Signing uses only Node builtins and the committed signing scripts. Never
  install dependencies, restore build caches, or execute downloaded binaries
  in `sign`. The key is set only on the signing step. Preserve the signed
  release artifact names and npm multicall payload names. Every consumer of
  signed binaries must wait for `sign`, including npm preparation and
  similar-code conformance.
- Keep `musl-smoke` outside the `release` environment, with `contents: read`
  only and no secrets. It executes the downloaded `unsigned-binaries-<platform>`
  artifact, so it must never receive a credential. It runs the binaries in an
  Alpine image pinned by digest, with read-only mounts and no container
  network. `release-verified` must need it, so a musl binary that cannot run
  blocks every publication job.
- Keep every checkout at `persist-credentials: false`.
- Keep repository dependency installation out of `npm-publish`, both VSIX
  publisher jobs, and `publish-crates`.
- Keep `--ignore-scripts` on every privileged `npm publish`.
- Keep publication tools and their transitive dependency graph pinned in the
  dedicated lockfiles. Invoke them with `npx --no-install --prefix` so execution
  cannot fetch a missing CLI. Review lockfile changes before release.
- Stage the `fallow` npm root, never publish it from the workflow. `fallow` pins
  its platform packages and `fallow-type-aware` to the exact release version,
  so those reach users only through a new root. The `fallow` trusted publisher
  grants stage publish only, and npm refuses a direct publish over OIDC with
  HTTP 403. The maintainer approves the stage with npm 2FA before the signed
  tag, after comparing `npm stage download` with the `20-cli-root` tarball in
  the `npm-tarballs` artifact of the same run. The published version keeps the
  workflow provenance attestation.
- Make exactly one `npm stage publish` call per staged name per run and never
  probe with a direct publish. Every attempt, including a refused one, signs
  and logs a provenance statement before the registry answers.
- Keep `NODE_AUTH_TOKEN` empty throughout `npm-publish`. Every package must
  have environment-scoped trusted publishing configured. Never restore a
  repository token fallback: a token bypasses the OIDC environment binding.
- Treat npm error code `E409` from the stage call as already staged. The job
  has no npm login, so it cannot read stages, and the `npm view` precheck does
  not see a staged version. The maintainer digest comparison is what proves
  which run built the staged bytes.
- Keep staged publishing on a reviewed npm pin of at least 11.15.0.
- Publish no VSIX before the approved `fallow` root is public. The VS Code
  extension downloads its binary from the GitHub Release of its own version and
  purges an installed binary of another version, and that release is created
  after the approval. `npm-root-approved` is the only job that waits for the
  approval. It has no credentials and polls the public registry with the digest
  that `npm-publish` recorded. It downloads the public tarball and requires its
  sha256 to equal that digest. Its wait is bounded below GitHub's job limit and
  names its recovery: approve, then rerun the failed jobs of the same run.
- Both VSIX publishers and `release-ready` need `npm-root-approved`. Thus a
  publisher holds no runner while the approval is pending. Because a publisher
  starts after the approval, the `release` environment asks for a second
  review at that time. When `npm-publish` fails, the publishers do not start.
- Each VSIX publisher checks the public bytes again before any upload. Its
  first steps use no credential. They download the `npm-tarballs` artifact and
  record the sha256 of the `20-cli-root` tarball, the file that `npm-publish`
  stages. Then they run the same wait step as `npm-root-approved`, with a
  bound of a few minutes, because the root is already public. Other bytes
  under the released version fail the job. A policy test keeps the three wait
  steps equal.
- Keep `publish-crates` and the direct npm publishes ahead of the approval.
  They do not depend on the GitHub Release, and gating them would only extend
  the time between the VSIX publication and the release.
- The residual window is the VSIX publication plus the public verification
  plus the maintainer's tag step. The maintainer flow creates the tag and the
  release immediately after `release-ready`. Inside that window the extension
  keeps serving its previously installed, verified binary and retries the
  download once the release exists, so the window delays the update instead
  of stranding the install.
- Keep the VSIX artifact closed to the seven universal and platform-specific
  packages, `inventory.json`, and `SHA256SUMS`. The inventory is universal
  first and publication follows that order.
- Make `vscode-host-smoke` download the exact `vscode-prep` artifact and the
  matching release CLI and LSP artifacts. It must verify and load the exact
  extracted `linux-x64`, `win32-x64`, and `darwin-x64` extension paths, preserve
  archive executable modes on Unix, and never rebuild those release binaries.
  Both VSIX publishers and `release-assets` must wait for this matrix.
- Give each VSIX publisher only its matching token and pinned CLI. It must not
  check out repository code or build packages. Publish every inventory entry
  with `--skip-duplicate`, attempt the remaining entries after an unexpected
  failure, and fail the job after the loop.
- Retry the failed set in later passes before recording a VSIX entry as failed.
  Both registries return intermittent per-target failures, Marketplace as a
  gallery request timeout and Open VSX as HTTP 503, and `--skip-duplicate`
  makes a repeated attempt idempotent. Attempt every entry in the first pass,
  then retry only the entries that failed, so the sleep budget is one finite
  schedule for the whole step rather than one schedule per target: 330 seconds
  of sleeping across six passes, plus the time the attempts themselves take.
  Log at most one warning per pass naming the failed targets, emit the error
  annotation once per target that never landed and only after the last pass,
  and do not branch on registry error text.
- Gate `release-ready` directly on `vscode-public-verify`. The verifier has no
  registry credentials. It waits for the exact version and target tuples with
  bounded retries, downloads each exact registry asset, and compares its
  normalized extension payload with the prepared inventory. Universal fallback
  never satisfies a platform target.
- Keep `cargo publish --no-verify` in the credential-bearing job. Compilation
  and validation happen before credentials are present.
- Keep the publishable crate list in dependency order and aligned with the
  release publish-list test.
- Keep artifact inventory and package-name constants aligned with the build
  matrix.
- Make `release-verified` wait for `similar-code-conformance`, which validates
  the exact Linux x64, macOS arm64, and macOS x64 sidecar artifacts against
  the committed F32 Candle baseline before any publication job can start. The
  macOS legs cover the Accelerate build.
- Keep the similar-code sidecar dependency audit in `release-validation.yml`.
  Its `cargo audit` call ignores exactly the advisory IDs in
  `tools/similar-code-sidecar/audit-allowlist.json`. The pre-flight
  validation dispatch runs it before a version bump, and `validate` runs it
  again before `release-verified`, so a new upstream advisory stops the
  release before any publication job.
- Require repository release immutability before publication. Verify it in the
  maintainer pre-flight, not in the workflow: reading
  `repos/{owner}/{repo}/immutable-releases` needs the Administration read
  permission, and `administration` is not a grantable workflow token scope, so
  declaring it makes the workflow unparseable while `contents: read` gets HTTP
  403 from the endpoint.
- Dispatch the release workflow from `main` with the strict semantic-version
  tag. Reject a mismatched version or existing remote tag before expensive work
  starts, then reconfirm tag absence before staging the final asset bundle.
- Flatten the complete binary inventory into the `release-assets` Actions
  artifact. Reject an empty inventory or duplicate asset name.
- Keep the version tag absent until validation, asset staging, every registry
  and marketplace publication, and the maintainer approval of the staged
  `fallow` root have completed successfully.
- Create and push the signed version tag near the end of the maintainer flow.
  Immediately create the GitHub Release with the curated notes and the exact
  `release-assets` bundle. GitHub CLI creates a draft, uploads every asset, and
  publishes only after upload, so release immutability is applied to a complete
  release.
- Do not generate release notes. Require a `vMAJOR.MINOR.PATCH: ` title prefix
  with a non-empty summary, a non-empty body, the exact repository comparison
  URL, and the complete asset inventory before creating the tag. The published
  release is immutable, so these are pre-publication gates and the maintainer
  flow owns them; the workflow cannot check a release that does not exist yet.
- Push rolling Action tags and refresh Dockerfile binary pins from the
  maintainer release workflow after published assets exist, not from the
  credential-bearing GitHub workflow.

## Verification

Run the repository release tests plus:

```bash
actionlint .github/workflows/release.yml
uvx zizmor@1.26.1 --config .github/zizmor.yml --min-confidence medium --format plain .github/workflows/release.yml
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))"
```

Then run the full repository verification from
[quality gates](quality-gates.md).

## Changing this contract

The publication order described here is mirrored by maintainer-side release
runbooks that this repository cannot see or gate. Whenever the job graph, the
dispatch inputs, the asset-bundle name, the final gate job, or the ownership of
a verification step changes, treat those runbooks as companions that must be
resynchronized in the same pass. A runbook left on the previous order does not
fail loudly: it can push a tag that no longer triggers anything and publish an
immutable release with no assets.
