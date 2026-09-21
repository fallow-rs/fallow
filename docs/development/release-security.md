# Release security

Use this reference when editing `.github/workflows/release.yml` or
`release-procedure.md`. The maintainer dispatches the workflow against the signed
release commit while the version tag is still absent. The workflow never
creates, moves, or publishes Git tags or GitHub Releases.

## Job boundaries

| Job | Responsibility | Credentials |
|---|---|---|
| `release-context` | Bind the dispatch to `main`, the release version, and an absent tag | Read only |
| `build` | Build and sign release artifacts | Artifact signing only |
| `validate` | Reusable release validation | Read only |
| `release-verified` | Join build, validation, and `similar-code-conformance` | None |
| `release-assets` | Flatten and store the complete GitHub asset bundle, including all VSIX targets | Read only |
| `release-ready` | Join publication jobs and prove the tag is still absent | Read only |
| `publish-crates` | Publish prevalidated crates in dependency order | crates.io OIDC |
| `npm-prep` | Install, assemble, and pack npm artifacts | Read only |
| `npm-publish` | Publish downloaded tarballs and stage the `fallow` root | npm OIDC, stage-only for `fallow` |
| `vscode-prep` | Build seven VSIX targets plus their inventory and checksums | Read only |
| `vscode-host-smoke` | Run the exact prepared x64 target VSIX on Linux, Windows, and macOS with matching release binaries | Read only |
| `vscode-publish-marketplace` | Publish the closed VSIX set to Visual Studio Marketplace | VSCE token only |
| `vscode-publish-open-vsx` | Publish the closed VSIX set to Open VSX | OVSX token only |
| `vscode-public-verify` | Verify exact public target payloads from both registries | Read only |

Preparation jobs may execute dependency code because they have no publication
credentials. Publication jobs must remain small. They must not install
repository dependencies, run package lifecycle scripts, or execute
repository-controlled build steps. They may install pinned publisher CLIs
globally with `--ignore-scripts`.

## Invariants

- Run every credential-bearing job in the `release` environment: `build`
  (binary signing key), `publish-crates`, `npm-publish`, and both VSIX
  publisher jobs. Its deployment branch policy admits `main` only. The ref
  check in `release-context` is part of the workflow file, so it stops a
  mistaken dispatch but not an edited copy of the workflow on another ref; the
  environment is enforced by GitHub outside the file.
- Keep `VSCE_PAT`, `OVSX_PAT`, and `ED25519_BINARY_SIGNING_PRIVATE_KEY` as
  `release` environment secrets only. A repository-level copy is readable by
  any workflow on any ref. The maintainer preflight verifies the branch policy
  and the absence of repository-level copies, because the workflow token cannot
  read environment settings.
- Pin the crates.io trusted publishing configs to the `release` environment.
  crates.io accepts an environment claim when a config sets none, and rejects a
  token without the matching claim once it does, so the pin is what stops an
  edited workflow on another ref from publishing crates over OIDC.
- Keep every checkout at `persist-credentials: false`.
- Keep repository dependency installation out of `npm-publish`, both VSIX
  publisher jobs, and `publish-crates`.
- Keep `--ignore-scripts` on every privileged `npm publish`.
- Keep global publication tools pinned to reviewed versions.
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
- Clear `NODE_AUTH_TOKEN` for the stage call so a bootstrap token never reaches
  a staged name.
- Treat npm error code `E409` from the stage call as already staged. The job
  has no npm login, so it cannot read stages, and the `npm view` precheck does
  not see a staged version. The maintainer digest comparison is what proves
  which run built the staged bytes.
- Keep staged publishing on a reviewed npm pin of at least 11.15.0.
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
  the exact Linux x64 sidecar artifact against the committed F32 Candle
  baseline before any publication job can start.
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
