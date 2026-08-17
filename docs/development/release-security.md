# Release security

Use this reference when editing `.github/workflows/release.yml` or the release
workflow skill. The maintainer dispatches the workflow against the signed
release commit while the version tag is still absent. The workflow never
creates, moves, or publishes Git tags or GitHub Releases.

## Job boundaries

| Job | Responsibility | Credentials |
|---|---|---|
| `release-context` | Bind the dispatch to `main`, the release version, and an absent tag | Read only |
| `build` | Build and sign release artifacts | Artifact signing only |
| `validate` | Reusable release validation | Read only |
| `release-verified` | Join build and validation | None |
| `release-assets` | Flatten and store the complete GitHub asset bundle, including all VSIX targets | Read only |
| `release-ready` | Join publication jobs and prove the tag is still absent | Read only |
| `publish-crates` | Publish prevalidated crates in dependency order | crates.io OIDC |
| `npm-prep` | Install, assemble, and pack npm artifacts | Read only |
| `npm-publish` | Publish downloaded tarballs | npm publication |
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

- Keep every checkout at `persist-credentials: false`.
- Keep repository dependency installation out of `npm-publish`, both VSIX
  publisher jobs, and `publish-crates`.
- Keep `--ignore-scripts` on every privileged `npm publish`.
- Keep global publication tools pinned to reviewed versions.
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
- Keep the version tag absent until validation, asset staging, and every
  registry and marketplace publication have completed successfully.
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
