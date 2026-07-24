# Release security

Use this reference when editing `.github/workflows/release.yml` or the release
workflow skill. The workflow is triggered by a version tag created by the
maintainer release flow. It never creates or moves Git tags itself.

## Job boundaries

| Job | Responsibility | Credentials |
|---|---|---|
| `build` | Build and sign release artifacts | Artifact signing only |
| `validate` | Reusable release validation | Read only |
| `release-verified` | Join build and validation | None |
| `release` | Create the GitHub Release from artifacts | GitHub contents write |
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
- Push rolling Action tags and refresh Dockerfile binary pins from the
  maintainer release workflow after published assets exist, not from the
  credential-bearing GitHub workflow.

## Verification

Run the repository release tests plus:

```bash
actionlint .github/workflows/release.yml
zizmor --persona auditor --no-online-audits --format plain --config .github/zizmor.yml --min-confidence medium .github/workflows/release.yml
python3 -c "import yaml; yaml.safe_load(open('.github/workflows/release.yml'))"
```

Then run the full repository verification from
[quality gates](quality-gates.md).
