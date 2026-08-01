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
| `release-assets` | Flatten and store the complete GitHub asset bundle as a run artifact | Read only |
| `release-ready` | Join publication jobs and prove the tag is still absent | Read only |
| `publish-crates` | Publish prevalidated crates in dependency order | crates.io OIDC |
| `npm-prep` | Install, assemble, and pack npm artifacts | Read only |
| `npm-publish` | Publish downloaded tarballs | npm publication |
| `vscode-prep` | Install, build, and package the extension | Read only |
| `vscode-publish` | Publish the downloaded VSIX | Marketplace tokens |

Preparation jobs may execute dependency code because they have no publication
credentials. Publication jobs must remain small. They must not install
repository dependencies, run package lifecycle scripts, or execute
repository-controlled build steps. They may install pinned publisher CLIs
globally with `--ignore-scripts`.

## Invariants

- Keep every checkout at `persist-credentials: false`.
- Keep repository dependency installation out of `npm-publish`,
  `vscode-publish`, and `publish-crates`.
- Keep `--ignore-scripts` on every privileged `npm publish`.
- Keep global publication tools pinned to reviewed versions.
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
