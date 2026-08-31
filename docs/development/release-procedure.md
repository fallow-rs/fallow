# Release procedure

Maintainer-only procedure for publishing a Fallow release: version, changelog,
generated contracts, companion repositories, registry publication, and
post-release verification. The policy tests under `scripts/` assert the
invariants in this file.

## Preflight

1. Read `docs/development/quality-gates.md`, the release row in
   `docs/development/task-context-map.md`, and
   `docs/development/release-security.md`.
2. Confirm `main` is clean, current with `origin/main`, reviewed, and green.
   Check that the planned version tag is absent remotely and that no
   unresolved draft release or concurrent release is in progress.
3. Require repository release immutability to be enabled. The release workflow
   cannot check this: the endpoint needs the Administration read permission,
   which is not a grantable workflow token scope. Verify it here, with
   maintainer credentials, as the only gate:

   ```bash
   enabled="$(gh api -H "X-GitHub-Api-Version: 2026-03-10" \
     repos/fallow-rs/fallow/immutable-releases --jq '.enabled')"
   [ "$enabled" = "true" ] || { echo "Release immutability is not enabled" >&2; exit 1; }
   ```
4. Derive the semantic-version bump from every commit since the prior release
   unless the user supplied an explicit bump. Confirm a major bump before
   mutating versions unless the user explicitly requested it.
5. Dispatch the reusable release-validation workflow against `main` and require
   a completed successful result before changing versions or creating a tag.

## Prepare

6. Verify the complete `[Unreleased]` changelog against the commit range,
   issues, discussions, and external contributors since the prior tag. Ground
   public names, flags, rule IDs, and contributor handles in source or GitHub,
   not memory.
7. Draft curated public GitHub release notes before starting the publication
   workflow. They must:

   - explain user-visible value rather than repeat commit subjects;
   - cover every material changelog entry and any breaking or migration detail;
   - use only public, generic examples;
   - name no competing or upstream third-party project;
   - contain no em-dash characters in the title or the body;
   - verify contributor attribution;
   - end with the exact full-changelog comparison URL.

   The published release is immutable, so every one of these is a
   pre-publication gate, not something to repair afterwards.

   Store the notes outside the repository, for example
   `/tmp/fallow-release-vX.Y.Z.md`, and require the file to be non-empty.

   `scripts/verify-release-metadata.mjs` enforces the machine-checkable half of
   this list, so skipping the runbook no longer skips the gates. Two surfaces,
   because the notes and the release exist at different moments:

   - `release.yml` runs it at dispatch against `CHANGELOG.md`, the source the
     notes are drafted from. It requires one dated `## [X.Y.Z]` section with a
     non-empty body and no em-dash, the matching compare-link definition back
     to the previous released version, and an empty `[Unreleased]` section so
     nothing ships uncredited. Publication cannot start without it.
   - `release-published.yml` runs it again on the `release: published` event,
     the first moment a title and body exist, against the title prefix, the
     non-empty body, the comparison URL, em-dashes, and third-party names.

   The changelog gate deliberately omits the third-party-name rule: changelog
   entries legitimately name a migration source or a documented parity gap,
   while the public release surface does not.
8. Apply version changes transactionally. Regenerate every public contract,
   adapter, packaged skill, and version-bearing artifact. Synchronize
   `fallow-docs` and `fallow-skills` from their canonical sources.

   Two version-bearing manifests sit outside the workspace bump and follow
   opposite rules. `scripts/sync-npm-versions.sh` rewrites both and is wired to
   no workflow, so a release can skip it silently.

   - `tools/type-aware-sidecar` bumps in lockstep with the CLI, inside the
     release commit. The CLI refuses a companion whose version differs from the
     binary, so a skipped bump turns `main` red on Windows validation rather
     than at publish time.
   - `crates/napi` and its `@fallow/*` platform manifests stay at the last
     published version through the release commit, keeping any dependabot bump
     that landed on top. Their platform packages do not exist on npm at the new
     version until publish runs, so bumping them early leaves a lockfile with
     unresolvable entries and `npm ci` fails. They sync in the post-publish
     catch-up step.

   Version-string assertions do not catch the second case. Verify a touched
   lockfile with `cd crates/napi && rm -rf node_modules && npm ci`.
9. Run package dry-runs, generated-contract checks, companion-repository
   checks, and the repository's full release gates. Review the exact staged
   paths before creating a signed release commit. Do not create the version tag
   yet.

## Publish

10. Push the release commit to `main`. While the version tag is still absent,
    dispatch the release workflow from `main` with the planned tag:

    ```bash
    # Use the verified values established during release preparation.
    TAG="v${VERSION}"
    TITLE="${TAG}: ${SUMMARY}"
    NOTES_FILE="/tmp/fallow-release-${TAG}.md"
    RELEASE_COMMIT="$(git rev-parse HEAD)"

    test -s "$NOTES_FILE"
    grep -qF "https://github.com/fallow-rs/fallow/compare/${PREVIOUS_TAG}...${TAG}" "$NOTES_FILE"

    # release.yml gates the changelog section these notes are drafted from, but
    # the release does not exist while it runs, so the title and the notes file
    # are checked here, before the dispatch that publishes every registry.
    case "$TITLE" in
     "${TAG}: "?*) ;;
     *) echo "release title must be '${TAG}: <summary>'" >&2; exit 1 ;;
    esac
    EM_DASH="$(printf '\xe2\x80\x94')"
    if printf '%s' "$TITLE" | grep -qF "$EM_DASH" || grep -qF "$EM_DASH" "$NOTES_FILE"; then
     echo "release title or notes contain an em-dash" >&2
     exit 1
    fi

    git push origin main
    git fetch origin main
    if [ "$(git rev-parse origin/main)" != "$RELEASE_COMMIT" ]; then
     echo "origin/main moved away from the prepared release commit" >&2
     exit 1
    fi
    if git ls-remote --exit-code --tags origin "refs/tags/${TAG}" >/dev/null 2>&1; then
     echo "release tag already exists: ${TAG}" >&2
     exit 1
    fi

    gh workflow run release.yml --ref main \
     -f tag="$TAG"
    ```

    The workflow deliberately has no tag trigger and never creates a tag or
    GitHub Release. It validates and builds the release, stores the complete
    flattened GitHub asset bundle as the `release-assets` Actions artifact, and
    publishes registries while the tag remains absent. The VS Code release is
    published by separate Marketplace and Open VSX jobs. A credential-free
    public verifier checks every exact target before the final release gate.
11. Monitor the specific workflow run through `status=completed` and
    `conclusion=success`; a successful watch command alone is not sufficient
    evidence. Require `similar-code-conformance` to validate the exact Linux x64
    sidecar artifact against the committed F32 Candle baseline before
    `release-verified` passes. Also require the `Publish VS Code Marketplace
    targets`, `Publish Open VSX targets`, `Verify public VS Code registry targets`,
    and `Release ready for signed tag` jobs to pass. The public verifier requires
    the exact universal plus six platform tuples and normalized payloads from
    both registries, without accepting a universal fallback.
    Download the `release-assets` artifact from that exact run and confirm it
    is non-empty. Confirm it contains the seven target VSIX files,
    `inventory.json`, and `SHA256SUMS`. Only then create and push the signed tag
    and create the immutable GitHub Release:

    ```bash
    ASSET_DIR="$(mktemp -d)"
    gh run download "$RUN_ID" \
      --name release-assets \
      --dir "$ASSET_DIR"
    test -n "$(find "$ASSET_DIR" -maxdepth 1 -type f -print -quit)"
    test "$(find "$ASSET_DIR" -maxdepth 1 -name 'fallow-vscode-*.vsix' -type f | wc -l | tr -d ' ')" -eq 7
    test -f "$ASSET_DIR/inventory.json"
    test -f "$ASSET_DIR/SHA256SUMS"

    if git ls-remote --exit-code --tags origin "refs/tags/${TAG}" >/dev/null 2>&1; then
      echo "release tag appeared before publication completed: ${TAG}" >&2
      exit 1
    fi

    git tag -s "$TAG" "$RELEASE_COMMIT" -m "Fallow ${VERSION}"
    git verify-tag "$TAG"
    git push origin "$TAG"

    gh release create "$TAG" "$ASSET_DIR"/* \
      --repo fallow-rs/fallow \
      --verify-tag \
      --title "$TITLE" \
      --notes-file "$NOTES_FILE"
    ```

    GitHub CLI creates a draft internally, uploads every asset, and only then
    publishes it, matching GitHub's immutable-release guidance. If the workflow
    fails, repair and rerun it without burning a tag. If release creation fails
    after tag push, keep the signed tag, remove only an incomplete draft if one
    exists, and retry release creation. Never recreate or move the signed tag.
    Update the rolling Action tags from maintainer credentials only after the
    immutable release is published.

## Verify and close

12. Query the GitHub Release and require a non-draft, non-prerelease release
    marked immutable, with the expected title, a non-empty body, the exact
    comparison link, the signed tag at `RELEASE_COMMIT`, and uploaded assets.
    Publication also triggers `release-published.yml`, which asserts the same
    title, body, comparison link, and naming rules. Require that run green.
    Assets, tag and target are frozen at publication, but the title and body
    are not, so a failure there is repaired with `gh release edit --title` or
    `--notes-file` rather than by recreating anything.
    Then verify every published crate, npm package, editor package, binary,
    schema, documentation deployment, and companion contract from its real
    public endpoint.
13. Complete the required post-publication NAPI, Docker, rolling-tag, issue,
    discussion, and companion-repository follow-ups. Require their resulting
    `main` workflows to finish green and every touched worktree to be clean.

Do not report a release complete while any publication, public release-note,
registry, companion, deployment, or post-release verification gate is pending.
